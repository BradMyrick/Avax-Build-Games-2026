//! Rule evaluation: composable hard/soft constraints over two player tickets.
//!
//! Each rule contributes a weighted score; `is_hard_constraint` rules gate
//! pairing outright (a failed hard rule sets `passes = false`). Backfill
//! (optional, time-triggered) can relax the skill gate after a configurable
//! queue-duration threshold.

use crate::types::{
    AvoidanceParams, BackfillPolicy, ConnectionQualityParams, CustomParams, InventoryParams,
    LanguageParams, LatencyParams, MatchQualityDetail, PartyParams, PingBasedParams, PlayerTicket,
    PreferenceParams, RecentMatchesParams, RegionParams, Rule, RuleParams, RuleType,
    ScheduleParams, SkillDecayParams, SkillParams, TeamBalanceParams,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock millis since UNIX_EPOCH. Library users can override this by
/// constructing their own `PlayerTicket.enqueued_at_ms`; the library uses
/// this default only when computing effective skill decay from queue duration.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct RuleEvaluationResult {
    pub passes: bool,
    pub quality: MatchQualityDetail,
}

/// Evaluate the full rule chain for a candidate pair. Pure function — no I/O,
/// no async. The `RuleSet` is passed by `&Arc` to match the AMP server's
/// hot-path shape (snapshot-then-iterate); library users can `RuleSet::default_arc()`
/// or build their own.
pub fn evaluate_rules(
    entry_a: &PlayerTicket,
    entry_b: &PlayerTicket,
    ruleset: &Arc<crate::types::RuleSet>,
) -> RuleEvaluationResult {
    let mut quality = MatchQualityDetail::default();
    let queue_duration_a = now_ms().saturating_sub(entry_a.enqueued_at_ms);
    let queue_duration_b = now_ms().saturating_sub(entry_b.enqueued_at_ms);
    let max_queue_duration = queue_duration_a.max(queue_duration_b);

    let effective_skill_diff = compute_effective_skill_diff(ruleset, max_queue_duration);

    // `Some(rule_type)` of the FIRST hard constraint that failed this pass,
    // `None` if all hard constraints passed. Backfill consults this to
    // decide whether to relax.
    let mut failed_hard_rule: Option<RuleType> = None;
    let mut total_weight: f32 = 0.0;

    for rule in &ruleset.rules {
        if rule.is_hard_constraint && failed_hard_rule.is_some() {
            break;
        }

        let result = evaluate_single_rule(
            rule,
            entry_a,
            entry_b,
            effective_skill_diff,
            max_queue_duration,
        );

        if rule.is_hard_constraint && !result.passes && failed_hard_rule.is_none() {
            failed_hard_rule = Some(rule.rule_type);
        }

        accumulate_quality(&mut quality, &rule.rule_type, result.score, rule.weight);
        total_weight += rule.weight;
    }

    // Compute `hard_pass` from `failed_hard_rule` AFTER the loop. If backfill
    // is active and the queue-duration threshold has elapsed, relax ONLY
    // skill-family hard failures (Skill / SkillDecay). Region / Language /
    // Latency / PingBased hard failures stay gating — backfill is meant to
    // widen the skill window, not override player-side invariants.
    let backfill_active = ruleset
        .backfill
        .as_ref()
        .map(|b| b.enabled && max_queue_duration > b.max_time_ms)
        .unwrap_or(false);

    if backfill_active && let Some(ref backfill) = ruleset.backfill {
        apply_backfill(
            &mut quality,
            backfill,
            entry_a,
            entry_b,
            effective_skill_diff,
        );
    }

    let hard_pass = match failed_hard_rule {
        None => true,
        Some(RuleType::Skill) | Some(RuleType::SkillDecay) if backfill_active => true,
        _ => false,
    };

    if total_weight > 0.0 {
        quality.total_score /= total_weight;
    }
    quality.total_score = quality.total_score.clamp(0.0, 1.0);

    RuleEvaluationResult {
        passes: hard_pass,
        quality,
    }
}

fn compute_effective_skill_diff(ruleset: &crate::types::RuleSet, queue_duration_ms: u64) -> f32 {
    let base = ruleset.max_skill_diff;
    for rule in &ruleset.rules {
        if let RuleParams::Skill(sp) = &rule.params
            && sp.time_decay
            && sp.decay_rate > 0.0
        {
            let minutes = queue_duration_ms as f32 / 60_000.0;
            return base + sp.decay_rate * minutes * base;
        }
    }
    base
}

struct SingleResult {
    passes: bool,
    score: f32,
}

fn evaluate_single_rule(
    rule: &Rule,
    a: &PlayerTicket,
    b: &PlayerTicket,
    effective_skill_diff: f32,
    _queue_duration_ms: u64,
) -> SingleResult {
    match &rule.params {
        RuleParams::Skill(params) => evaluate_skill(params, a, b, effective_skill_diff),
        RuleParams::Latency(params) => evaluate_latency(params, a, b),
        RuleParams::Region(params) => evaluate_region(params, a, b),
        RuleParams::Language(params) => evaluate_language(params, a, b),
        RuleParams::TeamBalance(params) => evaluate_team_balance(params, a, b),
        RuleParams::Avoidance(params) => evaluate_avoidance(params, a, b),
        RuleParams::Preference(params) => evaluate_preference(params, a, b),
        RuleParams::ConnectionQuality(params) => evaluate_connection_quality(params, a, b),
        RuleParams::Schedule(_) => SingleResult {
            passes: true,
            score: 1.0,
        },
        RuleParams::Inventory(_) => SingleResult {
            passes: true,
            score: 1.0,
        },
        RuleParams::Party(_) => SingleResult {
            passes: true,
            score: 1.0,
        },
        RuleParams::Custom(_) => SingleResult {
            passes: true,
            score: 1.0,
        },
        RuleParams::PingBased(params) => evaluate_latency(
            &LatencyParams {
                max_ping_ms: params.max_ping_ms,
                measurement_method: "direct".to_string(),
                allow_region_override: params.region_override,
            },
            a,
            b,
        ),
        RuleParams::SkillDecay(params) => evaluate_skill_decay(params, a, b),
        RuleParams::RecentMatches(_) => SingleResult {
            passes: true,
            score: 1.0,
        },
    }
}

fn evaluate_skill(
    _params: &SkillParams,
    a: &PlayerTicket,
    b: &PlayerTicket,
    effective_max: f32,
) -> SingleResult {
    let diff = (a.mmr - b.mmr).abs();
    let max_diff = effective_max.max(1.0);
    let score = (1.0 - (diff / max_diff).min(1.0)).max(0.0);
    SingleResult {
        passes: diff <= effective_max,
        score,
    }
}

fn evaluate_latency(params: &LatencyParams, a: &PlayerTicket, b: &PlayerTicket) -> SingleResult {
    let max_ping = a.max_ping_ms.min(b.max_ping_ms).min(params.max_ping_ms);
    let estimated_ping = estimate_ping(&a.region, &b.region);
    if estimated_ping <= max_ping as f32 {
        SingleResult {
            passes: true,
            score: 1.0 - (estimated_ping / max_ping as f32).min(1.0),
        }
    } else if params.allow_region_override {
        SingleResult {
            passes: true,
            score: 0.3,
        }
    } else {
        SingleResult {
            passes: false,
            score: 0.0,
        }
    }
}

fn evaluate_region(params: &RegionParams, a: &PlayerTicket, b: &PlayerTicket) -> SingleResult {
    if a.region == b.region {
        return SingleResult {
            passes: true,
            score: 1.0,
        };
    }
    if params.allowed_regions.contains(&a.region) && params.allowed_regions.contains(&b.region) {
        return SingleResult {
            passes: true,
            score: 0.5,
        };
    }
    SingleResult {
        passes: false,
        score: 0.0,
    }
}

fn evaluate_language(params: &LanguageParams, a: &PlayerTicket, b: &PlayerTicket) -> SingleResult {
    if !params.prefer_same {
        return SingleResult {
            passes: true,
            score: 1.0,
        };
    }
    if a.language == b.language {
        SingleResult {
            passes: true,
            score: 1.0,
        }
    } else {
        SingleResult {
            passes: true,
            score: 0.5,
        }
    }
}

fn evaluate_team_balance(
    params: &TeamBalanceParams,
    a: &PlayerTicket,
    b: &PlayerTicket,
) -> SingleResult {
    if params.required_roles.is_empty() {
        return SingleResult {
            passes: true,
            score: 1.0,
        };
    }
    if a.preferred_role == b.preferred_role && params.max_duplicates > 0 {
        return SingleResult {
            passes: true,
            score: 0.6,
        };
    }
    let both_have_roles = params.required_roles.iter().any(|r| r == &a.preferred_role)
        && params.required_roles.iter().any(|r| r == &b.preferred_role);
    SingleResult {
        passes: both_have_roles || params.flex_slots > 0,
        score: if both_have_roles { 1.0 } else { 0.5 },
    }
}

fn evaluate_avoidance(
    _params: &AvoidanceParams,
    _a: &PlayerTicket,
    _b: &PlayerTicket,
) -> SingleResult {
    SingleResult {
        passes: true,
        score: 1.0,
    }
}

fn evaluate_preference(
    params: &PreferenceParams,
    _a: &PlayerTicket,
    _b: &PlayerTicket,
) -> SingleResult {
    SingleResult {
        passes: true,
        score: if params.prefer_new_opponents {
            0.8
        } else {
            1.0
        },
    }
}

fn evaluate_connection_quality(
    _params: &ConnectionQualityParams,
    _a: &PlayerTicket,
    _b: &PlayerTicket,
) -> SingleResult {
    SingleResult {
        passes: true,
        score: 1.0,
    }
}

fn evaluate_skill_decay(
    params: &SkillDecayParams,
    a: &PlayerTicket,
    b: &PlayerTicket,
) -> SingleResult {
    let diff = (a.mmr - b.mmr).abs();
    let floor = params.min_mmr;
    let a_floor = a.mmr >= floor;
    let b_floor = b.mmr >= floor;
    SingleResult {
        passes: a_floor && b_floor,
        score: if diff < 200.0 { 1.0 } else { 0.5 },
    }
}

fn accumulate_quality(
    quality: &mut MatchQualityDetail,
    rule_type: &RuleType,
    score: f32,
    weight: f32,
) {
    let weighted = score * weight;
    quality.total_score += weighted;
    match rule_type {
        RuleType::Skill | RuleType::SkillDecay => quality.skill_balance += weighted,
        RuleType::Latency | RuleType::PingBased => quality.latency_score += weighted,
        RuleType::Region => quality.region_score += weighted,
        RuleType::Language => quality.language_score += weighted,
        RuleType::TeamBalance => quality.role_balance += weighted,
        _ => {}
    }
}

fn apply_backfill(
    quality: &mut MatchQualityDetail,
    policy: &BackfillPolicy,
    a: &PlayerTicket,
    b: &PlayerTicket,
    effective_skill_diff: f32,
) {
    let skill_diff = (a.mmr - b.mmr).abs();
    let relaxed_max = effective_skill_diff * policy.skill_tolerance_multiplier;
    if skill_diff <= relaxed_max {
        quality.skill_balance = (quality.skill_balance * 0.7).max(0.2);
    }
    if policy.role_flexibility {
        quality.role_balance = (quality.role_balance * 0.8).max(0.3);
    }
    if policy.connection_tolerance {
        quality.latency_score = (quality.latency_score * 0.8).max(0.2);
    }
    quality.total_score = quality.skill_balance
        + quality.role_balance
        + quality.region_score
        + quality.latency_score
        + quality.language_score;
}

/// Hardcoded inter-region ping estimates. Same-region: 20 ms; cross-region:
/// pairs of {na, eu, sa, as} range from 90-250 ms; unknown region: 150 ms.
pub fn estimate_ping(region_a: &str, region_b: &str) -> f32 {
    if region_a == region_b {
        return 20.0;
    }
    match (region_a, region_b) {
        ("na", "eu") | ("eu", "na") => 90.0,
        ("na", "sa") | ("sa", "na") => 120.0,
        ("na", "as") | ("as", "na") => 180.0,
        ("eu", "as") | ("as", "eu") => 160.0,
        ("eu", "sa") | ("sa", "eu") => 170.0,
        ("as", "sa") | ("sa", "as") => 250.0,
        _ => 150.0,
    }
}

// Unused-parameter imports kept for API symmetry; future rule implementations
// will consume them. Suppressing the dead-code warning here is preferable to
// re-importing the same types in a future change.
#[allow(dead_code)]
fn _silence_unused_import_warnings() {
    let _ = (
        std::marker::PhantomData::<ScheduleParams>,
        std::marker::PhantomData::<InventoryParams>,
        std::marker::PhantomData::<PartyParams>,
        std::marker::PhantomData::<CustomParams>,
        std::marker::PhantomData::<RecentMatchesParams>,
        std::marker::PhantomData::<PingBasedParams>,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PlayerTicket, RuleSet};

    fn ticket(player: &str, mmr: f32) -> PlayerTicket {
        PlayerTicket {
            player_id: player.into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr,
            mmr_uncertainty: 200.0,
            region: "na".into(),
            preferred_role: "tank".into(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: 0,
        }
    }

    #[test]
    fn default_skill_passes_within_range() {
        let a = ticket("a", 1500.0);
        let b = ticket("b", 1600.0);
        let rs = RuleSet::default_arc();
        let r = evaluate_rules(&a, &b, &rs);
        assert!(r.passes);
        assert!(r.quality.total_score > 0.5);
    }

    #[test]
    fn default_skill_fails_outside_range() {
        let a = ticket("a", 1500.0);
        let b = ticket("b", 3000.0);
        let rs = RuleSet::default_arc();
        let r = evaluate_rules(&a, &b, &rs);
        assert!(!r.passes);
    }

    use proptest::prelude::*;
    proptest! {
        #[test]
        fn prop_eval_is_symmetric(
            mmr_a in 0f32..3000f32,
            mmr_b in 0f32..3000f32,
        ) {
            let a = ticket("a", mmr_a);
            let b = ticket("b", mmr_b);
            let rs = RuleSet::default_arc();
            let r1 = evaluate_rules(&a, &b, &rs);
            let r2 = evaluate_rules(&b, &a, &rs);
            // Skill score is symmetric; total_score may differ only due to
            // queue_duration asymmetry, but the pass/fail gate is symmetric.
            prop_assert_eq!(r1.passes, r2.passes);
            prop_assert!((r1.quality.skill_balance - r2.quality.skill_balance).abs() < 0.001);
        }
    }

    /// Regression test for the backfill over-clear bug: backfill must relax
    /// ONLY skill-family hard-constraint failures, not region / language /
    /// latency / ping. Pre-fix, `if !hard_pass { hard_pass = true; }` ran
    /// unconditionally when backfill triggered, pairing players who had
    /// explicitly failed non-skill hard constraints.
    #[test]
    fn backfill_does_not_override_non_skill_hard_constraints() {
        use crate::types::{
            BackfillPolicy, PlayerTicket, RegionParams, Rule, RuleParams, RuleSet, RuleType,
        };

        let a = PlayerTicket {
            player_id: "a".into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr: 1500.0,
            mmr_uncertainty: 200.0,
            region: "na".into(),
            preferred_role: "tank".into(),
            language: "en".into(),
            max_ping_ms: 50,
            // Old enqueue time → backfill window has elapsed.
            enqueued_at_ms: now_ms().saturating_sub(60_000),
        };
        let b = PlayerTicket {
            player_id: "b".into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr: 1500.0,
            mmr_uncertainty: 200.0,
            region: "eu".into(),
            preferred_role: "dps".into(),
            language: "en".into(),
            max_ping_ms: 50,
            enqueued_at_ms: now_ms().saturating_sub(60_000),
        };

        // Region rule: na vs eu, allowed_regions = ["na"] only — b's region
        // is NOT in the allowlist, so this hard constraint MUST fail and
        // backfill MUST NOT save it.
        let region_rule = Rule {
            rule_id: "region".into(),
            name: "Region".into(),
            rule_type: RuleType::Region,
            params: RuleParams::Region(RegionParams {
                allowed_regions: vec!["na".into()],
                cross_region_after_ms: u64::MAX,
            }),
            weight: 0.5,
            is_hard_constraint: true,
            priority: 1,
        };

        let mut rs = RuleSet::default();
        rs.rules.push(region_rule);
        rs.backfill = Some(BackfillPolicy {
            enabled: true,
            max_time_ms: 5_000, // backfill active after 5 s
            ..BackfillPolicy::default()
        });

        let rs_arc = std::sync::Arc::new(rs);
        let r = evaluate_rules(&a, &b, &rs_arc);
        assert!(
            !r.passes,
            "backfill must NOT relax region hard constraints — region mismatch should still fail."
        );
    }

    /// Sanity: backfill DOES still relax a skill-only failure, otherwise the
    /// above fix could over-correct and break backfill entirely.
    #[test]
    fn backfill_relaxes_skill_hard_constraint() {
        use crate::types::{BackfillPolicy, PlayerTicket, RuleSet};

        let a = PlayerTicket {
            player_id: "a".into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr: 1500.0,
            mmr_uncertainty: 200.0,
            region: "na".into(),
            preferred_role: "tank".into(),
            language: "en".into(),
            max_ping_ms: 200,
            enqueued_at_ms: now_ms().saturating_sub(60_000),
        };
        let b = PlayerTicket {
            player_id: "b".into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            // 800 MMR gap — exceeds the default 500 skill diff, so the
            // default skill rule hard-fails without backfill.
            mmr: 2300.0,
            mmr_uncertainty: 200.0,
            region: "na".into(),
            preferred_role: "dps".into(),
            language: "en".into(),
            max_ping_ms: 200,
            enqueued_at_ms: now_ms().saturating_sub(60_000),
        };

        let rs = RuleSet {
            backfill: Some(BackfillPolicy {
                enabled: true,
                max_time_ms: 5_000,
                ..BackfillPolicy::default()
            }),
            ..RuleSet::default()
        };
        let rs_arc = std::sync::Arc::new(rs);
        let r = evaluate_rules(&a, &b, &rs_arc);
        assert!(
            r.passes,
            "backfill SHOULD relax skill-only hard failures after the max_time_ms window"
        );
    }
}
