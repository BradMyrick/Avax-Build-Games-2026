//! Ladder → rating pipeline: convert a finalized ranked placement array
//! into the per-player opponent/score vectors [`crate::glicko2_update_vs_many`]
//! consumes.
//!
//! Scoring rule (§5.1 of the v2 spec): for player `i`, every opponent `j`
//! ranked **below** `i` scores `1.0`, every opponent ranked **above** scores
//! `0.0`, and opponents sharing an exact rank score `0.5`.

use crate::types::PlayerTicket;

/// One player's rating-period input: opponents as `(rating, rd, score)`.
#[derive(Debug, Clone, PartialEq)]
pub struct LadderUpdate {
    pub player_id: String,
    pub opponents: Vec<(f32, f32, f32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LadderError {
    Empty,
    LengthMismatch { ranked: usize, ranks: usize },
    ZeroRank,
}

impl std::fmt::Display for LadderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LadderError::Empty => write!(f, "ladder is empty"),
            LadderError::LengthMismatch { ranked, ranks } => {
                write!(f, "{ranked} placements but {ranks} ranks")
            }
            LadderError::ZeroRank => write!(f, "ranks are 1-based; zero is invalid"),
        }
    }
}

impl std::error::Error for LadderError {}

/// Build rating-period vectors from a ranked placement array.
///
/// `ranked` is ordered best-first (index 0 = rank 1). `ranks` is parallel
/// and carries explicit rank numbers so ties can be expressed (two players
/// sharing rank 2, etc.). Lower rank number = better placement.
pub fn placement_vectors(
    ranked: &[PlayerTicket],
    ranks: &[u16],
) -> Result<Vec<LadderUpdate>, LadderError> {
    if ranked.is_empty() {
        return Err(LadderError::Empty);
    }
    if ranked.len() != ranks.len() {
        return Err(LadderError::LengthMismatch {
            ranked: ranked.len(),
            ranks: ranks.len(),
        });
    }
    if ranks.contains(&0) {
        return Err(LadderError::ZeroRank);
    }

    let mut out = Vec::with_capacity(ranked.len());
    for (i, me) in ranked.iter().enumerate() {
        let mut opponents = Vec::with_capacity(ranked.len() - 1);
        for (j, opp) in ranked.iter().enumerate() {
            if i == j {
                continue;
            }
            let score = if ranks[i] < ranks[j] {
                1.0
            } else if ranks[i] > ranks[j] {
                0.0
            } else {
                0.5
            };
            opponents.push((opp.mmr, opp.mmr_uncertainty, score));
        }
        out.push(LadderUpdate {
            player_id: me.player_id.clone(),
            opponents,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: &str, mmr: f32) -> PlayerTicket {
        PlayerTicket {
            player_id: id.into(),
            game_id: "g".into(),
            ruleset_id: "r".into(),
            mmr,
            mmr_uncertainty: 100.0,
            region: "na".into(),
            preferred_role: String::new(),
            language: "en".into(),
            max_ping_ms: 150,
            enqueued_at_ms: 0,
            party_size: 1,
        }
    }

    #[test]
    fn strict_ladder_scores_are_pairwise_consistent() {
        let ranked = [
            player("w", 1600.0),
            player("s", 1500.0),
            player("l", 1400.0),
        ];
        let ranks = [1, 2, 3];
        let v = placement_vectors(&ranked, &ranks).unwrap();
        // Winner beat two opponents.
        assert_eq!(v[0].opponents.iter().map(|(_, _, s)| s).sum::<f32>(), 2.0);
        // Second place: beat the loser, lost to the winner.
        assert_eq!(v[1].opponents.iter().map(|(_, _, s)| s).sum::<f32>(), 1.0);
        // Loser: two losses.
        assert_eq!(v[2].opponents.iter().map(|(_, _, s)| s).sum::<f32>(), 0.0);
        // Opponent metadata rides along for the rating update.
        assert_eq!(v[0].opponents.len(), 2);
        assert_eq!(v[0].opponents[0], (1500.0, 100.0, 1.0));
    }

    #[test]
    fn tied_ranks_score_half() {
        let ranked = [
            player("a", 1600.0),
            player("b", 1500.0),
            player("c", 1400.0),
        ];
        let ranks = [1, 2, 2]; // b and c tied for second
        let v = placement_vectors(&ranked, &ranks).unwrap();
        assert_eq!(v[1].opponents.iter().map(|(_, _, s)| s).sum::<f32>(), 0.5);
        assert_eq!(v[2].opponents.iter().map(|(_, _, s)| s).sum::<f32>(), 0.5);
    }

    #[test]
    fn round_trips_into_glicko2_vs_many() {
        let ranked: Vec<PlayerTicket> = (0..5)
            .map(|i| player(&format!("p{i}"), 1500.0 + i as f32 * 50.0))
            .collect();
        let ranks: Vec<u16> = (1..=5).collect();
        for v in placement_vectors(&ranked, &ranks).unwrap() {
            let opponents: Vec<(f32, f32)> =
                v.opponents.iter().map(|(r, rd, _)| (*r, *rd)).collect();
            let scores: Vec<f32> = v.opponents.iter().map(|(_, _, s)| *s).collect();
            let (r, rd, vol) =
                crate::glicko2_update_vs_many(1500.0, 200.0, 0.06, &opponents, &scores);
            assert!(r.is_finite() && rd.is_finite() && vol.is_finite());
        }
    }

    #[test]
    fn rejects_empty_mismatch_and_zero_rank() {
        assert!(matches!(
            placement_vectors(&[], &[]),
            Err(LadderError::Empty)
        ));
        let ranked = [player("a", 1.0)];
        assert!(matches!(
            placement_vectors(&ranked, &[1, 2]),
            Err(LadderError::LengthMismatch { .. })
        ));
        assert!(matches!(
            placement_vectors(&ranked, &[0]),
            Err(LadderError::ZeroRank)
        ));
    }
}
