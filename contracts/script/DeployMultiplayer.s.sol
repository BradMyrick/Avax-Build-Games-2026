// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "forge-std/Script.sol";
import "../src/AMPMultiplayer.sol";

/**
 * v2 multiplayer deployment (operator-first, no timelock for the beta).
 *
 * Env: PRIVATE_KEY, AMP_TREASURY, AMP_RELAYER_PAYOUT, AMP_RAKE_BPS,
 *      AMP_STUDIO_SPLIT_BPS, AMP_DEFAULT_GAME, AMP_DEFAULT_TIER_BPS
 *      (comma list for the default profile, e.g. "6000,3000,1000").
 *
 * Registers the default payout profile for game AMP_DEFAULT_GAME at every
 * lobby size 2..64 (tier list truncated/clamped per size — profiles are
 * immutable once written).
 */
contract DeployMultiplayer is Script {
    function run() external {
        uint256 key = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(key);
        address treasury = vm.envOr("AMP_TREASURY", deployer);
        address relayerPayout = vm.envOr("AMP_RELAYER_PAYOUT", deployer);
        uint16 rake = uint16(vm.envOr("AMP_RAKE_BPS", uint256(100)));
        uint16 studioSplit = uint16(vm.envOr("AMP_STUDIO_SPLIT_BPS", uint256(2000)));
        uint256 game = vm.envOr("AMP_DEFAULT_GAME", uint256(1));
        string memory tierList = vm.envOr("AMP_DEFAULT_TIER_BPS", string("6000,3000,1000"));

        vm.startBroadcast(key);
        AMPMultiplayer mp = new AMPMultiplayer(rake, studioSplit, treasury, relayerPayout);

        // Default profile at every lobby size: the podium tiers.
        for (uint64 n = 2; n <= 64; ++n) {
            uint16[] memory tiers = _parseTiers(tierList, n);
            mp.registerPayoutProfile(game, n, 1, tiers);
        }
        vm.stopBroadcast();

        console.log("AMPMultiplayer:", address(mp));
        console.log("rakeBps:       ", rake);
        console.log("studioSplitBps:", studioSplit);
        console.log("treasury:      ", treasury);
        console.log("relayerPayout: ", relayerPayout);
    }

    function _parseTiers(string memory list, uint64 n) private pure returns (uint16[] memory) {
        // comma-separated; tiers beyond the lobby size are dropped; a single
        // tier is used as-is (winner-takes-all at any size).
        bytes memory b = bytes(list);
        uint256 count = 1;
        for (uint256 i; i < b.length; ++i) {
            if (uint8(b[i]) == 0x2C) ++count;
        }
        uint16[] memory all = new uint16[](count);
        uint256 idx;
        uint256 val;
        bool any;
        for (uint256 i; i <= b.length; ++i) {
            if (i == b.length || uint8(b[i]) == 0x2C) {
                if (any) {
                    all[idx++] = uint16(val);
                }
                val = 0;
                any = false;
            } else if (uint8(b[i]) >= 0x30 && uint8(b[i]) <= 0x39) {
                val = val * 10 + (uint8(b[i]) - 0x30);
                any = true;
            }
        }
        // clamp to lobby size, keeping the sum a full 10000: if truncating,
        // fold the removed bps into the first tier.
        uint16[] memory out;
        if (idx > n) {
            out = new uint16[](n);
            uint16 folded;
            for (uint256 j; j < idx; ++j) {
                if (j < n) {
                    out[j] = all[j];
                } else {
                    folded += all[j];
                }
            }
            out[0] += folded;
        } else {
            out = new uint16[](idx);
            for (uint256 j; j < idx; ++j) {
                out[j] = all[j];
            }
        }
        return out;
    }
}
