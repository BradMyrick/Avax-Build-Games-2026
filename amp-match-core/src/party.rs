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

    /// Aggregate skill summary under the chosen method.
    pub fn skill(&self, method: PartySkillMethod) -> PartySkill {
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
            PartySkillMethod::AdjustedAverage => average + 0.5 * (max_mmr - min_mmr),
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
        // Majority region; deterministic ties → the region encountered
        // first in member order (the leader's region wins ties).
        let mut counts: Vec<(String, usize)> = Vec::new();
        for m in &self.members {
            match counts.iter_mut().find(|(r, _)| *r == m.region) {
                Some(c) => c.1 += 1,
                None => counts.push((m.region.clone(), 1)),
            }
        }
        let region = counts
            .into_iter()
            .fold(None::<(String, usize)>, |best, cur| match best {
                Some((_, c)) if cur.1 <= c => best, // strictly-greater keeps first max
                _ => Some(cur),
            })
            .map(|(r, _)| r)
            .unwrap_or_else(|| leader.region.clone());
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
        }
    }

    /// All member player ids (for recent-opponent avoidance and dispatch).
    pub fn member_ids(&self) -> Vec<&str> {
        self.members.iter().map(|m| m.player_id.as_str()).collect()
    }
}

pub use crate::types::PartySkillMethod;

/// Hand-rolled (the crate is dependency-free by contract: serde only).
#[derive(Debug, Clone, PartialEq)]
pub enum PartyError {
    Empty,
    TooLarge(usize),
    DuplicatePlayer(String),
}

impl std::fmt::Display for PartyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartyError::Empty => write!(f, "party must have at least one member"),
            PartyError::TooLarge(n) => write!(f, "party of {n} exceeds the 16-player cap"),
            PartyError::DuplicatePlayer(id) => write!(f, "player {id} appears twice in the party"),
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
            } else {
                "na".into()
            },
            preferred_role: "dps".into(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: 1_000,
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
        // AdjustedAverage = 1600 + 0.5 * (1700-1500) = 1700
        assert!((p.skill(PartySkillMethod::AdjustedAverage).mmr - 1700.0).abs() < 1e-4);
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
