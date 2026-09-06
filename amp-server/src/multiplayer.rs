//! Multiplayer match lifecycle: commit-reveal lobby formation, N-way
//! quorum settlement, and the prove-your-payout pipeline.

use alloy_primitives::{Address, B256, keccak256};
use uuid::Uuid;

use sqlx::Row;

use crate::error::ApiError;
use crate::store::Store;

#[allow(dead_code)] // wired into the lobby-formation tick in M3.6
pub const QUORUM_WINDOW_SECS: i64 = 120;
#[allow(dead_code)] // wired into the lobby-formation tick in M3.6
pub const GRACE_WINDOW_SECS: i64 = 300;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiPlayer {
    pub wallet: String,
    pub index: u8,
    pub rating: f64,
    pub rd: f64,
    pub region: String,
    pub party_id: Option<Uuid>,
    pub commit_hash: String,
    pub salt: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiMatchRow {
    pub id: Uuid,
    pub game_id: String,
    pub ruleset_id: String,
    pub lobby_size: usize,
    pub payout_profile_id: i16,
    pub stake_per_player: i64,
    pub bond_per_player: i64,
    pub on_chain_match_id: Option<i64>,
    pub state: String,
    pub players: Vec<MultiPlayer>,
    pub signer_mask: i64,
    pub ladder: Option<serde_json::Value>,
    pub transcript_hash: Option<String>,
    pub session_nonce: Option<i64>,
    pub ladder_hash: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub ready_at: Option<chrono::DateTime<chrono::Utc>>,
    pub quorum_until: Option<chrono::DateTime<chrono::Utc>>,
    pub grace_until: Option<chrono::DateTime<chrono::Utc>>,
    pub settled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LadderReport {
    pub wallet: String,
    pub ranked: Vec<(String, u16)>, // (wallet, rank) — rank 1 = winner
    pub transcript_hash: String,
    pub session_nonce: u64,
    pub signature: String,
}

/// The quorum threshold K = floor(2N/3) + 1 (or N for N ≤ 3).
pub fn quorum_of(n: usize) -> usize {
    if n <= 3 { n } else { (2 * n) / 3 + 1 }
}

/// Commit a blinded FFA queue entry: H = keccak256(addr ‖ stake ‖ salt).
/// The server sees the hash, not the salt — coordinators can't pre-select
/// lobby mates.
pub fn compute_commit(wallet: &str, stake_wei: i64, salt: &str) -> String {
    let mut buf = Vec::with_capacity(20 + 8 + 32);
    // Wallet address bytes (last 20 of the hex)
    if let Ok(addr) = wallet.parse::<Address>() {
        buf.extend_from_slice(addr.as_slice());
    }
    buf.extend_from_slice(&(stake_wei as u64).to_be_bytes());
    buf.extend_from_slice(salt.as_bytes());
    format!("{:#x}", keccak256(&buf))
}

/// Verify a revealed salt matches the original commit.
pub fn verify_commit(commit_hash: &str, wallet: &str, stake_wei: i64, salt: &str) -> bool {
    let computed = compute_commit(wallet, stake_wei, salt);
    // Case-insensitive (both should be 0x-prefixed hex)
    computed.eq_ignore_ascii_case(commit_hash)
}

impl Store {
    #[allow(dead_code)] // wired when the lobby-formation tick lands (M3.6)
    pub async fn insert_multi_match(&self, m: &MultiMatchRow) -> Result<(), ApiError> {
        sqlx::query(
            r#"INSERT INTO amp_multi_matches
                   (id, game_id, ruleset_id, lobby_size, payout_profile_id,
                    stake_per_player, bond_per_player, state, players,
                    session_nonce, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, $10, now())"#,
        )
        .bind(m.id)
        .bind(&m.game_id)
        .bind(&m.ruleset_id)
        .bind(m.lobby_size as i32)
        .bind(m.payout_profile_id)
        .bind(m.stake_per_player)
        .bind(m.bond_per_player)
        .bind(&m.state)
        .bind(serde_json::to_string(&m.players).unwrap())
        .bind(m.session_nonce.unwrap_or(0))
        .execute(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(())
    }

    pub async fn get_multi_match(&self, id: Uuid) -> Result<Option<MultiMatchRow>, ApiError> {
        let row = sqlx::query(
            "SELECT id, game_id, ruleset_id, lobby_size, payout_profile_id, \
             stake_per_player, bond_per_player, on_chain_match_id, state, \
             players::text, signer_mask, ladder::text, transcript_hash, \
             session_nonce, ladder_hash, created_at, ready_at, quorum_until, \
             grace_until, settled_at FROM amp_multi_matches WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(ApiError::Database)?;

        let Some(r) = row else { return Ok(None) };
        let players: Vec<MultiPlayer> = serde_json::from_str(&r.get::<String, _>("players"))
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bad players json: {e}")))?;
        Ok(Some(MultiMatchRow {
            id: r.get("id"),
            game_id: r.get("game_id"),
            ruleset_id: r.get("ruleset_id"),
            lobby_size: r.get::<i32, _>("lobby_size") as usize,
            payout_profile_id: r.get("payout_profile_id"),
            stake_per_player: r.get("stake_per_player"),
            bond_per_player: r.get("bond_per_player"),
            on_chain_match_id: r.get("on_chain_match_id"),
            state: r.get("state"),
            players,
            signer_mask: r.get("signer_mask"),
            ladder: r
                .get::<Option<String>, _>("ladder")
                .and_then(|s| serde_json::from_str(&s).ok()),
            transcript_hash: r.get("transcript_hash"),
            session_nonce: r.get("session_nonce"),
            ladder_hash: r.get("ladder_hash"),
            created_at: r.get("created_at"),
            ready_at: r.get("ready_at"),
            quorum_until: r.get("quorum_until"),
            grace_until: r.get("grace_until"),
            settled_at: r.get("settled_at"),
        }))
    }

    pub async fn update_multi_state(&self, id: Uuid, state: &str) -> Result<(), ApiError> {
        sqlx::query("UPDATE amp_multi_matches SET state = $2 WHERE id = $1")
            .bind(id)
            .bind(state)
            .execute(self.pool())
            .await
            .map_err(ApiError::Database)?;
        Ok(())
    }

    #[allow(dead_code)] // wired into the settlement pipeline (M3.5)
    pub async fn set_multi_ladder(
        &self,
        id: Uuid,
        ladder: &serde_json::Value,
        transcript_hash: &str,
        ladder_hash: &str,
        signer_mask: i64,
    ) -> Result<(), ApiError> {
        sqlx::query(
            "UPDATE amp_multi_matches SET ladder = $2::jsonb, transcript_hash = $3, \
             ladder_hash = $4, signer_mask = $5, settled_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(serde_json::to_string(ladder).unwrap())
        .bind(transcript_hash)
        .bind(ladder_hash)
        .bind(signer_mask)
        .execute(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(())
    }

    pub async fn insert_ladder_report(
        &self,
        match_id: Uuid,
        report: &LadderReport,
    ) -> Result<bool, ApiError> {
        let res = sqlx::query(
            r#"INSERT INTO amp_ladder_reports
                   (match_id, wallet, ranked, transcript_hash, session_nonce, signature)
               VALUES ($1, $2, $3::jsonb, $4, $5, $6)
               ON CONFLICT (match_id, wallet) DO NOTHING"#,
        )
        .bind(match_id)
        .bind(&report.wallet)
        .bind(serde_json::to_string(&report.ranked).unwrap())
        .bind(&report.transcript_hash)
        .bind(report.session_nonce as i64)
        .bind(&report.signature)
        .execute(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(res.rows_affected() == 1)
    }

    pub async fn get_ladder_reports(
        &self,
        match_id: Uuid,
    ) -> Result<Vec<(String, String, String, String)>, ApiError> {
        let rows = sqlx::query(
            "SELECT wallet, ranked::text, transcript_hash, signature FROM amp_ladder_reports WHERE match_id = $1",
        )
        .bind(match_id)
        .fetch_all(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("wallet"),
                    r.get::<String, _>("ranked"),
                    r.get::<String, _>("transcript_hash"),
                    r.get::<String, _>("signature"),
                )
            })
            .collect())
    }

    /// Count concordant reports sharing the same (ladder_hash, transcript_hash) pair.
    pub async fn concordant_quorum(
        &self,
        match_id: Uuid,
    ) -> Result<Option<(String, usize, serde_json::Value)>, ApiError> {
        let reports = self.get_ladder_reports(match_id).await?;
        if reports.is_empty() {
            return Ok(None);
        }
        // Group by (ranked_json, transcript_hash)
        let mut groups: std::collections::HashMap<(String, String), (usize, serde_json::Value)> =
            std::collections::HashMap::new();
        for (wallet, ranked_json, th, _sig) in &reports {
            let _ = wallet;
            let key = (ranked_json.clone(), th.clone());
            let entry = groups
                .entry(key)
                .or_insert((0, serde_json::from_str(ranked_json).unwrap_or_default()));
            entry.0 += 1;
        }
        // Find the largest concordant group
        let best = groups.into_iter().max_by_key(|(_, (count, _))| *count);
        match best {
            Some(((_ranked, th), (count, ladder))) => Ok(Some((th, count, ladder))),
            None => Ok(None),
        }
    }
}

/// Build the on-chain settlement job payload from a concordant quorum.
#[allow(clippy::too_many_arguments)]
pub async fn build_settle_multi_job(
    store: &Store,
    match_id: Uuid,
    chain_id: u64,
    multiplayer_addr: Address,
) -> Result<serde_json::Value, ApiError> {
    let m = store
        .get_multi_match(match_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    let reports = store.get_ladder_reports(match_id).await?;
    let k = quorum_of(m.lobby_size);
    if reports.len() < k {
        return Err(ApiError::Conflict(format!(
            "only {}/{} reports",
            reports.len(),
            k
        )));
    }
    let (th, count, ladder) = store
        .concordant_quorum(match_id)
        .await?
        .ok_or_else(|| ApiError::Conflict("no concordant reports".into()))?;
    if count < k {
        return Err(ApiError::Conflict(format!(
            "concordant group {count} < K {k}"
        )));
    }

    // Signer bitmask is computed below via cryptographic verification
    // (verified_mask), not by trusting the report list.

    let on_chain_id = m.on_chain_match_id.unwrap_or(0);
    let game_id_num: u64 = m.game_id.parse().unwrap_or(1);

    // Compute the EIP-712 digest for each signer and verify against their
    // recorded signature.
    let ranked_addrs: Vec<Address> = ladder
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v[0].as_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default();
    let th_b256: B256 = th.trim_start_matches("0x").parse().unwrap_or(B256::ZERO);
    let nonce = m.session_nonce.unwrap_or(0) as u64;

    let mut packed_sigs = Vec::new();
    let mut verified_mask: u64 = 0;
    for (wallet, _rj, _rth, sig) in &reports {
        let recovered = crate::ladder::recover_ladder_signer(
            chain_id,
            multiplayer_addr,
            on_chain_id as u64,
            game_id_num,
            &ranked_addrs,
            th_b256,
            nonce,
            sig,
        );
        if let Ok(addr) = recovered
            && format!("{addr:#x}").to_lowercase() == *wallet
            && let Some(player) = m.players.iter().find(|p| p.wallet == *wallet)
        {
            verified_mask |= 1u64 << player.index;
            packed_sigs.push(sig.trim_start_matches("0x"));
        }
    }
    let verified_count = verified_mask.count_ones() as usize;
    if verified_count < k {
        return Err(ApiError::Conflict(format!(
            "verified {verified_count} < K {k} (some signatures failed recovery)"
        )));
    }

    // Only include the first K verified signatures (ascending bit order).
    let mut final_sigs = String::new();
    let mut included = 0u64;
    for i in 0..64u32 {
        if verified_mask & (1u64 << i) != 0 && (included.count_ones() as usize) < k {
            included |= 1u64 << i;
            // Find the corresponding signature
            for (wallet, _rj, _rth, sig) in &reports {
                if let Some(p) = m
                    .players
                    .iter()
                    .find(|p| p.index as u32 == i && p.wallet == *wallet)
                {
                    let _ = p;
                    final_sigs.push_str(sig.trim_start_matches("0x"));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "matchUuid": match_id.to_string(),
        "onChainMatchId": on_chain_id,
        "rankedPlacements": ranked_addrs.iter().map(|a| format!("{a:#x}")).collect::<Vec<_>>(),
        "transcriptHash": th,
        "sessionNonce": nonce,
        "signerBitmask": format!("{:#x}", included),
        "packedSignatures": format!("0x{}", final_sigs),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quorum_thresholds() {
        assert_eq!(quorum_of(2), 2);
        assert_eq!(quorum_of(3), 3);
        assert_eq!(quorum_of(4), 3);
        assert_eq!(quorum_of(6), 5);
        assert_eq!(quorum_of(8), 6);
        assert_eq!(quorum_of(16), 11);
        assert_eq!(quorum_of(64), 43);
    }

    #[test]
    fn commit_round_trip() {
        let wallet = "0x95CC495dF579981d3Ffa4a8f77B93A17563E077a";
        let stake = 1_000_000_000_000i64;
        let salt = "my-salt-value-32-bytes-xxxxxxxxx";
        let hash = compute_commit(wallet, stake, salt);
        assert!(hash.starts_with("0x"));
        assert_eq!(hash.len(), 66);
        assert!(verify_commit(&hash, wallet, stake, salt));
        assert!(!verify_commit(&hash, wallet, stake, "wrong-salt"));
        assert!(!verify_commit(&hash, wallet, 999, salt));
    }

    #[test]
    fn commit_is_input_sensitive() {
        let wallet = "0x95CC495dF579981d3Ffa4a8f77B93A17563E077a";
        let a = compute_commit(wallet, 100, "salt");
        let b = compute_commit(wallet, 101, "salt");
        let c = compute_commit(wallet, 100, "salt2");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, compute_commit(wallet, 100, "salt"));
    }
}
