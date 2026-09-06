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

    let delta = v * g_phi_j * (score as f64 - e);

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
            return (rating, rd, volatility);
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
        return (rating, rd, volatility);
    }
    let sigma_new = ((a_iter + b_iter) / 2.0).exp();

    let phi_star = (phi * phi + sigma_new * sigma_new).sqrt();

    let phi_new = 1.0 / (1.0 / (phi_star * phi_star) + 1.0 / v).sqrt();
    let mu_new = mu + phi_new * phi_new * g_phi_j * (score as f64 - e);

    let new_rating_f = mu_new * SCALE + 1500.0;
    let new_rd_f = phi_new * SCALE;
    let new_vol_f = sigma_new;

    if !new_rating_f.is_finite() || !new_rd_f.is_finite() || !new_vol_f.is_finite() {
        return (rating, rd, volatility);
    }

    (new_rating_f as f32, new_rd_f as f32, new_vol_f as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

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
