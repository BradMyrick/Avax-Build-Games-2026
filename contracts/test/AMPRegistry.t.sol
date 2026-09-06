// SPDX-License-Identifier: MIT
pragma solidity ^0.8.33;

import "forge-std/Test.sol";
import "../src/AMPRegistry.sol";
import "../src/AMPTypes.sol";
import "openzeppelin-contracts/contracts/access/Ownable.sol";
import "openzeppelin-contracts/contracts/utils/Pausable.sol";

contract AMPRegistryTest is Test {
    AMPRegistry public registry;
    address public playerA = address(0x10);
    address public playerB = address(0x20);
    address public newOwner = address(0x40);

    function setUp() public {
        registry = new AMPRegistry();
        vm.deal(playerA, 10 ether);
        vm.deal(playerB, 10 ether);
    }

    function testRegisterGame() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0x123);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        assertEq(gameId, 0);
        assertEq(registry.nextGameId(), 1);
    }

    function testCreateMatch() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        (, address pA, AMPTypes.MatchState state, address pB,, uint256 stake,) = registry.matches(matchId);
        assertEq(pA, playerA);
        assertEq(pB, address(0));
        assertEq(stake, 0.1 ether);
        assertEq(uint256(state), uint256(AMPTypes.MatchState.OPEN));
    }

    function testJoinMatch() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 0.1 ether}(matchId);
        (, address pA, AMPTypes.MatchState state, address pB,,,) = registry.matches(matchId);
        assertEq(pA, playerA);
        assertEq(pB, playerB);
        assertEq(uint256(state), uint256(AMPTypes.MatchState.READY));
    }

    function testRegisterGameTooManyVerifiers() public {
        address[] memory verifiers = new address[](11);
        for (uint256 i = 0; i < 11; i++) {
            verifiers[i] = address(uint160(i + 1));
        }
        vm.expectRevert(AMPRegistry.TooManyVerifiers.selector);
        registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
    }

    function testUpdateGameVerifiers() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0x123);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        address[] memory newVerifiers = new address[](2);
        newVerifiers[0] = address(0x456);
        newVerifiers[1] = address(0x789);
        registry.updateGameVerifiers(gameId, newVerifiers);
        address[] memory stored = registry.getGameVerifiers(gameId);
        assertEq(stored.length, 2);
        assertEq(stored[0], address(0x456));
        assertEq(stored[1], address(0x789));
        assertFalse(registry.isVerifier(gameId, address(0x123)));
        assertTrue(registry.isVerifier(gameId, address(0x456)));
        assertTrue(registry.isVerifier(gameId, address(0x789)));
    }

    function testUpdateGameVerifiersNotAdmin() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0x123);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        address[] memory newVerifiers = new address[](1);
        newVerifiers[0] = address(0x456);
        vm.prank(playerA);
        vm.expectRevert(AMPRegistry.NotGameAdmin.selector);
        registry.updateGameVerifiers(gameId, newVerifiers);
    }

    function testSetSettlement() public {
        address settlementAddr = address(0x999);
        registry.setSettlement(settlementAddr);
        assertEq(registry.settlement(), settlementAddr);
    }

    function testSetSettlementNotOwner() public {
        vm.prank(playerA);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, playerA));
        registry.setSettlement(address(0x999));
    }

    function testSetSettlementRejectsZeroAddress() public {
        // Phase 3.3: setting the settlement to the zero address would brick
        // all settlement callbacks (onlySettlement would never match); refuse.
        vm.expectRevert(AMPRegistry.InvalidSettlementAddress.selector);
        registry.setSettlement(address(0));
    }

    function testSettleMatchRejectsInvalidNewState() public {
        // Phase 3.2: the settlement callback must not be able to roll a match
        // back to a pre-terminal state. Here the test acts as the trusted
        // settlement so it can invoke settleMatch directly.
        registry.setSettlement(address(this));

        // Create a match so there's something to (attempt to) settle.
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0xBEEF);
        registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0, address(0), address(0));
        uint256 matchId = uint256(keccak256("state-test"));
        registry.createMatch{value: 0}(0, matchId, 0);

        address[] memory recipients = new address[](0);
        uint256[] memory amounts = new uint256[](0);
        vm.expectRevert(AMPRegistry.InvalidNewState.selector);
        address[2] memory feeRecipients = [address(0), address(0)];
        uint256[2] memory feeAmounts = [uint256(0), uint256(0)];
        registry.settleMatch(matchId, AMPTypes.MatchState.OPEN, feeRecipients, feeAmounts, recipients, amounts);
    }

    function testCancelMatch() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        vm.prank(playerA);
        registry.cancelMatch(matchId);
        assertEq(registry.pendingWithdrawals(address(0), playerA), 0.1 ether);
        uint256 balanceBefore = playerA.balance;
        vm.prank(playerA);
        registry.withdraw(address(0));
        assertEq(playerA.balance, balanceBefore + 0.1 ether);
        (,, AMPTypes.MatchState state,,,,) = registry.matches(matchId);
        assertEq(uint256(state), uint256(AMPTypes.MatchState.SETTLED));
    }

    function testCancelMatchNotPlayerA() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        vm.prank(playerB);
        vm.expectRevert(AMPRegistry.NotPlayerA.selector);
        registry.cancelMatch(matchId);
    }

    function testExpireMatch() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 0.1 ether}(matchId);
        vm.warp(block.timestamp + 1 hours + 1);
        registry.expireMatch(matchId);
        assertEq(registry.pendingWithdrawals(address(0), playerA), 0.1 ether);
        assertEq(registry.pendingWithdrawals(address(0), playerB), 0.1 ether);
        uint256 balanceA = playerA.balance;
        uint256 balanceB = playerB.balance;
        vm.prank(playerA);
        registry.withdraw(address(0));
        vm.prank(playerB);
        registry.withdraw(address(0));
        assertEq(playerA.balance, balanceA + 0.1 ether);
        assertEq(playerB.balance, balanceB + 0.1 ether);
        (,, AMPTypes.MatchState state,,,,) = registry.matches(matchId);
        assertEq(uint256(state), uint256(AMPTypes.MatchState.EXPIRED));
    }

    function testExpireMatchTooEarly() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
        vm.prank(playerB);
        registry.joinMatch{value: 0.1 ether}(matchId);
        vm.expectRevert(AMPRegistry.NotExpiredYet.selector);
        registry.expireMatch(matchId);
    }

    function testPauseBlocksCreateMatch() public {
        address[] memory verifiers = new address[](1);
        registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        registry.pause();
        vm.prank(playerA);
        vm.expectRevert(abi.encodeWithSelector(Pausable.EnforcedPause.selector));
        registry.createMatch{value: 0.1 ether}(0, 0, 0.1 ether);
    }

    function testUnpauseResumes() public {
        address[] memory verifiers = new address[](1);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        registry.pause();
        registry.unpause();
        vm.prank(playerA);
        uint256 matchId = 0;
        registry.createMatch{value: 0.1 ether}(gameId, matchId, 0.1 ether);
    }

    function testIsVerifierTrue() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0x123);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        assertTrue(registry.isVerifier(gameId, address(0x123)));
    }

    function testIsVerifierFalse() public {
        address[] memory verifiers = new address[](1);
        verifiers[0] = address(0x123);
        uint256 gameId =
            registry.registerGame(AMPTypes.SettlementMode.ASYNC_VERIFIER, verifiers, 0.1 ether, address(0), address(0));
        assertFalse(registry.isVerifier(gameId, address(0xdead)));
    }

    function testOwnershipTransfer() public {
        registry.transferOwnership(newOwner);
        assertEq(registry.pendingOwner(), newOwner);
        assertEq(registry.owner(), address(this));
        vm.prank(newOwner);
        registry.acceptOwnership();
        assertEq(registry.owner(), newOwner);
        assertEq(registry.pendingOwner(), address(0));
    }
}
