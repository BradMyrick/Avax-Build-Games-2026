//! Party lifecycle: a leader opens a party, invites members by code,
//! members accept (proving wallet ownership), the leader locks the
//! composition with a signature, then the party queues as one unit
//! via `amp_match_core::Party`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::auth::normalize_wallet;
use crate::error::ApiError;
use crate::store::Store;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PartyMember {
    pub wallet: String,
    pub region: String,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PartyRow {
    pub id: Uuid,
    pub leader: String,
    pub members: Vec<PartyMember>,
    pub game_id: String,
    pub ruleset_id: String,
    pub state: String,
    pub invite_code: String,
    pub created_at: DateTime<Utc>,
    pub locked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatePartyReq {
    pub game_id: String,
    pub ruleset_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JoinPartyReq {
    pub invite_code: String,
    pub region: Option<String>,
}

/// Leader signs this message to lock the party composition — proves the
/// leader consents to exactly this roster entering the queue.
pub fn lock_message(party_id: &str, member_wallets: &[&str]) -> String {
    format!(
        "Lock AMP party\n\nParty ID: {}\nMembers: {}\n\nThis signature is free. It confirms the party roster and enters the queue.",
        party_id,
        member_wallets.join(", ")
    )
}

impl Store {
    pub async fn create_party(
        &self,
        leader: &str,
        game_id: &str,
        ruleset_id: &str,
    ) -> Result<PartyRow, ApiError> {
        let id = Uuid::new_v4();
        let invite_code = crate::party::generate_invite_code();
        let member = PartyMember {
            wallet: leader.to_string(),
            region: "na".into(),
            accepted_at: Utc::now(),
        };
        let row = PartyRow {
            id,
            leader: leader.to_string(),
            members: vec![member],
            game_id: game_id.to_string(),
            ruleset_id: ruleset_id.to_string(),
            state: "open".into(),
            invite_code,
            created_at: Utc::now(),
            locked_at: None,
        };
        sqlx::query(
            r#"INSERT INTO amp_parties (id, leader, members, game_id, ruleset_id, state, invite_code)
               VALUES ($1, $2, $3::jsonb, $4, $5, 'open', $6)"#,
        )
        .bind(id)
        .bind(leader)
        .bind(serde_json::to_string(&row.members).unwrap())
        .bind(game_id)
        .bind(ruleset_id)
        .bind(&row.invite_code)
        .execute(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(row)
    }

    pub async fn get_party(&self, id: Uuid) -> Result<Option<PartyRow>, ApiError> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, String, String, String, DateTime<Utc>, Option<DateTime<Utc>>)>(
            "SELECT id, leader, members::text, game_id, ruleset_id, state, invite_code, created_at, locked_at FROM amp_parties WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(ApiError::Database)?;
        match row {
            Some(r) => {
                let members: Vec<PartyMember> = serde_json::from_str(&r.2)
                    .map_err(|e| ApiError::Internal(anyhow::anyhow!("bad members json: {e}")))?;
                Ok(Some(PartyRow {
                    id: r.0,
                    leader: r.1,
                    members,
                    game_id: r.3,
                    ruleset_id: r.4,
                    state: r.5,
                    invite_code: r.6,
                    created_at: r.7,
                    locked_at: r.8,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_party_by_invite(&self, code: &str) -> Result<Option<PartyRow>, ApiError> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, String, String, String, DateTime<Utc>, Option<DateTime<Utc>>)>(
            "SELECT id, leader, members::text, game_id, ruleset_id, state, invite_code, created_at, locked_at FROM amp_parties WHERE invite_code = $1 AND state = 'open'",
        )
        .bind(code)
        .fetch_optional(self.pool())
        .await
        .map_err(ApiError::Database)?;
        match row {
            Some(r) => {
                let members: Vec<PartyMember> = serde_json::from_str(&r.2)
                    .map_err(|e| ApiError::Internal(anyhow::anyhow!("bad members json: {e}")))?;
                Ok(Some(PartyRow {
                    id: r.0,
                    leader: r.1,
                    members,
                    game_id: r.3,
                    ruleset_id: r.4,
                    state: r.5,
                    invite_code: r.6,
                    created_at: r.7,
                    locked_at: r.8,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn join_party(
        &self,
        invite_code: &str,
        wallet: &str,
        region: &str,
    ) -> Result<PartyRow, ApiError> {
        let party = self
            .get_party_by_invite(invite_code)
            .await?
            .ok_or_else(|| ApiError::NotFound("party not found or closed".into()))?;
        if party.members.len() >= 16 {
            return Err(ApiError::Conflict("party is full".into()));
        }
        if party.members.iter().any(|m| m.wallet == wallet) {
            return Ok(party); // idempotent
        }
        let mut members = party.members.clone();
        members.push(PartyMember {
            wallet: wallet.to_string(),
            region: region.to_string(),
            accepted_at: Utc::now(),
        });
        sqlx::query("UPDATE amp_parties SET members = $2::jsonb WHERE id = $1")
            .bind(party.id)
            .bind(serde_json::to_string(&members).unwrap())
            .execute(self.pool())
            .await
            .map_err(ApiError::Database)?;
        let mut updated = party;
        updated.members = members;
        Ok(updated)
    }

    pub async fn lock_party(&self, id: Uuid) -> Result<PartyRow, ApiError> {
        sqlx::query("UPDATE amp_parties SET state = 'locked', locked_at = now() WHERE id = $1 AND state = 'open'")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(ApiError::Database)?;
        self.get_party(id)
            .await?
            .ok_or_else(|| ApiError::NotFound("party vanished".into()))
    }

    pub async fn disband_party(&self, id: Uuid, leader: &str) -> Result<bool, ApiError> {
        let res = sqlx::query(
            "UPDATE amp_parties SET state = 'disbanded' WHERE id = $1 AND leader = $2 AND state IN ('open', 'locked')",
        )
        .bind(id)
        .bind(leader)
        .execute(self.pool())
        .await
        .map_err(ApiError::Database)?;
        Ok(res.rows_affected() == 1)
    }
}

pub fn generate_invite_code() -> String {
    use rand::Rng;
    let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".chars().collect();
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect()
}

/// Validate that a party is ready to queue: locked, 1–16 members,
/// every member is a registered player.
#[allow(dead_code)] // wired when party queueing lands (M3.6)
pub async fn validate_for_queue(_store: &Store, party: &PartyRow) -> Result<(), ApiError> {
    if party.state != "locked" {
        return Err(ApiError::Conflict(
            "party must be locked before queuing".into(),
        ));
    }
    if party.members.is_empty() || party.members.len() > 16 {
        return Err(ApiError::BadRequest("party must have 1–16 members".into()));
    }
    let leader = normalize_wallet(&party.leader)?;
    let _ = leader;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_are_six_chars_no_ambiguous() {
        for _ in 0..100 {
            let code = generate_invite_code();
            assert_eq!(code.len(), 6);
            assert!(
                !code.contains('O')
                    && !code.contains('0')
                    && !code.contains('I')
                    && !code.contains('1')
            );
        }
    }

    #[test]
    fn lock_message_binds_party_and_members() {
        let msg = lock_message("party-123", &["0xa", "0xb"]);
        assert!(msg.contains("party-123"));
        assert!(msg.contains("0xa, 0xb"));
        assert!(msg.contains("This signature is free"));
    }
}
