// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "./AMPTypes.sol";
import "openzeppelin-contracts/contracts/utils/cryptography/ECDSA.sol";
import "openzeppelin-contracts/contracts/utils/cryptography/EIP712.sol";
import "openzeppelin-contracts/contracts/access/Ownable2Step.sol";
import "openzeppelin-contracts/contracts/utils/Pausable.sol";
import "openzeppelin-contracts/contracts/utils/ReentrancyGuard.sol";

interface IAMPRegistry {
    function games(uint256 id)
        external
        view
        returns (
            address admin,
            AMPTypes.SettlementMode mode,
            uint256 minStake,
            address stakeToken,
            address arbiter,
            uint256 matchTimeout,
            uint16 studioFeeBps,
            address studioFeeRecipient
        );

    function getGameVerifiers(uint256 id) external view returns (address[] memory);

    function isVerifier(uint256 gameId, address verifier) external view returns (bool);

    // Returns (gameId, playerA, state, playerB, createdAt, stakeAmount, stakeAmountB).
    function matches(uint256 id)
        external
        view
        returns (uint256, address, AMPTypes.MatchState, address, uint64, uint256, uint256);

    function settleMatch(
        uint256 matchId,
        AMPTypes.MatchState newState,
        address[2] calldata feeRecipients,
        uint256[2] calldata feeAmounts,
        address[] calldata recipients,
        uint256[] calldata amounts
    ) external;
}

contract AMPSettlement is Ownable2Step, Pausable, EIP712, ReentrancyGuard {
    error FeeExceedsMax();
    error InvalidRecipient();
    error MatchIdMismatch();
    error WrongMode();
    error MatchNotSettlable();
    error NoOpponent();
    error InvalidVerifierSignature();
    error NotArbiter();
    error NotDisputed();
    error NotAPlayer();
    error TotalFeeTooHigh();
    error InvalidOutcome();

    address public immutable registry;
    uint16 public protocolFeeBps;
    address public protocolFeeRecipient;

    mapping(uint256 => AMPTypes.Settlement) public settlements;
    mapping(uint256 => mapping(address => AMPTypes.RealTimeHashResult)) public rtResults;

    event MatchSettled(uint256 indexed matchId, AMPTypes.OutcomeCode outcome, uint256 payout);
    event MatchDisputed(uint256 indexed matchId);
    event ProtocolFeeUpdated(uint16 feeBps);
    event ProtocolFeeRecipientUpdated(address indexed recipient);
    event MatchDisputeResolved(uint256 indexed matchId, AMPTypes.OutcomeCode enforcedOutcome);

    bytes32 public constant ASYNC_RESULT_TYPEHASH =
        keccak256("AsyncResult(uint256 matchId,uint8 outcome,bytes32 transcriptHash)");

    constructor(address _registry) Ownable(msg.sender) EIP712("AMPSettlement", "1") {
        registry = _registry;
        protocolFeeBps = 100;
        protocolFeeRecipient = msg.sender;
    }

    uint16 public constant MAX_PROTOCOL_FEE_BPS = 500;
    uint16 public constant MAX_TOTAL_FEE_BPS = 800; // studio + protocol combined (hard policy)

    function updateProtocolFeeBps(uint16 feeBps) external onlyOwner {
        if (feeBps > MAX_PROTOCOL_FEE_BPS) revert FeeExceedsMax();
        protocolFeeBps = feeBps;
        emit ProtocolFeeUpdated(feeBps);
    }

    function updateProtocolFeeRecipient(address recipient) external onlyOwner {
        if (recipient == address(0)) revert InvalidRecipient();
        protocolFeeRecipient = recipient;
        emit ProtocolFeeRecipientUpdated(recipient);
    }

    function submitAsyncResult(uint256 matchId, AMPTypes.AsyncResult calldata result)
        external
        whenNotPaused
        nonReentrant
    {
        if (result.matchId != matchId) revert MatchIdMismatch();

        (
            uint256 gameId,
            address playerA,
            AMPTypes.MatchState state,
            address playerB,
            uint64 createdAt,
            uint256 stakeAmount,
            uint256 stakeAmountB
        ) = IAMPRegistry(registry).matches(matchId);

        (, AMPTypes.SettlementMode mode,,,,,,) = IAMPRegistry(registry).games(gameId);

        if (mode != AMPTypes.SettlementMode.ASYNC_VERIFIER) revert WrongMode();
        if (state != AMPTypes.MatchState.READY) revert MatchNotSettlable();
        if (playerB == address(0)) revert NoOpponent();

        bytes32 structHash =
            keccak256(abi.encode(ASYNC_RESULT_TYPEHASH, result.matchId, result.outcome, result.transcriptHash));
        bytes32 digest = _hashTypedDataV4(structHash);
        address signer = ECDSA.recover(digest, result.signature);

        if (!IAMPRegistry(registry).isVerifier(gameId, signer)) revert InvalidVerifierSignature();

        settlements[matchId] = AMPTypes.Settlement({
            matchId: matchId, outcome: result.outcome, transcriptHash: result.transcriptHash, settledAt: block.timestamp
        });

        _payout(matchId, gameId, playerA, playerB, stakeAmount, stakeAmountB, result.outcome);

        emit MatchSettled(matchId, result.outcome, stakeAmount + stakeAmountB);
    }

    function submitRealTimeHashResult(uint256 matchId, AMPTypes.RealTimeHashResult calldata result)
        external
        whenNotPaused
        nonReentrant
    {
        if (result.matchId != matchId) revert MatchIdMismatch();
        (
            uint256 gameId,
            address playerA,
            AMPTypes.MatchState state,
            address playerB,
            uint64 createdAt,
            uint256 stakeAmount,
            uint256 stakeAmountB
        ) = IAMPRegistry(registry).matches(matchId);

        (, AMPTypes.SettlementMode mode,,,,,,) = IAMPRegistry(registry).games(gameId);

        if (mode != AMPTypes.SettlementMode.RT_HASH_AGREE) revert WrongMode();
        if (state != AMPTypes.MatchState.READY) revert MatchNotSettlable();
        if (msg.sender != playerA && msg.sender != playerB) revert NotAPlayer();

        rtResults[matchId][msg.sender] = result;

        address otherPlayer = msg.sender == playerA ? playerB : playerA;
        AMPTypes.RealTimeHashResult memory otherResult = rtResults[matchId][otherPlayer];

        if (otherResult.outcome != AMPTypes.OutcomeCode.NONE) {
            if (result.outcome == otherResult.outcome && result.transcriptHash == otherResult.transcriptHash) {
                settlements[matchId] = AMPTypes.Settlement({
                    matchId: matchId,
                    outcome: result.outcome,
                    transcriptHash: result.transcriptHash,
                    settledAt: block.timestamp
                });

                _payout(matchId, gameId, playerA, playerB, stakeAmount, stakeAmountB, result.outcome);
                emit MatchSettled(matchId, result.outcome, stakeAmount + stakeAmountB);
            } else {
                address[] memory recipients = new address[](0);
                uint256[] memory amounts = new uint256[](0);
                address[2] memory noFees = [address(0), address(0)];
                uint256[2] memory noAmounts = [uint256(0), uint256(0)];
                IAMPRegistry(registry)
                    .settleMatch(matchId, AMPTypes.MatchState.DISPUTED, noFees, noAmounts, recipients, amounts);
                emit MatchDisputed(matchId);
            }
        }
    }

    function resolveDispute(uint256 matchId, AMPTypes.OutcomeCode enforcedOutcome) external whenNotPaused nonReentrant {
        (
            uint256 gameId,
            address playerA,
            AMPTypes.MatchState state,
            address playerB,
            uint64 createdAt,
            uint256 stakeAmount,
            uint256 stakeAmountB
        ) = IAMPRegistry(registry).matches(matchId);

        (,,,, address arbiter,,,) = IAMPRegistry(registry).games(gameId);

        if (state != AMPTypes.MatchState.DISPUTED) revert NotDisputed();
        if (msg.sender != arbiter) revert NotArbiter();
        if (playerB == address(0)) revert NoOpponent();

        settlements[matchId] = AMPTypes.Settlement({
            matchId: matchId, outcome: enforcedOutcome, transcriptHash: bytes32(0), settledAt: block.timestamp
        });

        _payout(matchId, gameId, playerA, playerB, stakeAmount, stakeAmountB, enforcedOutcome);
        emit MatchDisputeResolved(matchId, enforcedOutcome);
        emit MatchSettled(matchId, enforcedOutcome, stakeAmount + stakeAmountB);
    }

    function pause() external onlyOwner {
        _pause();
    }

    function unpause() external onlyOwner {
        _unpause();
    }

    /// EIP-712 domain separator. Delegates to OpenZeppelin's audited
    /// `_domainSeparatorV4()` (release Phase 1.6 — replaces the hand-rolled
    /// implementation; byte-identical for name="AMPSettlement", version="1").
    function domainSeparator() external view returns (bytes32) {
        return _domainSeparatorV4();
    }

    function _payout(
        uint256 matchId,
        uint256 gameId,
        address playerA,
        address playerB,
        uint256 stakeAmount,
        uint256 stakeAmountB,
        AMPTypes.OutcomeCode outcome
    ) internal {
        // F1 fix: NONE (enum 0) is the "no result" sentinel and is never a
        // valid settlement outcome. Without this guard the if/else chain below
        // would fall through, marking the match SETTLED while crediting both
        // players 0 and silently trapping funds permanently.
        if (outcome == AMPTypes.OutcomeCode.NONE) revert InvalidOutcome();

        uint256 totalPool = stakeAmount + stakeAmountB;
        if (playerB == address(0)) {
            totalPool = stakeAmount;
        }

        // Fee-split router: studio rake + protocol treasury, enforced to a
        // combined cap so a misconfigured game can never confiscate the pool.
        (,,,,,, uint16 studioBps, address studioRecipient) = IAMPRegistry(registry).games(gameId);
        if (uint256(studioBps) + uint256(protocolFeeBps) > MAX_TOTAL_FEE_BPS) revert TotalFeeTooHigh();

        uint256 studioFee;
        if (outcome != AMPTypes.OutcomeCode.CANCELLED && studioBps > 0 && studioRecipient != address(0)) {
            studioFee = (totalPool * studioBps) / 10000;
        }
        uint256 protocolFee;
        if (outcome != AMPTypes.OutcomeCode.CANCELLED) {
            protocolFee = (totalPool * protocolFeeBps) / 10000;
        }
        uint256 payoutAmount = totalPool - studioFee - protocolFee;

        address[] memory recipients = new address[](2);
        recipients[0] = playerA;
        recipients[1] = playerB;
        uint256[] memory amounts = new uint256[](2);

        if (outcome == AMPTypes.OutcomeCode.WIN_A) {
            amounts[0] = payoutAmount;
            amounts[1] = 0;
        } else if (outcome == AMPTypes.OutcomeCode.WIN_B) {
            amounts[0] = 0;
            amounts[1] = payoutAmount;
        } else if (outcome == AMPTypes.OutcomeCode.DRAW) {
            amounts[0] = payoutAmount / 2;
            amounts[1] = payoutAmount - amounts[0];
        } else if (outcome == AMPTypes.OutcomeCode.CANCELLED) {
            protocolFee = 0;
            studioFee = 0;
            // Fee-on-transfer safe: refund each player exactly what they staked.
            amounts[0] = stakeAmount;
            amounts[1] = playerB == address(0) ? 0 : stakeAmountB;
        } else {
            // Defense in depth: any future enum value that isn't handled reverts
            // rather than silently trapping funds.
            revert InvalidOutcome();
        }

        address[2] memory feeRecipients = [studioRecipient, protocolFeeRecipient];
        uint256[2] memory feeAmounts = [studioFee, protocolFee];
        IAMPRegistry(registry)
            .settleMatch(matchId, AMPTypes.MatchState.SETTLED, feeRecipients, feeAmounts, recipients, amounts);
    }
}
