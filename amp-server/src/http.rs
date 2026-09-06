//! HTTP + WS API. Endpoint design follows the player journey end to end:
//! look (games) → login (one signature) → queue (live status) → play
//! (assignment push) → report (one call) → result (rating + attestation).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use alloy_primitives::Address;
use alloy_signer_local::PrivateKeySigner;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::{AuthService, Authed};
use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::matchsvc::{MatchService, Outcome};
use crate::queue::{QueueEntry, QueueService};
use crate::store::{Store, TicketRow};
use crate::ws::WsHub;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub queue: Arc<QueueService>,
    pub matches: Arc<MatchService>,
    pub hub: Arc<WsHub>,
    pub auth: Arc<AuthService>,
    pub verifier: Option<Arc<PrivateKeySigner>>,
    pub settlement: Option<Address>,
    pub live_matches: Arc<AtomicUsize>,
}

impl axum::extract::FromRef<AppState> for AuthService {
    fn from_ref(state: &AppState) -> Self {
        (*state.auth).clone()
    }
}

// ---- public catalog ---------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct RulesetDef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct GameDef {
    pub id: String,
    pub name: String,
    pub rulesets: Vec<RulesetDef>,
}

pub fn default_games() -> Vec<GameDef> {
    vec![GameDef {
        id: "amp-tactics".into(),
        name: "AMP Tactics (demo duel)".into(),
        rulesets: vec![RulesetDef {
            id: "ranked-1v1".into(),
            name: "Ranked 1v1 — free".into(),
        }],
    }]
}

pub fn load_games() -> Vec<GameDef> {
    std::env::var("AMP_GAMES_JSON")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|g: &Vec<GameDef>| !g.is_empty())
        .unwrap_or_else(default_games)
}

// ---- router -------------------------------------------------------------------

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/auth/challenge", post(auth_challenge))
        .route("/v1/auth/verify", post(auth_verify))
        .route("/v1/games", get(list_games))
        .route("/v1/me", get(me))
        .route("/v1/queue/join", post(queue_join))
        .route("/v1/queue/leave", post(queue_leave))
        .route("/v1/queue/status", get(queue_status))
        .route("/v1/queue/play-bot", post(play_bot_now))
        .route("/v1/matches/{id}/report", post(report_outcome))
        .route("/v1/matches/{id}", get(get_match))
        .route("/v1/matches/history", get(history))
        .route("/v1/admin/matches/{id}/arbitrate", post(admin_arbitrate))
        .route("/v1/matches/{id}/escrow/verify", post(escrow_verify))
        .route("/v1/players/{wallet}", get(player_profile))
        .route("/v1/parties", post(create_party))
        .route("/v1/parties/{id}", get(get_party))
        .route("/v1/parties/join", post(join_party))
        .route("/v1/parties/{id}/lock", post(lock_party))
        .route("/v1/parties/{id}/disband", post(disband_party))
        .route("/v1/multi/commit", post(multi_commit))
        .route("/v1/multi/reveal", post(multi_reveal))
        .route("/v1/multi/{id}", get(get_multi_match))
        .route("/v1/multi/{id}/report", post(multi_report))
        .route("/v1/multi/{id}/claim", post(multi_claim))
        .route("/v1/ws", get(ws_upgrade))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "amp-server" }))
}

// ---- auth ----------------------------------------------------------------------

#[derive(Deserialize)]
struct ChallengeReq {
    wallet: String,
}

async fn auth_challenge(
    State(st): State<AppState>,
    Json(req): Json<ChallengeReq>,
) -> ApiResult<Json<Value>> {
    let (challenge, expires_at) = st.auth.create_challenge(&req.wallet).await?;
    Ok(Json(
        json!({ "challenge": challenge, "expiresAt": expires_at.to_rfc3339() }),
    ))
}

#[derive(Deserialize)]
struct VerifyReq {
    wallet: String,
    /// 65-byte hex EIP-191 signature over the challenge string.
    signature: String,
    challenge: String,
    region: Option<String>,
}

async fn auth_verify(
    State(st): State<AppState>,
    Json(req): Json<VerifyReq>,
) -> ApiResult<Json<Value>> {
    let (token, expires_at, player) = st
        .auth
        .verify_login(
            &req.wallet,
            &req.signature,
            &req.challenge,
            req.region.as_deref(),
        )
        .await?;
    Ok(Json(json!({
        "token": token,
        "expiresAt": expires_at.to_rfc3339(),
        "player": player,
    })))
}

// ---- games / me ------------------------------------------------------------------

async fn list_games(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    let games = load_games();
    let mut out = Vec::with_capacity(games.len());
    for g in &games {
        let mut rulesets = Vec::with_capacity(g.rulesets.len());
        for r in &g.rulesets {
            let depth = st
                .store
                .queue_depth(&g.id, &r.id)
                .await
                .map_err(ApiError::Database)?;
            rulesets.push(json!({ "id": r.id, "name": r.name, "queueDepth": depth }));
        }
        out.push(json!({
            "id": g.id,
            "name": g.name,
            "rulesets": rulesets,
            "nextQueueWindowUtc": next_queue_window(&st.cfg, &g.id),
        }));
    }
    Ok(Json(json!({
        "games": out,
        "stakingEnabled": st.cfg.staking_enabled,
        "chainId": st.cfg.chain_id,
        "registryAddress": st.cfg.registry_address,
        "registryGameId": st.cfg.registry_game_id,
    })))
}

/// Next scheduled prime-time window (RFC3339) for a game, if configured.
/// Windows concentrate concurrent players — the honest fix for empty-lobby
/// matchmaking.
fn next_queue_window(cfg: &Config, game_id: &str) -> Option<String> {
    let windows = cfg.queue_windows.iter().find(|w| w.game_id == game_id)?;
    let now = chrono::Utc::now();
    let today = now.date_naive();
    let mut candidates: Vec<i64> = Vec::new();
    for day in 0..=2i64 {
        for t in &windows.times_utc {
            let Some((h, m)) = t.split_once(':') else {
                continue;
            };
            let (Ok(h), Ok(mi)): (Result<u32, _>, Result<u32, _>) = (h.parse(), m.parse()) else {
                continue;
            };
            if let Some(dt) = today
                .and_hms_opt(h, mi, 0)
                .and_then(|nd| nd.and_local_timezone(chrono::Utc).single())
            {
                candidates.push(dt.timestamp() + day * 86_400);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|ts| *ts > now.timestamp())
        .min()
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt| dt.to_rfc3339())
}

async fn me(State(st): State<AppState>, Authed(wallet): Authed) -> ApiResult<Json<Value>> {
    let ratings = sqlx::query(
        "SELECT game_id, ruleset_id, rating, rating_deviation, wins, losses, draws FROM amp_ratings WHERE wallet = $1",
    )
    .bind(&wallet)
    .fetch_all(st.store.pool())
    .await
    .map_err(ApiError::Database)?;

    let ratings: Vec<Value> = ratings
        .iter()
        .map(|r| {
            json!({
                "gameId": r.get::<String, _>("game_id"),
                "rulesetId": r.get::<String, _>("ruleset_id"),
                "rating": r.get::<f64, _>("rating"),
                "deviation": r.get::<f64, _>("rating_deviation"),
                "wins": r.get::<i64, _>("wins"),
                "losses": r.get::<i64, _>("losses"),
                "draws": r.get::<i64, _>("draws"),
            })
        })
        .collect();

    let live = st
        .store
        .live_match_for(&wallet)
        .await
        .map_err(ApiError::Database)?;
    let ticket = st
        .store
        .active_ticket(&wallet)
        .await
        .map_err(ApiError::Database)?;

    Ok(Json(json!({
        "wallet": wallet,
        "ratings": ratings,
        "liveMatchId": live.map(|m| m.id.to_string()),
        "queueTicket": ticket.map(|t| json!({
            "ticketId": t.id.to_string(),
            "gameId": t.game_id,
            "rulesetId": t.ruleset_id,
            "joinedAt": t.joined_at.to_rfc3339(),
        })),
    })))
}

// ---- queue -----------------------------------------------------------------------

#[derive(Deserialize)]
struct JoinReq {
    game_id: Option<String>,
    ruleset_id: Option<String>,
    #[serde(rename = "gameId")]
    game_id_c: Option<String>,
    #[serde(rename = "rulesetId")]
    ruleset_id_c: Option<String>,
    region: Option<String>,
    #[serde(rename = "stakeWei", default)]
    stake_wei: i64,
    /// Unix seconds — the signed intent's expiry.
    #[serde(rename = "intentDeadline")]
    intent_deadline: Option<i64>,
    /// EIP-712 signature over MatchIntent (65-byte hex). Required for staked joins.
    #[serde(rename = "intentSig")]
    intent_sig: Option<String>,
}

async fn queue_join(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Json(req): Json<JoinReq>,
) -> ApiResult<Json<Value>> {
    let game_id = req
        .game_id
        .or(req.game_id_c)
        .unwrap_or_else(|| "amp-tactics".into());
    let ruleset_id = req
        .ruleset_id
        .or(req.ruleset_id_c)
        .unwrap_or_else(|| "ranked-1v1".into());

    // Validate against the catalog so typos can't strand a ticket in a
    // bucket no one else can ever join.
    let games = load_games();
    let valid = games
        .iter()
        .any(|g| g.id == game_id && g.rulesets.iter().any(|r| r.id == ruleset_id));
    if !valid {
        return Err(ApiError::BadRequest(format!(
            "unknown game/ruleset: {game_id}/{ruleset_id}"
        )));
    }

    if req.stake_wei > 0 && !st.cfg.staking_enabled {
        return Err(ApiError::StakingDisabled);
    }

    // Staked joins require a signed EIP-712 MatchIntent — the gasless stake
    // commitment. Verify recovery against the joining wallet and enforce the
    // deadline window (no replays of stale intents).
    let mut intent_deadline: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut intent_sig: Option<String> = None;
    if req.stake_wei > 0 {
        let settlement = st.settlement.ok_or_else(|| {
            ApiError::BadRequest("staking requires AMP_SETTLEMENT_ADDRESS".into())
        })?;
        let deadline = req
            .intent_deadline
            .ok_or_else(|| ApiError::BadRequest("staked joins require intentDeadline".into()))?;
        let sig = req
            .intent_sig
            .clone()
            .ok_or_else(|| ApiError::BadRequest("staked joins require intentSig".into()))?;
        let player: alloy_primitives::Address = wallet
            .parse()
            .map_err(|_| ApiError::BadRequest("bad wallet".into()))?;
        let now = chrono::Utc::now().timestamp();
        if deadline <= now {
            return Err(ApiError::BadRequest("intent expired".into()));
        }
        if deadline > now + 86_400 {
            return Err(ApiError::BadRequest(
                "intent deadline too far in the future".into(),
            ));
        }
        let intent = crate::intent::MatchIntent {
            player,
            game_id: &game_id,
            ruleset_id: &ruleset_id,
            stake_wei: req.stake_wei as u64,
            deadline: deadline as u64,
        };
        let recovered =
            crate::intent::recover_intent_signer(st.cfg.chain_id, settlement, &intent, &sig)
                .map_err(|e| ApiError::BadRequest(format!("bad intent signature: {e}")))?;
        if format!("{recovered:#x}").to_lowercase() != wallet {
            return Err(ApiError::BadRequest(
                "intent signature does not match wallet".into(),
            ));
        }
        intent_deadline = Some(chrono::DateTime::from_timestamp(deadline, 0).unwrap_or_default());
        intent_sig = Some(sig);
    }

    // One live match at a time — finish (or report) before re-queuing.
    if st
        .store
        .live_match_for(&wallet)
        .await
        .map_err(ApiError::Database)?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "you already have a live match; report its outcome first".into(),
        ));
    }

    // Idempotent UX: already queued → return the existing ticket.
    if let Some(t) = st
        .store
        .active_ticket(&wallet)
        .await
        .map_err(ApiError::Database)?
    {
        let (depth, waited, window) = match st.queue.status_for(&wallet) {
            Some(q) => (q.depth, q.waited_ms, q.skill_window),
            None => (1, 0, st.cfg.skill_window_base),
        };
        return Ok(Json(json!({
            "ticketId": t.id.to_string(),
            "alreadyQueued": true,
            "queueDepth": depth,
            "waitedMs": waited,
            "skillWindow": window,
        })));
    }

    let rating = st
        .store
        .get_rating(&wallet, &game_id, &ruleset_id)
        .await
        .map_err(ApiError::Database)?;
    let region = req.region.unwrap_or_else(|| "na".into());
    let ticket_id = Uuid::new_v4();
    let joined_at = chrono::Utc::now();
    let joined_ms = joined_at.timestamp_millis().max(0) as u64;

    st.store
        .insert_ticket(&TicketRow {
            id: ticket_id,
            wallet: wallet.clone(),
            game_id: game_id.clone(),
            ruleset_id: ruleset_id.clone(),
            stake_wei: req.stake_wei,
            region: region.clone(),
            status: "queued".into(),
            match_id: None,
            joined_at,
            intent_deadline,
            intent_sig,
        })
        .await
        .map_err(ApiError::Database)?;

    let canonical_ruleset = ruleset_id.clone();
    st.queue.join(QueueEntry {
        ticket_id,
        stake_wei: req.stake_wei,
        canonical_ruleset,
        ticket: amp_match_core::PlayerTicket {
            player_id: wallet.clone(),
            game_id,
            ruleset_id,
            mmr: rating.rating as f32,
            mmr_uncertainty: rating.rating_deviation as f32,
            region,
            preferred_role: String::new(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: joined_ms,
            party_size: 1,
        },
    });

    st.hub.send(
        &wallet,
        "queue_status",
        json!({ "depth": st.queue.depth(), "waitedMs": 0 }),
    );

    Ok(Json(json!({
        "ticketId": ticket_id.to_string(),
        "alreadyQueued": false,
        "queueDepth": st.queue.depth(),
        "waitedMs": 0,
        "skillWindow": st.cfg.skill_window_base,
        "rating": rating.rating,
    })))
}

async fn queue_leave(State(st): State<AppState>, Authed(wallet): Authed) -> ApiResult<Json<Value>> {
    // Cancel every queued DB ticket for this wallet; the in-memory side is
    // keyed by player so one leave covers it.
    if let Some(t) = st
        .store
        .active_ticket(&wallet)
        .await
        .map_err(ApiError::Database)?
    {
        st.store
            .cancel_ticket(t.id, &wallet)
            .await
            .map_err(ApiError::Database)?;
    }
    let left = st.queue.leave(&wallet);
    Ok(Json(json!({ "left": left })))
}

async fn queue_status(
    State(st): State<AppState>,
    Authed(wallet): Authed,
) -> ApiResult<Json<Value>> {
    match st.queue.status_for(&wallet) {
        Some(q) => Ok(Json(json!({
            "queued": true,
            "depth": q.depth,
            "waitedMs": q.waited_ms,
            "skillWindow": q.skill_window,
        }))),
        None => Ok(Json(json!({ "queued": false }))),
    }
}

// ---- play-bot-now ----------------------------------------------------------------------

/// Skip the queue wait and play a bot immediately. Leaves the queue if
/// queued, creates a practice match, sends the match_found WS event.
async fn play_bot_now(
    State(st): State<AppState>,
    Authed(wallet): Authed,
) -> ApiResult<Json<Value>> {
    // Leave the queue if queued.
    if let Some(t) = st
        .store
        .active_ticket(&wallet)
        .await
        .map_err(ApiError::Database)?
    {
        st.store
            .cancel_ticket(t.id, &wallet)
            .await
            .map_err(ApiError::Database)?;
    }
    st.queue.leave(&wallet);

    // Build the player's entry from their rating.
    let rating = st
        .store
        .get_rating(&wallet, "amp-tactics", "ranked-1v1")
        .await
        .map_err(ApiError::Database)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let entry = crate::queue::QueueEntry {
        ticket_id: uuid::Uuid::new_v4(),
        stake_wei: 0,
        canonical_ruleset: "ranked-1v1".into(),
        ticket: amp_match_core::PlayerTicket {
            player_id: wallet.clone(),
            game_id: "amp-tactics".into(),
            ruleset_id: "ranked-1v1".into(),
            mmr: rating.rating as f32,
            mmr_uncertainty: rating.rating_deviation as f32,
            region: "na".into(),
            preferred_role: String::new(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: now_ms,
            party_size: 1,
        },
    };

    // Build the house bot entry at a similar rating.
    let mut house = crate::queue::QueueEntry {
        ticket_id: uuid::Uuid::new_v4(),
        stake_wei: 0,
        canonical_ruleset: "ranked-1v1".into(),
        ticket: entry.ticket.clone(),
    };
    house.ticket.player_id = st
        .cfg
        .house_wallet
        .clone()
        .map(|w| w.to_lowercase())
        .or_else(|| {
            st.verifier
                .as_ref()
                .map(|v| format!("{:#x}", v.address()).to_lowercase())
        })
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());

    let m = st
        .matches
        .create_match("amp-tactics", "ranked-1v1", &entry, &house, true)
        .await?;
    st.live_matches
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Send the match_found event with bot flag.
    let bot_wallet = house.ticket.player_id.clone();
    st.hub.send(
        &wallet,
        "match_found",
        serde_json::json!({
            "matchId": m.id.to_string(),
            "gameId": m.game_id,
            "rulesetId": m.ruleset_id,
            "bot": true,
            "opponent": {
                "wallet": bot_wallet,
                "rating": entry.ticket.mmr,
                "region": "house",
            },
            "yourRating": entry.ticket.mmr,
            "expiresAt": m.expires_at.to_rfc3339(),
        }),
    );

    Ok(Json(json!({
        "matchId": m.id.to_string(),
        "bot": true,
        "message": "playing the house bot",
    })))
}

// ---- matches --------------------------------------------------------------------

#[derive(Deserialize)]
struct ReportReq {
    /// Reporter-relative: "win" | "loss" | "draw".
    result: String,
    transcript_hash: Option<String>,
    /// EIP-191 signature (65-byte hex) over
    /// "AMP_REPORT:v1:{matchId}:{result}". Optional for free matches;
    /// REQUIRED when stakes are on the line — it is the non-repudiable
    /// evidence that lets the match settle on-chain without any operator.
    signature: Option<String>,
}

async fn report_outcome(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<ReportReq>,
) -> ApiResult<Json<Value>> {
    if !matches!(req.result.as_str(), "win" | "loss" | "draw") {
        return Err(ApiError::BadRequest(
            "result must be win, loss, or draw".into(),
        ));
    }

    let m = st
        .store
        .get_match(id)
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if m.player_a != wallet && m.player_b != wallet {
        return Err(ApiError::Forbidden("not a participant".into()));
    }
    if m.state != "live" {
        return Err(ApiError::Conflict(format!(
            "match is {}, not live",
            m.state
        )));
    }

    // Signed-report evidence: verify recovery against the reporting wallet.
    // Staked matches refuse unsigned reports outright.
    if let Some(sig) = req.signature.as_deref() {
        let msg = crate::matchsvc::report_message(&id.to_string(), &req.result);
        let recovered = crate::auth::recover_eip191(msg.as_bytes(), sig)
            .map_err(|e| ApiError::BadRequest(format!("bad report signature: {e}")))?;
        if format!("{recovered:#x}").to_lowercase() != wallet {
            return Err(ApiError::BadRequest(
                "report signature does not match your wallet".into(),
            ));
        }
    } else if m.stake_wei > 0 {
        return Err(ApiError::BadRequest(
            "staked matches require a signed report (sign AMP_REPORT:v1:{matchId}:{result})".into(),
        ));
    }

    let inserted = st
        .store
        .insert_report(
            id,
            &wallet,
            &req.result,
            req.transcript_hash.as_deref(),
            req.signature.as_deref(),
        )
        .await
        .map_err(ApiError::Database)?;
    if !inserted {
        return Ok(Json(json!({
            "matchId": id.to_string(),
            "state": "live",
            "note": "already reported; waiting for opponent",
        })));
    }

    // Practice-bot matches settle instantly from the single player report.
    if m.bot_match.unwrap_or(false) {
        let applied = st
            .matches
            .finalize_bot_match(&m, &wallet, &req.result)
            .await?;
        st.live_matches.fetch_sub(1, Ordering::Relaxed);
        notify_result(&st, &m, &applied);
        return Ok(Json(json!({
            "matchId": id.to_string(),
            "state": "agreed",
            "outcome": applied.outcome,
            "bot": true,
        })));
    }

    let reports = st.store.get_reports(id).await.map_err(ApiError::Database)?;
    if reports.len() < 2 {
        st.hub.send(
            &wallet,
            "match_update",
            json!({ "matchId": id.to_string(), "state": "reported", "waitingFor": "opponent" }),
        );
        return Ok(Json(
            json!({ "matchId": id.to_string(), "state": "live", "reported": true, "waitingFor": "opponent" }),
        ));
    }

    match crate::matchsvc::reconcile(&m.player_a, &m.player_b, &reports) {
        Some(outcome) => {
            let applied =
                finalize_match(&st, &m, outcome, req.transcript_hash.as_deref(), &reports).await?;
            notify_result(&st, &m, &applied);
            Ok(Json(
                json!({ "matchId": id.to_string(), "state": "agreed", "outcome": applied.outcome }),
            ))
        }
        None => {
            st.store
                .set_match_state(id, "disputed")
                .await
                .map_err(ApiError::Database)?;
            st.hub.send(
                &m.player_a,
                "match_update",
                json!({ "matchId": id.to_string(), "state": "disputed" }),
            );
            st.hub.send(
                &m.player_b,
                "match_update",
                json!({ "matchId": id.to_string(), "state": "disputed" }),
            );
            Ok(Json(
                json!({ "matchId": id.to_string(), "state": "disputed" }),
            ))
        }
    }
}

pub async fn finalize_match(
    st: &AppState,
    m: &crate::store::MatchRow,
    outcome: Outcome,
    transcript_hash: Option<&str>,
    reports: &[crate::store::ReportRow],
) -> ApiResult<crate::matchsvc::AppliedOutcome> {
    let applied = st
        .matches
        .apply_outcome(
            m,
            outcome,
            transcript_hash,
            st.verifier.as_deref(),
            st.cfg.chain_id,
            st.settlement,
            reports,
        )
        .await?;
    st.live_matches.fetch_sub(1, Ordering::Relaxed);
    Ok(applied)
}

/// Push a personalized result view to each player: their delta first.
pub fn notify_result(
    st: &AppState,
    m: &crate::store::MatchRow,
    applied: &crate::matchsvc::AppliedOutcome,
) {
    for (wallet, you, opp) in [
        (&m.player_a, &applied.player_a, &applied.player_b),
        (&m.player_b, &applied.player_b, &applied.player_a),
    ] {
        st.hub.send(
            wallet,
            "match_result",
            json!({
                "matchId": m.id.to_string(),
                "outcome": applied.outcome,
                "won": applied.winner.as_deref() == Some(wallet.as_str()),
                "you": { "ratingBefore": you.rating_before, "ratingAfter": you.rating_after, "deviationAfter": you.deviation_after },
                "opponent": { "ratingBefore": opp.rating_before, "ratingAfter": opp.rating_after },
                "attestation": applied.attestation,
            }),
        );
    }
}

async fn get_match(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let m = st
        .store
        .get_match(id)
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if m.player_a != wallet && m.player_b != wallet {
        return Err(ApiError::Forbidden("not a participant".into()));
    }
    Ok(Json(match_view(&m, &wallet)))
}

pub fn match_view(m: &crate::store::MatchRow, viewer: &str) -> Value {
    let you_a = m.player_a == viewer;
    let (you, opponent) = if you_a {
        (&m.player_a, &m.player_b)
    } else {
        (&m.player_b, &m.player_a)
    };
    json!({
        "matchId": m.id.to_string(),
        "gameId": m.game_id,
        "rulesetId": m.ruleset_id,
        "state": m.state,
        "stakeWei": m.stake_wei,
        "bot": m.bot_match.unwrap_or(false),
        "you": { "wallet": you, "ratingSnapshot": if you_a { &m.rating_a_snapshot } else { &m.rating_b_snapshot } },
        "opponent": { "wallet": opponent, "ratingSnapshot": if you_a { &m.rating_b_snapshot } else { &m.rating_a_snapshot } },
        "outcome": m.outcome,
        "winner": m.winner,
        "attestation": m.attestation,
        "onChainMatchId": m.on_chain_match_id,
        "settleDeadline": m.settle_deadline.as_ref().map(|d| d.to_rfc3339()),
        "expiresAt": m.expires_at.to_rfc3339(),
    })
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn history(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = st
        .store
        .history(&wallet, limit, offset)
        .await
        .map_err(ApiError::Database)?;
    let items: Vec<Value> = rows.iter().map(|m| match_view(m, &wallet)).collect();
    Ok(Json(json!({ "matches": items })))
}

// ---- admin ------------------------------------------------------------------------

#[derive(Deserialize)]
struct ArbitrateReq {
    /// "win_a" | "win_b" | "draw" | "cancelled"
    outcome: String,
}

async fn admin_arbitrate(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ArbitrateReq>,
) -> ApiResult<Json<Value>> {
    let token = st
        .cfg
        .admin_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| ApiError::Forbidden("admin API disabled".into()))?;
    let provided = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    // Constant-time compare — tokens are secrets.
    if !constant_time_eq(token.as_bytes(), provided.as_bytes()) {
        return Err(ApiError::Forbidden("bad admin token".into()));
    }

    let m = st
        .store
        .get_match(id)
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;

    match req.outcome.as_str() {
        "win_a" | "win_b" | "draw" => {
            let outcome = match req.outcome.as_str() {
                "win_a" => Outcome::WinA,
                "win_b" => Outcome::WinB,
                _ => Outcome::Draw,
            };
            let applied = finalize_match(&st, &m, outcome, None, &[]).await?;
            notify_result(&st, &m, &applied);
            Ok(Json(
                json!({ "matchId": id.to_string(), "state": "agreed", "outcome": applied.outcome }),
            ))
        }
        "cancelled" => {
            st.store
                .set_match_outcome(id, "cancelled", "cancelled", None)
                .await
                .map_err(ApiError::Database)?;
            st.live_matches.fetch_sub(1, Ordering::Relaxed);
            st.hub.send(
                &m.player_a,
                "match_update",
                json!({ "matchId": id.to_string(), "state": "cancelled" }),
            );
            st.hub.send(
                &m.player_b,
                "match_update",
                json!({ "matchId": id.to_string(), "state": "cancelled" }),
            );
            Ok(Json(
                json!({ "matchId": id.to_string(), "state": "cancelled" }),
            ))
        }
        other => Err(ApiError::BadRequest(format!(
            "outcome must be win_a, win_b, draw, or cancelled; got {other}"
        ))),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---- escrow ------------------------------------------------------------------------

/// Confirm on-chain escrow for a staked match: both players funded the
/// registry, the match is READY, and the wallets match. Only then does the
/// server flip the match to live. Players run createMatch/joinMatch from
/// their own wallets — the server verifies, never custodies.
async fn escrow_verify(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let m = st
        .store
        .get_match(id)
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if m.player_a != wallet && m.player_b != wallet {
        return Err(ApiError::Forbidden("not a participant".into()));
    }
    if m.state != "escrow_pending" {
        return Err(ApiError::Conflict(format!(
            "match is {}, not escrow_pending",
            m.state
        )));
    }
    let registry: alloy_primitives::Address = st
        .cfg
        .registry_address
        .as_deref()
        .and_then(|a| a.parse().ok())
        .ok_or_else(|| {
            ApiError::BadRequest("escrow not configured (AMP_REGISTRY_ADDRESS)".into())
        })?;
    let on_chain_id = m
        .on_chain_match_id
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("staked match without on-chain id")))?
        as u64;

    let oc = crate::escrow::read_match(&st.cfg.rpc_url, registry, on_chain_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("chain read failed: {e}")))?;
    let oc =
        oc.ok_or_else(|| ApiError::BadRequest("no on-chain match at the expected id".into()))?;
    if oc.state != crate::escrow::STATE_READY {
        return Ok(Json(json!({
            "matchId": id.to_string(),
            "escrowState": "waiting",
            "onChainState": oc.state,
            "note": "opponent has not funded yet",
        })));
    }
    if format!("{:#x}", oc.player_a).to_lowercase() != m.player_a
        || format!("{:#x}", oc.player_b).to_lowercase() != m.player_b
    {
        return Err(ApiError::BadRequest(
            "on-chain escrow players do not match this match".into(),
        ));
    }

    let confirmed = st
        .store
        .confirm_escrow(id, st.cfg.match_ttl_minutes)
        .await
        .map_err(ApiError::Database)?;
    if confirmed {
        st.live_matches.fetch_add(1, Ordering::Relaxed);
        for w in [&m.player_a, &m.player_b] {
            st.hub.send(
                w,
                "match_update",
                json!({ "matchId": id.to_string(), "state": "live", "escrow": "confirmed" }),
            );
        }
    }
    Ok(Json(json!({
        "matchId": id.to_string(),
        "escrowState": if confirmed { "confirmed" } else { "already" },
        "stakeWei": m.stake_wei,
    })))
}

// ---- public profile ------------------------------------------------------------------

/// Sovereign, wallet-keyed cross-game MMR: public ratings + recent matches
/// for any wallet. No auth — this is the portable skill record.
async fn player_profile(
    State(st): State<AppState>,
    Path(wallet): Path<String>,
) -> ApiResult<Json<Value>> {
    let wallet = crate::auth::normalize_wallet(&wallet)?;
    let ratings = sqlx::query(
        "SELECT game_id, ruleset_id, rating, rating_deviation, wins, losses, draws, updated_at \
         FROM amp_ratings WHERE wallet = $1 ORDER BY updated_at DESC",
    )
    .bind(&wallet)
    .fetch_all(st.store.pool())
    .await
    .map_err(ApiError::Database)?;
    let ratings: Vec<Value> = ratings
        .iter()
        .map(|r| {
            json!({
                "gameId": r.get::<String, _>("game_id"),
                "rulesetId": r.get::<String, _>("ruleset_id"),
                "rating": r.get::<f64, _>("rating"),
                "deviation": r.get::<f64, _>("rating_deviation"),
                "wins": r.get::<i64, _>("wins"),
                "losses": r.get::<i64, _>("losses"),
                "draws": r.get::<i64, _>("draws"),
            })
        })
        .collect();
    let rows = st
        .store
        .history(&wallet, 20, 0)
        .await
        .map_err(ApiError::Database)?;
    let matches: Vec<Value> = rows.iter().map(|m| match_view(m, &wallet)).collect();
    Ok(Json(
        json!({ "wallet": wallet, "ratings": ratings, "matches": matches }),
    ))
}

// ---- parties ------------------------------------------------------------------------

async fn create_party(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Json(req): Json<crate::party::CreatePartyReq>,
) -> ApiResult<Json<Value>> {
    let party = st
        .store
        .create_party(&wallet, &req.game_id, &req.ruleset_id)
        .await?;
    Ok(Json(json!({
        "partyId": party.id.to_string(),
        "inviteCode": party.invite_code,
        "leader": party.leader,
        "state": party.state,
    })))
}

async fn get_party(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let party = st
        .store
        .get_party(id)
        .await?
        .ok_or_else(|| ApiError::NotFound("party not found".into()))?;
    let is_member = party.members.iter().any(|m| m.wallet == wallet);
    if !is_member && party.leader != wallet {
        return Err(ApiError::Forbidden("not a party member".into()));
    }
    Ok(Json(json!({
        "partyId": party.id.to_string(),
        "leader": party.leader,
        "members": party.members,
        "state": party.state,
        "inviteCode": party.invite_code,
        "gameId": party.game_id,
        "rulesetId": party.ruleset_id,
    })))
}

async fn join_party(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Json(req): Json<crate::party::JoinPartyReq>,
) -> ApiResult<Json<Value>> {
    let party = st
        .store
        .join_party(
            &req.invite_code,
            &wallet,
            req.region.as_deref().unwrap_or("na"),
        )
        .await?;
    Ok(Json(json!({
        "partyId": party.id.to_string(),
        "members": party.members.len(),
        "state": party.state,
    })))
}

async fn lock_party(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let party = st
        .store
        .get_party(id)
        .await?
        .ok_or_else(|| ApiError::NotFound("not found".into()))?;
    if party.leader != wallet {
        return Err(ApiError::Forbidden("only the leader can lock".into()));
    }
    let locked = st.store.lock_party(id).await?;
    let msg = crate::party::lock_message(
        &locked.id.to_string(),
        &locked
            .members
            .iter()
            .map(|m| m.wallet.as_str())
            .collect::<Vec<_>>(),
    );
    Ok(Json(json!({
        "partyId": locked.id.to_string(),
        "state": locked.state,
        "lockMessage": msg,
    })))
}

async fn disband_party(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let ok = st.store.disband_party(id, &wallet).await?;
    Ok(Json(json!({ "disbanded": ok })))
}

// ---- multiplayer commit-reveal ---------------------------------------------------------

#[derive(Deserialize)]
struct MultiCommitReq {
    game_id: Option<String>,
    ruleset_id: Option<String>,
    #[serde(rename = "gameId")]
    game_id_c: Option<String>,
    #[serde(rename = "rulesetId")]
    ruleset_id_c: Option<String>,
    #[serde(rename = "commitHash")]
    commit_hash: String,
    #[serde(rename = "stakeWei", default)]
    stake_wei: i64,
    #[serde(rename = "lobbySize", default = "default_lobby")]
    lobby_size: usize,
}

fn default_lobby() -> usize {
    8
}

async fn multi_commit(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Json(req): Json<MultiCommitReq>,
) -> ApiResult<Json<Value>> {
    let game_id = req
        .game_id
        .or(req.game_id_c)
        .unwrap_or_else(|| "amp-tactics".into());
    let ruleset_id = req
        .ruleset_id
        .or(req.ruleset_id_c)
        .unwrap_or_else(|| "ranked-1v1".into());

    sqlx::query(
        r#"INSERT INTO amp_commits (wallet, game_id, ruleset_id, commit_hash, stake_wei)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (wallet, game_id, ruleset_id) DO UPDATE SET
             commit_hash = EXCLUDED.commit_hash, stake_wei = EXCLUDED.stake_wei,
             state = 'committed', salt = NULL, revealed_at = NULL"#,
    )
    .bind(&wallet)
    .bind(&game_id)
    .bind(&ruleset_id)
    .bind(&req.commit_hash)
    .bind(req.stake_wei)
    .execute(st.store.pool())
    .await
    .map_err(ApiError::Database)?;

    // Check how many committed entries exist; if enough for a lobby, signal reveal.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM amp_commits WHERE game_id = $1 AND ruleset_id = $2 AND state = 'committed'",
    )
    .bind(&game_id)
    .bind(&ruleset_id)
    .fetch_one(st.store.pool())
    .await
    .map_err(ApiError::Database)?;

    Ok(Json(json!({
        "committed": true,
        "committedCount": count,
        "lobbySize": req.lobby_size,
        "ready": count >= req.lobby_size as i64,
    })))
}

#[derive(Deserialize)]
struct MultiRevealReq {
    #[serde(rename = "gameId")]
    game_id: String,
    #[serde(rename = "rulesetId")]
    ruleset_id: String,
    salt: String,
}

async fn multi_reveal(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Json(req): Json<MultiRevealReq>,
) -> ApiResult<Json<Value>> {
    // Verify the reveal matches the commit.
    let row = sqlx::query(
        "SELECT commit_hash, stake_wei FROM amp_commits WHERE wallet = $1 AND game_id = $2 AND ruleset_id = $3 AND state = 'committed'",
    )
    .bind(&wallet)
    .bind(&req.game_id)
    .bind(&req.ruleset_id)
    .fetch_optional(st.store.pool())
    .await
    .map_err(ApiError::Database)?
    .ok_or_else(|| ApiError::NotFound("no active commit".into()))?;

    let commit_hash: String = row.get("commit_hash");
    let stake_wei: i64 = row.get("stake_wei");

    if !crate::multiplayer::verify_commit(&commit_hash, &wallet, stake_wei, &req.salt) {
        return Err(ApiError::BadRequest("salt does not match commit".into()));
    }

    sqlx::query(
        "UPDATE amp_commits SET state = 'revealed', salt = $3, revealed_at = now() WHERE wallet = $1 AND game_id = $2 AND ruleset_id = $2",
    )
    .bind(&wallet)
    .bind(&req.game_id)
    .bind(&req.salt)
    .execute(st.store.pool())
    .await
    .map_err(ApiError::Database)?;

    let revealed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM amp_commits WHERE game_id = $1 AND ruleset_id = $2 AND state = 'revealed'",
    )
    .bind(&req.game_id)
    .bind(&req.ruleset_id)
    .fetch_one(st.store.pool())
    .await
    .map_err(ApiError::Database)?;

    Ok(Json(json!({
        "revealed": true,
        "revealedCount": revealed,
    })))
}

async fn get_multi_match(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let m = st
        .store
        .get_multi_match(id)
        .await?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    let is_player = m.players.iter().any(|p| p.wallet == wallet);
    if !is_player {
        return Err(ApiError::Forbidden("not a participant".into()));
    }
    let reports = st.store.get_ladder_reports(id).await?;
    Ok(Json(json!({
        "matchId": m.id.to_string(),
        "state": m.state,
        "lobbySize": m.lobby_size,
        "stakePerPlayer": m.stake_per_player,
        "bondPerPlayer": m.bond_per_player,
        "players": m.players.iter().map(|p| json!({
            "wallet": p.wallet,
            "index": p.index,
            "rating": p.rating,
            "region": p.region,
        })).collect::<Vec<_>>(),
        "ladder": m.ladder,
        "signerCount": m.signer_mask.count_ones(),
        "quorumNeeded": crate::multiplayer::quorum_of(m.lobby_size),
        "reportCount": reports.len(),
        "transcriptHash": m.transcript_hash,
        "quorumUntil": m.quorum_until.map(|q| q.to_rfc3339()),
    })))
}

#[derive(Deserialize)]
struct MultiReportReq {
    ranked: Vec<(String, u16)>,
    #[serde(rename = "transcriptHash")]
    transcript_hash: String,
    #[serde(rename = "sessionNonce")]
    session_nonce: u64,
    signature: String,
}

async fn multi_report(
    State(st): State<AppState>,
    Authed(wallet): Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<MultiReportReq>,
) -> ApiResult<Json<Value>> {
    let m = st
        .store
        .get_multi_match(id)
        .await?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if !m.players.iter().any(|p| p.wallet == wallet) {
        return Err(ApiError::Forbidden("not a participant".into()));
    }
    if m.state != "live" && m.state != "quorum" {
        return Err(ApiError::Conflict(format!("match is {}", m.state)));
    }

    let report = crate::multiplayer::LadderReport {
        wallet: wallet.clone(),
        ranked: req.ranked,
        transcript_hash: req.transcript_hash,
        session_nonce: req.session_nonce,
        signature: req.signature,
    };

    let inserted = st.store.insert_ladder_report(id, &report).await?;
    if !inserted {
        return Ok(Json(
            json!({ "matchId": id.to_string(), "note": "already reported" }),
        ));
    }

    // Check for concordant quorum.
    let k = crate::multiplayer::quorum_of(m.lobby_size);
    if let Some((_th, count, _ladder)) = st.store.concordant_quorum(id).await?
        && count >= k
    {
        st.store.update_multi_state(id, "quorum").await?;
        return Ok(Json(json!({
            "matchId": id.to_string(),
            "state": "quorum",
            "concordant": count,
            "quorumNeeded": k,
        })));
    }

    let reports = st.store.get_ladder_reports(id).await?;
    Ok(Json(json!({
        "matchId": id.to_string(),
        "state": m.state,
        "reported": true,
        "reportCount": reports.len(),
        "quorumNeeded": k,
    })))
}

async fn multi_claim(
    State(st): State<AppState>,
    Authed(_wallet): Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let m = st
        .store
        .get_multi_match(id)
        .await?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if m.state != "quorum" {
        return Err(ApiError::Conflict(format!(
            "match is {}, not quorum",
            m.state
        )));
    }

    let multiplayer_addr: Address = st
        .cfg
        .multiplayer_address
        .as_deref()
        .and_then(|a| a.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("multiplayer address not configured".into()))?;

    let payload = crate::multiplayer::build_settle_multi_job(
        &st.store,
        id,
        st.cfg.chain_id,
        multiplayer_addr,
    )
    .await?;

    // Enqueue the settlement job for the relayer.
    sqlx::query("INSERT INTO relayer_jobs (kind, payload, status) VALUES ('settle_multi', $1::jsonb, 'pending')")
        .bind(payload.to_string())
        .execute(st.store.pool())
        .await
        .map_err(ApiError::Database)?;

    // Apply Glicko-2 rating updates from the settled ladder.
    let ladder: Vec<(String, u16)> = serde_json::from_value(
        payload["rankedPlacements"]
            .as_array()
            .map(|arr| {
                serde_json::Value::Array(
                    arr.iter()
                        .enumerate()
                        .map(|(i, addr)| {
                            serde_json::json!([addr.as_str().unwrap_or(""), (i + 1) as u16])
                        })
                        .collect(),
                )
            })
            .unwrap_or_default(),
    )
    .unwrap_or_default();

    let rating_updates = crate::rating_pipeline::apply_multi_ratings(
        &st.store,
        id,
        &m.game_id,
        &m.ruleset_id,
        &ladder,
        0.7, // γ anti-boost (configurable via env in a future pass)
    )
    .await;

    st.store.update_multi_state(id, "settling").await?;

    // Notify every player with their personalized rating delta.
    match rating_updates {
        Ok(updates) => {
            for u in &updates {
                st.hub.send(
                    &u.wallet,
                    "multi_result",
                    serde_json::json!({
                        "matchId": id.to_string(),
                        "outcome": {
                            "ratingBefore": u.rating_before,
                            "ratingAfter": u.rating_after,
                            "delta": u.delta,
                            "deviationAfter": u.deviation_after,
                        },
                    }),
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                error = format!("{e:#}"),
                "rating pipeline failed (match still settles)"
            );
        }
    }

    Ok(Json(json!({
        "matchId": id.to_string(),
        "state": "settling",
        "message": "settlement submitted",
    })))
}

// ---- websocket ---------------------------------------------------------------------

#[derive(Deserialize)]
struct WsQuery {
    token: String,
}

async fn ws_upgrade(
    State(st): State<AppState>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> ApiResult<axum::response::Response> {
    let wallet = st.auth.session_wallet(&q.token).await?;
    Ok(ws.on_upgrade(move |socket| ws_loop(st, wallet, socket)))
}

async fn ws_loop(st: AppState, wallet: String, mut socket: WebSocket) {
    let mut rx = st.hub.register(&wallet);
    // Tell the client who they are (useful for multi-tab debugging).
    if socket
        .send(Message::Text(
            json!({ "type": "hello", "data": { "wallet": wallet } })
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(msg) => {
                    if socket.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    // Lightweight client→server ping: {"type":"ping"}
                    if t.as_str().starts_with("{\"type\":\"ping\"") {
                        let _ = socket.send(Message::Text(json!({ "type": "pong", "data": {} }).to_string().into())).await;
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            },
        }
    }
    // Dropping the socket closes the connection.
}

// ---- status helper used by main's readiness log ---------------------------------------

pub async fn status_json(st: &AppState) -> Value {
    json!({
        "service": "amp-server",
        "verifierConfigured": st.verifier.is_some(),
        "stakingEnabled": st.cfg.staking_enabled,
        "queueDepth": st.queue.depth(),
        "liveMatches": st.live_matches.load(Ordering::Relaxed),
        "connectedSockets": st.hub.connected_count(),
    })
}
