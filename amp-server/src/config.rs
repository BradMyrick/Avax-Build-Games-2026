//! Runtime configuration, all via environment (12-factor). Every knob has a
//! sane beta default so a fresh checkout runs with just `DATABASE_URL`.

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub cors_origins: Vec<String>,
    /// EIP-712 verifier key (hex). Signs match-outcome attestations. If unset
    /// the server runs in free-play mode: matches + ratings work, attestations
    /// and staked settlement are disabled.
    pub verifier_key: Option<String>,
    pub chain_id: u64,
    pub settlement_address: Option<String>,
    /// Enable staked queues (requires escrow wiring + verifier key).
    pub staking_enabled: bool,
    pub session_ttl_hours: i64,
    pub match_ttl_minutes: i64,
    /// Matchmaker tick cadence.
    pub tick_ms: u64,
    /// Base skill window and how fast it widens while a player waits.
    pub skill_window_base: f32,
    pub skill_window_expansion_per_sec: f32,
    pub skill_window_cap: f32,
    pub max_active_matches: usize,
    pub admin_token: Option<String>,
    /// Cold-start: offer a house practice-bot match after this much queue
    /// wait. Bot matches settle instantly, never touch ratings or stakes.
    pub bot_fill_enabled: bool,
    pub bot_after_ms: u64,
    /// House opponent identity. Defaults to the verifier address when a key
    /// is configured, else a fixed well-known placeholder player.
    pub house_wallet: Option<String>,
    /// Scheduled prime-time queue windows per game, UTC "HH:MM" list:
    /// [{"gameId":"amp-tactics","timesUtc":["18:00","21:00"]}]
    pub queue_windows: Vec<QueueWindow>,
    /// Read-only RPC for escrow verification (defaults to Fuji public RPC).
    pub rpc_url: String,
    /// AMPRegistry address — escrow target for staked matches.
    pub registry_address: Option<String>,
    /// Registry game id our catalog maps to (operator registers the game).
    pub registry_game_id: u64,
    /// Window players have to fund escrow after pairing before the match
    /// is cancelled (minutes).
    pub escrow_window_minutes: i64,
    /// Grace period for direct RT settlement before the relayer takes over
    /// (minutes).
    pub rt_grace_minutes: i64,
    /// AMPMultiplayer.sol address (v2 N-player escrow + quorum settlement).
    pub multiplayer_address: Option<String>,
    /// Public site name rendered inside the wallet's sign-in message.
    pub site_name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct QueueWindow {
    pub game_id: String,
    #[serde(rename = "timesUtc")]
    pub times_utc: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
        let bind = std::env::var("AMP_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let cors_origins = std::env::var("AMP_CORS_ORIGINS")
            .unwrap_or_else(|_| "*".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let verifier_key = std::env::var("AMP_VERIFIER_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        let has_verifier = verifier_key.is_some();
        let settlement_address = std::env::var("AMP_SETTLEMENT_ADDRESS")
            .ok()
            .filter(|a| !a.is_empty());
        let staking_enabled = std::env::var("AMP_STAKING_ENABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        Ok(Self {
            database_url,
            bind,
            cors_origins,
            verifier_key,
            chain_id: std::env::var("AMP_CHAIN_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(43113), // Fuji
            settlement_address,
            staking_enabled: staking_enabled && has_verifier,
            session_ttl_hours: std::env::var("AMP_SESSION_TTL_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24 * 7),
            match_ttl_minutes: std::env::var("AMP_MATCH_TTL_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            tick_ms: std::env::var("AMP_TICK_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&ms| ms >= 20)
                .unwrap_or(100),
            skill_window_base: std::env::var("AMP_SKILL_WINDOW_BASE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(350.0),
            skill_window_expansion_per_sec: std::env::var("AMP_SKILL_WINDOW_EXPANSION_PER_SEC")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8.0),
            skill_window_cap: std::env::var("AMP_SKILL_WINDOW_CAP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1200.0),
            max_active_matches: std::env::var("AMP_MAX_ACTIVE_MATCHES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000),
            admin_token: std::env::var("AMP_ADMIN_TOKEN")
                .ok()
                .filter(|t| !t.is_empty()),
            bot_fill_enabled: std::env::var("AMP_BOT_FILL_ENABLED")
                .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
                .unwrap_or(true),
            bot_after_ms: std::env::var("AMP_BOT_AFTER_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&ms| ms >= 5_000)
                .unwrap_or(45_000),
            house_wallet: std::env::var("AMP_HOUSE_WALLET")
                .ok()
                .filter(|w| !w.is_empty()),
            queue_windows: std::env::var("AMP_QUEUE_WINDOWS_JSON")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            rpc_url: std::env::var("AMP_RPC_URL")
                .ok()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| "https://api.avax-test.network/ext/bc/C/rpc".into()),
            registry_address: std::env::var("AMP_REGISTRY_ADDRESS")
                .ok()
                .filter(|a| !a.is_empty()),
            registry_game_id: std::env::var("AMP_REGISTRY_GAME_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            escrow_window_minutes: std::env::var("AMP_ESCROW_WINDOW_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&m| m >= 1)
                .unwrap_or(10),
            rt_grace_minutes: std::env::var("AMP_RT_GRACE_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&m| m >= 1)
                .unwrap_or(30),
            multiplayer_address: std::env::var("AMP_MULTIPLAYER_ADDRESS")
                .ok()
                .filter(|a| !a.is_empty()),
            site_name: std::env::var("AMP_SITE_NAME")
                .ok()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "AMP Arena".into()),
        })
    }
}
