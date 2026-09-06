// SPDX-License-Identifier: MIT
pragma solidity ^0.8.33;

import "forge-std/Test.sol";
import "../src/AMPSettlement.sol";
import "../src/AMPRegistry.sol";
import "../src/AMPTypes.sol";
import "./mocks/FeeOnTransferToken.sol";
import "openzeppelin-contracts/contracts/utils/Pausable.sol";

contract AMPSettlementTest is Test {
    AMPRegistry public registry;
    AMPSettlement public settlement;
    address public admin = address(0x10);
    address public playerA = address(0x20);
    address public playerB = address(0x30);
    address public nonPlayer = address(0x50);
    uint256 public verifierPrivKey = 0x1234;
    address public verifierPubKey;

    function setUp() public {
        registry = new AMPRegistry();
        settlement = new AMPSettlement(address(registry));
        registry.setSettlement(address(settlement));
        verifierPubKey = vm.addr(verifierPrivKey);
        vm.deal(playerA, 10 ether);
        vm.deal(playerB, 10 ether);
    }

    function _setupAsyncGameAndMatch() internal returns (uint256 matchId) {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 1 ether, address(0), address(0));
        matchId = 123;
        vm.prank(playerA);
        registry.createMatch{value: 1 ether}(gameId, matchId, 1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 1 ether}(matchId);
    }

    function _signResult(uint256 matchId, AMPTypes.OutcomeCode outcome, bytes32 transcriptHash, uint256 privKey)
        internal
        view
        returns (bytes memory)
    {
        bytes32 structHash = keccak256(abi.encode(settlement.ASYNC_RESULT_TYPEHASH(), matchId, outcome, transcriptHash));
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", settlement.domainSeparator(), structHash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privKey, digest);
        return abi.encodePacked(r, s, v);
    }

    function _setupRTGameAndMatch() internal returns (uint256 matchId) {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.RT_HASH_AGREE, verifiers, 1 ether, address(0), admin);
        matchId = 456;
        vm.prank(playerA);
        registry.createMatch{value: 1 ether}(gameId, matchId, 1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 1 ether}(matchId);
    }

    function _withdrawNative(address player, uint256 expectedAmount) internal {
        uint256 balanceBefore = player.balance;
        vm.prank(player);
        registry.withdraw(address(0));
        assertEq(player.balance, balanceBefore + expectedAmount);
    }

    function testFullAsyncFlow() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.WIN_A;
        bytes32 transcriptHash = bytes32(uint256(0x5678));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        settlement.submitAsyncResult(matchId, result);
        (, AMPTypes.OutcomeCode actualOutcome,,) = settlement.settlements(matchId);
        assertEq(uint256(actualOutcome), uint256(outcome));
        assertEq(registry.pendingWithdrawals(address(0), playerA), 1.98 ether);
        // Phase 3.1: protocol fees now route to the Settlement's
        // `protocolFeeRecipient` (defaults to the deployer = address(this))
        // via `pendingWithdrawals`, not to the Registry's `feeBalances` pot.
        assertEq(registry.pendingWithdrawals(address(0), address(this)), 0.02 ether);
        assertEq(registry.feeBalances(address(0)), 0 ether);
        _withdrawNative(playerA, 1.98 ether);
    }

    function testResolveDispute() public {
        uint256 matchId = _setupRTGameAndMatch();
        AMPTypes.RealTimeHashResult memory resultA = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(1))
        });
        AMPTypes.RealTimeHashResult memory resultB = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_B, transcriptHash: bytes32(uint256(2))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, resultA);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, resultB);
        vm.prank(admin);
        settlement.resolveDispute(matchId, AMPTypes.OutcomeCode.WIN_B);
        assertEq(registry.pendingWithdrawals(address(0), playerB), 1.98 ether);
        (, AMPTypes.OutcomeCode outcome,,) = settlement.settlements(matchId);
        assertEq(uint256(outcome), uint256(AMPTypes.OutcomeCode.WIN_B));
        _withdrawNative(playerB, 1.98 ether);
    }

    function testDrawOutcome() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.DRAW;
        bytes32 transcriptHash = bytes32(uint256(0xabcd));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        settlement.submitAsyncResult(matchId, result);
        assertEq(registry.pendingWithdrawals(address(0), playerA), 0.99 ether);
        assertEq(registry.pendingWithdrawals(address(0), playerB), 0.99 ether);
        _withdrawNative(playerA, 0.99 ether);
        _withdrawNative(playerB, 0.99 ether);
    }

    function testCancelledOutcome() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.CANCELLED;
        bytes32 transcriptHash = bytes32(uint256(0xef01));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        settlement.submitAsyncResult(matchId, result);
        assertEq(registry.pendingWithdrawals(address(0), playerA), 1 ether);
        assertEq(registry.pendingWithdrawals(address(0), playerB), 1 ether);
        assertEq(registry.feeBalances(address(0)), 0);
        _withdrawNative(playerA, 1 ether);
        _withdrawNative(playerB, 1 ether);
    }

    function testInvalidVerifierSignature() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        uint256 wrongPrivKey = 0xdeadbeef;
        bytes memory signature = _signResult(matchId, AMPTypes.OutcomeCode.WIN_A, bytes32(uint256(0x11)), wrongPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId,
            outcome: AMPTypes.OutcomeCode.WIN_A,
            transcriptHash: bytes32(uint256(0x11)),
            signature: signature
        });
        vm.expectRevert(AMPSettlement.InvalidVerifierSignature.selector);
        settlement.submitAsyncResult(matchId, result);
    }

    function testWrongMode() public {
        uint256 matchId = _setupRTGameAndMatch();
        bytes memory signature = _signResult(matchId, AMPTypes.OutcomeCode.WIN_A, bytes32(0), verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(0), signature: signature
        });
        vm.expectRevert(AMPSettlement.WrongMode.selector);
        settlement.submitAsyncResult(matchId, result);
    }

    function testRTAgreementSuccess() public {
        uint256 matchId = _setupRTGameAndMatch();
        AMPTypes.RealTimeHashResult memory resultA = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(0xabc))
        });
        AMPTypes.RealTimeHashResult memory resultB = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(0xabc))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, resultA);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, resultB);
        (, AMPTypes.OutcomeCode outcome,,) = settlement.settlements(matchId);
        assertEq(uint256(outcome), uint256(AMPTypes.OutcomeCode.WIN_A));
        assertEq(registry.pendingWithdrawals(address(0), playerA), 1.98 ether);
        _withdrawNative(playerA, 1.98 ether);
    }

    function testRTNotPlayer() public {
        uint256 matchId = _setupRTGameAndMatch();
        AMPTypes.RealTimeHashResult memory result = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(0x1))
        });
        vm.prank(nonPlayer);
        vm.expectRevert(AMPSettlement.NotAPlayer.selector);
        settlement.submitRealTimeHashResult(matchId, result);
    }

    function testDisputeNotArbiter() public {
        uint256 matchId = _setupRTGameAndMatch();
        AMPTypes.RealTimeHashResult memory resultA = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(1))
        });
        AMPTypes.RealTimeHashResult memory resultB = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_B, transcriptHash: bytes32(uint256(2))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, resultA);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, resultB);
        vm.prank(nonPlayer);
        vm.expectRevert(AMPSettlement.NotArbiter.selector);
        settlement.resolveDispute(matchId, AMPTypes.OutcomeCode.WIN_A);
    }

    function testFeeZeroPercent() public {
        settlement.updateProtocolFeeBps(0);
        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.WIN_A;
        bytes32 transcriptHash = bytes32(uint256(0x5678));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        settlement.submitAsyncResult(matchId, result);
        assertEq(registry.pendingWithdrawals(address(0), playerA), 2 ether);
        assertEq(registry.feeBalances(address(0)), 0);
        _withdrawNative(playerA, 2 ether);
    }

    function testFeeInvalidBps() public {
        vm.expectRevert(AMPSettlement.FeeExceedsMax.selector);
        settlement.updateProtocolFeeBps(501);
    }

    function testUpdateFeeRecipient() public {
        address newRecipient = address(0x8888);
        settlement.updateProtocolFeeRecipient(newRecipient);
        assertEq(settlement.protocolFeeRecipient(), newRecipient);
    }

    function testPauseBlocksSubmit() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        settlement.pause();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.WIN_A;
        bytes32 transcriptHash = bytes32(uint256(0x5678));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        vm.expectRevert(abi.encodeWithSelector(Pausable.EnforcedPause.selector));
        settlement.submitAsyncResult(matchId, result);
    }

    function testPauseBlocksResolveDispute() public {
        // Phase 3.4: resolveDispute is now whenNotPaused, restoring pause
        // symmetry (it was previously callable while paused).
        uint256 matchId = _setupRTGameAndMatch();
        // Drive the match into DISPUTED via mismatched RT submissions.
        AMPTypes.RealTimeHashResult memory resultA = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(1))
        });
        AMPTypes.RealTimeHashResult memory resultB = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_B, transcriptHash: bytes32(uint256(2))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, resultA);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, resultB);

        settlement.pause();
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(Pausable.EnforcedPause.selector));
        settlement.resolveDispute(matchId, AMPTypes.OutcomeCode.WIN_B);
    }

    function testSettlementFeeRoutesToRecipient() public {
        // Phase 3.1: an explicit protocolFeeRecipient receives the fee via
        // pendingWithdrawals (not the Registry's owner fee pot).
        address recipient = address(0xFEE);
        settlement.updateProtocolFeeRecipient(recipient);

        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.WIN_A;
        bytes32 transcriptHash = bytes32(uint256(0x9999));
        bytes memory signature = _signResult(matchId, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        settlement.submitAsyncResult(matchId, result);

        assertEq(registry.pendingWithdrawals(address(0), recipient), 0.02 ether);
        assertEq(registry.feeBalances(address(0)), 0 ether);
    }

    function testMatchIdMismatch() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        AMPTypes.OutcomeCode outcome = AMPTypes.OutcomeCode.WIN_A;
        bytes32 transcriptHash = bytes32(uint256(0x5678));
        bytes memory signature = _signResult(999, outcome, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: 999, outcome: outcome, transcriptHash: transcriptHash, signature: signature
        });
        vm.expectRevert(AMPSettlement.MatchIdMismatch.selector);
        settlement.submitAsyncResult(matchId, result);
    }

    function testWithdrawNoPending() public {
        vm.prank(playerA);
        vm.expectRevert(AMPRegistry.NoPendingWithdrawal.selector);
        registry.withdraw(address(0));
    }

    // ---- Phase 1.1: F1 fix — OutcomeCode.NONE must never settle. ----
    // A verifier (or arbiter) supplying outcome NONE previously caused _payout
    // to fall through its if-chain, marking the match SETTLED while crediting
    // both players 0 and permanently trapping ~99% of the pool.

    function testSubmitAsyncRevertsOnNoneOutcome() public {
        uint256 matchId = _setupAsyncGameAndMatch();
        bytes32 transcriptHash = bytes32(uint256(0x5678));
        bytes memory signature = _signResult(matchId, AMPTypes.OutcomeCode.NONE, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.NONE, transcriptHash: transcriptHash, signature: signature
        });
        vm.expectRevert(AMPSettlement.InvalidOutcome.selector);
        settlement.submitAsyncResult(matchId, result);
    }

    function testResolveDisputeRevertsOnNoneOutcome() public {
        uint256 matchId = _setupRTGameAndMatch();
        AMPTypes.RealTimeHashResult memory resultA = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_A, transcriptHash: bytes32(uint256(1))
        });
        AMPTypes.RealTimeHashResult memory resultB = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.WIN_B, transcriptHash: bytes32(uint256(2))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, resultA);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, resultB);
        // Match is now DISPUTED. Arbiter may NOT resolve to NONE.
        vm.prank(admin);
        vm.expectRevert(AMPSettlement.InvalidOutcome.selector);
        settlement.resolveDispute(matchId, AMPTypes.OutcomeCode.NONE);
    }

    // ---- Phase 1.6: Solidity-side EIP-712 KAT vs the cross-language vector. ----
    // Pins the contract's domain/struct hashing to the same digest the Rust,
    // Go, C#, Python, and JS SDKs produce for the shared vector:
    //   matchId=1, outcome=1(WIN_A), transcript=zero[32],
    //   chainId=43113, verifyingContract=0x0
    //   -> 0x2d2525ad5098ca8f82a2a6cabc6775c40a55df96dfa2fbb46d7c0e372b99096c
    function testEIP712DigestMatchesCrossLangVector() public view {
        bytes32 eip712DomainTypehash =
            keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
        bytes32 domain = keccak256(
            abi.encode(
                eip712DomainTypehash,
                keccak256(bytes("AMPSettlement")),
                keccak256(bytes("1")),
                uint256(43113),
                address(0)
            )
        );
        bytes32 structHash = keccak256(abi.encode(settlement.ASYNC_RESULT_TYPEHASH(), uint256(1), uint8(1), bytes32(0)));
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", domain, structHash));
        assertEq(
            digest,
            bytes32(0x2d2525ad5098ca8f82a2a6cabc6775c40a55df96dfa2fbb46d7c0e372b99096c),
            "Solidity digest must match the cross-language SDK KAT vector"
        );
    }

    // ---- Phase 1.5: fee-on-transfer token support (joined stakes tracked). ----
    // After the fix, a 1% fee token lets both players join and settle; each
    // player's payout reflects the actual received (post-fee) stake rather than
    // reverting on the stale `received != stakeAmount` equality check.
    function testFeeOnTransferTokenSettlesAndRefundsActual() public {
        FeeOnTransferToken token = new FeeOnTransferToken();
        token.transfer(playerA, 10 ether);
        token.transfer(playerB, 10 ether);

        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId = registry.registerGame(
            AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 1 ether, address(token), address(0)
        );

        uint256 matchId = 777;
        vm.startPrank(playerA);
        token.approve(address(registry), 1 ether);
        registry.createMatch(gameId, matchId, 1 ether);
        vm.stopPrank();

        vm.startPrank(playerB);
        token.approve(address(registry), 1 ether);
        registry.joinMatch(matchId);
        vm.stopPrank();

        // Each player sent 1e18; A's actual received stake is 0.99e18 (1% fee),
        // and B transfers that 0.99e18 and receives 0.99*0.99 = 0.9801e18.
        (,,,,,, uint256 stakeB) = registry.matches(matchId);
        assertEq(stakeB, 0.9801 ether, "player B actual stake must be post-fee (compounded)");

        // Settle CANCELLED — each player refunds exactly what they staked.
        bytes32 transcriptHash = bytes32(uint256(0xC0FFEE));
        bytes memory signature = _signResult(matchId, AMPTypes.OutcomeCode.CANCELLED, transcriptHash, verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId,
            outcome: AMPTypes.OutcomeCode.CANCELLED,
            transcriptHash: transcriptHash,
            signature: signature
        });
        settlement.submitAsyncResult(matchId, result);

        assertEq(registry.pendingWithdrawals(address(token), playerA), 0.99 ether);
        assertEq(registry.pendingWithdrawals(address(token), playerB), 0.9801 ether);
    }

    // ---- fee-split router ---------------------------------------------------

    address public studio = address(0x77);

    function _setupAsyncGameWithStudioFee(uint16 studioBps) internal returns (uint256 matchId) {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 1 ether, address(0), address(0));
        vm.prank(admin);
        registry.setStudioFee(gameId, studioBps, studio);
        matchId = 777;
        vm.prank(playerA);
        registry.createMatch{value: 1 ether}(gameId, matchId, 1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 1 ether}(matchId);
    }

    function testFeeSplitRoutesStudioAndProtocol() public {
        // studio 200 bps (2%), protocol default 100 bps (1%) on a 2-ether pool:
        // studio 0.04, protocol 0.02, winner 1.94.
        uint256 matchId = _setupAsyncGameWithStudioFee(200);
        bytes memory signature =
            _signResult(matchId, AMPTypes.OutcomeCode.WIN_A, bytes32(uint256(0x9)), verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId,
            outcome: AMPTypes.OutcomeCode.WIN_A,
            transcriptHash: bytes32(uint256(0x9)),
            signature: signature
        });
        vm.prank(nonPlayer);
        settlement.submitAsyncResult(matchId, result);

        assertEq(registry.pendingWithdrawals(address(0), studio), 0.04 ether, "studio rake");
        assertEq(registry.pendingWithdrawals(address(0), settlement.protocolFeeRecipient()), 0.02 ether, "protocol fee");
        assertEq(registry.pendingWithdrawals(address(0), playerA), 1.94 ether, "winner payout");
        assertEq(registry.pendingWithdrawals(address(0), playerB), 0, "loser payout");
    }

    function testFeeSplitRTModeAlsoSplits() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.RT_HASH_AGREE, verifiers, 1 ether, address(0), admin);
        vm.prank(admin);
        registry.setStudioFee(gameId, 300, studio);
        uint256 matchId = 778;
        vm.prank(playerA);
        registry.createMatch{value: 1 ether}(gameId, matchId, 1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 1 ether}(matchId);

        AMPTypes.RealTimeHashResult memory r = AMPTypes.RealTimeHashResult({
            matchId: matchId, outcome: AMPTypes.OutcomeCode.DRAW, transcriptHash: bytes32(uint256(0xa))
        });
        vm.prank(playerA);
        settlement.submitRealTimeHashResult(matchId, r);
        vm.prank(playerB);
        settlement.submitRealTimeHashResult(matchId, r);

        // studio 3% = 0.06, protocol 1% = 0.02, players split 1.92 → 0.96 each.
        assertEq(registry.pendingWithdrawals(address(0), studio), 0.06 ether, "studio rake");
        assertEq(registry.pendingWithdrawals(address(0), settlement.protocolFeeRecipient()), 0.02 ether, "protocol");
        assertEq(registry.pendingWithdrawals(address(0), playerA), 0.96 ether);
        assertEq(registry.pendingWithdrawals(address(0), playerB), 0.96 ether);
    }

    function testCancelRefundsTakeNoFees() public {
        uint256 matchId = _setupAsyncGameWithStudioFee(200);
        bytes memory signature =
            _signResult(matchId, AMPTypes.OutcomeCode.CANCELLED, bytes32(uint256(0x9)), verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId,
            outcome: AMPTypes.OutcomeCode.CANCELLED,
            transcriptHash: bytes32(uint256(0x9)),
            signature: signature
        });
        vm.prank(nonPlayer);
        settlement.submitAsyncResult(matchId, result);

        assertEq(registry.pendingWithdrawals(address(0), studio), 0, "no studio rake on cancel");
        assertEq(
            registry.pendingWithdrawals(address(0), settlement.protocolFeeRecipient()), 0, "no protocol fee on cancel"
        );
        assertEq(registry.pendingWithdrawals(address(0), playerA), 1 ether, "full refund A");
        assertEq(registry.pendingWithdrawals(address(0), playerB), 1 ether, "full refund B");
    }

    function testTotalFeeCapReverts() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 1 ether, address(0), address(0));
        vm.prank(admin);
        registry.setStudioFee(gameId, 301, studio);
        vm.prank(settlement.owner());
        settlement.updateProtocolFeeBps(500); // 500 + 301 > 800 combined cap

        uint256 matchId = 779;
        vm.prank(playerA);
        registry.createMatch{value: 1 ether}(gameId, matchId, 1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 1 ether}(matchId);

        bytes memory signature =
            _signResult(matchId, AMPTypes.OutcomeCode.WIN_A, bytes32(uint256(0x9)), verifierPrivKey);
        AMPTypes.AsyncResult memory result = AMPTypes.AsyncResult({
            matchId: matchId,
            outcome: AMPTypes.OutcomeCode.WIN_A,
            transcriptHash: bytes32(uint256(0x9)),
            signature: signature
        });
        vm.prank(nonPlayer);
        vm.expectRevert(AMPSettlement.TotalFeeTooHigh.selector);
        settlement.submitAsyncResult(matchId, result);
    }

    function testStudioFeeSetterAuthAndBounds() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = verifierPubKey;
        vm.prank(admin);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 1 ether, address(0), address(0));

        vm.prank(nonPlayer);
        vm.expectRevert(AMPRegistry.NotGameAdmin.selector);
        registry.setStudioFee(gameId, 100, studio);

        vm.prank(admin);
        vm.expectRevert(AMPRegistry.InvalidStudioFee.selector);
        registry.setStudioFee(gameId, 501, studio); // over per-game cap

        vm.prank(admin);
        vm.expectRevert(AMPRegistry.InvalidStudioFee.selector);
        registry.setStudioFee(gameId, 100, address(0)); // needs recipient

        vm.prank(admin);
        registry.setStudioFee(gameId, 500, studio); // max allowed
        (,,,,,, uint16 bps, address recipient) = registry.games(gameId);
        assertEq(bps, 500);
        assertEq(recipient, studio);
    }
}
