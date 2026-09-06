# amp-match-core

**Embeddable Glicko-2 matchmaking and rule-evaluation library. Pull this in if you want AMP-quality matchmaking without the AMP server.**

[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../LICENSE)

`amp-match-core` is the pure-logic heart of the [Avalanche Matchmaking Protocol](..): the rating math, the N-player party engine, the rule engine, and the queue. It contains **zero** RPC, persistence, or async machinery (plus exactly one audited crypto primitive — keccak256 — for v2 ticket commitments). If you want to embed AMP-quality matchmaking inside your own game server, peer-to-peer game, tournament platform, or analytics tool — without running the full AMP service — this is the crate.

## What's inside

| Module           | What it gives you                                                  |
|------------------|--------------------------------------------------------------------|
| `glicko2`        | Hardened Glicko-2 updates: the pairwise `glicko2_update`, and `glicko2_update_vs_many` — the Glickman-2013 rating-period form for N-player/team fields. **Bit-identical under any permutation of opponents** (fuzz-gated). |
| `party`          | N-player parties: validation (1–16, no duplicates), four skill aggregations (`Highest` / `Average` / `Weighted` / `AdjustedAverage` with tunable anti-boost lambda), aggregate tickets that flow through the queue unchanged, deterministic region resolution (strict majority → lowest-latency hub, 120ms spread gate), and gamma anti-boost rating recalibration. |
| `queue`          | Bucketed matchmaking: 1v1 pairing, `try_match_teams` (exact-size team packing with rating-window gates), `try_match_bucket_ffa` (all-or-nothing FFA lobbies), `drain_battle_royale` (wait-priority 16–64 player drainage with time-decayed windows). Generic over any `T: AsRef<PlayerTicket>`. |
| `ladder`         | Final placement arrays → per-player opponent/score vectors for `glicko2_update_vs_many`, with tie-aware ranks. |
| `commit`         | Anti-collusion: keccak256 ticket commitments (`H = keccak256(addr ‖ stake ‖ salt)`) and the blockhash-seeded, bias-free Fisher-Yates lobby shuffle. |
| `rules`          | Composable hard/soft constraint evaluation over skill, region, language, latency; `evaluate_parties` for team-vs-party formation. |
| `types`          | The canonical data model: `PlayerTicket` (now with `party_size`), `RuleSet`, `Rule`, `MatchQualityDetail`, all `*Params`. |

## What's NOT inside

- Cap'n Proto / RPC / networking
- Persistence (sled / redb / bincode)
- Signature *verification* / EIP-712 / Ethereum anything (keccak256 *hashing* is included for commitments)
- Async / tokio

## Formal gates

`tests/fuzz_gates.rs` enforces the v2 spec's test matrix:
- 100,000 randomized party graphs — validation accepts exactly the legal graphs, every accepted party materializes finite aggregates (no panics).
- Rating-period order-independence — any permutation of the opponent field yields bit-identical `(R', RD', sigma')`.
- Ladder complementarity — `score(i,j) + score(j,i) = 1` for every pair, ties included.
- Commitment binding — no collisions across distinct `(address, stake, salt)` triples.

## Install

```toml
[dependencies]
amp-match-core = "0.1"
```

(Fork-and-vendor also fine — this crate is intentionally narrow and self-contained.)

## Minimal example

```rust
use amp_match_core::{glicko2, MatchQueue, PlayerTicket, RuleSet};
use std::collections::HashMap;

let mut queue: MatchQueue<PlayerTicket> = MatchQueue::new();
let mut ratings: HashMap<String, (f32, f32, f32)> = HashMap::new();

// Players request matches.
queue.push(PlayerTicket {
    player_id: "alice".into(), game_id: "chess".into(), ruleset_id: "blitz".into(),
    mmr: 1500.0, mmr_uncertainty: 200.0, region: "na".into(),
    preferred_role: "white".into(), language: "en".into(),
    max_ping_ms: 100, enqueued_at_ms: 0,
});

// Each tick, try to pair within each (game, ruleset) bucket.
let ruleset = RuleSet::default_arc();
for key in queue.bucket_keys() {
    while let Some(matched) = queue.try_match_bucket(&key, &ruleset, 10_000, 0) {
        // Snapshot both ratings before updating either — this is the
        // correctness fix the audit flagged (the bug: sequential updates
        // gave the second player an asymmetric read of the first's
        // post-update rating, producing slow rating inflation).
        let a_pre = *ratings.entry(matched.entry_a.player_id.clone())
            .or_insert((1500.0, 200.0, 0.06));
        let b_pre = *ratings.entry(matched.entry_b.player_id.clone())
            .or_insert((1500.0, 200.0, 0.06));

        let a_score = 1.0; // alice wins
        let a_post = glicko2(a_pre.0, a_pre.1, a_pre.2, b_pre.0, b_pre.1, a_score);
        let b_post = glicko2(b_pre.0, b_pre.1, b_pre.2, a_pre.0, a_pre.1, 1.0 - a_score);

        ratings.insert(matched.entry_a.player_id, a_post);
        ratings.insert(matched.entry_b.player_id, b_post);
    }
}
```

Run a fuller end-to-end demo with `cargo run --example embedded`.

## Honest scope

This library is a **competent 2-player Glicko-2 matchmaker with composable rule evaluation**, not a FlexMatch/TrueSkill replacement. Specifically:

- **2-player only.** No team matchmaking, no party/squad support. The `TeamBalanceParams` types exist but the evaluator only scores pair-wise role complementarity.
- **No TrueSkill.** `SkillParams.use_trueskill` is reserved for future use; the algorithm is Glicko-2 only.
- **Several rule evaluators are stubs.** `Schedule`, `Inventory`, `Party`, `Custom`, `Avoidance`, `RecentMatches`, `ConnectionQuality` all return `{ passes: true, score: 1.0 }`. Their type definitions exist for parity with the AMP server and for forward compatibility.
- **Greedy, not globally optimal.** Pairing picks the best partner for each anchor sequentially. A globally-optimal (bipartite-matching) optimizer is a future option; the current path is fast and fine for moderate queue sizes.
- **Per-tick cost approaches O(N²) for very large buckets within a narrow MMR band.** For a 50 ms tick budget, the practical ceiling is ~2 000 players per `(game, ruleset)` bucket before tick overrun.
- **Glicko-2 stores ratings as f32.** Precision-degrading but consistent with the AMP server's persisted shape.
- **No RD inflation for inactive players.** The Glicko-2 spec's per-rating-period `sqrt(phi² + sigma²)` step is not run; `mmr_uncertainty` only changes when a match is recorded.

These limitations are documented and tracked in the AMP project's roadmap. Forks and PRs welcome.
