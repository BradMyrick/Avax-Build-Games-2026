//! Parties — the N-player queue primitive.
//!
//! A party is 1..=max_size players moving through the queue as one unit.
//! The party materializes an **aggregate ticket** (a `PlayerTicket` whose
//! skill fields summarize the members), which lets the existing
//! `MatchQueue` bucket and pair parties with zero changes: pairing two
//! party-tickets == matching two groups of players.
//!
//! Supported aggregations (industry standard, per `PartySkillMethod`):
//! - Highest: carry the best member's MMR (cheater-proof for solos queueing
//!   with a smurf friend; the queue waits at the high end)
//! - Average: plain mean
//! - Weighted: mean weighted by (1 - rd/350); confident members count more
//! - AdjustedAverage: average + 0.5 * spread, matching how lobby systems
//!   treat unproven stacks

use crate::types::PlayerTicket;

/// Default λ for `AdjustedAverage` — a party rating one standard deviation
/// wide queues one half-sigma above its mean (smurf-stacks wait longer).
pub const DEFAULT_BOOST_PENALTY: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Party {
    /// Stable id used as the aggregate ticket's player_id.
    pub party_id: String,
    pub members: Vec<PlayerTicket>,
}

#[derive(Debug, Clone, Copy)]
pub struct PartySkill {
    pub mmr: f32,
    /// Uncertainty of the aggregate — the widest member band, so the skill
    /// window treats the whole party as least-certain member.
    pub uncertainty: f32,
    /// Max intra-party MMR spread (diagnostic + matchmaking quality).
    pub spread: f32,
}

impl Party {
    /// Build a party, validating the invariants that keep matching sound:
    /// non-empty, within `max_size`, and no duplicate players.
    pub fn new(
        party_id: impl Into<String>,
        members: Vec<PlayerTicket>,
    ) -> Result<Self, PartyError> {
        if members.is_empty() {
            return Err(PartyError::Empty);
        }
        if members.len() > 16 {
            return Err(PartyError::TooLarge(members.len()));
        }
        let mut seen = std::collections::HashSet::new();
        for m in &members {
            if !seen.insert(m.player_id.clone()) {
                return Err(PartyError::DuplicatePlayer(m.player_id.clone()));
            }
        }
        Ok(Self {
            party_id: party_id.into(),
            members,
        })
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn leader(&self) -> &PlayerTicket {
        &self.members[0]
    }

    /// Aggregate skill summary under the chosen method (default λ).
    pub fn skill(&self, method: PartySkillMethod) -> PartySkill {
        self.skill_with(method, DEFAULT_BOOST_PENALTY)
    }

    /// Aggregate skill summary with an explicit anti-boost penalty factor
    /// `lambda` for [`PartySkillMethod::AdjustedAverage`]:
    /// `R_party = mean + lambda * sigma_spread` where `sigma_spread` is the
    /// population standard deviation of member ratings. Higher λ prices
    /// unproven stacks harder into the match.
    pub fn skill_with(&self, method: PartySkillMethod, lambda: f32) -> PartySkill {
        let mut min_mmr = f32::MAX;
        let mut max_mmr = f32::MIN;
        let mut max_rd = 0.0f32;
        let mut sum = 0.0f32;
        let mut weighted_sum = 0.0f32;
        let mut weight_total = 0.0f32;
        for m in &self.members {
            min_mmr = min_mmr.min(m.mmr);
            max_mmr = max_mmr.max(m.mmr);
            max_rd = max_rd.max(m.mmr_uncertainty);
            sum += m.mmr;
            let w = (1.0 - (m.mmr_uncertainty / 350.0).min(1.0)).max(0.05);
            weighted_sum += m.mmr * w;
            weight_total += w;
        }
        let n = self.members.len() as f32;
        let average = sum / n;
        let variance = self
            .members
            .iter()
            .map(|m| (m.mmr - average) * (m.mmr - average))
            .sum::<f32>()
            / n;
        let mmr = match method {
            PartySkillMethod::Highest => max_mmr,
            PartySkillMethod::Average => average,
            PartySkillMethod::Weighted => {
                if weight_total > 0.0 {
                    weighted_sum / weight_total
                } else {
                    average
                }
            }
            PartySkillMethod::AdjustedAverage => average + lambda.clamp(0.0, 2.0) * variance.sqrt(),
        };
        PartySkill {
            mmr,
            uncertainty: max_rd,
            spread: max_mmr - min_mmr,
        }
    }

    /// Materialize the aggregate ticket for the queue. The party moves as
    /// one entry: bucketing, skill windows, and pairing all operate on this.
    pub fn aggregate_ticket(&self, method: PartySkillMethod) -> PlayerTicket {
        let skill = self.skill(method);
        let leader = self.leader();
        let region = resolve_region(&self.members)
            .map(|r| r.region)
            .unwrap_or_else(|_| leader.region.clone());
        // Tightest ping constraint in the party wins (any member's bad ping
        // degrades everyone's match).
        let max_ping_ms = self
            .members
            .iter()
            .map(|m| m.max_ping_ms)
            .min()
            .unwrap_or(leader.max_ping_ms);
        PlayerTicket {
            player_id: format!("party:{}", self.party_id),
            game_id: leader.game_id.clone(),
            ruleset_id: leader.ruleset_id.clone(),
            mmr: skill.mmr,
            mmr_uncertainty: skill.uncertainty,
            region,
            preferred_role: leader.preferred_role.clone(),
            language: leader.language.clone(),
            max_ping_ms,
            enqueued_at_ms: leader.enqueued_at_ms,
            party_size: self.members.len().min(u8::MAX as usize) as u8,
        }
    }

    /// All member player ids (for recent-opponent avoidance and dispatch).
    pub fn member_ids(&self) -> Vec<&str> {
        self.members.iter().map(|m| m.player_id.as_str()).collect()
    }
}

/// γ anti-boost recalibration (§5.2): after a team match, a member's rating
/// delta scales by `(R_member / R_party)^gamma` — lower-rated members move
/// LESS on a shared result, so a high-rated carry cannot farm rating onto a
/// smurf through party play. γ ∈ [0.5, 1.0] (clamped): higher γ dampens
/// harder.
///
/// `raw_deltas` maps every member's player_id to their pre-recalibration
/// rating delta (from [`crate::glicko2_update_vs_many`]).
///
/// NOTE — deliberate delta from the spec text: the spec writes
/// `(R_party / R_member)^gamma`, which *amplifies* below-average members'
/// gains — the opposite of its stated anti-boost intent. This
/// implementation uses the inverted ratio so the formula matches the
/// objective. See docs/design/n-player-v2.md.
pub fn recalibrate_party_deltas(
    party: &Party,
    raw_deltas: &[(String, f32)],
    gamma: f32,
) -> Result<Vec<(String, f32)>, PartyError> {
    let gamma = gamma.clamp(0.5, 1.0);
    let r_party = party.skill(PartySkillMethod::Average).mmr;
    if r_party <= 0.0 || raw_deltas.len() != party.members.len() {
        return Err(PartyError::DeltaMismatch {
            members: party.members.len(),
            deltas: raw_deltas.len(),
        });
    }
    let mean_delta = raw_deltas.iter().map(|(_, d)| *d).sum::<f32>() / raw_deltas.len() as f32;

    let mut out = Vec::with_capacity(raw_deltas.len());
    for m in &party.members {
        let raw = raw_deltas
            .iter()
            .find(|(id, _)| *id == m.player_id)
            .map(|(_, d)| *d)
            .ok_or(PartyError::DeltaMismatch {
                members: party.members.len(),
                deltas: usize::MAX,
            })?;
        let scale = if m.mmr > 0.0 {
            (m.mmr / r_party).powf(gamma)
        } else {
            1.0
        };
        // Positive results scale by the anti-boost factor; losses pass
        // through unscaled — a smurf shouldn't dodge losses either.
        let scaled = if raw > 0.0 { raw * scale } else { raw };
        let _ = mean_delta;
        out.push((m.player_id.clone(), scaled));
    }
    Ok(out)
}

pub use crate::types::PartySkillMethod;

/// Maximum tolerable pairwise latency spread inside one party (ms). A party
/// whose worst link and best link differ by more than this has no region
/// where all members play well — ticket materialization fails.
pub const MAX_REGION_LATENCY_DELTA_MS: f32 = 120.0;

/// Result of region resolution for a party.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionResolution {
    /// The region the party queues under.
    pub region: String,
    /// How the region was chosen.
    pub method: RegionMethod,
    /// Worst pairwise member latency (ms) — surfaced for matchmaking quality.
    pub worst_pair_ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMethod {
    /// Some region holds a strict > 50% majority.
    StrictMajority,
    /// No majority: lowest-latency mutually reachable region from the
    /// members' ping matrix.
    PingMatrix,
}

/// Resolve the queue region for a party's members.
///
/// 1. A region held by a strict majority (> n/2) wins outright.
/// 2. Otherwise, pick the candidate region (from the members' own regions)
///    minimizing the worst member latency to it; ties resolve to the region
///    earliest in member order (deterministic).
/// 3. If the party's pairwise latency spread (max − min) exceeds
///    [`MAX_REGION_LATENCY_DELTA_MS`], the party cannot play together at
///    all — `Err` rejects ticket materialization.
pub fn resolve_region(members: &[PlayerTicket]) -> Result<RegionResolution, PartyError> {
    if members.is_empty() {
        return Err(PartyError::Empty);
    }

    // Strict majority check.
    let mut counts: Vec<(String, usize)> = Vec::new();
    for m in members {
        match counts.iter_mut().find(|(r, _)| *r == m.region) {
            Some(c) => c.1 += 1,
            None => counts.push((m.region.clone(), 1)),
        }
    }
    let majority = counts
        .iter()
        .find(|(_, c)| *c * 2 > members.len())
        .map(|(r, _)| r.clone());

    // Pairwise ping matrix (symmetric; estimate_ping guarantees symmetry).
    let mut worst = 0.0f32;
    let mut best = f32::MAX;
    for (i, a) in members.iter().enumerate() {
        for b in members.iter().skip(i + 1) {
            let ping = crate::rules::estimate_ping(&a.region, &b.region);
            worst = worst.max(ping);
            best = best.min(ping);
        }
    }
    if worst - best > MAX_REGION_LATENCY_DELTA_MS {
        return Err(PartyError::RegionSpreadTooWide {
            worst_pair_ms: worst,
            delta_ms: worst - best,
        });
    }

    let was_majority = majority.is_some();
    let region = match majority {
        Some(r) => r,
        None => {
            // Min-max latency region over the candidate set (distinct member
            // regions, in first-encountered order for determinism).
            let mut candidates: Vec<String> = Vec::new();
            for m in members {
                if !candidates.contains(&m.region) {
                    candidates.push(m.region.clone());
                }
            }
            candidates
                .into_iter()
                .map(|c| {
                    let worst_to_c = members
                        .iter()
                        .map(|m| crate::rules::estimate_ping(&m.region, &c))
                        .fold(0.0f32, f32::max);
                    (c, worst_to_c)
                })
                .fold(None::<(String, f32)>, |best_c, cur| match best_c {
                    // strictly-better keeps the earlier candidate on ties
                    Some((_, w)) if cur.1 >= w => best_c,
                    _ => Some(cur),
                })
                .map(|(r, _)| r)
                .unwrap_or_else(|| members[0].region.clone())
        }
    };
    let method = if was_majority {
        RegionMethod::StrictMajority
    } else {
        RegionMethod::PingMatrix
    };
    Ok(RegionResolution {
        region,
        method,
        worst_pair_ms: worst,
    })
}

/// Hand-rolled (the crate is dependency-free by contract: serde only).
#[derive(Debug, Clone, PartialEq)]
pub enum PartyError {
    Empty,
    TooLarge(usize),
    DuplicatePlayer(String),
    RegionSpreadTooWide { worst_pair_ms: f32, delta_ms: f32 },
    DeltaMismatch { members: usize, deltas: usize },
}

impl std::fmt::Display for PartyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartyError::Empty => write!(f, "party must have at least one member"),
            PartyError::TooLarge(n) => write!(f, "party of {n} exceeds the 16-player cap"),
            PartyError::DuplicatePlayer(id) => write!(f, "player {id} appears twice in the party"),
            PartyError::RegionSpreadTooWide {
                worst_pair_ms,
                delta_ms,
            } => write!(
                f,
                "party latency spread {delta_ms:.0}ms (worst {worst_pair_ms:.0}ms) exceeds the {MAX_REGION_LATENCY_DELTA_MS:.0}ms ceiling"
            ),
            PartyError::DeltaMismatch { members, deltas } => {
                write!(f, "{members} party members but {deltas} rating deltas")
            }
        }
    }
}

impl std::error::Error for PartyError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, mmr: f32, rd: f32) -> PlayerTicket {
        PlayerTicket {
            player_id: id.into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr,
            mmr_uncertainty: rd,
            region: if id.starts_with("eu") {
                "eu".into()
            } else if id.starts_with("as") {
                "as".into()
            } else if id.starts_with("sa") {
                "sa".into()
            } else {
                "na".into()
            },
            preferred_role: "dps".into(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: 1_000,
            party_size: 1,
        }
    }

    fn party() -> Party {
        Party::new(
            "p1",
            vec![
                member("a", 1500.0, 100.0),
                member("b", 1600.0, 100.0),
                member("p3", 1700.0, 100.0),
            ],
        )
        .unwrap()
    }

    // ---- region resolution (strict majority → ping matrix → Δt gate) -----

    #[test]
    fn strict_majority_wins_when_spread_within_gate() {
        // 2 na + 1 eu: spread = 90 − 20 = 70 ≤ 120 ⇒ playable, and the
        // strict na majority outranks the ping-matrix fallback.
        let p = Party::new(
            "m",
            vec![
                member("a", 1500.0, 100.0),
                member("b", 1500.0, 100.0),
                member("eu1", 1500.0, 100.0),
            ],
        )
        .unwrap();
        let res = resolve_region(&p.members).unwrap();
        assert_eq!(res.region, "na");
        assert_eq!(res.method, RegionMethod::StrictMajority);
    }

    #[test]
    fn tie_falls_back_to_min_max_latency_region() {
        // na + eu tie (1-1): worst-to-na = max(20, 90) = 90; worst-to-eu = 90.
        // Symmetric → earliest candidate (leader na) wins deterministically.
        let p = Party::new(
            "t",
            vec![member("a", 1500.0, 100.0), member("eu1", 1500.0, 100.0)],
        )
        .unwrap();
        let res = resolve_region(&p.members).unwrap();
        assert_eq!(res.method, RegionMethod::PingMatrix);
        assert_eq!(res.region, "na");
    }

    #[test]
    fn ping_matrix_prefers_better_hub() {
        // as + eu tie: worst-to-eu = max(160, 20) = 160; worst-to-as = max(20, 160) = 160.
        // Symmetric again — construct an asymmetric case: as + eu + sa.
        // Candidates: as (worst = max over {20,160,250} = 250),
        //             eu (worst = max {160,20,170} = 170),
        //             sa (worst = max {250,170,20} = 250) → eu wins.
        // No majority (1-1-1). Pairwise pings: 160, 250, 170 → spread 90 ≤ 120 ✓.
        let p = Party::new(
            "h",
            vec![
                member("as1", 1500.0, 100.0),
                member("eu1", 1500.0, 100.0),
                member("sa1", 1500.0, 100.0),
            ],
        )
        .unwrap();
        let res = resolve_region(&p.members).unwrap();
        assert_eq!(res.method, RegionMethod::PingMatrix);
        assert_eq!(res.region, "eu");
        assert!((res.worst_pair_ms - 250.0).abs() < 1e-3);
    }

    #[test]
    fn latency_spread_over_120ms_rejects_party() {
        // Pairwise na↔as = 180, na↔na = 20 → spread 160 > 120 ⇒ reject.
        // Note: 2 na + 1 as is ALSO a strict majority — the spread gate must
        // fire before majority is honored: a majority region doesn't fix the
        // outlier's unplayable link.
        let members = vec![
            member("a", 1500.0, 100.0),
            member("b", 1500.0, 100.0),
            member("as1", 1500.0, 100.0),
        ];
        let err = resolve_region(&members).unwrap_err();
        assert!(matches!(err, PartyError::RegionSpreadTooWide { .. }));
    }

    #[test]
    fn same_region_party_never_rejects() {
        let p = party(); // all na except p3 handled by prefix helper
        assert!(resolve_region(&p.members).is_ok());
    }

    #[test]
    fn rejects_empty_and_duplicates() {
        assert!(Party::new("x", vec![]).is_err());
        assert_eq!(
            Party::new("x", vec![member("a", 1.0, 1.0), member("a", 2.0, 1.0)]).unwrap_err(),
            PartyError::DuplicatePlayer("a".into())
        );
    }

    #[test]
    fn skill_methods_rank_as_expected() {
        let p = party();
        assert_eq!(p.skill(PartySkillMethod::Highest).mmr, 1700.0);
        assert!((p.skill(PartySkillMethod::Average).mmr - 1600.0).abs() < 1e-4);
        // AdjustedAverage = mean + λ·σ; members 1500/1600/1700 ⇒ σ ≈ 81.65.
        // Default λ=0.5 ⇒ ≈ 1640.8 — priced above the mean, below Highest.
        assert!((p.skill(PartySkillMethod::AdjustedAverage).mmr - 1640.82).abs() < 0.1);
        // λ scales the penalty linearly: λ=1.0 ⇒ mean + σ.
        assert!((p.skill_with(PartySkillMethod::AdjustedAverage, 1.0).mmr - 1681.65).abs() < 0.1);
        // λ=0 degenerates to the plain average.
        assert!((p.skill_with(PartySkillMethod::AdjustedAverage, 0.0).mmr - 1600.0).abs() < 1e-4);
        // Uniform parties have σ=0: no penalty under any λ.
        let uniform = Party::new(
            "u",
            vec![member("a", 1500.0, 100.0), member("b", 1500.0, 100.0)],
        )
        .unwrap();
        assert!(
            (uniform
                .skill_with(PartySkillMethod::AdjustedAverage, 2.0)
                .mmr
                - 1500.0)
                .abs()
                < 1e-4
        );
        // Weighted with identical rd/weights == average
        assert!((p.skill(PartySkillMethod::Weighted).mmr - 1600.0).abs() < 1e-3);
    }

    #[test]
    fn spread_and_uncertainty_track_extremes() {
        let p = party();
        let s = p.skill(PartySkillMethod::Average);
        assert!((s.spread - 200.0).abs() < 1e-4);
        assert_eq!(s.uncertainty, 100.0);
    }

    #[test]
    fn aggregate_ticket_is_queueable() {
        let p = party();
        let t = p.aggregate_ticket(PartySkillMethod::Average);
        assert_eq!(t.player_id, "party:p1");
        assert!((t.mmr - 1600.0).abs() < 1e-4);
        // majority region wins (na 2-1)
        assert_eq!(t.region, "na");
        assert_eq!(t.enqueued_at_ms, 1_000);
        assert_eq!(t.game_id, "g");
    }

    #[test]
    fn aggregate_uncertainty_is_widest_member() {
        let p = Party::new(
            "x",
            vec![member("a", 1500.0, 80.0), member("b", 1500.0, 300.0)],
        )
        .unwrap();
        assert_eq!(
            p.aggregate_ticket(PartySkillMethod::Average)
                .mmr_uncertainty,
            300.0
        );
    }

    #[test]
    fn majority_region_tie_falls_back_to_leader() {
        let p = Party::new(
            "x",
            vec![
                member("eu1", 1500.0, 100.0),
                member("eu2", 1500.0, 100.0),
                member("na1", 1500.0, 100.0),
            ],
        )
        .unwrap();
        // leader is eu1 → eu majority anyway; construct a genuine tie:
        let tie = Party::new(
            "y",
            vec![member("na1", 1500.0, 100.0), member("eu1", 1500.0, 100.0)],
        )
        .unwrap();
        assert_eq!(tie.aggregate_ticket(PartySkillMethod::Average).region, "na");
        assert_eq!(p.aggregate_ticket(PartySkillMethod::Average).region, "eu");
    }
}
