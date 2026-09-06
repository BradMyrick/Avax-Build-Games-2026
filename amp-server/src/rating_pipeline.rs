//! Rating pipeline for N-player matches: converts a settled ladder into
//! per-player Glicko-2 field updates, applies party anti-boost
//! recalibration, and persists the results.

use amp_match_core::Party as CoreParty;
use amp_match_core::glicko2_update_vs_many;
use amp_match_core::ladder::placement_vectors;
use amp_match_core::party::recalibrate_party_deltas;
use amp_match_core::types::PlayerTicket;

use crate::error::ApiError;
use crate::store::Store;

/// Apply Glicko-2 rating updates to every player in a settled multiplayer
/// match, using the rating-period (vs_many) formulation with the ladder's
/// pairwise score vectors. Party members get γ-anti-boost recalibration.
pub async fn apply_multi_ratings(
    store: &Store,
    match_id: uuid::Uuid,
    game_id: &str,
    ruleset_id: &str,
    ladder: &[(String, u16)], // (wallet, rank)
    gamma: f32,
) -> Result<Vec<RatingUpdate>, ApiError> {
    let m = store
        .get_multi_match(match_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("match not found".into()))?;
    if m.state != "quorum" && m.state != "settled" && m.state != "settling" {
        return Err(ApiError::Conflict(format!(
            "match is {}, not settled",
            m.state
        )));
    }
    if ladder.len() != m.players.len() {
        return Err(ApiError::BadRequest(format!(
            "ladder has {} entries but match has {} players",
            ladder.len(),
            m.players.len()
        )));
    }

    // Build PlayerTickets from the match's player records.
    let tickets: Vec<PlayerTicket> = ladder
        .iter()
        .filter_map(|(wallet, _rank)| {
            m.players
                .iter()
                .find(|p| p.wallet == *wallet)
                .map(|p| PlayerTicket {
                    player_id: p.wallet.clone(),
                    game_id: game_id.to_string(),
                    ruleset_id: ruleset_id.to_string(),
                    mmr: p.rating as f32,
                    mmr_uncertainty: p.rd as f32,
                    region: p.region.clone(),
                    preferred_role: String::new(),
                    language: "en".into(),
                    max_ping_ms: 150,
                    enqueued_at_ms: 0,
                    party_size: 1,
                })
        })
        .collect();
    if tickets.len() != ladder.len() {
        return Err(ApiError::BadRequest(
            "ladder references unknown players".into(),
        ));
    }

    // Extract ranks aligned with the ticket order.
    let ranks: Vec<u16> = ladder.iter().map(|(_, rank)| *rank).collect();

    // Build pairwise score vectors.
    let updates = placement_vectors(&tickets, &ranks)
        .map_err(|e| ApiError::BadRequest(format!("ladder vectors: {e}")))?;

    // Group players by party for anti-boost recalibration.
    let party_groups = group_by_party(&m);

    let mut results = Vec::with_capacity(tickets.len());
    let mut raw_deltas: Vec<(String, f32)> = Vec::with_capacity(tickets.len());

    for (i, update) in updates.iter().enumerate() {
        let ticket = &tickets[i];
        let opponents: Vec<(f32, f32)> = update
            .opponents
            .iter()
            .map(|(r, rd, _)| (*r, *rd))
            .collect();
        let scores: Vec<f32> = update.opponents.iter().map(|(_, _, s)| *s).collect();

        let (new_r, new_rd, new_vol) = glicko2_update_vs_many(
            ticket.mmr,
            ticket.mmr_uncertainty,
            0.06,
            &opponents,
            &scores,
        );

        let delta = new_r - ticket.mmr;
        raw_deltas.push((ticket.player_id.clone(), delta));

        results.push(RatingUpdate {
            wallet: ticket.player_id.clone(),
            rating_before: ticket.mmr,
            rating_after: new_r,
            deviation_after: new_rd,
            volatility_after: new_vol,
            delta,
        });
    }

    // Apply γ-anti-boost recalibration to party members.
    let recalibrated = if !party_groups.is_empty() {
        let mut adjusted = raw_deltas.clone();
        for (_party_id, member_wallets) in &party_groups {
            // Build a core Party from member tickets.
            let members: Vec<PlayerTicket> = member_wallets
                .iter()
                .filter_map(|w| tickets.iter().find(|t| t.player_id == *w).cloned())
                .collect();
            if members.len() < 2 {
                continue; // solos don't need recalibration
            }
            let core_party = match CoreParty::new("match-party", members) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let party_deltas: Vec<(String, f32)> = raw_deltas
                .iter()
                .filter(|(w, _)| member_wallets.contains(w))
                .cloned()
                .collect();
            if let Ok(scaled) = recalibrate_party_deltas(&core_party, &party_deltas, gamma) {
                for (wallet, new_delta) in scaled {
                    if let Some(entry) = adjusted.iter_mut().find(|(w, _)| *w == wallet) {
                        // Update the delta and the resulting rating.
                        if let Some(r) = results.iter_mut().find(|r| r.wallet == wallet) {
                            r.delta = new_delta;
                            r.rating_after = r.rating_before + new_delta;
                        }
                        entry.1 = new_delta;
                    }
                }
            }
        }
        adjusted
    } else {
        raw_deltas
    };
    let _ = recalibrated;

    // Persist: upsert every player's rating.
    for r in &results {
        let won = r.delta > 0.0;
        let lost = r.delta < 0.0;
        let drew = r.delta == 0.0;
        store
            .apply_rating(
                &r.wallet,
                game_id,
                ruleset_id,
                r.rating_after,
                r.deviation_after,
                r.volatility_after,
                won,
                lost,
                drew,
            )
            .await?;
    }

    Ok(results)
}

/// Group players by their party_id field (from the match record).
fn group_by_party(m: &crate::multiplayer::MultiMatchRow) -> Vec<(uuid::Uuid, Vec<String>)> {
    let mut groups: std::collections::HashMap<uuid::Uuid, Vec<String>> =
        std::collections::HashMap::new();
    for p in &m.players {
        if let Some(pid) = p.party_id {
            groups.entry(pid).or_default().push(p.wallet.clone());
        }
    }
    groups.into_iter().collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RatingUpdate {
    pub wallet: String,
    pub rating_before: f32,
    pub rating_after: f32,
    pub deviation_after: f32,
    pub volatility_after: f32,
    pub delta: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_update_math_is_consistent() {
        // A 3-player match: rank 1 beats both, rank 2 beats rank 3.
        let tickets = vec![
            test_ticket("0xa", 1500.0),
            test_ticket("0xb", 1500.0),
            test_ticket("0xc", 1500.0),
        ];
        let ranks = vec![1, 2, 3];
        let updates = placement_vectors(&tickets, &ranks).unwrap();

        // Winner: two wins
        let opp_a: Vec<(f32, f32)> = updates[0]
            .opponents
            .iter()
            .map(|(r, rd, _)| (*r, *rd))
            .collect();
        let scores_a: Vec<f32> = updates[0].opponents.iter().map(|(_, _, s)| *s).collect();
        let (r_a, _, _) = glicko2_update_vs_many(1500.0, 200.0, 0.06, &opp_a, &scores_a);
        assert!(r_a > 1500.0, "winner should gain rating, got {r_a}");

        // Loser: two losses
        let opp_c: Vec<(f32, f32)> = updates[2]
            .opponents
            .iter()
            .map(|(r, rd, _)| (*r, *rd))
            .collect();
        let scores_c: Vec<f32> = updates[2].opponents.iter().map(|(_, _, s)| *s).collect();
        let (r_c, _, _) = glicko2_update_vs_many(1500.0, 200.0, 0.06, &opp_c, &scores_c);
        assert!(r_c < 1500.0, "loser should lose rating, got {r_c}");
    }

    fn test_ticket(id: &str, mmr: f32) -> PlayerTicket {
        PlayerTicket {
            player_id: id.into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr,
            mmr_uncertainty: 200.0,
            region: "na".into(),
            preferred_role: String::new(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: 0,
            party_size: 1,
        }
    }
}
