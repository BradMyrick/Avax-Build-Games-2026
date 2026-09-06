//! amp-server — the AMP matchmaker.
//!
//! Fast off-chain matchmaking (memory-resident queue, ~100ms tick) with
//! durable Postgres state, wallet login via one EIP-191 signature, Glicko-2
//! ratings, and EIP-712 outcome attestations that settle to AMPSettlement
//! on Avalanche when stakes are involved.
//!
//! Loops:
//! - tick:     pair queues → create matches → notify both players
//! - sweep:    expire stale matches (default-on-timeout, cancel, dispute)
//! - janitor:  purge dead auth challenges

mod attest;
mod auth;
mod config;
mod error;
mod escrow;
mod http;
mod intent;
mod ladder;
mod lobby;
mod matchsvc;
mod multiplayer;
mod party;
mod queue;
mod rating_pipeline;
mod store;
mod ws;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use alloy_primitives::Address;
use alloy_signer_local::PrivateKeySigner;
use serde_json::json;
use tokio::net::TcpListener;

use crate::auth::AuthService;
use crate::config::Config;
use crate::http::AppState;
use crate::matchsvc::MatchService;
use crate::queue::QueueService;
use crate::store::Store;
use crate::ws::WsHub;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Arc::new(Config::from_env()?);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let store = Store::connect(&cfg.database_url).await?;
    sqlx::migrate!("../migrations").run(store.pool()).await?;

    let verifier = match &cfg.verifier_key {
        Some(key) => {
            let signer: PrivateKeySigner = key
                .trim_start_matches("0x")
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid AMP_VERIFIER_KEY: {e}"))?;
            tracing::info!(verifier = %signer.address(), "verifier attestations enabled");
            Some(Arc::new(signer))
        }
        None => {
            tracing::warn!(
                "AMP_VERIFIER_KEY not set — free-play mode (no attestations, no staking)"
            );
            None
        }
    };
    let settlement: Option<Address> = cfg
        .settlement_address
        .as_deref()
        .and_then(|s| s.parse().ok());

    let queue = Arc::new(QueueService::new(Arc::clone(&cfg)));
    let hub = Arc::new(WsHub::new());
    let auth = Arc::new(AuthService::new(
        store.clone(),
        cfg.session_ttl_hours,
        cfg.site_name.clone(),
    ));
    let matches = Arc::new(MatchService::new(
        store.clone(),
        cfg.match_ttl_minutes,
        cfg.escrow_window_minutes,
        cfg.registry_game_id,
        cfg.rt_grace_minutes,
    ));
    let live_matches = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // The house opponent for cold-start practice matches. Defaults to the
    // verifier identity so the operator owns it; registered as a player so
    // match foreign keys hold.
    let house_wallet = cfg
        .house_wallet
        .clone()
        .map(|w| w.to_lowercase())
        .or_else(|| {
            verifier
                .as_ref()
                .map(|v| format!("{:#x}", v.address()).to_lowercase())
        });
    if let Some(house) = &house_wallet {
        store.upsert_player(house, "house", "en").await?;
        tracing::info!(house = %house, "practice-bot opponent registered");
    }

    let state = AppState {
        cfg: Arc::clone(&cfg),
        store: store.clone(),
        queue: Arc::clone(&queue),
        matches: Arc::clone(&matches),
        hub: Arc::clone(&hub),
        auth: Arc::clone(&auth),
        verifier,
        settlement,
        live_matches: Arc::clone(&live_matches),
    };

    // Rehydrate queued tickets from before a restart, preserving wait time.
    let tickets = store.rehydrate_tickets().await?;
    let mut rehydrated = 0usize;
    for t in tickets {
        let rating = store
            .get_rating(&t.wallet, &t.game_id, &t.ruleset_id)
            .await?;
        let joined_ms = t.joined_at.timestamp_millis().max(0) as u64;
        let canonical = t.ruleset_id.clone();
        queue.join(crate::queue::QueueEntry {
            ticket_id: t.id,
            stake_wei: t.stake_wei,
            canonical_ruleset: canonical,
            ticket: amp_match_core::PlayerTicket {
                player_id: t.wallet,
                game_id: t.game_id,
                ruleset_id: t.ruleset_id,
                mmr: rating.rating as f32,
                mmr_uncertainty: rating.rating_deviation as f32,
                region: t.region,
                preferred_role: String::new(),
                language: "en".into(),
                max_ping_ms: 150,
                enqueued_at_ms: joined_ms,
                party_size: 1,
            },
        });
        rehydrated += 1;
    }

    // Count live matches for the active-match cap.
    live_matches.store(store.count_live_matches().await?, Ordering::Relaxed);
    if rehydrated > 0 {
        tracing::info!(tickets = rehydrated, "queue rehydrated");
    }

    let cors = if cfg.cors_origins.iter().any(|o| o == "*") || cfg.cors_origins.is_empty() {
        tower_http::cors::CorsLayer::very_permissive()
    } else {
        let origins: Vec<_> = cfg
            .cors_origins
            .iter()
            .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
            .collect();
        tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::AllowOrigin::list(origins))
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
    };

    let app = crate::http::router(state.clone())
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = TcpListener::bind(&cfg.bind).await?;
    tracing::info!(bind = %cfg.bind, status = ?crate::http::status_json(&state).await, "amp-server listening");

    // tick loop
    {
        let st = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(st.cfg.tick_ms));
            loop {
                interval.tick().await;
                if let Err(e) = tick_once(&st).await {
                    tracing::error!(error = format!("{e:#}"), "matchmaker tick failed");
                }
                // N-player lobby formation from revealed commits.
                let mp_addr = st.cfg.multiplayer_address.clone().unwrap_or_default();
                if let Err(e) = crate::lobby::form_lobbies_from_reveals(
                    &st.store,
                    &st.hub,
                    &mp_addr,
                    st.cfg.chain_id,
                )
                .await
                {
                    tracing::warn!(error = format!("{e:#}"), "lobby formation failed");
                }
            }
        });
    }

    // sweep loop
    {
        let st = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = sweep_once(&st).await {
                    tracing::error!(error = format!("{e:#}"), "expiry sweep failed");
                }
                if let Err(e) = crate::lobby::multi_sweep(&st.store, &st.hub).await {
                    tracing::warn!(error = format!("{e:#}"), "multi sweep failed");
                }
            }
        });
    }

    // janitor loop
    {
        let st = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                match st.store.purge_expired_challenges().await {
                    Ok(n) if n > 0 => tracing::debug!(purged = n, "auth challenges purged"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "challenge purge failed"),
                }
            }
        });
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;

    Ok(())
}

/// One matchmaker tick: pair → create match → notify both players.
async fn tick_once(st: &AppState) -> anyhow::Result<()> {
    let active = st.live_matches.load(Ordering::Relaxed);
    let pairs = st.queue.tick(active);
    for (a, b) in pairs {
        let (game_id, ruleset_id) = (a.ticket.game_id.clone(), a.canonical_ruleset.clone());
        match st
            .matches
            .create_match(&game_id, &ruleset_id, &a, &b, false)
            .await
        {
            Ok(m) => {
                st.live_matches.fetch_add(1, Ordering::Relaxed);
                // Personalized match_found views: each player sees themselves first.
                for (you, opp) in [(&a, &b), (&b, &a)] {
                    st.hub.send(
                        &you.ticket.player_id,
                        "match_found",
                        json!({
                            "matchId": m.id.to_string(),
                            "gameId": m.game_id,
                            "rulesetId": m.ruleset_id,
                            "bot": false,
                            "opponent": {
                                "wallet": opp.ticket.player_id,
                                "rating": opp.ticket.mmr,
                                "region": opp.ticket.region,
                            },
                            "yourRating": you.ticket.mmr,
                            "expiresAt": m.expires_at.to_rfc3339(),
                        }),
                    );
                }
            }
            Err(e) => {
                // Match creation failed (DB) — put both tickets back so the
                // players aren't silently dropped; they'll pair next tick.
                tracing::error!(
                    error = format!("{e:#}"),
                    "match creation failed; re-queuing pair"
                );
                st.queue.join(a);
                st.queue.join(b);
            }
        }
    }

    // Cold-start valve: entries waiting past the bot threshold get a house
    // practice opponent so nobody bounces off an empty lobby.
    if st.cfg.bot_fill_enabled {
        let house = st
            .cfg
            .house_wallet
            .clone()
            .map(|w| w.to_lowercase())
            .or_else(|| {
                st.verifier
                    .as_ref()
                    .map(|v| format!("{:#x}", v.address()).to_lowercase())
            });
        if let Some(house) = house {
            for entry in st.queue.take_stale(st.cfg.bot_after_ms) {
                let (game_id, ruleset_id) = (
                    entry.ticket.game_id.clone(),
                    entry.ticket.ruleset_id.clone(),
                );
                let mut house_entry = crate::queue::QueueEntry {
                    ticket_id: uuid::Uuid::new_v4(),
                    stake_wei: 0,
                    canonical_ruleset: entry.canonical_ruleset.clone(),
                    ticket: entry.ticket.clone(),
                };
                house_entry.ticket.player_id = house.clone();
                match st
                    .matches
                    .create_match(&game_id, &ruleset_id, &entry, &house_entry, true)
                    .await
                {
                    Ok(m) => {
                        st.live_matches.fetch_add(1, Ordering::Relaxed);
                        st.hub.send(
                            &entry.ticket.player_id,
                            "match_found",
                            json!({
                                "matchId": m.id.to_string(),
                                "gameId": m.game_id,
                                "rulesetId": m.ruleset_id,
                                "bot": true,
                                "opponent": {
                                    "wallet": house,
                                    "rating": entry.ticket.mmr,
                                    "region": "house",
                                },
                                "yourRating": entry.ticket.mmr,
                                "expiresAt": m.expires_at.to_rfc3339(),
                            }),
                        );
                        // The durable ticket became a bot match: close it out.
                        if let Some(t) = st.store.active_ticket(&entry.ticket.player_id).await? {
                            st.store.cancel_ticket(t.id, &t.wallet).await?;
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            error = format!("{e:#}"),
                            "bot match creation failed; re-queuing"
                        );
                        st.queue.join(entry);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Reconcile expired live matches, lapsed escrow windows, and RT fallbacks.
async fn sweep_once(st: &AppState) -> anyhow::Result<()> {
    // Escrow windows that closed without both players funding: cancel the
    // match row. Any one-sided on-chain funding is refundable by the player
    // via registry cancelMatch/expireMatch — the server never touches it.
    for m in st.store.expired_escrow_matches().await? {
        st.store.set_match_state(m.id, "cancelled").await?;
        for w in [&m.player_a, &m.player_b] {
            st.hub.send(
                w,
                "match_update",
                json!({ "matchId": m.id.to_string(), "state": "cancelled", "reason": "escrow window lapsed" }),
            );
        }
    }

    // Direct-RT windows that lapsed: fall back to the verifier-attested
    // relayer settlement using the attestation stored at agreement time.
    for m in st.store.rt_overdue_matches().await? {
        if m.stake_wei == 0 || m.on_chain_match_id.is_none() {
            st.store.set_match_state(m.id, "agreed").await?;
            continue;
        }
        let Some(att) = m.attestation.as_ref() else {
            st.store.set_match_state(m.id, "agreed").await?;
            continue;
        };
        let payload = serde_json::json!({
            "matchUuid": m.id.to_string(),
            "onChainMatchId": att["matchId"],
            "outcomeCode": att["outcomeCode"],
            "transcriptHash": att["transcriptHash"],
            "signature": att["signature"],
        });
        if st.store.insert_settle_job(payload).await.is_ok() {
            st.store.set_match_state(m.id, "settling").await?;
            tracing::info!(match_id = %m.id, "RT window lapsed — relayer fallback enqueued");
        }
    }

    live_sweep(st).await
}

async fn live_sweep(st: &AppState) -> anyhow::Result<()> {
    for m in st.store.expired_live_matches().await? {
        let reports = st.store.get_reports(m.id).await?;
        match reports.len() {
            0 => {
                st.store.set_match_state(m.id, "cancelled").await?;
                st.hub.send(&m.player_a, "match_update", json!({ "matchId": m.id.to_string(), "state": "cancelled", "reason": "expired" }));
                st.hub.send(&m.player_b, "match_update", json!({ "matchId": m.id.to_string(), "state": "cancelled", "reason": "expired" }));
                st.live_matches.fetch_sub(1, Ordering::Relaxed);
            }
            1 => {
                // Opponent silent past expiry → reporter's result stands.
                let reporter = &reports[0].wallet;
                if let Some(outcome) =
                    crate::matchsvc::reconcile_default(reporter, &m.player_a, &m.player_b, &reports)
                {
                    match crate::http::finalize_match(
                        st,
                        &m,
                        outcome,
                        reports[0].transcript_hash.as_deref(),
                        &reports,
                    )
                    .await
                    {
                        Ok(applied) => crate::http::notify_result(st, &m, &applied),
                        Err(e) => {
                            tracing::error!(error = format!("{e:#}"), "default settlement failed");
                            continue;
                        }
                    }
                }
            }
            _ => {
                // Two reports that never reconciled → dispute for arbitration.
                st.store.set_match_state(m.id, "disputed").await?;
                st.hub.send(
                    &m.player_a,
                    "match_update",
                    json!({ "matchId": m.id.to_string(), "state": "disputed" }),
                );
                st.hub.send(
                    &m.player_b,
                    "match_update",
                    json!({ "matchId": m.id.to_string(), "state": "disputed" }),
                );
            }
        }
    }
    Ok(())
}
