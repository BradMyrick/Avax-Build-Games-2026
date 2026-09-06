//! Match lifecycle: creation from queue pairings, outcome reconciliation
//! (agreement / dispute / default-on-timeout), Glicko-2 application, and
//! EIP-712 attestation.
//!
//! The reconciliation rules are the player-trust core of the protocol:
//! - both players report consistent results → agreed, ratings + attestation
//! - both report conflicting results → disputed, operator arbitrates
//! - one reports and the match expires → reporter's result stands
//!   (an opponent who goes silent forfeits the argument)
//! - neither reports and the match expires → cancelled, ratings untouched

use amp_match_core::glicko2_update;
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::attest::{outcome_code, sign_attestation};
use crate::error::ApiError;
use crate::store::{MatchRow, ReportRow, Store};

/// Reporter-relative results mapped against the match's player_a/player_b.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    WinA,
    WinB,
    Draw,
}

/// Reconcile a set of reports into an outcome, if determinable.
/// Reports are (wallet, reporter-relative result) pairs; exactly the two
/// players' reports are considered.
///
/// Evidence rule: if BOTH players supplied a transcript hash and the hashes
/// disagree, the match is disputed even when the claimed results are
/// consistent — mismatched state roots are contradiction, not agreement.
pub fn reconcile(a: &str, b: &str, reports: &[ReportRow]) -> Option<Outcome> {
    let rep_a = reports.iter().find(|r| r.wallet == a)?;
    let rep_b = reports.iter().find(|r| r.wallet == b)?;

    if let (Some(ta), Some(tb)) = (
        rep_a.transcript_hash.as_deref(),
        rep_b.transcript_hash.as_deref(),
    ) && ta != tb
    {
        return None; // contradicting state roots → disputed
    }

    match (rep_a.result.as_str(), rep_b.result.as_str()) {
        ("win", "loss") => Some(Outcome::WinA),
        ("loss", "win") => Some(Outcome::WinB),
        ("draw", "draw") => Some(Outcome::Draw),
        _ => None, // conflicting accounts → disputed
    }
}

/// The canonical EIP-191 report message a player signs. Byte-exact contract
/// with the web client and (future) SDKs — changing this breaks evidence
/// portability.
/// How a settled match pays out on-chain. Pure — unit-testable.
pub enum SettleRoute {
    /// Free match: nothing on-chain, ratings + optional attestation only.
    None,
    /// Full RT evidence (both players signed, identical transcript hashes):
    /// players settle directly via RT_HASH_AGREE; the relayer only acts as
    /// fallback once the grace window lapses.
    DirectRt,
    /// Incomplete evidence: the verifier attests and the relayer settles
    /// immediately via submitAsyncResult.
    ImmediateAsync,
}

pub fn settle_route(m: &MatchRow, reports: &[ReportRow]) -> SettleRoute {
    if m.stake_wei == 0 {
        return SettleRoute::None;
    }
    let both_signed = reports.len() >= 2 && reports.iter().all(|r| r.signature.is_some());
    let hashes_agree = match (
        reports.first().and_then(|r| r.transcript_hash.as_deref()),
        reports.get(1).and_then(|r| r.transcript_hash.as_deref()),
    ) {
        (Some(a), Some(b)) => !a.is_empty() && a == b,
        _ => false,
    };
    if both_signed && hashes_agree {
        SettleRoute::DirectRt
    } else {
        SettleRoute::ImmediateAsync
    }
}

pub fn report_message(match_id: &str, result: &str) -> String {
    format!("AMP_REPORT:v1:{match_id}:{result}")
}

/// Single-report default: the reporter's claim stands on expiry.
pub fn reconcile_default(
    reporter: &str,
    a: &str,
    _b: &str,
    reports: &[ReportRow],
) -> Option<Outcome> {
    let rep = reports.iter().find(|r| r.wallet == reporter)?;
    let is_a = reporter == a;
    match rep.result.as_str() {
        "win" => Some(if is_a { Outcome::WinA } else { Outcome::WinB }),
        "loss" => Some(if is_a { Outcome::WinB } else { Outcome::WinA }),
        "draw" => Some(Outcome::Draw),
        _ => None,
    }
}

pub fn outcome_str(o: Outcome) -> &'static str {
    match o {
        Outcome::WinA => "win_a",
        Outcome::WinB => "win_b",
        Outcome::Draw => "draw",
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RatingDelta {
    pub rating_before: f32,
    pub rating_after: f32,
    pub deviation_after: f32,
}

pub struct MatchService {
    store: Store,
    match_ttl_minutes: i64,
    escrow_window_minutes: i64,
    registry_game_id: u64,
    rt_grace_minutes: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppliedOutcome {
    pub outcome: &'static str,
    pub winner: Option<String>,
    pub player_a: RatingDelta,
    pub player_b: RatingDelta,
    pub attestation: Option<serde_json::Value>,
}

impl MatchService {
    pub fn new(
        store: Store,
        match_ttl_minutes: i64,
        escrow_window_minutes: i64,
        registry_game_id: u64,
        rt_grace_minutes: i64,
    ) -> Self {
        Self {
            store,
            match_ttl_minutes,
            escrow_window_minutes,
            registry_game_id,
            rt_grace_minutes,
        }
    }

    /// Create a live match from a queue pairing. Ratings come from the
    /// tickets (snapshotted at join time) so post-queue changes can't skew
    /// the update. `bot_match` marks a house practice-bot pairing: settled
    /// instantly from the player's single report, never rated.
    pub async fn create_match(
        &self,
        game_id: &str,
        ruleset_id: &str,
        a: &crate::queue::QueueEntry,
        b: &crate::queue::QueueEntry,
        bot_match: bool,
    ) -> Result<MatchRow, ApiError> {
        let id = Uuid::new_v4();
        let stake = if bot_match {
            0
        } else {
            a.stake_wei.max(b.stake_wei)
        };
        // Escrow window for staked pairs, play window otherwise.
        let expires_at = Utc::now()
            + Duration::minutes(if stake > 0 {
                self.escrow_window_minutes
            } else {
                self.match_ttl_minutes
            });
        // Deterministic on-chain match id for the registry escrow: the UUID's
        // upper 64 bits (122 bits of v4 randomness available).
        let on_chain_match_id = (id.as_u128() >> 64) as u64;
        let row = MatchRow {
            id,
            game_id: game_id.to_string(),
            ruleset_id: ruleset_id.to_string(),
            stake_wei: stake,
            state: if stake > 0 {
                "escrow_pending".into()
            } else {
                "live".into()
            },
            player_a: a.ticket.player_id.clone(),
            player_b: b.ticket.player_id.clone(),
            rating_a_snapshot: rating_json(a.ticket.mmr, a.ticket.mmr_uncertainty),
            rating_b_snapshot: rating_json(b.ticket.mmr, b.ticket.mmr_uncertainty),
            winner: None,
            outcome: None,
            attestation: None,
            on_chain_match_id: if stake > 0 {
                Some(on_chain_match_id as i64)
            } else {
                None
            },
            escrow_game_id: if stake > 0 {
                Some(self.registry_game_id as i64)
            } else {
                None
            },
            agreed_at: None,
            settle_deadline: None,
            bot_match: Some(bot_match),
            created_at: Utc::now(),
            expires_at,
            settled_at: None,
        };
        self.store
            .insert_match(&row, bot_match)
            .await
            .map_err(ApiError::Database)?;
        self.store
            .mark_tickets_matched(a.ticket_id, b.ticket_id, id)
            .await
            .map_err(ApiError::Database)?;
        Ok(row)
    }

    /// Settle a practice-bot match from the player's single report. No
    /// ratings, no attestation, no settlement — the bot exists to keep the
    /// player engaged through cold queues, not to mint skill.
    pub async fn finalize_bot_match(
        &self,
        m: &MatchRow,
        reporter: &str,
        result: &str,
    ) -> Result<AppliedOutcome, ApiError> {
        let outcome = match (result, reporter == m.player_a) {
            ("win", true) | ("loss", false) => Outcome::WinA,
            ("loss", true) | ("win", false) => Outcome::WinB,
            ("draw", _) => Outcome::Draw,
            other => return Err(ApiError::BadRequest(format!("bad result: {other:?}"))),
        };
        let winner = match outcome {
            Outcome::WinA => Some(m.player_a.clone()),
            Outcome::WinB => Some(m.player_b.clone()),
            Outcome::Draw => None,
        };
        self.store
            .set_match_outcome(m.id, "agreed", outcome_str(outcome), winner.as_deref())
            .await
            .map_err(ApiError::Database)?;
        // Mirror the player's own rating snapshot so the delta reads as zero.
        let (ra, rda, _) = rating_parts(&m.rating_a_snapshot);
        let (rb, rdb, _) = rating_parts(&m.rating_b_snapshot);
        Ok(AppliedOutcome {
            outcome: outcome_str(outcome),
            winner,
            player_a: RatingDelta {
                rating_before: ra,
                rating_after: ra,
                deviation_after: rda,
            },
            player_b: RatingDelta {
                rating_before: rb,
                rating_after: rb,
                deviation_after: rdb,
            },
            attestation: None,
        })
    }

    /// Apply an agreed outcome: update both Glicko-2 profiles from the
    /// pre-match snapshots, persist, sign the attestation, and (for staked
    /// matches) enqueue settlement.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_outcome(
        &self,
        m: &MatchRow,
        outcome: Outcome,
        transcript_hash: Option<&str>,
        signer: Option<&alloy_signer_local::PrivateKeySigner>,
        chain_id: u64,
        settlement: Option<alloy_primitives::Address>,
        reports: &[ReportRow],
    ) -> Result<AppliedOutcome, ApiError> {
        let (ra, rda, _va) = rating_parts(&m.rating_a_snapshot);
        let (rb, rdb, _vb) = rating_parts(&m.rating_b_snapshot);

        let (score_a, score_b) = match outcome {
            Outcome::WinA => (1.0, 0.0),
            Outcome::WinB => (0.0, 1.0),
            Outcome::Draw => (0.5, 0.5),
        };
        let (na, nda, nva) = glicko2_update(ra, rda, 0.06, rb, rdb, score_a);
        let (nb, ndb, nvb) = glicko2_update(rb, rdb, 0.06, ra, rda, score_b);

        let winner = match outcome {
            Outcome::WinA => Some(m.player_a.clone()),
            Outcome::WinB => Some(m.player_b.clone()),
            Outcome::Draw => None,
        };

        let route = settle_route(m, reports);
        let defer_settlement = matches!(route, SettleRoute::DirectRt);
        let terminal_state = match route {
            SettleRoute::DirectRt => "settling_rt",
            _ => "agreed",
        };
        self.store
            .mark_agreed(
                m.id,
                terminal_state,
                outcome_str(outcome),
                winner.as_deref(),
                self.rt_grace_minutes,
            )
            .await
            .map_err(ApiError::Database)?;

        self.store
            .apply_rating(
                &m.player_a,
                &m.game_id,
                &m.ruleset_id,
                na,
                nda,
                nva,
                score_a == 1.0,
                score_a == 0.0,
                score_a == 0.5,
            )
            .await
            .map_err(ApiError::Database)?;
        self.store
            .apply_rating(
                &m.player_b,
                &m.game_id,
                &m.ruleset_id,
                nb,
                ndb,
                nvb,
                score_b == 1.0,
                score_b == 0.0,
                score_b == 0.5,
            )
            .await
            .map_err(ApiError::Database)?;

        // Attestation: always sign when a verifier key is configured — even
        // free matches get a portable, verifiable skill record.
        let attestation = sign_and_store(
            self,
            m,
            outcome,
            transcript_hash,
            signer,
            chain_id,
            settlement,
            defer_settlement,
        )
        .await?;

        Ok(AppliedOutcome {
            outcome: outcome_str(outcome),
            winner,
            player_a: RatingDelta {
                rating_before: ra,
                rating_after: na,
                deviation_after: nda,
            },
            player_b: RatingDelta {
                rating_before: rb,
                rating_after: nb,
                deviation_after: ndb,
            },
            attestation,
        })
    }
}

fn rating_json(rating: f32, deviation: f32) -> serde_json::Value {
    serde_json::json!({
        "rating": rating,
        "deviation": deviation,
        "volatility": 0.06,
    })
}

fn rating_parts(v: &serde_json::Value) -> (f32, f32, f32) {
    (
        v["rating"].as_f64().unwrap_or(1500.0) as f32,
        v["deviation"].as_f64().unwrap_or(350.0) as f32,
        v["volatility"].as_f64().unwrap_or(0.06) as f32,
    )
}

#[allow(clippy::too_many_arguments)]
async fn sign_and_store(
    svc: &MatchService,
    m: &MatchRow,
    outcome: Outcome,
    transcript_hash: Option<&str>,
    signer: Option<&alloy_signer_local::PrivateKeySigner>,
    chain_id: u64,
    settlement: Option<alloy_primitives::Address>,
    defer_settlement: bool,
) -> Result<Option<serde_json::Value>, ApiError> {
    let (Some(signer), Some(settlement)) = (signer, settlement) else {
        return Ok(None);
    };
    let on_chain_id = m.on_chain_match_id.unwrap_or(0) as u64;
    let code = outcome_code(outcome_str(outcome)).unwrap_or(1);
    let th: alloy_primitives::B256 = transcript_hash
        .and_then(|h| h.trim_start_matches("0x").parse().ok())
        .unwrap_or(alloy_primitives::B256::ZERO);

    let (digest, sig) = sign_attestation(signer, chain_id, settlement, on_chain_id, code, th)
        .map_err(ApiError::Internal)?;

    let attestation = serde_json::json!({
        "type": "AsyncResult",
        "domain": { "name": "AMPSettlement", "version": "1", "chainId": chain_id, "verifyingContract": format!("{settlement:#x}") },
        "matchId": on_chain_id,
        "outcome": outcome_str(outcome),
        "outcomeCode": code,
        "transcriptHash": format!("{th:#x}"),
        "digest": format!("{digest:#x}"),
        "signature": format!("0x{}", hex::encode(&sig)),
        "signer": format!("{:#x}", signer.address()),
        "issuedAt": Utc::now().to_rfc3339(),
    });

    svc.store
        .set_attestation(m.id, attestation.clone())
        .await
        .map_err(ApiError::Database)?;

    // Staked + on-chain: hand off to the relayer for settlement (bps rake
    // is taken by the contract itself). Deferred when the players hold full
    // RT evidence — they get the grace window to settle directly; the sweep
    // enqueues this same job as fallback if they don't.
    if !defer_settlement && m.stake_wei > 0 && m.on_chain_match_id.is_some() {
        let payload = serde_json::json!({
            "matchUuid": m.id.to_string(),
            "onChainMatchId": on_chain_id,
            "outcomeCode": code,
            "transcriptHash": format!("{th:#x}"),
            "signature": format!("0x{}", hex::encode(&sig)),
        });
        svc.store
            .insert_settle_job(payload)
            .await
            .map_err(ApiError::Database)?;
        svc.store
            .set_match_state(m.id, "settling")
            .await
            .map_err(ApiError::Database)?;
    }

    Ok(Some(attestation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ReportRow;
    use chrono::Utc;

    fn report(wallet: &str, result: &str) -> ReportRow {
        ReportRow {
            wallet: wallet.into(),
            result: result.into(),
            transcript_hash: None,
            signature: None,
            submitted_at: Utc::now(),
        }
    }

    #[test]
    fn agreement_win_a() {
        let r = vec![report("a", "win"), report("b", "loss")];
        assert_eq!(reconcile("a", "b", &r), Some(Outcome::WinA));
    }

    #[test]
    fn agreement_win_b() {
        let r = vec![report("a", "loss"), report("b", "win")];
        assert_eq!(reconcile("a", "b", &r), Some(Outcome::WinB));
    }

    #[test]
    fn agreement_draw() {
        let r = vec![report("a", "draw"), report("b", "draw")];
        assert_eq!(reconcile("a", "b", &r), Some(Outcome::Draw));
    }

    #[test]
    fn conflict_is_disputed() {
        let r = vec![report("a", "win"), report("b", "win")];
        assert_eq!(reconcile("a", "b", &r), None);
    }

    #[test]
    fn matching_transcript_hashes_strengthen_agreement() {
        let mut ra = report("a", "win");
        ra.transcript_hash = Some("0xabc".into());
        let mut rb = report("b", "loss");
        rb.transcript_hash = Some("0xabc".into());
        assert_eq!(reconcile("a", "b", &[ra, rb]), Some(Outcome::WinA));
    }

    #[test]
    fn mismatched_transcript_hashes_dispute_even_if_results_agree() {
        let mut ra = report("a", "win");
        ra.transcript_hash = Some("0xabc".into());
        let mut rb = report("b", "loss");
        rb.transcript_hash = Some("0xdef".into());
        assert_eq!(reconcile("a", "b", &[ra, rb]), None);
    }

    #[test]
    fn one_sided_transcript_hash_does_not_block() {
        let mut ra = report("a", "win");
        ra.transcript_hash = Some("0xabc".into());
        let rb = report("b", "loss");
        assert_eq!(reconcile("a", "b", &[ra, rb]), Some(Outcome::WinA));
    }

    #[test]
    fn report_message_format_is_stable() {
        assert_eq!(report_message("m-123", "win"), "AMP_REPORT:v1:m-123:win");
    }

    #[test]
    fn partial_reports_do_not_agree() {
        let r = vec![report("a", "win")];
        assert_eq!(reconcile("a", "b", &r), None);
    }

    #[test]
    fn default_favors_reporter_on_expiry() {
        let r = vec![report("b", "win")];
        assert_eq!(reconcile_default("b", "a", "b", &r), Some(Outcome::WinB));
        let r2 = vec![report("a", "loss")];
        assert_eq!(reconcile_default("a", "a", "b", &r2), Some(Outcome::WinB));
        let r3 = vec![report("a", "draw")];
        assert_eq!(reconcile_default("a", "a", "b", &r3), Some(Outcome::Draw));
    }
}
