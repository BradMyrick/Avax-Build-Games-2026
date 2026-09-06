// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "forge-std/Script.sol";
import "../src/AMPRegistry.sol";
import "../src/AMPSettlement.sol";

/**
 * v1 1v1 baseline deployment — operator-first (no timelock for the beta;
 * timelock governance returns at mainnet per the roadmap).
 *
 * Deploys:
 *   1. AMPRegistry                     (escrow + game registry + fee-split config)
 *   2. AMPSettlement(registry)         (verifier-attested + RT hash-agree settlement,
 *                                       studio/protocol fee-split router)
 *   3. registry.setSettlement(settlement)
 *   4. registry.registerGame(...)      (game 0: ASYNC_VERIFIER, native AVAX)
 *
 * Env:
 *   AMP_VERIFIER_ADDRESS  — amp-server's EIP-712 verifier (whitelisted for game 0)
 *   AMP_ARBITER_ADDRESS   — dispute arbiter (defaults to the deployer)
 *   AMP_MIN_STAKE_WEI     — minimum escrow stake (default 1e15 = 0.001 AVAX)
 *
 * Run (Avalanche C-Chain serves no pending-block state — skip simulation):
 *   forge script script/DeployV1.s.sol --rpc-url fuji --broadcast --skip-simulation
 */
contract DeployV1 is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);

        address verifier = vm.envOr("AMP_VERIFIER_ADDRESS", deployer);
        address arbiter = vm.envOr("AMP_ARBITER_ADDRESS", deployer);
        uint256 minStake = vm.envOr("AMP_MIN_STAKE_WEI", uint256(1e15));

        vm.startBroadcast(deployerKey);

        AMPRegistry registry = new AMPRegistry();
        AMPSettlement settlement = new AMPSettlement(address(registry));
        registry.setSettlement(address(settlement));

        address[] memory verifiers = new address[](1);
        verifiers[0] = verifier;
        uint256 gameId = registry.registerGame(
            AMPTypes.SettlementMode.ASYNC_VERIFIER,
            verifiers,
            minStake,
            address(0), // native AVAX stakes
            arbiter
        );

        vm.stopBroadcast();

        console.log("AMPRegistry:  ", address(registry));
        console.log("AMPSettlement:", address(settlement));
        console.log("gameId:       ", gameId);
        console.log("verifier:     ", verifier);
        console.log("arbiter:      ", arbiter);
        console.log("minStakeWei:  ", minStake);
    }
}
