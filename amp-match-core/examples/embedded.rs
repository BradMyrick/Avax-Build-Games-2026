//! Standalone example: a complete match-tick loop using only `amp-match-core`.
//!
//! ```sh
//! cargo run --example embedded
//! ```
//!
//! This is what pulling AMP-quality matchmaking into your own game server
//! looks like — no Cap'n Proto, no server, no Avalanche anything. Just the
//! rating math + queue + rule evaluation.

use amp_match_core::{MatchQueue, PlayerTicket, RuleSet, glicko2};
use std::collections::HashMap;

/// Per-player persistent rating state: (rating, rd, volatility).
type Rating = (f32, f32, f32);

fn main() {
    let mut queue: MatchQueue<PlayerTicket> = MatchQueue::new();
    let mut ratings: HashMap<String, Rating> = HashMap::new();

    // Seed a population.
    for i in 0..6 {
        let name = format!("player_{i}");
        ratings.insert(name.clone(), (1500.0, 200.0, 0.06));
        enqueue_ticket(&mut queue, &name, 1500.0);
    }

    let ruleset = RuleSet::default_arc();

    // One match-tick: try to pair everyone we can.
    let mut round = 0;
    loop {
        let before = queue.len();
        for key in queue.bucket_keys() {
            while let Some(matched) = queue.try_match_bucket(&key, &ruleset, 10_000, 0) {
                round += 1;
                println!(
                    "Round {round}: {:<10} vs {:<10} (quality {:.2})",
                    matched.entry_a.player_id,
                    matched.entry_b.player_id,
                    matched.quality.total_score
                );

                // Snapshot BOTH ratings before updating either. This is the
                // correctness fix the audit flagged (P1 bug: sequential updates
                // gave the second player an asymmetric read of the first's
                // post-update rating).
                let a_pre = *ratings.get(&matched.entry_a.player_id).unwrap();
                let b_pre = *ratings.get(&matched.entry_b.player_id).unwrap();

                // Pretend the higher-rated player wins (deterministic demo).
                let a_score = if a_pre.0 >= b_pre.0 { 1.0 } else { 0.0 };

                let a_post =
                    glicko2::glicko2_update(a_pre.0, a_pre.1, a_pre.2, b_pre.0, b_pre.1, a_score);
                let b_post = glicko2::glicko2_update(
                    b_pre.0,
                    b_pre.1,
                    b_pre.2,
                    a_pre.0,
                    a_pre.1,
                    1.0 - a_score,
                );

                ratings.insert(matched.entry_a.player_id, a_post);
                ratings.insert(matched.entry_b.player_id, b_post);
            }
        }
        if queue.len() == before {
            break;
        }
    }

    println!("\nFinal ratings:");
    let mut sorted: Vec<_> = ratings.iter().collect();
    sorted.sort_by(|a, b| b.1.0.partial_cmp(&a.1.0).unwrap());
    for (name, (r, rd, _)) in sorted {
        println!("  {name:<10} rating={r:>7.1}  rd={rd:>5.1}");
    }

    println!("\nUnmatched in queue: {}", queue.len());
}

fn enqueue_ticket(queue: &mut MatchQueue<PlayerTicket>, name: &str, mmr: f32) {
    queue.push(PlayerTicket {
        player_id: name.into(),
        game_id: "demo".into(),
        ruleset_id: "default".into(),
        mmr,
        mmr_uncertainty: 200.0,
        region: "na".into(),
        preferred_role: "any".into(),
        language: "en".into(),
        max_ping_ms: 150,
        enqueued_at_ms: 0,
    });
}
