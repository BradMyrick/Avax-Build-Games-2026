// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "forge-std/Test.sol";
import "../src/AMPMultiplayer.sol";

/// Reverts on any incoming value — proves pull-payments isolate hostile
/// receivers (the §6 DoS gate).
contract RevertingReceiver {
    receive() external payable {
        revert("no thanks");
    }
}

contract AMPMultiplayerTest is Test {
    AMPMultiplayer public mp;
    address public owner = address(0xBEEF);
    address public treasury = address(0x1111);
    address public relayer = address(0x2222);
    address public studio = address(0x3333);

    uint256 constant N = 8;
    uint256 constant K = 6; // floor(2*8/3)+1
    uint256 constant STAKE = 0.1 ether;
    uint256 constant BOND = 0.005 ether;

    uint256[N] playerKeys;
    address[N] players;

    bytes32 constant MATCH_ID = bytes32(uint256(0xA11CE));
    uint256 constant GAME_ID = 7;
    uint256 constant NONCE = 42;
    bytes32 constant TRANSCRIPT = bytes32(uint256(0x7));

    function setUp() public {
        vm.startPrank(owner);
        mp = new AMPMultiplayer(100, 2000, treasury, relayer); // 1% rake, 20% of rake -> studio
        mp.setStudioRecipient(GAME_ID, studio);
        uint16[] memory tiers = new uint16[](3);
        tiers[0] = 6000;
        tiers[1] = 3000;
        tiers[2] = 1000;
        mp.registerPayoutProfile(GAME_ID, uint64(N), 1, tiers);
        vm.stopPrank();

        for (uint256 i; i < N; ++i) {
            playerKeys[i] = 0x1000 + i;
            players[i] = vm.addr(playerKeys[i]);
            vm.deal(players[i], 10 ether);
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────

    function _createAndFill() internal returns (AMPMultiplayer.Match memory m) {
        vm.prank(relayer);
        mp.createLobby(MATCH_ID, GAME_ID, uint64(N), STAKE, BOND, 1, 10 minutes);
        for (uint256 i; i < N; ++i) {
            vm.prank(players[i]);
            mp.joinLobby{value: STAKE + BOND}(MATCH_ID);
        }
        return mp.getMatch(MATCH_ID);
    }

    function _defaultLadder() internal pure returns (address[] memory) {
        address[] memory ranked = new address[](N);
        for (uint256 i; i < N; ++i) {
            ranked[i] = address(uint160(0x1000 + i)); // placeholder; overwritten by caller
        }
        return ranked;
    }

    function _ladder() internal view returns (address[] memory) {
        address[] memory ranked = new address[](N);
        for (uint256 i; i < N; ++i) {
            ranked[i] = players[i]; // players[i] at rank i+1
        }
        return ranked;
    }

    function _ladderReversed() internal view returns (address[] memory) {
        address[] memory ranked = new address[](N);
        for (uint256 i; i < N; ++i) {
            ranked[i] = players[N - 1 - i];
        }
        return ranked;
    }

    function _packSigs(uint256 signerMask, bytes32 digest) internal view returns (bytes memory) {
        bytes memory out;
        for (uint256 i; i < N; ++i) {
            if ((signerMask & (1 << i)) != 0) {
                (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[i], digest);
                out = abi.encodePacked(out, r, s, v);
            }
        }
        return out;
    }

    function _digest(address[] memory ranked) internal view returns (bytes32) {
        return mp.ladderDigest(MATCH_ID, GAME_ID, ranked, TRANSCRIPT, NONCE);
    }

    /// Sum of every credited balance (the conservation right-hand side).
    function _totalCredited() internal view returns (uint256) {
        uint256 sum = mp.claimable(treasury) + mp.claimable(relayer) + mp.claimable(studio);
        for (uint256 i; i < N; ++i) {
            sum += mp.claimable(players[i]);
        }
        return sum;
    }

    /// All participants claim their payout (prove-your-payout). Losers in
    /// dispute mode revert — caught and skipped.
    function _claimAll(bytes32 matchId) internal {
        address[] memory ranked = _ladder();
        for (uint256 i; i < N; ++i) {
            vm.prank(players[i]);
            try mp.claimPayout(matchId, ranked, TRANSCRIPT) {} catch {}
        }
    }

    /// Fee recipients claim their shares (studio / treasury / relayer).
    function _claimAllFees() internal {
        address[] memory ranked = _ladder();
        vm.prank(studio);
        try mp.claimFees(MATCH_ID, ranked, TRANSCRIPT) {} catch {}
        vm.prank(treasury);
        try mp.claimFees(MATCH_ID, ranked, TRANSCRIPT) {} catch {}
        vm.prank(relayer);
        try mp.claimFees(MATCH_ID, ranked, TRANSCRIPT) {} catch {}
    }

    /// Everyone claims against the given ladder (try/catch: losers revert).
    function _claimAllRanked(bytes32 matchId, address[] memory ranked) internal {
        for (uint256 i; i < N; ++i) {
            vm.prank(players[i]);
            try mp.claimPayout(matchId, ranked, TRANSCRIPT) {} catch {}
        }
    }

    // ── lifecycle ───────────────────────────────────────────────────────

    function test_LobbyFillsAndTransitionsToReady() public {
        AMPMultiplayer.Match memory m = _createAndFill();
        assertEq(uint8(m.state), uint8(AMPMultiplayer.State.Ready));
        assertEq(mp.quorumOf(uint64(N)), K);
        assertEq(mp.participantIndex(MATCH_ID, players[3]), 4); // index+1
        assertEq(address(mp).balance, (STAKE + BOND) * N);
    }

    function test_JoinRejectsWrongDepositAndDuplicates() public {
        vm.prank(relayer);
        mp.createLobby(MATCH_ID, GAME_ID, uint64(N), STAKE, BOND, 1, 10 minutes);
        vm.prank(players[0]);
        mp.joinLobby{value: STAKE + BOND}(MATCH_ID);
        vm.prank(players[0]);
        vm.expectRevert(AMPMultiplayer.AlreadyJoined.selector);
        mp.joinLobby{value: STAKE + BOND}(MATCH_ID);
        vm.prank(players[1]);
        vm.expectRevert(AMPMultiplayer.WrongDeposit.selector);
        mp.joinLobby{value: STAKE}(MATCH_ID);
    }

    function test_CancelUnfilledRefundsDeposits() public {
        vm.prank(relayer);
        mp.createLobby(MATCH_ID, GAME_ID, uint64(N), STAKE, BOND, 1, 1 hours);
        vm.prank(players[0]);
        mp.joinLobby{value: STAKE + BOND}(MATCH_ID);
        vm.warp(2 hours);
        mp.cancelLobby(MATCH_ID);
        assertEq(mp.claimable(players[0]), STAKE + BOND);
        assertEq(_totalCredited(), STAKE + BOND);
    }

    // ── quorum settlement ───────────────────────────────────────────────

    function test_QuorumSettlement_Pays_Fees_Bonds() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        uint256 mask = (1 << K) - 1; // first K players sign
        bytes memory sigs = _packSigs(mask, _digest(ranked));

        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, mask, sigs);

        // Fees and payouts are all lazy: claim, then assert.
        uint256 grossPool = STAKE * N;
        uint256 grossRake = grossPool * 100 / 10000; // 1%
        uint256 netPool = grossPool - grossRake;
        uint256 studioFee = grossRake * 2000 / 10000;

        _claimAll(MATCH_ID);
        _claimAllFees();

        assertEq(mp.claimable(studio), studioFee, "studio split");
        assertEq(mp.claimable(treasury), grossRake - studioFee, "treasury rake");

        uint256 slashed = BOND * 2;
        assertEq(
            mp.claimable(players[0]),
            netPool * 6000 / 10000 + BOND + (slashed - slashed / 2),
            "rank 1: tier + bond + slash half"
        );
        assertEq(mp.claimable(players[1]), netPool * 3000 / 10000 + BOND, "rank 2 tier + bond");
        assertEq(mp.claimable(players[2]), netPool * 1000 / 10000 + BOND, "rank 3 tier + bond");
        for (uint256 i = 3; i < K; ++i) {
            assertEq(mp.claimable(players[i]), BOND, "signer bond refund");
        }
        assertEq(mp.claimable(players[6]), 0, "non-signer slashed");
        assertEq(mp.claimable(players[7]), 0, "non-signer slashed");
        assertEq(mp.claimable(relayer), slashed / 2, "relayer slash half");

        // CONSERVATION: every wei deposited is credited somewhere.
        assertEq(_totalCredited(), (STAKE + BOND) * N, "conservation");
    }

    function test_QuorumGasBound_At_N8_Under_250k() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        uint256 mask = (1 << K) - 1;
        bytes memory sigs = _packSigs(mask, _digest(ranked));
        uint256 gasBefore = gasleft();
        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, mask, sigs);
        uint256 used = gasBefore - gasleft();
        // The §6 gate: 8-player ladder settlement under 250k.
        assertLt(used, 250_000, "8-player quorum settlement gas");
        emit log_named_uint("settlement gas (N=8, K=6)", used);

        // Claim gas (paid by each claimer) — informational + bounded.
        uint256 claimBefore = gasleft();
        vm.prank(players[0]);
        mp.claimPayout(MATCH_ID, ranked, TRANSCRIPT);
        emit log_named_uint("claim gas (rank 1)", claimBefore - gasleft());
        assertLt(claimBefore - gasleft() + 1, 120_000, "claim stays cheap");
    }

    function test_BelowQuorumRejected() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        uint256 mask = 0x1F; // 5 < K=6
        bytes memory sigs = _packSigs(mask, _digest(ranked));
        vm.expectRevert(abi.encodeWithSelector(AMPMultiplayer.QuorumNotReached.selector, 5, K));
        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, mask, sigs);
    }

    function test_MisattributedSignatureRejected() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        // Full K-sized mask, but the signature for bit 0 comes from
        // player 1's key — recovery must mismatch participants[0].
        bytes32 digest = _digest(ranked);
        (uint8 v0, bytes32 r0, bytes32 s0) = vm.sign(playerKeys[1], digest); // WRONG signer at bit 0
        bytes memory sigs = abi.encodePacked(r0, s0, v0);
        for (uint256 i = 1; i < K; ++i) {
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[i], digest);
            sigs = abi.encodePacked(sigs, r, s, v);
        }
        vm.expectRevert(abi.encodeWithSelector(AMPMultiplayer.BadSigner.selector, players[1]));
        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, (1 << K) - 1, sigs);
    }

    function test_NonPermutationPlacementRejected() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        ranked[7] = ranked[0]; // duplicate rank holder
        uint256 mask = (1 << K) - 1;
        bytes memory sigs = _packSigs(mask, _digest(ranked));
        vm.expectRevert(AMPMultiplayer.NotPermutation.selector);
        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, mask, sigs);
    }

    function test_SettlementIsTerminal() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        uint256 mask = (1 << K) - 1;
        bytes memory sigs = _packSigs(mask, _digest(ranked));
        mp.settleMultiplayer(MATCH_ID, ranked, TRANSCRIPT, NONCE, mask, sigs);

        // A conflicting K-quorum on the same match cannot land afterwards —
        // two K-quorums provably intersect, so the second would implicate a
        // double-signer; the terminal state enforces it regardless.
        address[] memory other = _ladderReversed();
        bytes memory otherSigs = _packSigs((1 << K) - 1, _digest(other));
        vm.expectRevert(
            abi.encodeWithSelector(
                AMPMultiplayer.WrongState.selector,
                uint8(AMPMultiplayer.State.Settled),
                uint8(AMPMultiplayer.State.Ready)
            )
        );
        mp.settleMultiplayer(MATCH_ID, other, TRANSCRIPT, NONCE, (1 << K) - 1, otherSigs);
    }

    // ── grace path ──────────────────────────────────────────────────────

    function test_GraceClaim_Finalizes_Unchallenged() public {
        _createAndFill();
        vm.warp(block.timestamp + 121); // quorum window closed
        address[] memory ranked = _ladder();
        bytes32 digest = _digest(ranked);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[0], digest); // rank 1 signs
        bytes memory sig = abi.encodePacked(r, s, v);

        mp.unilateralClaim(MATCH_ID, ranked, TRANSCRIPT, NONCE, sig);
        vm.warp(block.timestamp + 301); // challenge window closed
        mp.finalizeGraceWith(MATCH_ID, ranked, TRANSCRIPT);
        _claimAll(MATCH_ID);
        _claimAllFees();

        // Claimant is the only signer: 7 bonds slashed, half to relayer.
        uint256 slashed = BOND * 7;
        assertEq(mp.claimable(relayer), slashed / 2);
        assertEq(_totalCredited(), (STAKE + BOND) * N, "conservation via grace");
    }

    function test_GraceClaim_RequiresRankOneSigner() public {
        _createAndFill();
        vm.warp(block.timestamp + 121);
        address[] memory ranked = _ladder();
        bytes32 digest = _digest(ranked);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[5], digest); // NOT rank 1
        vm.expectRevert(AMPMultiplayer.NotRankOne.selector);
        mp.unilateralClaim(MATCH_ID, ranked, TRANSCRIPT, NONCE, abi.encodePacked(r, s, v));
    }

    function test_GraceWindow_ClaimBeforeQuorumLapse_Rejected() public {
        _createAndFill();
        address[] memory ranked = _ladder();
        bytes32 digest = _digest(ranked);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[0], digest);
        vm.expectRevert(AMPMultiplayer.QuorumWindowStillOpen.selector);
        mp.unilateralClaim(MATCH_ID, ranked, TRANSCRIPT, NONCE, abi.encodePacked(r, s, v));
    }

    // ── dispute path ────────────────────────────────────────────────────

    function _verdictSig(uint256 verifierKey, address[] memory ranked, bool factionAWon)
        internal
        view
        returns (bytes memory)
    {
        bytes32 structHash = keccak256(
            abi.encode(
                mp.DISPUTE_VERDICT_TYPEHASH(), MATCH_ID, keccak256(abi.encodePacked(ranked)), TRANSCRIPT, factionAWon
            )
        );
        // Domain separator via the contract's own view (OZ exposes it
        // through EIP712.domainSeparator in v5).
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", mp.domainSeparator(), structHash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(verifierKey, digest);
        return abi.encodePacked(r, s, v);
    }

    function test_Dispute_ChallengersWin_SlashClaimant() public {
        _createAndFill();
        vm.warp(block.timestamp + 121);
        address[] memory rankedA = _ladder();
        bytes32 digestA = _digest(rankedA);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[0], digestA);
        mp.unilateralClaim(MATCH_ID, rankedA, TRANSCRIPT, NONCE, abi.encodePacked(r, s, v));

        // Challengers: 4 signers on the reversed ladder + challenge stake.
        address[] memory rankedB = _ladderReversed();
        uint256 maskB = 0xF0; // players 4..7
        bytes memory sigsB = _packSigs(maskB, _digest(rankedB));
        uint256 stake = (STAKE + BOND) * N / 10;
        vm.deal(address(0x999), 1 ether);
        vm.prank(address(0x999));
        mp.challengeClaim{value: stake}(MATCH_ID, rankedB, TRANSCRIPT, NONCE, maskB, sigsB);

        // Verdict: faction B (challengers) wins.
        uint256 verifierKey = 0xABCD;
        vm.prank(owner);
        mp.setDisputeVerifier(vm.addr(verifierKey));
        bytes memory verdict = _verdictSig(verifierKey, rankedB, false);
        vm.prank(vm.addr(verifierKey));
        mp.resolveDispute(MATCH_ID, rankedB, TRANSCRIPT, false, verdict);

        AMPMultiplayer.Match memory m = mp.getMatch(MATCH_ID);
        assertEq(uint8(m.state), uint8(AMPMultiplayer.State.Settled));

        // The claimant (loser) cannot claim at all.
        vm.prank(players[0]);
        vm.expectRevert(AMPMultiplayer.NotParticipant.selector);
        mp.claimPayout(MATCH_ID, rankedB, TRANSCRIPT);
        _claimAllRanked(MATCH_ID, rankedB);

        // Slash pool = loser bond + challenge stake (no valid-ladder tier
        // lands on a loser — ladder-B ranks 1..3 are all faction B).
        uint256 slashPool = BOND + stake;
        uint256 reward = slashPool * 7000 / 10000;
        assertEq(mp.claimable(treasury), slashPool - reward, "treasury 30% at resolve");

        // Bystanders (1,2,3) reclaim bonds only.
        for (uint256 i = 1; i < 4; ++i) {
            assertEq(mp.claimable(players[i]), BOND, "bystander bond refund");
        }
        // Faction B (players 4..7): bond + tier on the valid (reversed)
        // ladder — ranks: p7=1st(60), p6=2nd(30), p5=3rd(10), p4=4th(none)
        // — plus an equal share of the 70% reward.
        uint256 gross = STAKE * N;
        uint256 share = reward / 4;
        assertEq(mp.claimable(players[4]), BOND + share, "faction B p4: bond + share (no tier)");
        assertEq(mp.claimable(players[5]), BOND + gross * 1000 / 10000 + share, "faction B p5: bond + 3rd tier + share");
        assertEq(mp.claimable(players[6]), BOND + gross * 3000 / 10000 + share, "faction B p6: bond + 2nd tier + share");
        assertEq(mp.claimable(players[7]), BOND + gross * 6000 / 10000 + share, "faction B p7: bond + 1st tier + share");

        assertEq(_totalCredited(), (STAKE + BOND) * N + stake, "conservation incl. challenge stake");
    }

    function test_Dispute_ClaimantWins_SlashChallengers() public {
        _createAndFill();
        vm.warp(block.timestamp + 121);
        address[] memory rankedA = _ladder();
        bytes32 digestA = _digest(rankedA);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[0], digestA);
        mp.unilateralClaim(MATCH_ID, rankedA, TRANSCRIPT, NONCE, abi.encodePacked(r, s, v));

        address[] memory rankedB = _ladderReversed();
        uint256 maskB = 0xF0;
        bytes memory sigsB = _packSigs(maskB, _digest(rankedB));
        uint256 stake = (STAKE + BOND) * N / 10;
        vm.deal(address(0x999), 1 ether);
        vm.prank(address(0x999));
        mp.challengeClaim{value: stake}(MATCH_ID, rankedB, TRANSCRIPT, NONCE, maskB, sigsB);

        uint256 verifierKey = 0xABCD;
        vm.prank(owner);
        mp.setDisputeVerifier(vm.addr(verifierKey));
        bytes memory verdict = _verdictSig(verifierKey, rankedA, true);
        vm.prank(vm.addr(verifierKey));
        mp.resolveDispute(MATCH_ID, rankedA, TRANSCRIPT, true, verdict);
        _claimAll(MATCH_ID); // _ladder() == rankedA

        // Faction B (players 4..7) forfeit everything; the valid ladder
        // (rankedA) pays ranks 1..3 = p0,p1,p2 — none are losers — so the
        // slash pool is loser bonds + challenge stake.
        uint256 gross = STAKE * N;
        uint256 slashPool = BOND * 4 + stake;
        uint256 reward = slashPool * 7000 / 10000;
        assertEq(mp.claimable(players[4]), 0, "challenger slashed");
        assertEq(mp.claimable(players[5]), 0, "challenger slashed");
        assertEq(mp.claimable(players[6]), 0, "challenger slashed");
        assertEq(mp.claimable(players[7]), 0, "challenger slashed");
        // Claimant (faction of one): bond + rank-1 tier + full reward.
        uint256 rank1Tier = gross * 6000 / 10000;
        assertEq(mp.claimable(players[0]), BOND + rank1Tier + reward, "claimant bond + tier + reward");
        assertEq(mp.claimable(players[1]), BOND + gross * 3000 / 10000, "bystander p1: bond + 2nd tier");
        assertEq(mp.claimable(players[2]), BOND + gross * 1000 / 10000, "bystander p2: bond + 3rd tier");
        assertEq(mp.claimable(players[3]), BOND, "bystander p3: bond");
        assertEq(mp.claimable(treasury), slashPool - reward, "treasury 30%");
        assertEq(_totalCredited(), (STAKE + BOND) * N + stake, "conservation incl. challenge stake");
    }

    function test_Dispute_VerdictMustMatchWinnerLadder() public {
        _createAndFill();
        vm.warp(block.timestamp + 121);
        address[] memory rankedA = _ladder();
        bytes32 digestA = _digest(rankedA);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(playerKeys[0], digestA);
        mp.unilateralClaim(MATCH_ID, rankedA, TRANSCRIPT, NONCE, abi.encodePacked(r, s, v));

        address[] memory rankedB = _ladderReversed();
        uint256 maskB = 0xF0;
        bytes memory sigsB = _packSigs(maskB, _digest(rankedB));
        vm.deal(address(0x999), 1 ether);
        vm.prank(address(0x999));
        mp.challengeClaim{value: (STAKE + BOND) * N / 10}(MATCH_ID, rankedB, TRANSCRIPT, NONCE, maskB, sigsB);

        uint256 verifierKey = 0xABCD;
        vm.prank(owner);
        mp.setDisputeVerifier(vm.addr(verifierKey));
        // Verdict says A won but submits ladder B — must reject.
        bytes memory bad = _verdictSig(verifierKey, rankedB, true);
        vm.prank(vm.addr(verifierKey));
        vm.expectRevert(AMPMultiplayer.LadderMismatch.selector);
        mp.resolveDispute(MATCH_ID, rankedB, TRANSCRIPT, true, bad);
    }

    // ── expiry & DoS gates ──────────────────────────────────────────────

    function test_ExpireRefund_AfterTotalSilence() public {
        _createAndFill();
        vm.warp(block.timestamp + 121 + 301); // quorum + grace lapsed
        mp.expireRefund(MATCH_ID);
        for (uint256 i; i < N; ++i) {
            assertEq(mp.claimable(players[i]), STAKE + BOND);
        }
        assertEq(_totalCredited(), (STAKE + BOND) * N, "conservation on refund");
    }

    function test_WithdrawIsolatedFromRevertingReceiver() public {
        _createAndFill();
        vm.warp(block.timestamp + 121 + 301);
        mp.expireRefund(MATCH_ID);

        // A hostile participant credit: fund a RevertingReceiver's slot by
        // building a fresh lobby with it.
        RevertingReceiver hostile = new RevertingReceiver();
        vm.deal(address(hostile), 1 ether);
        vm.startPrank(owner);
        uint16[] memory t2 = new uint16[](1);
        t2[0] = 10000; // N=2: winner takes all
        mp.registerPayoutProfile(GAME_ID, 2, 2, t2);
        vm.stopPrank();
        vm.prank(relayer);
        mp.createLobby(bytes32(uint256(0xBAD)), GAME_ID, 2, STAKE, BOND, 2, 10 minutes);
        vm.prank(address(hostile));
        mp.joinLobby{value: STAKE + BOND}(bytes32(uint256(0xBAD)));
        vm.prank(players[0]);
        mp.joinLobby{value: STAKE + BOND}(bytes32(uint256(0xBAD)));

        // Lobby fills but nobody settles — full refund after quorum+grace.
        // NOTE: warp from the externally-read deadline — via_ir can CSE a
        // `block.timestamp + N` argument against a pre-warp read in this
        // frame (observed: warp computed 1+422 while the clock read 423).
        vm.warp(mp.getMatch(bytes32(uint256(0xBAD))).graceUntil + 1);
        mp.expireRefund(bytes32(uint256(0xBAD)));

        // The hostile receiver's withdraw fails — but only theirs.
        (bool ok,) = address(hostile).call(abi.encodeWithSelector(AMPMultiplayer.withdraw.selector));
        assertFalse(ok, "hostile withdraw must revert");

        vm.prank(players[0]);
        mp.withdraw();
        assertEq(mp.claimable(players[0]), 0);
        // Two deposits paid (0.21), two credits withdrawn (0.21) — net zero.
        assertEq(players[0].balance, 10 ether, "player collects both refunds");
    }

    // ── gas profile at scale (documented bounds, §6 scope note) ──────────

    function _settleAt(uint64 n, uint256 k) internal {
        // fresh lobby of n players, winner-takes-all profile
        vm.startPrank(owner);
        uint16[] memory t1 = new uint16[](1);
        t1[0] = 10000;
        mp.registerPayoutProfile(99, n, 1, t1);
        vm.stopPrank();
        bytes32 id = bytes32(uint256(0x6000 + n));
        vm.prank(relayer);
        mp.createLobby(id, 99, n, STAKE, BOND, 1, 10 minutes);
        address[] memory ranked = new address[](n);
        for (uint64 i; i < n; ++i) {
            address p = vm.addr(0x40000 + i);
            vm.deal(p, 1 ether);
            vm.prank(p);
            mp.joinLobby{value: STAKE + BOND}(id);
            ranked[i] = p;
        }
        bytes32 digest = mp.ladderDigest(id, 99, ranked, TRANSCRIPT, NONCE);
        uint256 mask = (1 << k) - 1;
        bytes memory sigs;
        for (uint64 i; i < k; ++i) {
            (uint8 v, bytes32 rr, bytes32 ss) = vm.sign(0x40000 + i, digest);
            sigs = abi.encodePacked(sigs, rr, ss, v);
        }
        uint256 before = gasleft();
        mp.settleMultiplayer(id, ranked, TRANSCRIPT, NONCE, mask, sigs);
        emit log_named_uint("settlement gas", before - gasleft());
    }

    function test_GasProfile_N16() public {
        _settleAt(16, 11); // K = floor(32/3)+1
    }

    function test_GasProfile_BR64() public {
        _settleAt(64, 43); // K = floor(128/3)+1 — documented BR bound
    }

    receive() external payable {}
}
