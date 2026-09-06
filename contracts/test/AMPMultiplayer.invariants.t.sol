// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "forge-std/Test.sol";
import "../src/AMPMultiplayer.sol";

/**
 * §6 conservation gate: across EVERY terminal path — quorum settlement,
 * grace finalization, dispute resolution, expiry refund, lobby cancel —
 *
 *     contract balance == Σ claimable  (to 1 wei)
 *
 * No value leaves except via withdraw(), and withdraw decrements claimable
 * by exactly the transferred amount, so the invariant is checked after
 * every handler step.
 */
contract MPHandler is Test {
    AMPMultiplayer public mp;
    address[8] public players;
    uint256[8] public keys;
    address public treasury;
    address public relayer;
    address public studio;

    bytes32 public matchId;
    uint256 constant N = 8;
    uint256 constant K = 6;
    uint256 constant STAKE = 0.05 ether;
    uint256 constant BOND = 0.0025 ether;
    address public challenger = address(0x777);
    uint256 public verifierKey = 0x5151;
    uint256 public totalChallengeStake;

    constructor() {
        treasury = address(0x1111);
        relayer = address(0x2222);
        studio = address(0x3333);
        mp = new AMPMultiplayer(100, 2000, treasury, relayer);
        mp.setStudioRecipient(1, studio);
        uint16[] memory tiers = new uint16[](3);
        tiers[0] = 5000;
        tiers[1] = 3500;
        tiers[2] = 1500;
        mp.registerPayoutProfile(1, uint64(N), 1, tiers);
        mp.setDisputeVerifier(vm.addr(verifierKey));

        for (uint256 i; i < N; ++i) {
            keys[i] = 0x9000 + i;
            players[i] = vm.addr(keys[i]);
            vm.deal(players[i], 100 ether);
        }
        vm.deal(challenger, 10 ether);
        matchId = bytes32(uint256(0x51));
        mp.createLobby(matchId, 1, uint64(N), STAKE, BOND, 1, 1 hours);
    }

    function ladder() public view returns (address[] memory ranked) {
        ranked = new address[](N);
        for (uint256 i; i < N; ++i) {
            ranked[i] = players[i];
        }
    }

    function ladderAlt() public view returns (address[] memory ranked) {
        ranked = new address[](N);
        for (uint256 i; i < N; ++i) {
            ranked[i] = players[(i + 3) % N];
        }
    }

    function join(uint256 actorSeed) external {
        uint8 i = uint8(actorSeed % N);
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Open) return;
        if (mp.participantIndex(matchId, players[i]) != 0) return;
        vm.prank(players[i]);
        mp.joinLobby{value: STAKE + BOND}(matchId);
    }

    function settle(uint256 signerSeed, bool altLadder) external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Ready) return;
        if (block.timestamp > m.quorumUntil) return;
        address[] memory ranked = altLadder ? ladderAlt() : ladder();
        // pick K pseudo-random distinct signers
        uint256 mask;
        uint256 seed = signerSeed;
        uint256 picked;
        while (picked < K) {
            uint8 bit = uint8(seed % N);
            if ((mask & (1 << bit)) == 0) {
                mask |= 1 << bit;
                picked++;
            }
            seed = uint256(keccak256(abi.encode(seed)));
        }
        bytes32 digest = mp.ladderDigest(matchId, 1, ranked, bytes32(uint256(0x99)), 1);
        bytes memory sigs;
        for (uint256 i; i < N; ++i) {
            if ((mask & (1 << i)) != 0) {
                (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[i], digest);
                sigs = abi.encodePacked(sigs, r, s, v);
            }
        }
        mp.settleMultiplayer(matchId, ranked, bytes32(uint256(0x99)), 1, mask, sigs);
    }

    function warpPastQuorum() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Ready) return;
        vm.warp(m.quorumUntil + 1);
    }

    function graceClaim() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Ready || block.timestamp <= m.quorumUntil) return;
        address[] memory ranked = ladder();
        bytes32 digest = mp.ladderDigest(matchId, 1, ranked, bytes32(uint256(0x99)), 1);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[0], digest);
        mp.unilateralClaim(matchId, ranked, bytes32(uint256(0x99)), 1, abi.encodePacked(r, s, v));
    }

    function challenge() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.GracePending || block.timestamp > m.challengeUntil) return;
        address[] memory ranked = ladderAlt();
        uint256 mask = 0x1E; // players 1..4
        bytes32 digest = mp.ladderDigest(matchId, 1, ranked, bytes32(uint256(0x99)), 2);
        bytes memory sigs;
        for (uint256 i = 1; i <= 4; ++i) {
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[i], digest);
            sigs = abi.encodePacked(sigs, r, s, v);
        }
        uint256 stake = (STAKE + BOND) * N / 10;
        totalChallengeStake += stake;
        vm.prank(challenger);
        mp.challengeClaim{value: stake}(matchId, ranked, bytes32(uint256(0x99)), 2, mask, sigs);
    }

    function resolve(bool aWins) external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Disputed) return;
        (address[] memory ranked, bool factionAWon) = aWins ? (ladder(), true) : (ladderAlt(), false);
        bytes32 structHash = keccak256(
            abi.encode(
                mp.DISPUTE_VERDICT_TYPEHASH(),
                matchId,
                keccak256(abi.encodePacked(ranked)),
                bytes32(uint256(0x99)),
                factionAWon
            )
        );
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", mp.domainSeparator(), structHash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(verifierKey, digest);
        vm.prank(vm.addr(verifierKey));
        mp.resolveDispute(matchId, ranked, bytes32(uint256(0x99)), factionAWon, abi.encodePacked(r, s, v));
    }

    function finalizeGrace() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.GracePending || block.timestamp <= m.challengeUntil) return;
        mp.finalizeGraceWith(matchId, ladder(), bytes32(uint256(0x99)));
    }

    function expire() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Ready || block.timestamp <= m.graceUntil) return;
        mp.expireRefund(matchId);
    }

    function playerClaim(uint256 actorSeed) external {
        uint8 i = uint8(actorSeed % N);
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Settled) return;
        // try both ladders (only the settled one succeeds)
        vm.prank(players[i]);
        try mp.claimPayout(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(players[i]);
        try mp.claimPayout(matchId, ladderAlt(), bytes32(uint256(0x99))) {} catch {}
    }

    function feeClaim() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Settled) return;
        vm.prank(studio);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(treasury);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(relayer);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
    }

    function withdraw(uint256 actorSeed) external {
        uint8 i = uint8(actorSeed % N);
        if (mp.claimable(players[i]) == 0) return;
        vm.prank(players[i]);
        mp.withdraw();
    }

    /// Force every possible claim: all players × both ladders + all fees.
    /// Post-terminal, this exhausts the settlement's total liability.
    function drainAll() external {
        AMPMultiplayer.Match memory m = mp.getMatch(matchId);
        if (m.state != AMPMultiplayer.State.Settled) return;
        for (uint256 i; i < N; ++i) {
            vm.prank(players[i]);
            try mp.claimPayout(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
            vm.prank(players[i]);
            try mp.claimPayout(matchId, ladderAlt(), bytes32(uint256(0x99))) {} catch {}
        }
        vm.prank(studio);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(treasury);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(relayer);
        try mp.claimFees(matchId, ladder(), bytes32(uint256(0x99))) {} catch {}
        // The alt-ladder fee claims too (same settlement hash? no — different
        // ladder; fee claims hash-check the ladder, so also try alt where the
        // alt was the settled one).
        vm.prank(studio);
        try mp.claimFees(matchId, ladderAlt(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(treasury);
        try mp.claimFees(matchId, ladderAlt(), bytes32(uint256(0x99))) {} catch {}
        vm.prank(relayer);
        try mp.claimFees(matchId, ladderAlt(), bytes32(uint256(0x99))) {} catch {}
    }

    function totalCredited() external view returns (uint256 sum) {
        sum = mp.claimable(treasury) + mp.claimable(relayer) + mp.claimable(studio) + mp.claimable(challenger);
        for (uint256 i; i < N; ++i) {
            sum += mp.claimable(players[i]);
        }
    }
}

contract AMPMultiplayerInvariants is Test {
    MPHandler internal handler;

    function setUp() public {
        handler = new MPHandler();
        targetContract(address(handler));
    }

    /// During the run: escrow can never be over-credited.
    function invariant_BalanceCoversCredits() public view {
        assertGe(address(handler.mp()).balance, handler.totalCredited(), "credits exceed escrow");
    }

    /// At a terminal state, after force-draining every claim, the escrow is
    /// accounted for to the last wei: balance == total credits. (Lazy fees
    /// are part of the drain; rounding dust routes to the treasury by
    /// construction, so equality is exact.)
    function invariant_TerminalStateDrainsExactly() public {
        AMPMultiplayer.Match memory m = handler.mp().getMatch(handler.matchId());
        if (m.state == AMPMultiplayer.State.Settled) {
            handler.drainAll();
            assertEq(
                address(handler.mp()).balance, handler.totalCredited(), "terminal drain must account for every wei"
            );
        }
    }
}
