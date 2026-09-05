//! Integration test: prove the library works without any amp-server / async /
//! crypto / RPC machinery. If this test compiles and passes, the embeddable
//! library surface is complete.

use amp_match_core::{MatchQueue, PlayerTicket, RuleSet, evaluate_rules, glicko2_update};
use std::sync::Arc;

fn ticket(p: &str, mmr: f32) -> PlayerTicket {
    PlayerTicket {
        player_id: p.into(),
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
fn full_match_then_rate_loop() {
    let mut queue: MatchQueue<PlayerTicket> = MatchQueue::new();
    queue.push(ticket("alice", 1500.0));
    queue.push(ticket("bob", 1490.0));
    queue.push(ticket("carol", 1495.0));

    let ruleset: Arc<RuleSet> = RuleSet::default_arc();

    // First tick: alice+bob OR alice+carol OR bob+carol, one pair.
    let keys = queue.bucket_keys();
    assert_eq!(keys.len(), 1);

    let first = queue
        .try_match_bucket(&keys[0], &ruleset, 10_000, 0)
        .expect("at least one pair in range");

    // Both players were removed; one remains.
    assert_eq!(queue.len(), 1);
    assert_ne!(first.entry_a.player_id, first.entry_b.player_id);

    // Quality is sane.
    assert!(first.quality.total_score > 0.0);

    // Rating update — symmetric (snapshot both, update both against the
    // pre-update opponent values).
    let alice_pre = (1500.0_f32, 200.0_f32, 0.06_f32);
    let bob_pre = (1500.0_f32, 200.0_f32, 0.06_f32);
    let alice_wins = 1.0;
    let alice_post = glicko2_update(
        alice_pre.0,
        alice_pre.1,
        alice_pre.2,
        bob_pre.0,
        bob_pre.1,
        alice_wins,
    );
    let bob_post = glicko2_update(
        bob_pre.0,
        bob_pre.1,
        bob_pre.2,
        alice_pre.0,
        alice_pre.1,
        1.0 - alice_wins,
    );
    assert!(alice_post.0 > alice_pre.0, "winner rating must rise");
    assert!(bob_post.0 < bob_pre.0, "loser rating must fall");
}

#[test]
fn library_has_no_amplifier_dependencies() {
    // Smoke check: evaluate_rules is callable without any server / RPC /
    // persistence context. If this fails to compile, the library surface
    // leaked an unwanted dependency.
    let a = ticket("a", 1500.0);
    let b = ticket("b", 1550.0);
    let rs = RuleSet::default_arc();
    let r = evaluate_rules(&a, &b, &rs);
    assert!(r.passes);
}

#[test]
fn glicko2_handles_corrupt_inputs_without_poisoning() {
    let (r, _, _) = glicko2_update(f32::NAN, 200.0, 0.06, 1400.0, 30.0, 1.0);
    assert!(
        r.is_nan(),
        "NaN echoes back unchanged — not synthesized fresh"
    );

    let (r, _, _) = glicko2_update(1500.0, 200.0, 0.0, 1400.0, 30.0, 1.0);
    assert_eq!(
        r, 1500.0,
        "non-positive volatility returns the original rating"
    );
}
