//! # amp-match-core
//!
//! Embeddable Glicko-2 matchmaking and rule-evaluation library.
//!
//! Pulls in **zero** server / RPC / crypto machinery — just the algorithms.
//! Use this if you want AMP-quality matchmaking inside your own game server,
//! peer-to-peer game, tournament platform, or analytics tool, without running
//! the full AMP service.
//!
//! ## What's inside
//!
//! - [`glicko2::glicko2_update`] — a hardened Glicko-2 rating update with
//!   NaN/Inf/non-positive-volatility guards and a bounded Illinois-method
//!   solver. Returns the original profile unchanged on any pathological input
//!   so a corrupt profile can never poison the queue.
//! - [`rules::evaluate_rules`] — composable hard/soft constraint evaluation
//!   over skill, region, language, latency, and more. Each rule contributes a
//!   weighted score; hard constraints gate pairing outright.
//! - [`queue::MatchQueue`] — a bucketed matchmaking queue that pairs entries
//!   within the same `(game, ruleset)` bucket via a sorted-MMR scan with
//!   skill-window gating. Generic over any entry type that exposes a
//!   [`types::PlayerTicket`] via `AsRef`, so it works equally well for a
//!   server's wrapped `QueueEntry { ticket, sender }` or a library user's
//!   plain `PlayerTicket`.
//! - [`types`] — the canonical data model: `PlayerTicket`, `RuleSet`,
//!   `Rule`, `MatchQualityDetail`, `MatchOutcome`, all `*Params`.
//!
//! ## What's NOT inside
//!
//! - Cap'n Proto / RPC / networking
//! - Persistence (sled / redb / bincode)
//! - Signatures / EIP-712 / Ethereum anything
//! - Async / tokio
//!
//! ## Example: embed in a game loop
//!
//! ```no_run
//! use amp_match_core::{glicko2_update, MatchQueue, PlayerTicket, RuleSet};
//! use std::collections::HashMap;
//!
//! let mut queue: MatchQueue<PlayerTicket> = MatchQueue::new();
//! let mut ratings: HashMap<String, (f32, f32, f32)> = HashMap::new();
//!
//! // Players request matches.
//! queue.push(PlayerTicket {
//!     player_id: "alice".into(), game_id: "chess".into(), ruleset_id: "blitz".into(),
//!     mmr: 1500.0, mmr_uncertainty: 200.0, region: "na".into(),
//!     preferred_role: "white".into(), language: "en".into(),
//!     max_ping_ms: 100, enqueued_at_ms: 0,
//! });
//!
//! // Each tick, try to pair within each (game, ruleset) bucket.
//! let ruleset = RuleSet::default_arc();
//! for key in queue.bucket_keys() {
//!     while let Some(matched) = queue.try_match_bucket(&key, &ruleset, 10_000, 0) {
//!         // Run your game simulation for matched.entry_a vs matched.entry_b,
//!         // then snapshot BOTH ratings before updating either.
//!         let (ra, rd, rv) = ratings.entry(matched.entry_a.player_id.clone())
//!             .or_insert((1500.0, 200.0, 0.06));
//!         let (ra, rd, rv) = (*ra, *rd, *rv);
//!         let (rb, rbd, rbv) = ratings.entry(matched.entry_b.player_id.clone())
//!             .or_insert((1500.0, 200.0, 0.06));
//!         let (rb, rbd, rbv) = (*rb, *rbd, *rbv);
//!         let (new_ra, new_rda, new_va) =
//!             glicko2_update(ra, rd, rv, rb, rbd, 1.0 /* a wins */);
//!         let (new_rb, new_rdb, new_vb) =
//!             glicko2_update(rb, rbd, rbv, ra, rd, 0.0 /* b loses */);
//!         ratings.insert(matched.entry_a.player_id, (new_ra, new_rda, new_va));
//!         ratings.insert(matched.entry_b.player_id, (new_rb, new_rdb, new_vb));
//!     }
//! }
//! ```

pub mod glicko2;
pub mod queue;
pub mod rules;
pub mod types;

pub use glicko2::glicko2_update;
pub use queue::{MatchOutcome, MatchQueue};
pub use rules::{RuleEvaluationResult, evaluate_rules};
pub use types::{
    AvoidanceParams, BackfillPolicy, ConnectionQualityParams, CustomParams, InventoryParams,
    LanguageParams, LatencyParams, MatchQualityDetail, PartyParams, PartySkillMethod,
    PingBasedParams, PlayerTicket, PreferenceParams, RecentMatchesParams, RegionParams, Rule,
    RuleParams, RuleSet, RuleType, ScheduleParams, SkillDecayParams, SkillParams,
    TeamBalanceParams,
};
