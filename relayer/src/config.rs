//! Relayer configuration — every network/contract value is env-driven with a
//! documented Fuji default, mirroring `amp-server/src/config.rs`. This is the
//! repo standard: no network constants without an env override path
//! (12-factor; deployments differ, code shouldn't).

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    /// Chain id the relayer signs for.
    pub chain_id: u64,
    /// JSON-RPC endpoint (read + tx submission).
    pub rpc_url: String,
    /// AMPTournamentCup address (sponsor prize path).
    pub cup_address: String,
    /// AMPSettlement address (staked-match settlement path).
    pub settlement_address: String,
    /// Idle poll cadence between empty job dequeues.
    pub poll_idle_ms: u64,
    /// Backoff after a poll error.
    pub poll_error_ms: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let chain_id = read_env("AMP_CHAIN_ID", "43113")?
            .parse()
            .context("AMP_CHAIN_ID must be a u64")?;

        Ok(Self {
            chain_id,
            rpc_url: read_env("AMP_RPC_URL", "https://api.avax-test.network/ext/bc/C/rpc")?,
            cup_address: read_env(
                "AMP_CUP_ADDRESS",
                // Deployed + source-verified on Fuji; see
                // contracts/deployment-fuji-tournament.json
                "0x7c743c1c9ae3e7a65d030098f2249b7787d66dff",
            )?,
            settlement_address: read_env(
                "AMP_SETTLEMENT_ADDRESS",
                // v1 1v1 baseline on Fuji; see contracts/deployment-fuji-v1.json
                "0x78ec93e66255a74873d20DD62C6595A389272126",
            )?,
            poll_idle_ms: read_env("AMP_POLL_IDLE_MS", "3000")?
                .parse()
                .context("AMP_POLL_IDLE_MS must be a u64")?,
            poll_error_ms: read_env("AMP_POLL_ERROR_MS", "10000")?
                .parse()
                .context("AMP_POLL_ERROR_MS must be a u64")?,
        })
    }
}

/// Read an env var, falling back to a documented default when unset/empty.
fn read_env(name: &str, default: &str) -> Result<String> {
    Ok(std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string()))
}
