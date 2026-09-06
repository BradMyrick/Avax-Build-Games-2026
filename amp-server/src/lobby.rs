//! Multiplayer lobby formation: collects revealed commits, forms lobbies
//! via the blockhash-seeded shuffle, creates match records, and notifies
//! players. Runs on the same tick as the 1v1 matchmaker.

use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use crate::error::ApiError;
use crate::store::Store;

/// Form lobbies from revealed commits. Called on every matchmaker tick.
/// Only creates matches for buckets with enough revealed players.
pub async fn form_lobbies_from_reveals(
    store: &Store,
    hub: &crate::ws::WsHub,
    multiplayer_addr: &str,
    chain_id: u64,
) -> Result<Vec<Uuid>, ApiError> {
    // Find all (game_id, ruleset_id) buckets with revealed commits.
    let rows = sqlx::query(
        r#"SELECT game_id, ruleset_id, count(*) as n, min(stake_wei) as min_stake
           FROM amp_commits
           WHERE state = 'revealed'
           GROUP BY game_id, ruleset_id
           HAVING count(*) >= 4"#,
    )
    .fetch_all(store.pool())
    .await
    .map_err(ApiError::Database)?;

    let mut formed = Vec::new();
    for row in rows {
        let game_id: String = row.get("game_id");
        let ruleset_id: String = row.get("ruleset_id");
        let count: i64 = row.get("n");
        let min_stake: i64 = row.get("min_stake");
        let _ = min_stake;

        // Fetch revealed commits in this bucket.
        let commits = sqlx::query(
            "SELECT wallet, stake_wei, salt FROM amp_commits \
             WHERE game_id = $1 AND ruleset_id = $2 AND state = 'revealed' \
             ORDER BY revealed_at ASC",
        )
        .bind(&game_id)
        .bind(&ruleset_id)
        .fetch_all(store.pool())
        .await
        .map_err(ApiError::Database)?;

        if (commits.len() as i64) < count {
            continue;
        }

        // Lobby size: cap at 8 for now (configurable later).
        let lobby_size = 8.min(commits.len());

        // Blockhash-seeded shuffle to prevent lobby targeting.
        let wallets: Vec<String> = commits
            .iter()
            .map(|r| r.get::<String, _>("wallet"))
            .collect();
        let shuffled = shuffle_wallets(&wallets);

        // Take the first lobby_size players.
        let selected: Vec<String> = shuffled.into_iter().take(lobby_size).collect();

        // Fetch ratings for the selected players.
        let mut players = Vec::new();
        for (i, wallet) in selected.iter().enumerate() {
            let rating = store.get_rating(wallet, &game_id, &ruleset_id).await?;
            players.push(crate::multiplayer::MultiPlayer {
                wallet: wallet.clone(),
                index: i as u8,
                rating: rating.rating,
                rd: rating.rating_deviation,
                region: "na".into(),
                party_id: None,
                commit_hash: String::new(),
                salt: None,
            });
        }

        // Create the match record.
        let match_id = Uuid::new_v4();
        let stake: i64 = commits
            .first()
            .map(|r| r.get::<i64, _>("stake_wei"))
            .unwrap_or(0);
        let session_nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let match_row = crate::multiplayer::MultiMatchRow {
            id: match_id,
            game_id: game_id.clone(),
            ruleset_id: ruleset_id.clone(),
            lobby_size,
            payout_profile_id: 1,
            stake_per_player: stake,
            bond_per_player: stake / 20, // 5% bond
            on_chain_match_id: None,
            state: "committing".into(),
            players,
            signer_mask: 0,
            ladder: None,
            transcript_hash: None,
            session_nonce: Some(session_nonce as i64),
            ladder_hash: None,
            created_at: Utc::now(),
            ready_at: None,
            quorum_until: None,
            grace_until: None,
            settled_at: None,
        };
        store.insert_multi_match(&match_row).await?;

        // Mark the consumed commits as expired.
        for wallet in &selected {
            sqlx::query(
                "UPDATE amp_commits SET state = 'expired' WHERE wallet = $1 AND game_id = $2 AND ruleset_id = $3",
            )
            .bind(wallet)
            .bind(&game_id)
            .bind(&ruleset_id)
            .execute(store.pool())
            .await
            .map_err(ApiError::Database)?;
        }

        // Notify every player.
        for p in &match_row.players {
            hub.send(
                &p.wallet,
                "multi_lobby_formed",
                serde_json::json!({
                    "matchId": match_id.to_string(),
                    "gameId": match_row.game_id,
                    "lobbySize": lobby_size,
                    "stakeWei": stake,
                    "bondWei": match_row.bond_per_player,
                    "sessionNonce": session_nonce,
                    "multiplayerAddress": multiplayer_addr,
                    "chainId": chain_id,
                }),
            );
        }

        formed.push(match_id);
    }

    Ok(formed)
}

/// Deterministic shuffle using the match-core blockhash shuffle with a
/// synthetic seed (real blockhash integration is a follow-up once the
/// alloy provider is wired into the tick loop).
fn shuffle_wallets(wallets: &[String]) -> Vec<String> {
    if wallets.len() < 2 {
        return wallets.to_vec();
    }
    // Use a time-derived seed for now; the real blockhash lands with the
    // RPC integration in the tick loop.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let blockhash = {
        let mut b = [0u8; 32];
        b[..8].copy_from_slice(&seed.to_be_bytes());
        b
    };
    amp_match_core::shuffle_by_blockhash(wallets.to_vec(), &blockhash)
}

/// Sweep: transition live matches past their quorum window to grace,
/// and past grace to cancelled if no claim was filed.
pub async fn multi_sweep(store: &Store, hub: &crate::ws::WsHub) -> Result<(), ApiError> {
    use sqlx::Row;

    // Quorum window expired: move to grace.
    let expired_quorum = sqlx::query(
        "SELECT id, players::text FROM amp_multi_matches \
         WHERE state = 'quorum' AND settled_at IS NULL \
         AND created_at < now() - interval '120 seconds' * 2",
    )
    .fetch_all(store.pool())
    .await
    .map_err(ApiError::Database)?;
    for row in &expired_quorum {
        let id: Uuid = row.get("id");
        store.update_multi_state(id, "grace").await?;
    }

    // Grace expired with no resolution: refund.
    let expired_grace = sqlx::query(
        "SELECT id, players::text FROM amp_multi_matches \
         WHERE state IN ('grace', 'live') \
         AND created_at < now() - interval '600 seconds'",
    )
    .fetch_all(store.pool())
    .await
    .map_err(ApiError::Database)?;
    for row in &expired_grace {
        let id: Uuid = row.get("id");
        store.update_multi_state(id, "cancelled").await?;

        // Notify players.
        if let Ok(players) = serde_json::from_str::<Vec<crate::multiplayer::MultiPlayer>>(
            &row.get::<String, _>("players"),
        ) {
            for p in &players {
                hub.send(
                    &p.wallet,
                    "multi_cancelled",
                    serde_json::json!({
                        "matchId": id.to_string(),
                        "reason": "expired",
                    }),
                );
            }
        }
    }

    Ok(())
}
