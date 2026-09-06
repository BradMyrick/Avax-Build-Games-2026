//! Independent single-elimination winner derivation for the relayer (P0-1).
//!
//! The relayer MUST NOT trust payout addresses written by the web. It
//! rebuilds the bracket from the durable `players` + `results` rows and
//! re-derives the placement order deterministically. This is a faithful
//! port of `web/src/lib/engine/{seeding,singleElim,tournament}.ts` — the
//! load-bearing invariant is that both engines allocate match ids
//! round-major starting at 1 and propagate winners identically, so the
//! relayer's derivation and the web's `winners()` agree on the same
//! recorded results.

use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerRow {
    pub id: i64,
    pub seed: i64,
    pub wallet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResultRow {
    #[serde(rename = "matchId")]
    pub match_id: i64,
    /// "A" | "B" | "Draw" | "Void"
    pub outcome: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    A,
    B,
}

#[derive(Debug, Clone)]
struct Entrant {
    id: i64,
    seed: i64,
    #[allow(dead_code)] // carried for future dispute evidence; ordering uses ids
    wallet: String,
}

#[derive(Debug, Clone)]
struct Match {
    id: i64,
    round: usize,
    a: Option<Entrant>,
    b: Option<Entrant>,
    outcome: Option<String>,
    winner_to: Option<(i64, Side)>,
}

fn next_pow2(n: usize) -> usize {
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p
}

fn bit_reverse(mut x: usize, k: u32) -> usize {
    let mut r = 0usize;
    for _ in 0..k {
        r = (r << 1) | (x & 1);
        x >>= 1;
    }
    r
}

/// Slot index for each seed rank (0-based), power-of-two `size`.
fn seed_slots(size: usize) -> Vec<usize> {
    let k = size.trailing_zeros();
    (0..size).map(|s| bit_reverse(s, k)).collect()
}

fn outcome_winner(outcome: &str) -> Option<Side> {
    match outcome {
        "A" => Some(Side::A),
        "B" => Some(Side::B),
        _ => None, // Draw | Void produce no winner
    }
}

fn side_entrant(m: &Match, s: Side) -> Option<&Entrant> {
    match s {
        Side::A => m.a.as_ref(),
        Side::B => m.b.as_ref(),
    }
}

fn build_single_elim(players: &[PlayerRow]) -> Result<Vec<Match>> {
    if players.len() < 2 {
        bail!("bracket needs >= 2 players");
    }
    let mut ordered: Vec<Entrant> = players
        .iter()
        .map(|p| Entrant {
            id: p.id,
            seed: p.seed,
            wallet: p.wallet.clone(),
        })
        .collect();
    // Seed asc (TS tiebreaks stable-sort by input order; ids are distinct
    // in practice and the web passes distinct seeds).
    ordered.sort_by_key(|e| e.seed);

    let bracket = next_pow2(ordered.len());
    let rounds = bracket.trailing_zeros() as usize;

    let slots = seed_slots(bracket);
    let mut slot_entrant: Vec<Option<Entrant>> = vec![None; bracket];
    for (rank, slot) in slots.iter().enumerate() {
        if rank < ordered.len() {
            slot_entrant[*slot] = Some(ordered[rank].clone());
        }
    }

    // Round-major id allocation, starting at 1 — mirrors Tournament::nextId.
    let mut ids: Vec<Vec<i64>> = Vec::with_capacity(rounds);
    let mut next_id: i64 = 1;
    for r in 0..rounds {
        let m = bracket >> (r + 1);
        let row: Vec<i64> = (0..m)
            .map(|_| {
                let id = next_id;
                next_id += 1;
                id
            })
            .collect();
        ids.push(row);
    }

    let mut matches = Vec::new();
    for r in 0..rounds {
        let m_count = bracket >> (r + 1);
        for m_idx in 0..m_count {
            let (a, b) = if r == 0 {
                (
                    slot_entrant[m_idx * 2].clone(),
                    slot_entrant[m_idx * 2 + 1].clone(),
                )
            } else {
                (None, None)
            };
            let winner_to = if r + 1 < rounds {
                let next = m_idx / 2;
                let side = if m_idx % 2 == 0 { Side::A } else { Side::B };
                Some((ids[r + 1][next], side))
            } else {
                None
            };
            matches.push(Match {
                id: ids[r][m_idx],
                round: r,
                a,
                b,
                outcome: None,
                winner_to,
            });
        }
    }
    Ok(matches)
}

/// Port of `Tournament::advanceSingleElim` — auto-complete byes, propagate
/// winners — run to a fixed point.
fn advance_single_elim(matches: &mut [Match]) {
    loop {
        let mut changed = false;

        // 1. Auto-complete round-0 byes.
        for m in matches.iter_mut() {
            if m.round == 0 && m.outcome.is_none() {
                if m.a.is_some() && m.b.is_none() {
                    m.outcome = Some("A".into());
                    changed = true;
                } else if m.a.is_none() && m.b.is_some() {
                    m.outcome = Some("B".into());
                    changed = true;
                }
            }
        }

        // 2. Propagate winners into their target slots.
        let mut updates: Vec<(i64, Side, Entrant)> = Vec::new();
        for m in matches.iter() {
            let Some(outcome) = m.outcome.as_deref() else {
                continue;
            };
            let Some(winner_side) = outcome_winner(outcome) else {
                continue;
            };
            let Some(winner) = side_entrant(m, winner_side) else {
                continue;
            };
            let Some((tgt, side)) = m.winner_to else {
                continue;
            };
            let occupied = matches.iter().any(|t| {
                t.id == tgt
                    && match side {
                        Side::A => t.a.is_some(),
                        Side::B => t.b.is_some(),
                    }
            });
            if !occupied {
                updates.push((tgt, side, winner.clone()));
            }
        }
        for (tgt, side, winner) in updates {
            for t in matches.iter_mut() {
                if t.id == tgt {
                    match side {
                        Side::A => t.a = Some(winner.clone()),
                        Side::B => t.b = Some(winner.clone()),
                    }
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }
}

/// Derive the ordered winner wallets for a recorded single-elim bracket.
/// Returns ids ordered champion-first, then losers by elimination round
/// (desc) and seed (asc) — byte-for-byte the web engine's `winners()`.
pub fn derive_single_elim_winners(
    players: &[PlayerRow],
    results: &[ResultRow],
) -> Result<Vec<String>> {
    let mut matches = build_single_elim(players)?;
    // Tournament::new runs one advance at construction so round-0 byes
    // resolve before any result is recorded — mirror that here.
    advance_single_elim(&mut matches);
    let by_id: HashMap<i64, usize> = matches.iter().enumerate().map(|(i, m)| (m.id, i)).collect();

    // Canonical replay order: matchId ascending.
    let mut ordered_results = results.to_vec();
    ordered_results.sort_by_key(|r| r.match_id);

    for r in &ordered_results {
        let idx = *by_id
            .get(&r.match_id)
            .ok_or_else(|| anyhow::anyhow!("result references unknown match {}", r.match_id))?;
        let m = &matches[idx];
        if m.outcome.is_some() {
            bail!("match {} already resolved (replayed result)", r.match_id);
        }
        if m.a.is_none() || m.b.is_none() {
            bail!(
                "match {} recorded but not ready (waiting on a prior winner)",
                r.match_id
            );
        }
        if r.outcome == "Draw" {
            bail!("single-elim match {} cannot draw", r.match_id);
        }
        matches[idx].outcome = Some(r.outcome.clone());
        advance_single_elim(&mut matches);
    }

    // Completeness: every match resolved and a champion exists.
    if matches.iter().any(|m| m.outcome.is_none()) {
        bail!("bracket incomplete: unresolved matches remain");
    }
    let final_match = matches
        .iter()
        .find(|m| m.winner_to.is_none())
        .ok_or_else(|| anyhow::anyhow!("no final match"))?;
    let champion_id = outcome_winner(final_match.outcome.as_deref().unwrap_or("Void"))
        .and_then(|s| side_entrant(final_match, s))
        .map(|e| e.id)
        .ok_or_else(|| anyhow::anyhow!("final match has no winner (void/draw)"))?;

    // Elimination round per entrant.
    let mut elim_round: HashMap<i64, Option<usize>> =
        players.iter().map(|p| (p.id, None)).collect();
    let seed_of: HashMap<i64, i64> = players.iter().map(|p| (p.id, p.seed)).collect();
    for m in &matches {
        let Some(outcome) = m.outcome.as_deref() else {
            continue;
        };
        let Some(winner_side) = outcome_winner(outcome) else {
            continue;
        };
        let loser_side = if winner_side == Side::A {
            Side::B
        } else {
            Side::A
        };
        if let Some(loser) = side_entrant(m, loser_side) {
            elim_round.insert(loser.id, Some(m.round));
        }
    }

    let mut ordered_ids = vec![champion_id];
    let mut rest: Vec<i64> = elim_round
        .keys()
        .copied()
        .filter(|id| *id != champion_id)
        .collect();
    rest.sort_by(|a, b| {
        let ea = elim_round.get(a).copied().flatten().unwrap_or(0);
        let eb = elim_round.get(b).copied().flatten().unwrap_or(0);
        eb.cmp(&ea).then_with(|| {
            seed_of
                .get(a)
                .copied()
                .unwrap_or(i64::MAX)
                .cmp(&seed_of.get(b).copied().unwrap_or(i64::MAX))
        })
    });
    ordered_ids.extend(rest);

    let wallet_of: HashMap<i64, &str> = players.iter().map(|p| (p.id, p.wallet.as_str())).collect();
    ordered_ids
        .into_iter()
        .map(|id| {
            wallet_of
                .get(&id)
                .copied()
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("winner id {} has no wallet", id))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players(n: i64) -> Vec<PlayerRow> {
        (1..=n)
            .map(|i| PlayerRow {
                id: i,
                seed: i,
                wallet: format!("0x{i:040x}"),
            })
            .collect()
    }

    fn results(pairs: &[(i64, &str)]) -> Vec<ResultRow> {
        pairs
            .iter()
            .map(|(m, o)| ResultRow {
                match_id: *m,
                outcome: o.to_string(),
            })
            .collect()
    }

    /// Spec-mandated parity: fixtures generated by the TS engine
    /// (web/src/lib/engine) via the vitest generator in
    /// web/src/lib/engine/__tests__/parity-fixture.test.ts. The relayer's
    /// derivation must produce byte-identical wallet order.
    #[test]
    fn ts_engine_parity_corpus() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/single_elim_parity.json"
        ));
        #[derive(Deserialize)]
        #[allow(dead_code)] // field-level read via serde_json below
        struct Case {
            #[serde(rename = "winnerWallets")]
            winner_wallets: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Corpus {
            cases: Vec<serde_json::Value>,
        }
        let corpus: Corpus = serde_json::from_str(raw).expect("fixture parses");
        assert!(!corpus.cases.is_empty(), "empty parity corpus");
        for case in &corpus.cases {
            let players: Vec<PlayerRow> =
                serde_json::from_value(case["players"].clone()).expect("players");
            let results: Vec<ResultRow> =
                serde_json::from_value(case["results"].clone()).expect("results");
            let expected: Vec<String> =
                serde_json::from_value(case["winnerWallets"].clone()).expect("winnerWallets");
            let derived =
                derive_single_elim_winners(&players, &results).expect("derivation succeeds");
            assert_eq!(
                derived, expected,
                "parity failure: n={} variant={}",
                case["n"], case["variant"]
            );
        }
    }

    #[test]
    fn two_players_top_seed_wins() {
        let w = derive_single_elim_winners(&players(2), &results(&[(1, "B")])).unwrap();
        // seed 2 (id 2) won match 1 → champion; runner-up is id 1.
        assert_eq!(w[0], format!("0x{:040x}", 2));
        assert_eq!(w[1], format!("0x{:040x}", 1));
    }

    #[test]
    fn four_players_champion_then_final_loser_then_round0_losers() {
        // seedSlots(4) = [0,2,1,3] → match1 = seed1 v seed3, match2 = seed2 v seed4.
        // A wins both semis; final (match3) A wins → champion seed1.
        let w = derive_single_elim_winners(&players(4), &results(&[(1, "A"), (2, "A"), (3, "A")]))
            .unwrap();
        assert_eq!(w[0], format!("0x{:040x}", 1)); // champion
        assert_eq!(w[1], format!("0x{:040x}", 2)); // lost the final
        // then round-0 losers ordered by seed asc: seed3 before seed4
        assert_eq!(w[2], format!("0x{:040x}", 3));
        assert_eq!(w[3], format!("0x{:040x}", 4));
    }

    #[test]
    fn byes_auto_complete() {
        // 3 players, bracket of 4: seedSlots(4)=[0,2,1,3] → match1 = s1 v s3,
        // match2 = s2 v (empty) → auto-completed bye; only real matches recorded.
        let w = derive_single_elim_winners(
            &players(3),
            &results(&[(1, "A"), (3, "A")]), // semi + final
        )
        .unwrap();
        assert_eq!(w[0], format!("0x{:040x}", 1));
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn incomplete_bracket_rejected() {
        assert!(derive_single_elim_winners(&players(4), &results(&[(1, "A")])).is_err());
    }

    #[test]
    fn draw_rejected() {
        assert!(derive_single_elim_winners(&players(2), &results(&[(1, "Draw")])).is_err());
    }

    #[test]
    fn unknown_match_rejected() {
        assert!(derive_single_elim_winners(&players(2), &results(&[(99, "A")])).is_err());
    }

    #[test]
    fn ordering_is_elimination_round_desc_then_seed_asc() {
        // seedSlots(8) = [0,4,2,6,1,5,3,7] →
        // m1 = s1 v s5, m2 = s3 v s7, m3 = s2 v s6, m4 = s4 v s8
        // m5 = w(m1) v w(m2), m6 = w(m3) v w(m4), m7 = final.
        // Scenario: s8 wins it all, beating s1 in the final.
        let rs = vec![
            ResultRow {
                match_id: 1,
                outcome: "A".into(),
            }, // s1
            ResultRow {
                match_id: 2,
                outcome: "A".into(),
            }, // s3
            ResultRow {
                match_id: 3,
                outcome: "A".into(),
            }, // s2
            ResultRow {
                match_id: 4,
                outcome: "B".into(),
            }, // s8
            ResultRow {
                match_id: 5,
                outcome: "A".into(),
            }, // s1 (s3 out, round 1)
            ResultRow {
                match_id: 6,
                outcome: "B".into(),
            }, // s8 (s2 out, round 1)
            ResultRow {
                match_id: 7,
                outcome: "B".into(),
            }, // s8 champion (s1 out, round 2)
        ];
        let w = derive_single_elim_winners(&players(8), &rs).unwrap();
        let ids: Vec<i64> = w
            .iter()
            .map(|s| s.trim_start_matches("0x").parse::<i64>().unwrap_or(0))
            .collect();
        // champion 8; final loser 1; round-1 losers {2,3} by seed; round-0 losers {4,5,6,7} by seed
        assert_eq!(ids, vec![8, 1, 2, 3, 4, 5, 6, 7]);
    }
}
