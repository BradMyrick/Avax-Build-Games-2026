//! Postgres data access. All queries are runtime-checked (non-macro) so the
//! crate builds without a live database, matching the relayer's style.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)] // status/match_id are read by FromRow round-trips and future queries
pub struct TicketRow {
    pub id: Uuid,
    pub wallet: String,
    pub game_id: String,
    pub ruleset_id: String,
    pub stake_wei: i64,
    pub region: String,
    pub status: String,
    pub match_id: Option<Uuid>,
    pub joined_at: DateTime<Utc>,
    pub intent_deadline: Option<DateTime<Utc>>,
    pub intent_sig: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)] // created_at/settled_at are read by FromRow round-trips and future queries
pub struct MatchRow {
    pub id: Uuid,
    pub game_id: String,
    pub ruleset_id: String,
    pub stake_wei: i64,
    pub state: String,
    pub player_a: String,
    pub player_b: String,
    pub rating_a_snapshot: serde_json::Value,
    pub rating_b_snapshot: serde_json::Value,
    pub winner: Option<String>,
    pub outcome: Option<String>,
    pub attestation: Option<serde_json::Value>,
    pub on_chain_match_id: Option<i64>,
    pub escrow_game_id: Option<i64>,
    pub agreed_at: Option<DateTime<Utc>>,
    pub settle_deadline: Option<DateTime<Utc>>,
    pub bot_match: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)] // timestamp + signature kept for audit / on-chain evidence
pub struct ReportRow {
    pub wallet: String,
    pub result: String,
    pub transcript_hash: Option<String>,
    pub signature: Option<String>,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)] // W/L/D read by the /v1/me handler via raw SQL
pub struct RatingRow {
    pub rating: f64,
    pub rating_deviation: f64,
    pub volatility: f64,
    pub wins: i64,
    pub losses: i64,
    pub draws: i64,
}

impl Store {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ---- players / ratings -------------------------------------------------

    pub async fn upsert_player(
        &self,
        wallet: &str,
        region: &str,
        language: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"INSERT INTO amp_players (wallet, region, language)
               VALUES ($1, $2, $3)
               ON CONFLICT (wallet) DO UPDATE SET last_seen_at = now()"#,
        )
        .bind(wallet)
        .bind(region)
        .bind(language)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_rating(
        &self,
        wallet: &str,
        game_id: &str,
        ruleset_id: &str,
    ) -> sqlx::Result<RatingRow> {
        let row = sqlx::query_as::<_, RatingRow>(
            r#"SELECT rating, rating_deviation, volatility, wins, losses, draws
               FROM amp_ratings WHERE wallet = $1 AND game_id = $2 AND ruleset_id = $3"#,
        )
        .bind(wallet)
        .bind(game_id)
        .bind(ruleset_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.unwrap_or(RatingRow {
            rating: 1500.0,
            rating_deviation: 350.0,
            volatility: 0.06,
            wins: 0,
            losses: 0,
            draws: 0,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_rating(
        &self,
        wallet: &str,
        game_id: &str,
        ruleset_id: &str,
        rating: f32,
        deviation: f32,
        volatility: f32,
        won: bool,
        lost: bool,
        drew: bool,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"INSERT INTO amp_ratings
                   (wallet, game_id, ruleset_id, rating, rating_deviation, volatility,
                    wins, losses, draws, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
               ON CONFLICT (wallet, game_id, ruleset_id) DO UPDATE SET
                   rating = EXCLUDED.rating,
                   rating_deviation = EXCLUDED.rating_deviation,
                   volatility = EXCLUDED.volatility,
                   wins = amp_ratings.wins + EXCLUDED.wins,
                   losses = amp_ratings.losses + EXCLUDED.losses,
                   draws = amp_ratings.draws + EXCLUDED.draws,
                   updated_at = now()"#,
        )
        .bind(wallet)
        .bind(game_id)
        .bind(ruleset_id)
        .bind(rating as f64)
        .bind(deviation as f64)
        .bind(volatility as f64)
        .bind(won as i64)
        .bind(lost as i64)
        .bind(drew as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ---- auth ---------------------------------------------------------------

    pub async fn insert_challenge(
        &self,
        nonce: &str,
        wallet: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO amp_auth_challenges (nonce, wallet, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(nonce)
        .bind(wallet)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn outstanding_challenges(&self, wallet: &str) -> sqlx::Result<i64> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM amp_auth_challenges WHERE wallet = $1 AND used = false AND expires_at > now()",
        )
        .bind(wallet)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("n"))
    }

    /// Atomically consume a challenge: single-use, unexpired, wallet-bound.
    pub async fn consume_challenge(&self, nonce: &str, wallet: &str) -> sqlx::Result<bool> {
        let res = sqlx::query(
            r#"UPDATE amp_auth_challenges SET used = true
               WHERE nonce = $1 AND wallet = $2 AND used = false AND expires_at > now()"#,
        )
        .bind(nonce)
        .bind(wallet)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    pub async fn purge_expired_challenges(&self) -> sqlx::Result<u64> {
        let res = sqlx::query(
            "DELETE FROM amp_auth_challenges WHERE expires_at < now() - interval '1 hour'",
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn insert_session(
        &self,
        token_hash: &str,
        wallet: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO amp_sessions (token_hash, wallet, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(token_hash)
        .bind(wallet)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn session_wallet(&self, token_hash: &str) -> sqlx::Result<Option<String>> {
        let row = sqlx::query(
            r#"SELECT wallet FROM amp_sessions
               WHERE token_hash = $1 AND revoked = false AND expires_at > now()"#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<String, _>("wallet")))
    }

    // ---- queue ---------------------------------------------------------------

    pub async fn insert_ticket(&self, row: &TicketRow) -> sqlx::Result<()> {
        sqlx::query(
            r#"INSERT INTO amp_queue_tickets
                   (id, wallet, game_id, ruleset_id, stake_wei, region, status, joined_at,
                    intent_deadline, intent_sig)
               VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $8, $9)"#,
        )
        .bind(row.id)
        .bind(&row.wallet)
        .bind(&row.game_id)
        .bind(&row.ruleset_id)
        .bind(row.stake_wei)
        .bind(&row.region)
        .bind(row.joined_at)
        .bind(row.intent_deadline)
        .bind(row.intent_sig.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn active_ticket(&self, wallet: &str) -> sqlx::Result<Option<TicketRow>> {
        sqlx::query_as::<_, TicketRow>(
            "SELECT * FROM amp_queue_tickets WHERE wallet = $1 AND status = 'queued' LIMIT 1",
        )
        .bind(wallet)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn cancel_ticket(&self, id: Uuid, wallet: &str) -> sqlx::Result<bool> {
        let res = sqlx::query(
            "UPDATE amp_queue_tickets SET status = 'cancelled' WHERE id = $1 AND wallet = $2 AND status = 'queued'",
        )
        .bind(id)
        .bind(wallet)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    pub async fn mark_tickets_matched(&self, a: Uuid, b: Uuid, match_id: Uuid) -> sqlx::Result<()> {
        for id in [a, b] {
            sqlx::query(
                "UPDATE amp_queue_tickets SET status = 'matched', match_id = $2 WHERE id = $1",
            )
            .bind(id)
            .bind(match_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Tickets still marked queued at boot — push them back into the in-memory
    /// queue preserving original join time so wait-based widening survives restarts.
    pub async fn rehydrate_tickets(&self) -> sqlx::Result<Vec<TicketRow>> {
        sqlx::query_as::<_, TicketRow>(
            "SELECT * FROM amp_queue_tickets WHERE status = 'queued' ORDER BY joined_at ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn queue_depth(&self, game_id: &str, ruleset_id: &str) -> sqlx::Result<i64> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM amp_queue_tickets WHERE status = 'queued' AND game_id = $1 AND ruleset_id = $2",
        )
        .bind(game_id)
        .bind(ruleset_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("n"))
    }

    // ---- matches --------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_match(&self, row: &MatchRow, bot_match: bool) -> sqlx::Result<()> {
        let state = if bot_match {
            "live"
        } else if row.stake_wei > 0 {
            "escrow_pending"
        } else {
            "live"
        };
        sqlx::query(
            r#"INSERT INTO amp_matches
                   (id, game_id, ruleset_id, stake_wei, state, player_a, player_b,
                    rating_a_snapshot, rating_b_snapshot, expires_at, bot_match,
                    on_chain_match_id, escrow_game_id)
               VALUES ($1, $2, $3, $4, $11, $5, $6, $7, $8, $9, $10, $12, $13)"#,
        )
        .bind(row.id)
        .bind(&row.game_id)
        .bind(&row.ruleset_id)
        .bind(row.stake_wei)
        .bind(&row.player_a)
        .bind(&row.player_b)
        .bind(&row.rating_a_snapshot)
        .bind(&row.rating_b_snapshot)
        .bind(row.expires_at)
        .bind(bot_match)
        .bind(state)
        .bind(row.on_chain_match_id)
        .bind(row.escrow_game_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_match(&self, id: Uuid) -> sqlx::Result<Option<MatchRow>> {
        sqlx::query_as::<_, MatchRow>("SELECT * FROM amp_matches WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn live_match_for(&self, wallet: &str) -> sqlx::Result<Option<MatchRow>> {
        sqlx::query_as::<_, MatchRow>(
            r#"SELECT * FROM amp_matches
               WHERE state = 'live' AND (player_a = $1 OR player_b = $1)
               ORDER BY created_at DESC LIMIT 1"#,
        )
        .bind(wallet)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn history(
        &self,
        wallet: &str,
        limit: i64,
        offset: i64,
    ) -> sqlx::Result<Vec<MatchRow>> {
        sqlx::query_as::<_, MatchRow>(
            r#"SELECT * FROM amp_matches
               WHERE (player_a = $1 OR player_b = $1) AND state <> 'live'
               ORDER BY created_at DESC LIMIT $2 OFFSET $3"#,
        )
        .bind(wallet)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn set_match_outcome(
        &self,
        id: Uuid,
        state: &str,
        outcome: &str,
        winner: Option<&str>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"UPDATE amp_matches
               SET state = $2, outcome = $3, winner = $4
               WHERE id = $1 AND state = 'live'"#,
        )
        .bind(id)
        .bind(state)
        .bind(outcome)
        .bind(winner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_match_state(&self, id: Uuid, state: &str) -> sqlx::Result<()> {
        sqlx::query("UPDATE amp_matches SET state = $2 WHERE id = $1")
            .bind(id)
            .bind(state)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_attestation(
        &self,
        id: Uuid,
        attestation: serde_json::Value,
    ) -> sqlx::Result<()> {
        sqlx::query("UPDATE amp_matches SET attestation = $2 WHERE id = $1")
            .bind(id)
            .bind(attestation)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    #[allow(dead_code)] // wired when the relayer settlement confirmation lands
    pub async fn mark_settled(&self, id: Uuid) -> sqlx::Result<()> {
        sqlx::query("UPDATE amp_matches SET state = 'settled', settled_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Live matches past expiry — the sweep loop reconciles each one.
    pub async fn expired_live_matches(&self) -> sqlx::Result<Vec<MatchRow>> {
        sqlx::query_as::<_, MatchRow>(
            "SELECT * FROM amp_matches WHERE state = 'live' AND expires_at < now()",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn count_live_matches(&self) -> sqlx::Result<usize> {
        let row = sqlx::query("SELECT count(*) AS n FROM amp_matches WHERE state = 'live'")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("n") as usize)
    }

    // ---- reports ---------------------------------------------------------------

    /// Insert a player report; returns false if this player already reported
    /// (idempotency — the first report stands).
    pub async fn insert_report(
        &self,
        match_id: Uuid,
        wallet: &str,
        result: &str,
        transcript_hash: Option<&str>,
        signature: Option<&str>,
    ) -> sqlx::Result<bool> {
        let res = sqlx::query(
            r#"INSERT INTO amp_match_reports (match_id, wallet, result, transcript_hash, signature)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (match_id, wallet) DO NOTHING"#,
        )
        .bind(match_id)
        .bind(wallet)
        .bind(result)
        .bind(transcript_hash)
        .bind(signature)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    pub async fn get_reports(&self, match_id: Uuid) -> sqlx::Result<Vec<ReportRow>> {
        sqlx::query_as::<_, ReportRow>(
            "SELECT * FROM amp_match_reports WHERE match_id = $1 ORDER BY submitted_at ASC",
        )
        .bind(match_id)
        .fetch_all(&self.pool)
        .await
    }

    // ---- settlement jobs ---------------------------------------------------------

    pub async fn insert_settle_job(&self, payload: serde_json::Value) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO relayer_jobs (kind, payload, status) VALUES ('settle_match', $1::jsonb, 'pending')",
        )
        .bind(payload.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl Store {
    /// Escrow confirmed on-chain: flip escrow_pending → live and give the
    /// players a full match window.
    pub async fn confirm_escrow(&self, id: Uuid, ttl_minutes: i64) -> sqlx::Result<bool> {
        let res = sqlx::query(
            r#"UPDATE amp_matches
               SET state = 'live',
                   expires_at = now() + make_interval(mins => $2::int)
               WHERE id = $1 AND state = 'escrow_pending'"#,
        )
        .bind(id)
        .bind(ttl_minutes)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_agreed(
        &self,
        id: Uuid,
        state: &str,
        outcome: &str,
        winner: Option<&str>,
        rt_grace_minutes: i64,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"UPDATE amp_matches
               SET state = $2, outcome = $3, winner = $4,
                   agreed_at = now(),
                   settle_deadline = CASE WHEN $2 = 'settling_rt'
                       THEN now() + make_interval(mins => $5::int) ELSE NULL END
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(state)
        .bind(outcome)
        .bind(winner)
        .bind(rt_grace_minutes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Matches that agreed to direct RT settlement but whose window lapsed —
    /// the relayer fallback picks them up.
    pub async fn rt_overdue_matches(&self) -> sqlx::Result<Vec<MatchRow>> {
        sqlx::query_as::<_, MatchRow>(
            "SELECT * FROM amp_matches WHERE state = 'settling_rt' AND settle_deadline < now()",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Escrow windows that lapsed without both players funding.
    pub async fn expired_escrow_matches(&self) -> sqlx::Result<Vec<MatchRow>> {
        sqlx::query_as::<_, MatchRow>(
            "SELECT * FROM amp_matches WHERE state = 'escrow_pending' AND expires_at < now()",
        )
        .fetch_all(&self.pool)
        .await
    }
}
