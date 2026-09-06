//! Glicko-2 rating update (Glickman 2013). Pure function — no I/O, no async.
//!
//! NaN/Inf inputs, non-positive volatility, negative rd, or solver
//! non-convergence all return the original profile unchanged. Both the
//! Illinois-method root finder and the bracketing loop are bounded by
//! `MAX_GLINKO_ITERATIONS = 100` (typical convergence is 5-10).

const TAU: f64 = 0.5;
const SCALE: f64 = 173.7178;
const MAX_GLINKO_ITERATIONS: f64 = 100.0;

/// Computes the Glicko-2 rating update after a single game.
///
/// `score` is 1.0 (win), 0.5 (draw), or 0.0 (loss).
///
/// **Safety:** any NaN/Inf input, non-positive volatility, or solver
/// non-convergence returns the original `(rating, rd, volatility)` unchanged.
pub fn glicko2_update(
    rating: f32,
    rd: f32,
    volatility: f32,
    opponent_rating: f32,
    opponent_rd: f32,
    score: f32,
) -> (f32, f32, f32) {
    let inputs_f64 = [
        rating as f64,
        rd as f64,
        volatility as f64,
        opponent_rating as f64,
        opponent_rd as f64,
        score as f64,
    ];
    if inputs_f64.iter().any(|v| !v.is_finite()) || volatility <= 0.0 || rd < 0.0 {
        return (rating, rd, volatility);
    }

    let mu = (rating as f64 - 1500.0) / SCALE;
    let phi = rd as f64 / SCALE;
    let sigma = volatility as f64;

    let mu_j = (opponent_rating as f64 - 1500.0) / SCALE;
    let phi_j = opponent_rd as f64 / SCALE;

    let g_phi_j =
        1.0 / (1.0 + 3.0 * phi_j * phi_j / (std::f64::consts::PI * std::f64::consts::PI)).sqrt();

    let e = 1.0 / (1.0 + (-g_phi_j * (mu - mu_j)).exp());

    let v = 1.0 / (g_phi_j * g_phi_j * e * (1.0 - e));
    if !v.is_finite() {
        return (rating, rd, volatility);
    }

    let outcome_sum = g_phi_j * (score as f64 - e);

    match period_update(mu, phi, sigma, v, outcome_sum) {
        Some((mu_new, phi_new, sigma_new)) => {
            let new_rating_f = mu_new * SCALE + 1500.0;
            let new_rd_f = phi_new * SCALE;
            let new_vol_f = sigma_new;
            if new_rating_f.is_finite() && new_rd_f.is_finite() && new_vol_f.is_finite() {
                (new_rating_f as f32, new_rd_f as f32, new_vol_f as f32)
            } else {
                (rating, rd, volatility)
            }
        }
        None => (rating, rd, volatility),
    }
}

/// Update one player's rating against a **field of opponents** (N-player
/// or team match), using the Glickman 2013 rating-period treatment of
/// multiple simultaneous games: one aggregate `v` / `Δ` over the whole
/// field, solved in a single volatility step. Order-independent by
/// construction — every opponent contributes from the same pre-match
/// profile.
///
/// `scores` aligns with `opponents`: the player's result (1.0/0.5/0.0)
/// against each opponent individually. Team matches map naturally — every
/// member of the winning team scores 1.0 against every member of the
/// losing team, 0.5 across a draw.
///
/// Same safety contract as [`glicko2_update`]: any non-finite input or
/// solver failure returns the original profile unchanged.
pub fn glicko2_update_vs_many(
    rating: f32,
    rd: f32,
    volatility: f32,
    opponents: &[(f32, f32)],
    scores: &[f32],
) -> (f32, f32, f32) {
    if opponents.len() != scores.len() || opponents.is_empty() {
        return (rating, rd, volatility);
    }
    let all_finite = opponents
        .iter()
        .all(|(r, rdj)| (*r as f64).is_finite() && (*rdj as f64).is_finite())
        && scores.iter().all(|s| (*s as f64).is_finite())
        && (rating as f64).is_finite()
        && (rd as f64).is_finite()
        && (volatility as f64).is_finite();
    if !all_finite || volatility <= 0.0 || rd < 0.0 {
        return (rating, rd, volatility);
    }

    let mu = (rating as f64 - 1500.0) / SCALE;
    let phi = rd as f64 / SCALE;
    let sigma = volatility as f64;

    // Aggregate the period: v = (Σ g²E(1−E))⁻¹, outcome_sum = Σ g(s−E).
    let mut inv_v = 0.0f64;
    let mut outcome_sum = 0.0f64;
    for ((opp_rating, opp_rd), score) in opponents.iter().zip(scores.iter()) {
        let mu_j = (*opp_rating as f64 - 1500.0) / SCALE;
        let phi_j = *opp_rd as f64 / SCALE;
        let g_j = 1.0
            / (1.0 + 3.0 * phi_j * phi_j / (std::f64::consts::PI * std::f64::consts::PI)).sqrt();
        let e_j = 1.0 / (1.0 + (-g_j * (mu - mu_j)).exp());
        inv_v += g_j * g_j * e_j * (1.0 - e_j);
        outcome_sum += g_j * (*score as f64 - e_j);
    }
    if inv_v <= 0.0 || !inv_v.is_finite() || !outcome_sum.is_finite() {
        return (rating, rd, volatility);
    }
    let v = 1.0 / inv_v;

    match period_update(mu, phi, sigma, v, outcome_sum) {
        Some((mu_new, phi_new, sigma_new)) => {
            let new_rating_f = mu_new * SCALE + 1500.0;
            let new_rd_f = phi_new * SCALE;
            let new_vol_f = sigma_new;
            if new_rating_f.is_finite() && new_rd_f.is_finite() && new_vol_f.is_finite() {
                (new_rating_f as f32, new_rd_f as f32, new_vol_f as f32)
            } else {
                (rating, rd, volatility)
            }
        }
        None => (rating, rd, volatility),
    }
}

/// The shared Glicko-2 rating-period step (paper steps 5–7): volatility
/// solve via the Illinois method, then φ and μ updates from the period's
/// aggregate `v` and `outcome_sum = Δ / v`. `None` = solver failure.
fn period_update(
    mu: f64,
    phi: f64,
    sigma: f64,
    v: f64,
    outcome_sum: f64,
) -> Option<(f64, f64, f64)> {
    let delta = v * outcome_sum;

    let a = sigma.ln();
    let f = |x: f64| -> f64 {
        let ex = x.exp();
        let phi2 = phi * phi;
        let delta2 = delta * delta;
        let num = ex * (delta2 - phi2 - v - ex);
        let denom = 2.0 * (phi2 + v + ex).powi(2);
        num / denom - (x - a) / (TAU * TAU)
    };

    let mut a_iter = a;
    let mut b_iter = if delta * delta > phi * phi + v {
        (delta * delta - phi * phi - v).ln()
    } else {
        let mut k = 1.0;
        while k <= MAX_GLINKO_ITERATIONS && f(a - k * TAU) < 0.0 {
            k += 1.0;
        }
        if k > MAX_GLINKO_ITERATIONS {
            return None;
        }
        a - k * TAU
    };
    let epsilon = 1e-6;
    let mut fa = f(a_iter);
    let mut fb = f(b_iter);
    for _ in 0..MAX_GLINKO_ITERATIONS as usize {
        let denom = fb - fa;
        if denom.abs() < epsilon {
            break;
        }
        let c = a_iter + (a_iter - b_iter) * fa / denom;
        let fc = f(c);
        if fc * fb < 0.0 {
            a_iter = b_iter;
            fa = fb;
        } else {
            fa /= 2.0;
        }
        b_iter = c;
        fb = fc;
        if (b_iter - a_iter).abs() < epsilon {
            break;
        }
    }
    if !a_iter.is_finite() || !b_iter.is_finite() {
        return None;
    }
    let sigma_new = ((a_iter + b_iter) / 2.0).exp();

    let phi_star = (phi * phi + sigma_new * sigma_new).sqrt();
    let phi_new = 1.0 / (1.0 / (phi_star * phi_star) + 1.0 / v).sqrt();
    let mu_new = mu + phi_new * phi_new * outcome_sum;

    Some((mu_new, phi_new, sigma_new))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- N-player / rating-period semantics -------------------------------

    #[test]
    fn vs_many_single_opponent_matches_pairwise_update() {
        let single = glicko2_update(1500.0, 200.0, 0.06, 1600.0, 30.0, 0.0);
        let many = glicko2_update_vs_many(1500.0, 200.0, 0.06, &[(1600.0, 30.0)], &[0.0]);
        assert_eq!(single, many, "field of one == pairwise update");
    }

    #[test]
    fn vs_many_is_order_independent() {
        let opponents = [(1400.0, 30.0), (1700.0, 100.0), (1525.0, 60.0)];
        let scores = [1.0, 0.0, 0.5];
        let a = glicko2_update_vs_many(1500.0, 200.0, 0.06, &opponents, &scores);
        let shuffled = [opponents[2], opponents[0], opponents[1]];
        let shuffled_scores = [scores[2], scores[0], scores[1]];
        let b = glicko2_update_vs_many(1500.0, 200.0, 0.06, &shuffled, &shuffled_scores);
        assert_eq!(
            a, b,
            "one rating period => opponents contribute symmetrically"
        );
    }

    #[test]
    fn vs_many_win_raises_loses_lower_uncertainty() {
        let opponents = [(1400.0, 100.0), (1450.0, 80.0), (1350.0, 120.0)];
        let (r, rd, _) = glicko2_update_vs_many(1500.0, 350.0, 0.06, &opponents, &[1.0, 1.0, 1.0]);
        assert!(r > 1500.0, "sweeping the field must raise rating, got {r}");
        assert!(rd < 350.0, "playing a field must tighten uncertainty");
    }

    #[test]
    fn vs_many_confident_field_resists_change() {
        // Same 50/50 split against increasingly confident opposition.
        let wide = glicko2_update_vs_many(1500.0, 200.0, 0.06, &[(1500.0, 300.0); 2], &[0.5, 0.5]);
        let tight = glicko2_update_vs_many(1500.0, 200.0, 0.06, &[(1500.0, 30.0); 2], &[0.5, 0.5]);
        // Draws at par: no rating move either way, but tight opposition
        // should leave the player with MORE information (lower rd).
        assert!((wide.0 - 1500.0).abs() < 10.0);
        assert!((tight.0 - 1500.0).abs() < 10.0);
        assert!(tight.1 < wide.1, "confident opponents must inform more");
    }

    #[test]
    fn vs_many_rejects_empty_and_mismatched() {
        let p = (1500.0, 200.0, 0.06);
        assert_eq!(glicko2_update_vs_many(p.0, p.1, p.2, &[], &[]), p);
        assert_eq!(
            glicko2_update_vs_many(p.0, p.1, p.2, &[(1500.0, 30.0)], &[1.0, 1.0]),
            p,
            "score/opponent length mismatch returns profile unchanged"
        );
    }

    #[test]
    fn vs_many_nan_and_bad_vol_guards() {
        assert!(
            glicko2_update_vs_many(f32::NAN, 200.0, 0.06, &[(1500.0, 30.0)], &[1.0])
                .0
                .is_nan()
        );
        // Non-positive volatility echoes the *input* profile unchanged.
        assert_eq!(
            glicko2_update_vs_many(1500.0, 200.0, 0.0, &[(1500.0, 30.0)], &[1.0]),
            (1500.0, 200.0, 0.0)
        );
    }

    #[test]
    fn vs_many_extreme_field_no_corruption() {
        let (r, rd, vol) = glicko2_update_vs_many(
            1500.0,
            200.0,
            0.06,
            &[(9000.0, 30.0), (200.0, 30.0), (10000.0, 350.0)],
            &[1.0, 0.0, 0.5],
        );
        assert!(r.is_finite() && rd.is_finite() && vol.is_finite());
    }

    #[test]
    fn win_increases_rating() {
        let (r, _, _) = glicko2_update(1500.0, 200.0, 0.06, 1400.0, 30.0, 1.0);
        assert!(r > 1500.0);
    }

    #[test]
    fn loss_decreases_rating() {
        let (r, _, _) = glicko2_update(1500.0, 200.0, 0.06, 1600.0, 30.0, 0.0);
        assert!(r < 1500.0);
    }

    #[test]
    fn draw_near_stable() {
        let (r, _, _) = glicko2_update(1500.0, 200.0, 0.06, 1500.0, 30.0, 0.5);
        assert!((r - 1500.0).abs() < 10.0);
    }

    #[test]
    fn nan_input_returns_original() {
        let (r, _, _) = glicko2_update(f32::NAN, 200.0, 0.06, 1400.0, 30.0, 1.0);
        assert!(r.is_nan(), "NaN rating input echoes back unchanged");
    }

    #[test]
    fn inf_input_returns_original() {
        let (r, _, _) = glicko2_update(1500.0, f32::INFINITY, 0.06, 1400.0, 30.0, 1.0);
        assert_eq!(r, 1500.0);
    }

    #[test]
    fn non_positive_volatility_returns_original() {
        let (r, _, _) = glicko2_update(1500.0, 200.0, 0.0, 1400.0, 30.0, 1.0);
        assert_eq!(r, 1500.0);
    }

    #[test]
    fn extreme_rating_gap_no_corruption() {
        let (r, rd, vol) = glicko2_update(1500.0, 200.0, 0.06, 10000.0, 30.0, 1.0);
        assert!(r.is_finite());
        assert!(rd.is_finite());
        assert!(vol.is_finite());
    }
}
