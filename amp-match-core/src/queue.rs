//! Bucketed matchmaking queue. Generic over any entry type `T` that exposes
//! a [`PlayerTicket`] via `AsRef`, so the queue works equally well for:
//!
//! - **Library users**: `MatchQueue<PlayerTicket>` — the simplest case.
//! - **The AMP server**: `MatchQueue<QueueEntry>` where `QueueEntry` wraps a
//!   `PlayerTicket` + a notification sender. The matching algorithm never
//!   touches the sender — it operates on `ticket.mmr` etc. via `AsRef`.
//!
//! Pairing strategy: within each `(game_id, ruleset_id)` bucket, entries are
//! kept sorted by MMR. On each `try_match_bucket` call, we anchor on each
//! entry in order, scan forward until the candidate MMR exceeds the anchor's
//! upper bound, and select the best-scoring partner (via
//! [`crate::rules::evaluate_rules`]). Both entries are removed atomically.
//!
//! This is O(N) per anchor within the skill window, not O(log N) — the
//! "binary search" name in older docs referred to the sorted-insert step,
//! not the search step. For sparse buckets this is fine; for very large
//! buckets within a narrow MMR band (e.g. 10 000 players within 500 MMR
//! points), per-tick cost approaches O(N²). The AMP server mitigates this
//! with a 50 ms tick budget; library users should be aware of the scaling
//! shape.

use crate::rules::evaluate_rules;
use crate::types::{MatchQualityDetail, PlayerTicket, RuleSet};
use std::collections::HashMap;
use std::sync::Arc;

type BucketKey = (String, String);

/// A successful pairing. `entry_a` and `entry_b` are the two consumed queue
/// entries (with their wrappers intact, so the server can pull senders out);
/// `quality` is the score breakdown from rule evaluation.
pub struct MatchOutcome<T> {
    pub entry_a: T,
    pub entry_b: T,
    pub quality: MatchQualityDetail,
}

pub struct MatchQueue<T: AsRef<PlayerTicket>> {
    buckets: HashMap<BucketKey, Vec<T>>,
    total_count: usize,
}

impl<T: AsRef<PlayerTicket>> MatchQueue<T> {
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            total_count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.total_count
    }

    /// `true` if no entries are queued.
    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }

    /// Enqueue `entry`. A duplicate request (same `player_id`) replaces any
    /// existing entry across all buckets and returns `true`; the caller is
    /// responsible for dropping / superseding the prior entry's side data
    /// (e.g. the AMP server's `oneshot::Sender`).
    pub fn push(&mut self, entry: T) -> bool {
        let replaced = self.remove_player(&entry.as_ref().player_id);

        let key = (
            entry.as_ref().game_id.clone(),
            entry.as_ref().ruleset_id.clone(),
        );
        let bucket = self.buckets.entry(key).or_default();
        let mmr = entry.as_ref().mmr;
        let pos = match bucket.as_slice().binary_search_by(|e: &T| {
            e.as_ref()
                .mmr
                .partial_cmp(&mmr)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Ok(p) => p,
            Err(p) => p,
        };
        bucket.insert(pos, entry);
        self.total_count += 1;
        replaced
    }

    /// Remove any queued entry for `player_id`. Returns `true` if an entry
    /// was removed.
    pub fn remove_player(&mut self, player_id: &str) -> bool {
        let mut removed = false;
        for bucket in self.buckets.values_mut() {
            let before = bucket.len();
            bucket.retain(|e| e.as_ref().player_id != player_id);
            let gone = before - bucket.len();
            self.total_count = self.total_count.saturating_sub(gone);
            removed |= gone > 0;
        }
        removed
    }

    pub fn contains_player(&self, player_id: &str) -> bool {
        self.buckets
            .values()
            .any(|b| b.iter().any(|e| e.as_ref().player_id == player_id))
    }

    pub fn drain_all(&mut self) -> Vec<T> {
        let mut all = Vec::with_capacity(self.total_count);
        for (_, bucket) in self.buckets.drain() {
            all.extend(bucket);
        }
        self.total_count = 0;
        all
    }

    /// Try to produce one pairing from the named bucket. Returns `None` if
    /// the bucket has fewer than 2 entries, no pairing passes the ruleset,
    /// or the active-match cap is hit.
    pub fn try_match_bucket(
        &mut self,
        key: &BucketKey,
        ruleset: &Arc<RuleSet>,
        max_active: usize,
        current_active: usize,
    ) -> Option<MatchOutcome<T>> {
        if current_active >= max_active {
            return None;
        }

        let bucket = self.buckets.get_mut(key)?;
        if bucket.len() < 2 {
            return None;
        }

        let max_diff = ruleset.max_skill_diff;

        let mut i = 0;
        while i < bucket.len() {
            let entry_a_mmr = bucket[i].as_ref().mmr;
            let hi = entry_a_mmr + max_diff;
            let entry_a_id = bucket[i].as_ref().player_id.clone();

            let search_start = i + 1;

            let mut best_j: Option<usize> = None;
            let mut best_score = 0.0f32;

            for (j, candidate) in bucket.iter().enumerate().skip(search_start) {
                if candidate.as_ref().mmr > hi {
                    break;
                }
                if candidate.as_ref().player_id == entry_a_id {
                    continue;
                }

                let result = evaluate_rules(bucket[i].as_ref(), candidate.as_ref(), ruleset);
                if result.passes && result.quality.total_score > best_score {
                    best_score = result.quality.total_score;
                    best_j = Some(j);
                }
            }

            if let Some(j) = best_j {
                let (entry_a, entry_b) = if j > i {
                    let b = bucket.remove(j);
                    let a = bucket.remove(i);
                    (a, b)
                } else {
                    let a = bucket.remove(i);
                    let b = bucket.remove(j);
                    (a, b)
                };
                self.total_count -= 2;

                let quality =
                    crate::rules::evaluate_rules(entry_a.as_ref(), entry_b.as_ref(), ruleset)
                        .quality;

                return Some(MatchOutcome {
                    entry_a,
                    entry_b,
                    quality,
                });
            }

            i += 1;
        }

        None
    }

    pub fn bucket_keys(&self) -> Vec<BucketKey> {
        self.buckets.keys().cloned().collect()
    }
}

impl<T: AsRef<PlayerTicket>> Default for MatchQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuleSet;
    use proptest::prelude::*;

    fn ticket(player: &str, game: &str, ruleset: &str, mmr: f32) -> PlayerTicket {
        PlayerTicket {
            player_id: player.into(),
            game_id: game.into(),
            ruleset_id: ruleset.into(),
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
    fn bucket_partitioning() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g2", "r1", 1500.0));
        q.push(ticket("p3", "g1", "r1", 1400.0));

        assert_eq!(q.len(), 3);
        assert_eq!(q.bucket_keys().len(), 2);
    }

    #[test]
    fn mmr_sorted_insert() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g1", "r1", 1200.0));
        q.push(ticket("p3", "g1", "r1", 1800.0));

        let bucket = &q.buckets[&("g1".into(), "r1".into())];
        assert_eq!(bucket[0].player_id, "p2");
        assert_eq!(bucket[1].player_id, "p1");
        assert_eq!(bucket[2].player_id, "p3");
    }

    #[test]
    fn match_within_skill_range() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g1", "r1", 1400.0));

        let ruleset = RuleSet::default_arc();
        let result = q.try_match_bucket(&("g1".into(), "r1".into()), &ruleset, 10_000, 0);

        assert!(result.is_some());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn no_match_outside_skill_range() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g1", "r1", 3000.0));

        let ruleset = std::sync::Arc::new(RuleSet {
            max_skill_diff: 500.0,
            ..RuleSet::default()
        });
        let result = q.try_match_bucket(&("g1".into(), "r1".into()), &ruleset, 10_000, 0);

        assert!(result.is_none());
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn no_match_at_capacity() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g1", "r1", 1400.0));

        let ruleset = RuleSet::default_arc();
        let result = q.try_match_bucket(&("g1".into(), "r1".into()), &ruleset, 100, 100);

        assert!(result.is_none());
    }

    #[test]
    fn drain_all() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g2", "r1", 1400.0));
        let all = q.drain_all();
        assert_eq!(all.len(), 2);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn push_dedups_existing_player() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        let replaced = q.push(ticket("p1", "g1", "r1", 1600.0));

        assert!(replaced);
        assert_eq!(q.len(), 1);
        assert!(q.contains_player("p1"));
        assert!(!q.contains_player("p2"));

        let replaced2 = q.push(ticket("p2", "g1", "r1", 1400.0));
        assert!(!replaced2);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn remove_player_clears_across_buckets() {
        let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
        q.push(ticket("p1", "g1", "r1", 1500.0));
        q.push(ticket("p2", "g2", "r1", 1400.0));

        assert!(q.remove_player("p1"));
        assert!(!q.contains_player("p1"));
        assert_eq!(q.len(), 1);

        assert!(!q.remove_player("nobody"));
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1024, ..proptest::test_runner::Config::default()
        })]
        #[test]
        fn prop_skill_pairing_respects_max_diff(
            mmr_a in 0f32..3000f32,
            mmr_b in 0f32..3000f32,
            max_diff in 1f32..1000f32,
        ) {
            let mut q: MatchQueue<PlayerTicket> = MatchQueue::new();
            q.push(ticket("pa", "g", "r", mmr_a));
            q.push(ticket("pb", "g", "r", mmr_b));

            let ruleset = std::sync::Arc::new(RuleSet {
                max_skill_diff: max_diff,
                ..RuleSet::default()
            });
            let matched = q.try_match_bucket(&("g".into(), "r".into()), &ruleset, 10_000, 0);

            let within = (mmr_a - mmr_b).abs() <= max_diff;
            prop_assert_eq!(matched.is_some(), within, "pairing must follow the skill gate");
            if let Some(m) = matched {
                prop_assert!(m.entry_a.player_id == "pa" || m.entry_a.player_id == "pb");
                prop_assert!(m.entry_a.player_id != m.entry_b.player_id);
            }
        }
    }
}
