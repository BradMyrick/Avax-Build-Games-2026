//! The matchmaking queue: an in-memory `MatchQueue` (from `amp-match-core`)
//! backed by durable `amp_queue_tickets` rows. The tick loop pairs players
//! per `(game, ruleset)` bucket with a skill window that widens as the
//! longest-waiting player in the bucket waits — tight matches early, any
//! match eventually.
//!
//! A side index (player → bucket → join time) tracks wait times so window
//! widening is O(1) per bucket per tick, with no scanning.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use amp_match_core::{MatchQueue, PlayerTicket, RuleSet};
use uuid::Uuid;

use crate::config::Config;

pub type BucketKey = (String, String);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// In-memory queue entry; wraps the pure `PlayerTicket` with server context.
/// `ticket.ruleset_id` carries the stake-qualified bucket form
/// (`ruleset@stake:<wei>`) so match-core's internal buckets separate stakes;
/// `canonical_ruleset` preserves the catalog id for match rows.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub ticket_id: Uuid,
    pub stake_wei: i64,
    pub canonical_ruleset: String,
    pub ticket: PlayerTicket,
}

impl AsRef<PlayerTicket> for QueueEntry {
    fn as_ref(&self) -> &PlayerTicket {
        &self.ticket
    }
}

struct Inner {
    queue: MatchQueue<QueueEntry>,
    /// enqueued_at per bucket, sorted — first element is the longest wait.
    waits: HashMap<BucketKey, BTreeSet<u64>>,
    /// player → (bucket, enqueued_at) for leave/cleanup.
    members: HashMap<String, (BucketKey, u64)>,
}

pub struct QueueService {
    inner: Mutex<Inner>,
    cfg: Arc<Config>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueStatus {
    pub depth: usize,
    pub waited_ms: u64,
    pub skill_window: f32,
}

impl QueueService {
    pub fn new(cfg: Arc<Config>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                queue: MatchQueue::new(),
                waits: HashMap::new(),
                members: HashMap::new(),
            }),
            cfg,
        }
    }

    /// Bucket key — matches match-core's internal (game, ruleset) keying
    /// because `join` stamps the stake-qualified ruleset into the ticket.
    fn bucket_of(e: &QueueEntry) -> BucketKey {
        (e.ticket.game_id.clone(), e.ticket.ruleset_id.clone())
    }

    pub fn join(&self, mut entry: QueueEntry) {
        let mut inner = self.inner.lock().unwrap();
        // Stamp the stake tier into the ticket's ruleset so match-core buckets
        // on the qualified id — free players never share a bucket with staked
        // players, and stakes pair only at identical amounts.
        if entry.stake_wei > 0 {
            entry.ticket.ruleset_id =
                format!("{}@stake:{}", entry.canonical_ruleset, entry.stake_wei);
        }
        // MatchQueue::push replaces any prior entry for this player; mirror
        // that in the side index so waits never leak.
        if let Some((old_bucket, old_time)) = inner.members.remove(&entry.ticket.player_id)
            && let Some(set) = inner.waits.get_mut(&old_bucket)
            && set.remove(&old_time)
            && set.is_empty()
        {
            inner.waits.remove(&old_bucket);
        }
        let bucket = Self::bucket_of(&entry);
        let player = entry.ticket.player_id.clone();
        let t = entry.ticket.enqueued_at_ms;
        inner.queue.push(entry);
        inner.waits.entry(bucket.clone()).or_default().insert(t);
        inner.members.insert(player, (bucket, t));
    }

    pub fn leave(&self, player: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let removed = inner.queue.remove_player(player);
        if let Some((bucket, t)) = inner.members.remove(player)
            && let Some(set) = inner.waits.get_mut(&bucket)
            && set.remove(&t)
            && set.is_empty()
        {
            inner.waits.remove(&bucket);
        }
        removed
    }

    pub fn status_for(&self, player: &str) -> Option<QueueStatus> {
        let inner = self.inner.lock().unwrap();
        let (_bucket, t) = inner.members.get(player)?;
        let waited = now_ms().saturating_sub(*t);
        let window = self.window_for_wait(waited);
        Some(QueueStatus {
            depth: inner.queue.len(),
            waited_ms: waited,
            skill_window: window,
        })
    }

    /// Expanding skill window: base + growth * seconds waited, clamped.
    pub fn window_for_wait(&self, waited_ms: u64) -> f32 {
        let secs = waited_ms as f32 / 1000.0;
        (self.cfg.skill_window_base + self.cfg.skill_window_expansion_per_sec * secs)
            .min(self.cfg.skill_window_cap)
    }

    /// Build a ruleset whose skill gate matches the current window.
    fn ruleset_for(&self, window: f32) -> Arc<RuleSet> {
        let mut rs = RuleSet {
            max_skill_diff: window,
            ..RuleSet::default()
        };
        for rule in &mut rs.rules {
            if let amp_match_core::RuleParams::Skill(ref mut p) = rule.params {
                p.max_difference = window;
            }
        }
        rs.new_sorted()
    }

    /// One matchmaker tick: pair every bucket as far as the rules allow.
    /// Returns pairs for match creation (DB + notifications happen outside
    /// the lock).
    pub fn tick(&self, active_matches: usize) -> Vec<(QueueEntry, QueueEntry)> {
        let mut inner = self.inner.lock().unwrap();
        let mut pairs = Vec::new();

        let mut buckets: Vec<BucketKey> = inner.waits.keys().cloned().collect();
        buckets.sort(); // deterministic tick order

        for key in buckets {
            while let Some(&earliest) = inner.waits.get(&key).and_then(|s| s.iter().next()) {
                let waited = now_ms().saturating_sub(earliest);
                let window = self.window_for_wait(waited);
                let ruleset = self.ruleset_for(window);
                let Some(outcome) = inner.queue.try_match_bucket(
                    &key,
                    &ruleset,
                    self.cfg.max_active_matches,
                    active_matches + pairs.len(),
                ) else {
                    break;
                };
                for e in [&outcome.entry_a, &outcome.entry_b] {
                    if let Some((b, t)) = inner.members.remove(&e.ticket.player_id)
                        && let Some(s) = inner.waits.get_mut(&b)
                        && s.remove(&t)
                        && s.is_empty()
                    {
                        inner.waits.remove(&b);
                    }
                }
                pairs.push((outcome.entry_a, outcome.entry_b));
            }
        }
        pairs
    }

    pub fn depth(&self) -> usize {
        self.inner.lock().unwrap().queue.len()
    }

    /// Cold-start valve: pull entries that have waited past `threshold_ms`
    /// out of the queue (for house practice-bot matches). Only buckets whose
    /// longest wait crosses the threshold are drained, and cold queues are
    /// small by definition, so the O(bucket) drain is acceptable.
    pub fn take_stale(&self, threshold_ms: u64) -> Vec<QueueEntry> {
        let mut inner = self.inner.lock().unwrap();
        let stale_before = now_ms().saturating_sub(threshold_ms);
        let mut stale_buckets = Vec::new();
        for (bucket, times) in &inner.waits {
            if times.iter().any(|&t| t <= stale_before) {
                stale_buckets.push(bucket.clone());
            }
        }
        if stale_buckets.is_empty() {
            return Vec::new();
        }

        let all = inner.queue.drain_all();
        let mut stale = Vec::new();
        let mut keep = Vec::with_capacity(all.len());
        for e in all {
            if stale_buckets.contains(&Self::bucket_of(&e))
                && e.ticket.enqueued_at_ms <= stale_before
            {
                if let Some((b, t)) = inner.members.remove(&e.ticket.player_id)
                    && let Some(s) = inner.waits.get_mut(&b)
                    && s.remove(&t)
                    && s.is_empty()
                {
                    inner.waits.remove(&b);
                }
                stale.push(e);
            } else {
                keep.push(e);
            }
        }
        for e in keep {
            inner.queue.push(e);
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Arc<Config> {
        Arc::new(Config {
            database_url: "unused".into(),
            bind: "0.0.0.0:0".into(),
            cors_origins: vec!["*".into()],
            verifier_key: None,
            chain_id: 43113,
            settlement_address: None,
            staking_enabled: false,
            session_ttl_hours: 168,
            match_ttl_minutes: 120,
            tick_ms: 100,
            skill_window_base: 350.0,
            skill_window_expansion_per_sec: 8.0,
            skill_window_cap: 1200.0,
            max_active_matches: 10_000,
            admin_token: None,
            bot_fill_enabled: false,
            bot_after_ms: 45_000,
            house_wallet: None,
            queue_windows: Vec::new(),
            rpc_url: "http://localhost:8545".into(),
            registry_address: None,
            registry_game_id: 0,
            escrow_window_minutes: 10,
            rt_grace_minutes: 30,
            multiplayer_address: None,
            site_name: "AMP Arena".into(),
        })
    }

    fn entry(wallet: &str, mmr: f32, game: &str) -> QueueEntry {
        QueueEntry {
            ticket_id: Uuid::new_v4(),
            stake_wei: 0,
            canonical_ruleset: "ranked-1v1".into(),
            ticket: PlayerTicket {
                player_id: wallet.into(),
                game_id: game.into(),
                ruleset_id: "ranked-1v1".into(),
                mmr,
                mmr_uncertainty: 350.0,
                region: "na".into(),
                preferred_role: String::new(),
                language: "en".into(),
                max_ping_ms: 150,
                enqueued_at_ms: now_ms(),
                party_size: 1,
            },
        }
    }

    #[test]
    fn pairs_close_ratings_immediately() {
        let svc = QueueService::new(cfg());
        svc.join(entry("0xa", 1500.0, "g"));
        svc.join(entry("0xb", 1550.0, "g"));
        let pairs = svc.tick(0);
        assert_eq!(pairs.len(), 1);
        assert_eq!(svc.depth(), 0);
    }

    #[test]
    fn far_ratings_wait_then_pair_after_widening() {
        let svc = QueueService::new(cfg());
        let mut a = entry("0xa", 1500.0, "g");
        let mut b = entry("0xb", 2400.0, "g");
        a.ticket.enqueued_at_ms = now_ms() - 90_000;
        b.ticket.enqueued_at_ms = now_ms() - 90_000;
        // 900 apart > 350 base window: no pair at zero wait.
        svc.join(entry("0xa", 1500.0, "g"));
        svc.join(entry("0xb", 2400.0, "g"));
        assert!(svc.tick(0).is_empty());
        svc.leave("0xa");
        svc.leave("0xb");
        // After 90s of waiting the window is 350 + 8*90 = 1070 > 900.
        svc.join(a);
        svc.join(b);
        assert_eq!(svc.tick(0).len(), 1);
    }

    #[test]
    fn window_never_exceeds_cap() {
        let svc = QueueService::new(cfg());
        assert_eq!(svc.window_for_wait(u64::MAX), svc.cfg.skill_window_cap);
    }

    #[test]
    fn leave_removes_from_queue() {
        let svc = QueueService::new(cfg());
        svc.join(entry("0xa", 1500.0, "g"));
        assert!(svc.leave("0xa"));
        assert!(!svc.leave("0xa"));
        assert_eq!(svc.depth(), 0);
        assert!(svc.status_for("0xa").is_none());
    }

    #[test]
    fn rejoin_resets_wait_and_no_side_leak() {
        let svc = QueueService::new(cfg());
        let mut a = entry("0xa", 1500.0, "g");
        a.ticket.enqueued_at_ms = now_ms() - 60_000;
        svc.join(a);
        let waited_before = svc.status_for("0xa").unwrap().waited_ms;
        assert!(waited_before > 50_000);
        // Re-join with fresh time replaces the entry, not duplicates it.
        svc.join(entry("0xa", 1500.0, "g"));
        assert_eq!(svc.depth(), 1);
        assert!(svc.status_for("0xa").unwrap().waited_ms < 5_000);
    }

    #[test]
    fn different_games_do_not_cross_pair() {
        let svc = QueueService::new(cfg());
        svc.join(entry("0xa", 1500.0, "chess"));
        svc.join(entry("0xb", 1500.0, "checkers"));
        assert!(svc.tick(0).is_empty());
    }

    #[test]
    fn take_stale_pulls_only_long_waiters() {
        let svc = QueueService::new(cfg());
        let mut patient = entry("0xa", 1500.0, "g");
        patient.ticket.enqueued_at_ms = now_ms() - 60_000;
        svc.join(patient);
        svc.join(entry("0xb", 1500.0, "g")); // fresh

        let stale = svc.take_stale(45_000);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].ticket.player_id, "0xa");
        // Fresh entry survives and can still pair.
        assert_eq!(svc.depth(), 1);
        assert!(svc.status_for("0xb").is_some());
        assert!(svc.status_for("0xa").is_none());
    }

    #[test]
    fn take_stale_noop_when_all_fresh() {
        let svc = QueueService::new(cfg());
        svc.join(entry("0xa", 1500.0, "g"));
        assert!(svc.take_stale(45_000).is_empty());
        assert_eq!(svc.depth(), 1);
    }

    #[test]
    fn staked_never_pairs_with_free_or_different_stake() {
        let svc = QueueService::new(cfg());
        let mut free = entry("0xa", 1500.0, "g");
        free.stake_wei = 0;
        let mut staked_1 = entry("0xb", 1500.0, "g");
        staked_1.stake_wei = 1_000_000_000_000; // 0.0001 AVAX-ish tier
        let mut staked_2 = entry("0xc", 1500.0, "g");
        staked_2.stake_wei = 2_000_000_000_000;
        svc.join(free);
        svc.join(staked_1);
        svc.join(staked_2);
        assert!(
            svc.tick(0).is_empty(),
            "free/staked and mismatched stakes must never pair"
        );
        // Same stake pairs fine.
        let mut staked_1b = entry("0xd", 1510.0, "g");
        staked_1b.stake_wei = 1_000_000_000_000;
        svc.join(staked_1b);
        let pairs = svc.tick(0);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.stake_wei, pairs[0].1.stake_wei);
    }
}
