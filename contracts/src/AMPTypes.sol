// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

library AMPTypes {
    enum MatchState {
        OPEN,
        READY,
        SETTLED,
        EXPIRED,
        DISPUTED
    }

    enum SettlementMode {
        ASYNC_VERIFIER,
        RT_HASH_AGREE
    }

    enum OutcomeCode {
        NONE,
        WIN_A,
        WIN_B,
        DRAW,
        CANCELLED
    }

    struct Game {
        address admin;
        SettlementMode mode;
        address[] verifiers;
        uint256 minStake;
        address stakeToken;
        address arbiter;
        uint256 matchTimeout;
        // Fee-split router: studio rake share of the pool, paid to
        // studioFeeRecipient on every settled (non-cancelled) match.
        uint16 studioFeeBps;
        address studioFeeRecipient;
    }

    struct Match {
        uint256 gameId;
        address playerA;
        AMPTypes.MatchState state;
        address playerB;
        uint64 createdAt;
        // Player A's actual received stake (balance-delta, fee-on-transfer safe).
        uint256 stakeAmount;
        // Player B's actual received stake (balance-delta, fee-on-transfer safe).
        // Zero until joinMatch. For standard tokens this equals `stakeAmount`.
        uint256 stakeAmountB;
    }

    struct Settlement {
        uint256 matchId;
        OutcomeCode outcome;
        bytes32 transcriptHash;
        uint256 settledAt;
    }

    struct AsyncResult {
        uint256 matchId;
        OutcomeCode outcome;
        bytes32 transcriptHash;
        bytes signature;
    }

    struct RealTimeHashResult {
        uint256 matchId;
        OutcomeCode outcome;
        bytes32 transcriptHash;
    }
}
