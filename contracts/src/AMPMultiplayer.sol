// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "openzeppelin-contracts/contracts/access/Ownable2Step.sol";
import "openzeppelin-contracts/contracts/utils/Pausable.sol";
import "openzeppelin-contracts/contracts/utils/ReentrancyGuard.sol";
import "openzeppelin-contracts/contracts/utils/cryptography/ECDSA.sol";
import "openzeppelin-contracts/contracts/utils/cryptography/EIP712.sol";

/**
 * @title AMPMultiplayer — N-player escrow + quorum settlement (v2).
 *
 * 1. DUAL-DEPOSIT ESCROW — each participant deposits `stake + reportingBond`.
 *    Stakes form the prize pool; bonds are slashed from non-signers and
 *    split 50/50 between the relayer gas fund and rank 1.
 *
 * 2. K-OF-N QUORUM SETTLEMENT — a ladder settles when K = floor(2N/3)+1
 *    participants (K = N when N <= 3) sign identical
 *    (rankedPlacements, transcriptHash) pairs over EIP-712. Two conflicting
 *    K-quorums intersect in >= f+1 signers (N = 3f+1; Lamport 1982 signed
 *    bound), so divergence always reaches the bonded verifier.
 *
 * 3. PULL-ONLY LEDGER — resolution never transfers; it increments
 *    `claimable[]`. Withdrawals are player-initiated and isolated.
 *
 * Timers: quorum window (default 120s) -> grace window (300s) with a
 * unilateral rank-1 claim -> challenge by a >=2-signer conflicting ladder
 * -> bonded-verifier verdict. Mass-disconnect no-claim lobbies refund all.
 */
contract AMPMultiplayer is Ownable2Step, Pausable, ReentrancyGuard, EIP712 {
    using ECDSA for bytes32;

    // ── errors ──────────────────────────────────────────────────────────

    error MatchExists();
    error MatchNotFound();
    error NotOpen();
    error WrongState(uint8 have, uint8 want);
    error QuorumWindowStillOpen();
    error LobbyFull();
    error AlreadyJoined();
    error WrongDeposit();
    error LobbyTooSmall();
    error LobbyTooLarge();
    error NotExpiredYet();
    error NotParticipant();
    error QuorumNotReached(uint256 have, uint256 want);
    error NotPermutation();
    error BadSigner(address recovered);
    error SigCountMismatch();
    error NotRankOne();
    error LadderMismatch();
    error NothingClaimable();
    error TransferFailed();
    error FeeTooHigh();
    error BadProfile();
    error ProfileNotFound();
    error ZeroAddress();
    error NotDisputed();
    error BadVerifier();
    error ChallengeStakeTooSmall();

    // ── types ───────────────────────────────────────────────────────────

    enum State {
        Empty, // 0
        Open, // 1 — escrow filling
        Ready, // 2 — playing / quorum window / grace window
        GracePending, // 3 — unilateral rank-1 claim posted, challenge open
        Disputed, // 4 — bonded verifier decides
        Settled, // 5 — terminal
        Cancelled // 6 — terminal, refunds credited
    }

    struct Match {
        uint256 gameId;
        uint64 lobbySize; // N (2..64)
        uint16 payoutProfileId;
        uint256 stakePerPlayer;
        uint256 bondPerPlayer;
        uint64 joinedUntil;
        uint64 readyAt;
        uint64 quorumUntil;
        uint64 graceUntil;
        uint64 challengeUntil;
        State state;
        uint256 joinedMask; // bit i = participants[i] funded
        address[64] participants;
        bytes32 ladderAHash; // grace claimant's keccak(ranked||transcript)
        bytes32 ladderBHash; // challengers' keccak(ranked||transcript)
        uint256 claimantMask; // faction A: the grace claimant's bit
        uint256 factionBMask; // challenger quorum bits
        uint256 challengeStakeEscrowed;
        // ── settlement record (prove-your-payout; packed for gas) ──
        // slot A: signer mask + claimed mask + rank-1 slash half + fee bits
        uint64 settledSignerMask; // bond-eligibility (or winner faction) bits
        uint64 claimedMask; // double-claim guard
        uint96 winnerSlashHalf; // non-signer bonds half due to rank 1
        uint32 feesClaimedBits; // [studio|treasury|relayer] fee claim flags
        // slot B: economics snapshot (fee amounts derive at claim)
        uint96 settledNetPool; // tier basis (rake applied at settle; gross in disputes)
        uint96 settledGrossRake;
        uint32 reserved;
        // slot C (dispute only): fee + reward economics
        uint96 settledStudioFee;
        uint96 disputeRewardShare; // per-winner 70% share
        uint32 reserved2;
        // remaining record fields
        bytes32 settledLadderHash; // the winning ladder
        uint64 disputeLoserMask; // dispute mode: forfeited (sentinel = max)
    }

    // ── storage ─────────────────────────────────────────────────────────

    uint16 public protocolRakeBps; // of gross stake pool
    uint16 public constant MAX_PROTOCOL_RAKE_BPS = 500;
    uint16 public studioSplitBps; // of the gross rake
    address public protocolFeeRecipient;
    address public relayerFeeRecipient;
    mapping(uint256 => address) public studioRecipientOf;
    // (game, lobbySize, profileId) => tier bps over the NET pool
    mapping(uint256 => mapping(uint64 => mapping(uint16 => uint16[]))) public payoutProfiles;
    mapping(uint256 => mapping(uint64 => mapping(uint16 => bool))) public payoutProfileActive;
    address public disputeVerifier;
    mapping(bytes32 => Match) private matches;
    mapping(bytes32 => mapping(address => uint8)) private indexOf; // player -> index+1
    mapping(address => uint256) public claimable;

    // ── events ──────────────────────────────────────────────────────────

    event LobbyCreated(
        bytes32 indexed matchId, uint256 indexed gameId, uint64 lobbySize, uint256 stake, uint256 bond, uint16 profileId
    );
    event PlayerJoined(bytes32 indexed matchId, address indexed player, uint64 index);
    event LobbyReady(bytes32 indexed matchId, uint64 quorumUntil, uint64 graceUntil);
    event Settled(
        bytes32 indexed matchId,
        bytes32 indexed transcriptHash,
        uint256 signerCount,
        uint256 slashedBonds,
        bool viaGrace
    );
    event UnilateralClaimed(bytes32 indexed matchId, address indexed claimant, uint64 challengeUntil);
    event Challenged(bytes32 indexed matchId, uint256 challengerMask, uint256 stake);
    event DisputeResolved(bytes32 indexed matchId, bool factionAWon);
    event LobbyCancelled(bytes32 indexed matchId);

    // ── EIP-712 ─────────────────────────────────────────────────────────

    bytes32 public constant MULTIPLAYER_LADDER_TYPEHASH = keccak256(
        "MultiplayerLadder(bytes32 matchId,bytes32 gameId,address[] rankedPlacements,bytes32 transcriptHash,uint256 sessionNonce)"
    );

    bytes32 public constant DISPUTE_VERDICT_TYPEHASH =
        keccak256("DisputeVerdict(bytes32 matchId,address[] rankedPlacements,bytes32 transcriptHash,bool factionAWon)");

    // ── constructor & admin ─────────────────────────────────────────────

    constructor(uint16 rakeBps_, uint16 studioSplitBps_, address protocolRecipient, address relayerRecipient)
        Ownable(msg.sender)
        EIP712("AMPMultiplayer", "1")
    {
        if (protocolRecipient == address(0) || relayerRecipient == address(0)) revert ZeroAddress();
        if (rakeBps_ > MAX_PROTOCOL_RAKE_BPS) revert FeeTooHigh();
        if (studioSplitBps_ > 10000) revert FeeTooHigh();
        protocolRakeBps = rakeBps_;
        studioSplitBps = studioSplitBps_;
        protocolFeeRecipient = protocolRecipient;
        relayerFeeRecipient = relayerRecipient;
    }

    function updateFees(uint16 rakeBps, uint16 splitBps) external onlyOwner {
        if (rakeBps > MAX_PROTOCOL_RAKE_BPS) revert FeeTooHigh();
        if (splitBps > 10000) revert FeeTooHigh();
        protocolRakeBps = rakeBps;
        studioSplitBps = splitBps;
    }

    function updateRecipients(address protocol, address relayer) external onlyOwner {
        if (protocol == address(0) || relayer == address(0)) revert ZeroAddress();
        protocolFeeRecipient = protocol;
        relayerFeeRecipient = relayer;
    }

    function setStudioRecipient(uint256 gameId, address studio) external onlyOwner {
        if (studio == address(0)) revert ZeroAddress();
        studioRecipientOf[gameId] = studio;
    }

    /// tierBps sums to exactly 10000 and is applied to the NET pool
    /// (after rake). Tiers beyond the lobby size simply never pay out.
    /// PROFILES ARE IMMUTABLE: re-registering an active id reverts, because
    /// settled matches recompute tiers at claim time (prove-your-payout).
    function registerPayoutProfile(uint256 gameId, uint64 lobbySize, uint16 profileId, uint16[] calldata tierBps)
        external
        onlyOwner
    {
        if (payoutProfileActive[gameId][lobbySize][profileId]) revert ProfileNotFound();
        if (tierBps.length == 0 || tierBps.length > 64) revert BadProfile();
        uint256 sum;
        for (uint256 i; i < tierBps.length; ++i) {
            sum += tierBps[i];
        }
        if (sum != 10000) revert BadProfile();
        payoutProfiles[gameId][lobbySize][profileId] = tierBps;
        payoutProfileActive[gameId][lobbySize][profileId] = true;
    }

    function setDisputeVerifier(address verifier) external onlyOwner {
        if (verifier == address(0)) revert ZeroAddress();
        disputeVerifier = verifier;
    }

    // ── lobby lifecycle ─────────────────────────────────────────────────

    function createLobby(
        bytes32 matchId,
        uint256 gameId,
        uint64 lobbySize,
        uint256 stakePerPlayer,
        uint256 bondPerPlayer,
        uint16 payoutProfileId,
        uint64 escrowFillSeconds
    ) external whenNotPaused {
        if (matches[matchId].state != State.Empty) revert MatchExists();
        if (lobbySize < 2) revert LobbyTooSmall();
        if (lobbySize > 64) revert LobbyTooLarge();
        if (!payoutProfileActive[gameId][lobbySize][payoutProfileId]) revert ProfileNotFound();
        Match storage m = matches[matchId];
        m.gameId = gameId;
        m.lobbySize = lobbySize;
        m.payoutProfileId = payoutProfileId;
        m.stakePerPlayer = stakePerPlayer;
        m.bondPerPlayer = bondPerPlayer;
        m.joinedUntil = uint64(block.timestamp) + escrowFillSeconds;
        m.state = State.Open;
        emit LobbyCreated(matchId, gameId, lobbySize, stakePerPlayer, bondPerPlayer, payoutProfileId);
    }

    function joinLobby(bytes32 matchId) external payable nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        if (m.state == State.Empty) revert MatchNotFound();
        if (m.state != State.Open) revert NotOpen();
        if (block.timestamp > m.joinedUntil) revert NotExpiredYet();
        if (msg.value != m.stakePerPlayer + m.bondPerPlayer) revert WrongDeposit();
        if (indexOf[matchId][msg.sender] != 0) revert AlreadyJoined();

        uint256 free = type(uint256).max;
        for (uint64 i; i < m.lobbySize; ++i) {
            if ((m.joinedMask & (uint256(1) << i)) == 0) {
                free = i;
                break;
            }
        }
        if (free == type(uint256).max) revert LobbyFull();

        m.participants[free] = msg.sender;
        indexOf[matchId][msg.sender] = uint8(free + 1);
        m.joinedMask |= uint256(1) << free;
        emit PlayerJoined(matchId, msg.sender, uint64(free));

        if (popcount(m.joinedMask) == m.lobbySize) {
            m.state = State.Ready;
            m.readyAt = uint64(block.timestamp);
            m.quorumUntil = uint64(block.timestamp) + 120;
            m.graceUntil = m.quorumUntil + 300;
            emit LobbyReady(matchId, m.quorumUntil, m.graceUntil);
        }
    }

    /// Unfilled past the escrow deadline: everyone refunds, lobby closes.
    function cancelLobby(bytes32 matchId) external nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.Open);
        if (block.timestamp <= m.joinedUntil) revert NotExpiredYet();
        m.state = State.Cancelled;
        _refundAll(m);
        emit LobbyCancelled(matchId);
    }

    // ── quorum settlement ───────────────────────────────────────────────

    function quorumOf(uint64 n) public pure returns (uint64 k) {
        // N <= 3 -> unanimity; else floor(2N/3)+1 (2f+1 of 3f+1).
        if (n <= 3) return n;
        return (2 * n) / 3 + 1;
    }

    function ladderDigest(
        bytes32 matchId,
        uint256 gameId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce
    ) public view returns (bytes32) {
        bytes32 structHash = keccak256(
            abi.encode(
                MULTIPLAYER_LADDER_TYPEHASH,
                matchId,
                bytes32(gameId),
                keccak256(abi.encodePacked(rankedPlacements)),
                transcriptHash,
                sessionNonce
            )
        );
        return _hashTypedDataV4(structHash);
    }

    /**
     * Settle with a K-of-N concordant quorum during the quorum window.
     *
     * @param signerBitmask bit i set = participants[i] signed this ladder
     * @param packedSignatures 65-byte signatures ordered by ascending bit
     */
    function settleMultiplayer(
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce,
        uint256 signerBitmask,
        bytes calldata packedSignatures
    ) external nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.Ready);
        if (block.timestamp > m.quorumUntil) revert QuorumWindowStillOpen();

        _verifyLadderQuorum(
            m,
            matchId,
            rankedPlacements,
            transcriptHash,
            sessionNonce,
            signerBitmask,
            packedSignatures,
            quorumOf(m.lobbySize)
        );
        _settleAndRecord(m, matchId, rankedPlacements, transcriptHash, signerBitmask);
    }

    // ── grace path ──────────────────────────────────────────────────────

    /**
     * Quorum window lapsed: the player the ladder ranks FIRST claims
     * unilaterally. Opens a 300s challenge window; unchallenged, the claim
     * finalizes. The claimant's signature must recover to rank 1.
     */
    function unilateralClaim(
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce,
        bytes calldata claimantSignature
    ) external nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.Ready);
        if (block.timestamp <= m.quorumUntil) revert QuorumWindowStillOpen();

        address claimant = _recoverSingle(m, matchId, rankedPlacements, transcriptHash, sessionNonce, claimantSignature);
        if (rankedPlacements.length == 0 || rankedPlacements[0] != claimant) revert NotRankOne();
        uint8 idx1 = indexOf[matchId][claimant];
        if (idx1 == 0) revert NotParticipant();

        m.state = State.GracePending;
        m.ladderAHash = _ladderHash(rankedPlacements, transcriptHash);
        m.claimantMask = uint256(1) << (idx1 - 1);
        m.challengeUntil = uint64(block.timestamp) + 300;
        emit UnilateralClaimed(matchId, claimant, m.challengeUntil);
    }

    /// Finalize an unchallenged grace claim: the ladder is resubmitted and
    /// must hash-match the stored claim; non-signers forfeit bonds.
    function finalizeGraceWith(bytes32 matchId, address[] calldata rankedPlacements, bytes32 transcriptHash)
        external
        nonReentrant
        whenNotPaused
    {
        Match storage m = matches[matchId];
        _requireState(m, State.GracePending);
        if (block.timestamp <= m.challengeUntil) revert NotExpiredYet();
        if (_ladderHash(rankedPlacements, transcriptHash) != m.ladderAHash) revert LadderMismatch();
        _settleAndRecord(m, matchId, rankedPlacements, transcriptHash, m.claimantMask);
    }

    /**
     * Challenge a grace claim with a >=2-signer concordant conflicting
     * ladder. Requires the challenge stake (min(one deposit, 10% of pool));
     * losers forfeit stake + bond + their share at resolution.
     */
    function challengeClaim(
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce,
        uint256 signerBitmask,
        bytes calldata packedSignatures
    ) external payable nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.GracePending);
        if (block.timestamp > m.challengeUntil) revert NotExpiredYet();
        if (msg.value < _challengeStake(m)) revert ChallengeStakeTooSmall();
        if (_ladderHash(rankedPlacements, transcriptHash) == m.ladderAHash) revert LadderMismatch();

        _verifyLadderQuorum(
            m, matchId, rankedPlacements, transcriptHash, sessionNonce, signerBitmask, packedSignatures, 2
        );

        m.state = State.Disputed;
        m.ladderBHash = _ladderHash(rankedPlacements, transcriptHash);
        m.factionBMask = signerBitmask;
        m.challengeStakeEscrowed = msg.value;
        emit Challenged(matchId, signerBitmask, msg.value);
    }

    /**
     * Bonded-verifier verdict on a dispute. factionAWon selects the valid
     * ladder (A = grace claimant, B = challengers). The invalid faction
     * forfeits stake + bond (+ their challenge contribution); 70% of the
     * slash rewards the VALID FACTION pro-rata, 30% to the treasury.
     * Non-faction bystanders receive full refunds; the valid ladder still
     * pays its tiers to whoever earned them (losers' tiers join the slash).
     */
    function resolveDispute(
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        bool factionAWon,
        bytes calldata verdictSignature
    ) external nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.Disputed);
        if (msg.sender != disputeVerifier) revert BadVerifier();

        _verifyVerdict(matchId, rankedPlacements, transcriptHash, factionAWon, verdictSignature);
        bytes32 winnerHash = _ladderHash(rankedPlacements, transcriptHash);
        if (factionAWon) {
            if (winnerHash != m.ladderAHash) revert LadderMismatch();
        } else {
            if (winnerHash != m.ladderBHash) revert LadderMismatch();
        }

        // The invalid REPORTING GROUP forfeits: faction B (challengers) when
        // the claimant was right, the claimant when the challengers were
        // right. Bystanders (signed neither ladder) only ever refund.
        uint256 loserMask = factionAWon ? m.factionBMask : m.claimantMask;
        uint256 winnerFactionMask = factionAWon ? m.claimantMask : m.factionBMask;

        _payoutDispute(m, matchId, rankedPlacements, transcriptHash, loserMask, winnerFactionMask);
        emit DisputeResolved(matchId, factionAWon);
    }

    /// Quorum + grace fully lapsed with no claim at all: full refunds.
    function expireRefund(bytes32 matchId) external nonReentrant whenNotPaused {
        Match storage m = matches[matchId];
        _requireState(m, State.Ready);
        if (block.timestamp <= m.graceUntil) revert NotExpiredYet();
        m.state = State.Cancelled;
        _refundAll(m);
        emit LobbyCancelled(matchId);
    }

    // ── withdrawals (pull-only) ─────────────────────────────────────────

    function withdraw() external nonReentrant {
        uint256 amount = claimable[msg.sender];
        if (amount == 0) revert NothingClaimable();
        claimable[msg.sender] = 0;
        (bool ok,) = msg.sender.call{value: amount}("");
        if (!ok) revert TransferFailed();
    }

    // ── internal: verification ──────────────────────────────────────────

    function _verifyLadderQuorum(
        Match storage m,
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce,
        uint256 signerBitmask,
        bytes calldata packedSignatures,
        uint256 minSigners
    ) private {
        uint64 n = m.lobbySize;
        uint256 validMask = _validMask(n);
        if ((signerBitmask & ~validMask) != 0) revert NotPermutation();
        if (packedSignatures.length % 65 != 0) revert SigCountMismatch();
        uint256 sigCount = packedSignatures.length / 65;
        if (popcount(signerBitmask) != sigCount) revert SigCountMismatch();
        if (sigCount < minSigners) revert QuorumNotReached(sigCount, minSigners);

        bytes32 digest = ladderDigest(matchId, m.gameId, rankedPlacements, transcriptHash, sessionNonce);
        uint256 sigCursor;
        for (uint64 i; i < n; ++i) {
            uint256 bit = uint256(1) << i;
            if ((signerBitmask & bit) != 0) {
                (bytes32 r, bytes32 s, uint8 v) = _sliceSig(packedSignatures, sigCursor * 65);
                address recovered = ECDSA.recover(digest, v, r, s);
                if (m.participants[i] != recovered) revert BadSigner(recovered);
                sigCursor++;
            }
        }

        // Placements must be an exact permutation of the participants.
        uint256 placementMask;
        for (uint256 i; i < rankedPlacements.length; ++i) {
            uint8 idx1 = indexOf[matchId][rankedPlacements[i]];
            if (idx1 == 0) revert NotPermutation();
            uint256 b = uint256(1) << (idx1 - 1);
            if ((placementMask & b) != 0) revert NotPermutation();
            placementMask |= b;
        }
        if (placementMask != validMask) revert NotPermutation();
    }

    function _recoverSingle(
        Match storage m,
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 sessionNonce,
        bytes calldata signature
    ) private view returns (address) {
        if (signature.length != 65) revert SigCountMismatch();
        bytes32 digest = ladderDigest(matchId, m.gameId, rankedPlacements, transcriptHash, sessionNonce);
        (bytes32 r, bytes32 s, uint8 v) = _sliceSig(signature, 0);
        return ECDSA.recover(digest, v, r, s);
    }

    function _verifyVerdict(
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        bool factionAWon,
        bytes calldata signature
    ) private view {
        if (signature.length != 65) revert SigCountMismatch();
        bytes32 structHash = keccak256(
            abi.encode(
                DISPUTE_VERDICT_TYPEHASH,
                matchId,
                keccak256(abi.encodePacked(rankedPlacements)),
                transcriptHash,
                factionAWon
            )
        );
        bytes32 digest = _hashTypedDataV4(structHash);
        (bytes32 r, bytes32 s, uint8 v) = _sliceSig(signature, 0);
        if (ECDSA.recover(digest, v, r, s) != disputeVerifier) revert BadVerifier();
    }

    // ── internal: payout engines ────────────────────────────────────────

    /**
     * Settlement = RECORD, not pay. Fees (studio / treasury / relayer) are
     * the only direct credits; every player claims later by resubmitting
     * the ladder (prove-your-payout). This keeps the settlement
     * transaction's storage writes constant regardless of N — the §6 gas
     * gate — with claimers paying their own claim gas.
     */
    function _settleAndRecord(
        Match storage m,
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 signerMask
    ) private {
        uint64 n = m.lobbySize;
        m.state = State.Settled;
        m.settledLadderHash = _ladderHash(rankedPlacements, transcriptHash);
        m.settledSignerMask = uint64(signerMask);
        m.disputeLoserMask = type(uint64).max; // sentinel: quorum/grace mode

        uint256 grossPool = m.stakePerPlayer * n;
        uint256 grossRake = (grossPool * protocolRakeBps) / 10000;
        m.settledNetPool = uint96(grossPool - grossRake);
        m.settledGrossRake = uint96(grossRake);
        m.settledStudioFee = uint96((grossRake * studioSplitBps) / 10000);

        // Bonds: signers reclaim at claim; non-signers' bonds slash 50/50 —
        // relayer half and rank-1 half both derive at claim time from the
        // recorded signer mask.
        uint256 nonSigners = n - popcount(signerMask & _validMask(n));
        uint256 slashedBonds = nonSigners * m.bondPerPlayer;
        m.winnerSlashHalf = uint96(slashedBonds - slashedBonds / 2);

        emit Settled(matchId, transcriptHash, popcount(signerMask), slashedBonds, false);
    }

    /// Lazy fee claim (studio / treasury / relayer): every fee amount
    /// derives from the settlement snapshot + immutable payout profile, so
    /// fee recipients resubmit the ladder and take exactly their share.
    /// Roles resolve from live config addresses; amounts from the snapshot.
    function claimFees(bytes32 matchId, address[] calldata rankedPlacements, bytes32 transcriptHash)
        external
        nonReentrant
        whenNotPaused
    {
        Match storage m = matches[matchId];
        _requireState(m, State.Settled);
        if (_ladderHash(rankedPlacements, transcriptHash) != m.settledLadderHash) revert LadderMismatch();

        bool isQuorumMode = m.disputeLoserMask == type(uint64).max;
        address studio = studioRecipientOf[m.gameId];

        if (msg.sender == studio) {
            if ((m.feesClaimedBits & 1) != 0) revert AlreadyJoined();
            if (studio != address(0) && isQuorumMode && m.settledStudioFee != 0) {
                m.feesClaimedBits |= 1;
                claimable[studio] += m.settledStudioFee;
            } else {
                revert NotParticipant(); // nothing to claim in this mode
            }
        } else if (msg.sender == protocolFeeRecipient) {
            if ((m.feesClaimedBits & 2) != 0) revert AlreadyJoined();
            m.feesClaimedBits |= 2;
            uint256 credit;
            if (isQuorumMode) {
                // rake minus studio + tier dust (tiers beyond placements)
                uint256 tierTotal = _tierTotal(m, rankedPlacements.length);
                credit = (m.settledGrossRake - m.settledStudioFee) + (m.settledNetPool - tierTotal);
            }
            // dispute mode: 30% + share dust already credited at resolve
            claimable[protocolFeeRecipient] += credit;
        } else if (msg.sender == relayerFeeRecipient) {
            if ((m.feesClaimedBits & 4) != 0) revert AlreadyJoined();
            m.feesClaimedBits |= 4;
            if (isQuorumMode) {
                uint256 nonSigners = m.lobbySize - popcount(uint256(m.settledSignerMask) & _validMask(m.lobbySize));
                uint256 slashed = nonSigners * m.bondPerPlayer;
                claimable[relayerFeeRecipient] += slashed / 2;
            }
            // dispute mode: no relayer share on challenge slashes
        } else {
            revert NotParticipant();
        }
    }

    /// Σ tier payouts actually payable given the resubmitted ladder length.
    function _tierTotal(Match storage m, uint256 placements) private view returns (uint256 total) {
        uint16[] storage tiers = payoutProfiles[m.gameId][m.lobbySize][m.payoutProfileId];
        uint256 tierCount = tiers.length < placements ? tiers.length : placements;
        for (uint256 t; t < tierCount; ++t) {
            total += (uint256(m.settledNetPool) * tiers[t]) / 10000;
        }
    }

    /**
     * Prove-your-payout claim: resubmit the settled ladder; the contract
     * verifies the hash and pays the caller's tier + bond + rank-1 slash
     * half (quorum/grace) or bond + tier + 70% share (dispute). One credit
     * write per claim; the claimed-mask bit makes it idempotent.
     */
    function claimPayout(bytes32 matchId, address[] calldata rankedPlacements, bytes32 transcriptHash)
        external
        nonReentrant
        whenNotPaused
    {
        Match storage m = matches[matchId];
        _requireState(m, State.Settled);
        if (_ladderHash(rankedPlacements, transcriptHash) != m.settledLadderHash) revert LadderMismatch();
        uint8 idx1 = indexOf[matchId][msg.sender];
        if (idx1 == 0) revert NotParticipant();
        uint256 bit = uint256(1) << (idx1 - 1);
        if ((uint256(m.claimedMask) & bit) != 0) revert AlreadyJoined(); // claimed already
        m.claimedMask = uint64(uint256(m.claimedMask) | bit);

        // Rank = position in the resubmitted ladder.
        uint256 rank = type(uint256).max;
        for (uint256 i; i < rankedPlacements.length; ++i) {
            if (rankedPlacements[i] == msg.sender) {
                rank = i + 1;
                break;
            }
        }
        if (rank == type(uint256).max) revert NotParticipant();

        if (m.disputeLoserMask == type(uint64).max) {
            // ── quorum / grace mode ──
            uint256 credit;
            uint16[] storage tiers = payoutProfiles[m.gameId][m.lobbySize][m.payoutProfileId];
            if (rank <= tiers.length) {
                credit += (uint256(m.settledNetPool) * tiers[rank - 1]) / 10000;
            }
            if ((uint256(m.settledSignerMask) & bit) != 0) credit += m.bondPerPlayer;
            if (rank == 1) credit += m.winnerSlashHalf;
            claimable[msg.sender] += credit;
        } else {
            // ── dispute mode ──
            if ((uint256(m.disputeLoserMask) & bit) != 0) revert NotParticipant(); // losers claim nothing
            uint256 credit = m.bondPerPlayer;
            // Tiers over the GROSS pool: settledNetPool is rake-less here.
            uint16[] storage tiers = payoutProfiles[m.gameId][m.lobbySize][m.payoutProfileId];
            if (rank <= tiers.length) {
                credit += (uint256(m.settledNetPool) * tiers[rank - 1]) / 10000;
            }
            // Winner faction bits were recorded into settledSignerMask.
            if ((uint256(m.settledSignerMask) & bit) != 0 && m.disputeRewardShare != 0) {
                credit += m.disputeRewardShare;
            }
            claimable[msg.sender] += credit;
        }
    }

    /// Dispute resolution records the winning ladder + faction economics;
    /// treasury takes the 30% immediately, winners claim the 70% shares.
    /// Losers can never claim (claimPayout rejects them), so loser-held
    /// tier value must flow to the winners through the share.
    function _payoutDispute(
        Match storage m,
        bytes32 matchId,
        address[] calldata rankedPlacements,
        bytes32 transcriptHash,
        uint256 loserMask,
        uint256 winnerFactionMask
    ) private {
        uint64 n = m.lobbySize;
        m.state = State.Settled;
        m.settledLadderHash = _ladderHash(rankedPlacements, transcriptHash);
        m.disputeLoserMask = uint64(loserMask);
        m.settledSignerMask = uint64(winnerFactionMask); // dispute mode: winner faction bits
        m.settledNetPool = uint96(m.stakePerPlayer * n); // tiers over gross: no rake in disputes

        uint16[] storage tiers = payoutProfiles[m.gameId][n][m.payoutProfileId];
        uint256 tierCount = tiers.length < rankedPlacements.length ? tiers.length : rankedPlacements.length;

        uint256 loserTierValue;
        for (uint256 t; t < tierCount; ++t) {
            uint8 idx1 = indexOf[matchId][rankedPlacements[t]];
            if (idx1 != 0 && (loserMask & (uint256(1) << (idx1 - 1))) != 0) {
                loserTierValue += (m.settledNetPool * tiers[t]) / 10000;
            }
        }
        uint256 losers = popcount(loserMask);
        uint256 slashPool = m.bondPerPlayer * losers + loserTierValue + m.challengeStakeEscrowed;

        uint256 reward = (slashPool * 7000) / 10000;
        uint256 winners = popcount(winnerFactionMask);
        m.disputeRewardShare = uint96(winners > 0 ? reward / winners : 0);
        // Treasury 30% + share rounding dust — conservation-exact. In
        // dispute mode the treasury credit happens at resolve (small, rare
        // path; not the gas-gated one).
        claimable[protocolFeeRecipient] += (slashPool - reward)
            + (winners > 0 ? reward - m.disputeRewardShare * winners : reward);
    }

    function _refundAll(Match storage m) private {
        uint256 refund = m.stakePerPlayer + m.bondPerPlayer;
        for (uint64 i; i < m.lobbySize; ++i) {
            if ((m.joinedMask & (uint256(1) << i)) != 0) {
                claimable[m.participants[i]] += refund;
            }
        }
    }

    // ── internal: small helpers ─────────────────────────────────────────

    function _requireState(Match storage m, State want) private view {
        if (m.state != want) revert WrongState(uint8(m.state), uint8(want));
    }

    function _ladderHash(address[] calldata rankedPlacements, bytes32 transcriptHash) private pure returns (bytes32) {
        return keccak256(abi.encode(rankedPlacements, transcriptHash));
    }

    function _sliceSig(bytes calldata sigs, uint256 offset) private pure returns (bytes32 r, bytes32 s, uint8 v) {
        r = bytes32(sigs[offset:offset + 32]);
        s = bytes32(sigs[offset + 32:offset + 64]);
        v = uint8(sigs[offset + 64]);
    }

    function _validMask(uint64 n) private pure returns (uint256) {
        // 1 << 64 is well-defined in uint256; no special case for n == 64
        // (the max-shortcut broke the placement-permutation check there).
        return (uint256(1) << n) - 1;
    }

    // ── views ───────────────────────────────────────────────────────────

    /// EIP-712 domain separator (for off-chain tooling / tests).
    function domainSeparator() external view returns (bytes32) {
        return _domainSeparatorV4();
    }

    function getMatch(bytes32 matchId) external view returns (Match memory) {
        return matches[matchId];
    }

    function participantIndex(bytes32 matchId, address player) external view returns (uint8) {
        return indexOf[matchId][player];
    }

    function popcount(uint256 x) public pure returns (uint256 c) {
        while (x != 0) {
            x &= x - 1;
            c++;
        }
    }

    function _challengeStake(Match storage m) private view returns (uint256) {
        uint256 perPlayer = m.stakePerPlayer + m.bondPerPlayer;
        uint256 pool = perPlayer * m.lobbySize;
        return perPlayer < pool / 10 ? perPlayer : pool / 10;
    }
}
