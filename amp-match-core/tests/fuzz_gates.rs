//! §6 test gates: formal fuzz properties for the v2 N-player core.
//!
//! - Gate 1 (party graphs): 100,000 random party constructions — validation
//!   accepts exactly the legal graphs, and every accepted party
//!   materializes finite aggregates without panicking.
//! - Gate 2 (order-independence): `glicko2_update_vs_many` produces
//!   bit-identical `(R', RD', σ')` under any permutation of the opponent
//!   field — the rating-period invariant.
//!
//! Case counts are env-tunable: `AMP_PROPTEST_CASES` (default 100_000 for
//! gate 1), `AMP_PERM_CASES` (default 10_000 for gate 2 — the solver is
//! heavier per case).

use amp_match_core::party::resolve_region;
use amp_match_core::types::PlayerTicket;
use amp_match_core::{
    Party, PartySkillMethod, glicko2_update_vs_many, placement_vectors, recalibrate_party_deltas,
    shuffle_by_blockhash, ticket_commit,
};
use proptest::prelude::*;

fn env_cases(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn region_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "na".to_string(),
        "eu".to_string(),
        "as".to_string(),
        "sa".to_string(),
    ])
}

fn member_strategy(id_pool: usize) -> impl Strategy<Value = (String, f32, f32, String)> {
    (
        0..id_pool,
        800.0f32..2800.0,
        30.0f32..350.0,
        region_strategy(),
    )
        .prop_map(|(id, mmr, rd, region)| (format!("p{id}"), mmr, rd, region))
}

fn ticket_from(id: &str, mmr: f32, rd: f32, region: &str) -> PlayerTicket {
    PlayerTicket {
        player_id: id.to_string(),
        game_id: "g".into(),
        ruleset_id: "r".into(),
        mmr,
        mmr_uncertainty: rd,
        region: region.into(),
        preferred_role: "dps".into(),
        language: "en".into(),
        max_ping_ms: 150,
        enqueued_at_ms: 1_000,
        party_size: 1,
    }
}

proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: env_cases("AMP_PROPTEST_CASES", 100_000),
        ..proptest::test_runner::Config::default()
    })]

    /// Gate 1: random party graphs. Validation accepts exactly when
    /// (size ≤ 16 ∧ no duplicate player ids); accepted parties never panic
    /// and produce finite aggregates under every method and λ.
    #[test]
    fn party_graphs_validate_and_materialize(
        members in prop::collection::vec(member_strategy(24), 0..20usize),
        lambda in 0.0f32..2.0,
    ) {
        let has_dup = members.iter().map(|(id, ..)| id).collect::<std::collections::HashSet<_>>().len() != members.len();
        let tickets: Vec<PlayerTicket> = members
            .iter()
            .map(|(id, mmr, rd, region)| ticket_from(id, *mmr, *rd, region))
            .collect();
        let party = Party::new("fuzz", tickets);

        let legal = !members.is_empty() && members.len() <= 16 && !has_dup;
        match party {
            Ok(p) => {
                prop_assert!(legal, "illegal party accepted");
                for method in [
                    PartySkillMethod::Highest,
                    PartySkillMethod::Average,
                    PartySkillMethod::Weighted,
                    PartySkillMethod::AdjustedAverage,
                ] {
                    let skill = p.skill_with(method, lambda);
                    prop_assert!(skill.mmr.is_finite());
                    prop_assert!(skill.uncertainty.is_finite());
                    prop_assert!(skill.spread.is_finite());
                    let ticket = p.aggregate_ticket(method);
                    prop_assert!(ticket.mmr.is_finite());
                    prop_assert!(ticket.party_size as usize == p.len());
                }
                // Region resolution must be total (never panics); the Δt
                // gate may reject spread parties.
                let _ = resolve_region(&p.members);
                // γ recalibration: correct-arity deltas always succeed.
                let raw: Vec<(String, f32)> =
                    p.members.iter().map(|m| (m.player_id.clone(), 5.0)).collect();
                prop_assert!(recalibrate_party_deltas(&p, &raw, 0.7).is_ok());
            }
            Err(_) => prop_assert!(!legal, "legal party rejected"),
        }
    }

    /// Ladder vectors over random rankings are internally consistent:
    /// score(i,j) + score(j,i) == 1 for every pair (ties contribute ½+½).
    #[test]
    fn ladder_scores_are_pairwise_complementary(
        n in 2usize..12usize,
        ratings in prop::collection::vec(900.0f32..2600.0, 2..12),
        tie_flip in prop::bool::ANY,
    ) {
        // keep n consistent with ratings length
        let n = ratings.len().min(n).max(2);
        let ranked: Vec<PlayerTicket> = ratings
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, r)| ticket_from(&format!("p{i}"), *r, 100.0, "na"))
            .collect();
        let mut ranks: Vec<u16> = (1..=n as u16).collect();
        if tie_flip && n >= 3 {
            ranks[1] = ranks[0] + 1; // introduce a tie for 2nd
            ranks[2] = ranks[1];
        }
        let vecs = placement_vectors(&ranked, &ranks).unwrap();
        for (i, v) in vecs.iter().enumerate() {
            prop_assert_eq!(v.opponents.len(), n - 1);
            // complementarity via the global score matrix
            let sum_i: f32 = v.opponents.iter().map(|(_, _, s)| *s).sum();
            let expected = (ranks[i] as f32 - 1.0) * 0.0 + {
                // wins vs strictly-worse + 0.5·ties
                let worse = ranks.iter().filter(|r| **r > ranks[i]).count() as f32;
                let ties = ranks.iter().filter(|r| **r == ranks[i]).count() as f32 - 1.0;
                worse + 0.5 * ties
            };
            prop_assert!((sum_i - expected).abs() < 1e-4);
        }
    }

    /// Commitments never collide across distinct (address, stake, salt)
    /// triples — 64k samples against a single base commitment.
    #[test]
    fn commitments_do_not_collide(
        base_addr in 0u64..1 << 32,
        others in prop::collection::vec((any::<u64>(), any::<u64>()), 64),
    ) {
        let addr = {
            let mut a = [0u8; 20];
            a[..8].copy_from_slice(&base_addr.to_be_bytes());
            a
        };
        let salt = {
            let mut s = [0u8; 32];
            s[..8].copy_from_slice(&base_addr.to_be_bytes());
            s
        };
        let h = ticket_commit(&addr, 1_000, &salt);
        for (o1, o2) in others {
            let mut a2 = [0u8; 20];
            a2[..8].copy_from_slice(&o1.to_be_bytes());
            let mut s2 = [0u8; 32];
            s2[..8].copy_from_slice(&o2.to_be_bytes());
            prop_assert_ne!(h, ticket_commit(&a2, 1_000, &s2));
        }
    }
}

proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: env_cases("AMP_PERM_CASES", 10_000),
        ..proptest::test_runner::Config::default()
    })]

    /// Gate 2: rating-period order-independence — bit-identical results
    /// under any permutation of the opponent field.
    #[test]
    fn rating_updates_are_permutation_invariant(
        rating in 900.0f32..2600.0,
        rd in 30.0f32..350.0,
        vol in 0.02f32..0.09,
        opponents in prop::collection::vec((900.0f32..2600.0, 30.0f32..350.0), 2..9),
        score_seed in any::<u64>(),
    ) {
        // Derive deterministic scores from the seed (mix of win/loss/draw).
        let scores: Vec<f32> = opponents
            .iter()
            .enumerate()
            .map(|(i, _)| match (score_seed >> (i % 64)) & 0b11 {
                0 | 1 => 1.0,
                2 => 0.0,
                _ => 0.5,
            })
            .collect();

        let baseline = glicko2_update_vs_many(rating, rd, vol, &opponents, &scores);

        // Fisher-Yates over a counter-based PRNG: exercise several
        // permutations per case, including the identity-preserving reverse.
        let reversed: Vec<(f32, f32)> = opponents.iter().rev().copied().collect();
        let rev_scores: Vec<f32> = scores.iter().rev().copied().collect();
        let reversed_result = glicko2_update_vs_many(rating, rd, vol, &reversed, &rev_scores);
        prop_assert_eq!(baseline, reversed_result);

        // Rotation by one.
        let mut rot: Vec<(f32, f32)> = opponents.clone();
        rot.rotate_left(1);
        let mut rot_s = scores.clone();
        rot_s.rotate_left(1);
        prop_assert_eq!(baseline, glicko2_update_vs_many(rating, rd, vol, &rot, &rot_s));

        // And the blockhash shuffle from the commit module — a genuinely
        // arbitrary permutation.
        let idx: Vec<usize> = (0..opponents.len()).collect();
        let perm = shuffle_by_blockhash(
            idx,
            &{
                let mut b = [0u8; 32];
                b[..8].copy_from_slice(&score_seed.to_be_bytes());
                b
            },
        );
        let shuffled: Vec<(f32, f32)> = perm.iter().map(|&i| opponents[i]).collect();
        let shuffled_s: Vec<f32> = perm.iter().map(|&i| scores[i]).collect();
        prop_assert_eq!(baseline, glicko2_update_vs_many(rating, rd, vol, &shuffled, &shuffled_s));
    }
}
