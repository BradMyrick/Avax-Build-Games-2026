//! Escrow verification: read-only chain access to confirm a staked match is
//! actually funded in `AMPRegistry` before the game goes live. The server
//! never signs escrow transactions — players lock their own stakes; we only
//! verify what landed on-chain.

use alloy_primitives::{Address, U256};
use alloy_sol_types::sol;

sol! {
    #[sol(rpc)]
    contract AMPRegistryView {
        function matches(uint256 id)
            external
            view
            returns (uint256 gameId, address playerA, uint8 state, address playerB, uint64 createdAt, uint256 stakeAmount, uint256 stakeAmountB);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnChainMatch {
    pub game_id: u64,
    pub player_a: Address,
    /// AMPTypes.MatchState: 0 OPEN, 1 READY, 2 SETTLED, 3 EXPIRED, 4 DISPUTED
    pub state: u8,
    pub player_b: Address,
    pub stake_amount: U256,
    pub stake_amount_b: U256,
}

#[allow(dead_code)] // OPEN is the pre-join escrow state, surfaced in verify responses
pub const STATE_OPEN: u8 = 0;
pub const STATE_READY: u8 = 1;

/// Read a match from the registry. Returns None when the slot is empty
/// (playerA == 0).
pub async fn read_match(
    rpc_url: &str,
    registry: Address,
    on_chain_match_id: u64,
) -> anyhow::Result<Option<OnChainMatch>> {
    let provider = alloy_provider::ProviderBuilder::new()
        .connect(rpc_url)
        .await?;
    let contract = AMPRegistryView::new(registry, provider);
    let AMPRegistryView::matchesReturn {
        gameId,
        playerA,
        state,
        playerB,
        createdAt: _,
        stakeAmount,
        stakeAmountB,
    } = contract
        .matches(U256::from(on_chain_match_id))
        .call()
        .await?;

    if playerA == Address::ZERO {
        return Ok(None);
    }
    Ok(Some(OnChainMatch {
        game_id: gameId.to::<u64>(),
        player_a: playerA,
        state,
        player_b: playerB,
        stake_amount: stakeAmount,
        stake_amount_b: stakeAmountB,
    }))
}
