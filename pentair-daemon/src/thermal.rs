//! Pure thermal model for the pool-temperature predictor (lean v1).
//!
//! Everything in this module is a pure function: no I/O, no network, no clock.
//! The current time is always passed in as `now_unix_ms` so the logic can be
//! unit-tested deterministically, exactly like the existing rate math in
//! `heat.rs`.
//!
//! Model (lumped-capacitance, single node, covered-when-idle):
//!
//! ```text
//! dT/dt = -k_eff * (T - T_eq) - q_evap
//! T_eq  = T_air + solar_bump                 (solar_bump small in v1; clouds-derived)
//! k_eff = k0 + k_wind * wind                 (wind folded into cooling)
//! q_evap = (a + b*wind) * (Psat(T_water) - RH*Psat(T_air))   (Dalton evaporation)
//! ```
//!
//! Within one constant-weather segment the evaporation offset is frozen at the
//! segment-start water temperature, which lets the ODE collapse to the
//! closed-form relaxation
//!
//! ```text
//! T(t1) = T_eq' + (T(t0) - T_eq') * exp(-k_eff * dt)
//! T_eq' = T_eq - q_evap / k_eff
//! ```
//!
//! and segments are chained hour-to-hour over the sensing gap.
//
// Wired into `heat.rs` in a later phase; the public surface is exercised by the
// in-module unit tests for now.
#![allow(dead_code)]

/// Lower bound on the effective cooling constant, guarding against divide-by-zero
/// in the evaporation offset and runaway equilibria.
const MIN_K_PER_HOUR: f64 = 1.0e-6;

/// Sanity bounds on the fitted relaxation time constant (hours).
const TAU_MIN_HOURS: f64 = 2.0;
const TAU_MAX_HOURS: f64 = 200.0;

const MS_PER_HOUR: f64 = 3_600_000.0;

/// One segment of (assumed constant) weather used as a projection step.
///
/// `wind_mph`/`humidity_fraction`/`cloud_fraction` are optional: when weather is
/// stale or unavailable the caller can still supply a cooling-only segment
/// carrying just the controller air temperature, which downgrades the basis to
/// `projected-cooling-only`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherSegment {
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    /// Outdoor air temperature in °F.
    pub air_temp_f: f64,
    /// Wind speed in mph; `None` disables the wind + evaporation terms.
    pub wind_mph: Option<f64>,
    /// Relative humidity as a fraction in `[0, 1]`; `None` disables evaporation.
    pub humidity_fraction: Option<f64>,
    /// Cloud cover as a fraction in `[0, 1]`; `None` disables the solar bump.
    pub cloud_fraction: Option<f64>,
}

impl WeatherSegment {
    /// True when this segment carries the full free-OpenWeather field set
    /// (wind + humidity), enabling the evaporation-aware model.
    fn is_full_weather(&self) -> bool {
        self.wind_mph.is_some() && self.humidity_fraction.is_some()
    }
}

/// A single reliable water-temperature observation (decoupled from heat.rs's
/// integer `ReliableTemperatureObservation` so this module stays pure).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReliableSample {
    pub temperature_f: f64,
    pub observed_at_unix_ms: i64,
}

/// Fitted cooling constants for the covered-when-idle body.
///
/// Lean v1 fits a single effective `k0`; the remaining coefficients come from
/// the physics seed and are refined by closed-loop calibration in later phases.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoolingParams {
    /// Base cooling constant (1/hour). `tau = 1/k0`.
    pub k0_per_hour: f64,
    /// Wind sensitivity of the cooling constant (1/hour per mph).
    pub k_wind_per_hour_per_mph: f64,
    /// Dalton evaporation base coefficient (°F/hour per kPa).
    pub evap_a: f64,
    /// Dalton evaporation wind coefficient (°F/hour per kPa per mph).
    pub evap_b: f64,
    /// Clear-sky solar bump applied to `T_eq` (°F); scaled by `1 - cloud`.
    pub solar_gain_f: f64,
    /// Hard cap on how far forward we project before reverting to measured
    /// (hours). The effective cutoff is `min(3*tau, max_projection_hours)`.
    pub max_projection_hours: f64,
}

impl CoolingParams {
    /// Physically-plausible seed used as the calibration starting point and as
    /// the degenerate-fit fallback.
    pub fn seed() -> Self {
        Self {
            // tau = 50h: a slow, well-insulated covered pool.
            k0_per_hour: 1.0 / 50.0,
            k_wind_per_hour_per_mph: 0.0008,
            evap_a: 0.10,
            evap_b: 0.02,
            solar_gain_f: 0.0,
            max_projection_hours: 12.0,
        }
    }

    /// Relaxation time constant in hours (`1/k0`).
    fn tau_hours(&self) -> f64 {
        1.0 / self.k0_per_hour.max(MIN_K_PER_HOUR)
    }

    /// Effective max-projection cutoff in hours: `min(3*tau, max_projection_hours)`.
    fn cutoff_hours(&self) -> f64 {
        (3.0 * self.tau_hours()).min(self.max_projection_hours.max(0.0))
    }
}

impl Default for CoolingParams {
    fn default() -> Self {
        Self::seed()
    }
}

/// Where a projected temperature came from. Serialized strings match the spec's
/// `prediction_basis` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionBasis {
    /// The reading is current (zero/negligible gap); no projection needed.
    Measured,
    /// Full evaporation-aware model with live weather.
    ProjectedWeather,
    /// Newtonian cooling toward an air temperature, without wind/evaporation.
    ProjectedCoolingOnly,
    /// Cannot honestly project (gap too long, or no anchor/air temp).
    None,
}

impl PredictionBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            PredictionBasis::Measured => "measured",
            PredictionBasis::ProjectedWeather => "projected-weather",
            PredictionBasis::ProjectedCoolingOnly => "projected-cooling-only",
            PredictionBasis::None => "none",
        }
    }
}

/// Confidence band for a projection / fit. Serialized strings match the spec's
/// `prediction_confidence` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionConfidence {
    High,
    Medium,
    Low,
    None,
}

impl PredictionConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            PredictionConfidence::High => "high",
            PredictionConfidence::Medium => "medium",
            PredictionConfidence::Low => "low",
            PredictionConfidence::None => "none",
        }
    }

    /// Drop one tier (e.g. when the rolling MAE is large, or when a projection
    /// rests on the less-trustworthy controller air sensor). A real projection
    /// never collapses below `Low`; only `None` stays `None`.
    pub fn downgraded(self) -> Self {
        match self {
            PredictionConfidence::High => PredictionConfidence::Medium,
            PredictionConfidence::Medium => PredictionConfidence::Low,
            PredictionConfidence::Low => PredictionConfidence::Low,
            PredictionConfidence::None => PredictionConfidence::None,
        }
    }
}

/// Result of projecting the anchor forward to `now`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedTemperature {
    /// Best estimate of the current water temperature in °F.
    pub predicted_f: f64,
    /// Symmetric uncertainty band (± °F).
    pub uncertainty_f: f64,
    pub confidence: PredictionConfidence,
    pub basis: PredictionBasis,
    /// The instant this prediction is for (echoes `now_unix_ms`).
    pub as_of_unix_ms: i64,
}

/// Outcome of `fit_cooling_params`: the fitted parameters plus how much we trust
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoolingFit {
    pub params: CoolingParams,
    pub confidence: PredictionConfidence,
    /// Mean absolute one-step residual of the fit (°F); `NaN` when degenerate.
    pub residual_mae_f: f64,
    /// Number of usable consecutive-sample pairs the fit consumed.
    pub sample_count: usize,
}

/// Physical clamp on any temperature fed into the vapor-pressure `exp`, so a
/// wild/garbage input can never push `q_evap` to `inf`/`NaN`.
const TEMP_CLAMP_MIN_F: f64 = -40.0;
const TEMP_CLAMP_MAX_F: f64 = 140.0;

/// Saturation vapor pressure of water (kPa) for a temperature in °F, via the
/// Magnus-Tetens approximation. The input is clamped to a physical range first
/// so the exponential can never overflow to a non-finite value.
fn saturation_vapor_pressure_kpa(temp_f: f64) -> f64 {
    let clamped_f = temp_f.clamp(TEMP_CLAMP_MIN_F, TEMP_CLAMP_MAX_F);
    let tc = (clamped_f - 32.0) * 5.0 / 9.0;
    0.610_94 * (17.625 * tc / (tc + 243.04)).exp()
}

/// Advance the water temperature across one constant-weather segment using the
/// closed-form relaxation with a frozen evaporation offset.
fn relax_over_segment(
    water_f: f64,
    segment: &WeatherSegment,
    params: &CoolingParams,
    dt_hours: f64,
) -> f64 {
    if dt_hours <= 0.0 {
        return water_f;
    }

    let wind = segment.wind_mph.unwrap_or(0.0).max(0.0);
    let k_eff = (params.k0_per_hour + params.k_wind_per_hour_per_mph * wind).max(MIN_K_PER_HOUR);

    let solar_bump = match segment.cloud_fraction {
        Some(cloud) => params.solar_gain_f * (1.0 - cloud.clamp(0.0, 1.0)),
        None => 0.0,
    };
    let t_eq = segment.air_temp_f + solar_bump;

    // Dalton evaporation, frozen at the segment-start water temperature. Only
    // present when humidity is known (full weather); never adds heat.
    let q_evap = match segment.humidity_fraction {
        Some(rh) => {
            let rh = rh.clamp(0.0, 1.0);
            let driving = saturation_vapor_pressure_kpa(water_f)
                - rh * saturation_vapor_pressure_kpa(segment.air_temp_f);
            ((params.evap_a + params.evap_b * wind) * driving).max(0.0)
        }
        None => 0.0,
    };

    // Fold the constant evaporation loss into an effective equilibrium so the
    // ODE stays a single exponential relaxation.
    let t_eq_eff = t_eq - q_evap / k_eff;
    t_eq_eff + (water_f - t_eq_eff) * (-k_eff * dt_hours).exp()
}

/// Heuristic uncertainty band that grows with the projected gap.
fn uncertainty_for_gap(gap_hours: f64) -> f64 {
    (0.5 + 0.4 * gap_hours).min(6.0)
}

/// Project the last reliable temperature forward to `now_unix_ms` across the
/// supplied hourly weather segments.
///
/// Behaviour:
/// - non-positive gap → returns the anchor unchanged (`basis = measured`);
/// - gap beyond `min(3*tau, max_projection_hours)` → `basis = none`
///   (revert UI to "measured N ago"), predicted value held at the anchor;
/// - no usable segments (no air temperature) → `basis = none`;
/// - segments carry full weather → `basis = projected-weather`;
/// - segments carry air only → `basis = projected-cooling-only`.
pub fn project_temperature(
    anchor: ReliableSample,
    segments: &[WeatherSegment],
    params: &CoolingParams,
    now_unix_ms: i64,
) -> ProjectedTemperature {
    let gap_ms = now_unix_ms - anchor.observed_at_unix_ms;

    // Zero / negative gap: the reading is effectively current.
    if gap_ms <= 0 {
        return ProjectedTemperature {
            predicted_f: anchor.temperature_f,
            uncertainty_f: 0.0,
            confidence: PredictionConfidence::High,
            basis: PredictionBasis::Measured,
            as_of_unix_ms: now_unix_ms,
        };
    }

    let gap_hours = gap_ms as f64 / MS_PER_HOUR;

    // Max-projection cutoff: never fabricate a long-gap number.
    if gap_hours > params.cutoff_hours() {
        return ProjectedTemperature {
            predicted_f: anchor.temperature_f,
            uncertainty_f: uncertainty_for_gap(gap_hours),
            confidence: PredictionConfidence::None,
            basis: PredictionBasis::None,
            as_of_unix_ms: now_unix_ms,
        };
    }

    // Collect segments overlapping (t0, now], sorted by start time.
    let mut usable: Vec<WeatherSegment> = segments
        .iter()
        .copied()
        .filter(|s| s.end_unix_ms > anchor.observed_at_unix_ms && s.start_unix_ms < now_unix_ms)
        .collect();
    usable.sort_by_key(|s| s.start_unix_ms);

    if usable.is_empty() {
        // No air temperature to relax toward → cannot honestly project.
        return ProjectedTemperature {
            predicted_f: anchor.temperature_f,
            uncertainty_f: uncertainty_for_gap(gap_hours),
            confidence: PredictionConfidence::None,
            basis: PredictionBasis::None,
            as_of_unix_ms: now_unix_ms,
        };
    }

    let mut full_weather = true;
    let mut water_f = anchor.temperature_f;
    let mut cursor = anchor.observed_at_unix_ms;
    let mut last_segment = usable[usable.len() - 1];

    for segment in &usable {
        let seg_end = segment.end_unix_ms.min(now_unix_ms);
        if seg_end <= cursor {
            continue;
        }
        let dt_hours = (seg_end - cursor) as f64 / MS_PER_HOUR;
        water_f = relax_over_segment(water_f, segment, params, dt_hours);
        cursor = seg_end;
        full_weather &= segment.is_full_weather();
        last_segment = *segment;
    }

    // If the segments stop short of now, hold the final segment's weather.
    if cursor < now_unix_ms {
        let dt_hours = (now_unix_ms - cursor) as f64 / MS_PER_HOUR;
        water_f = relax_over_segment(water_f, &last_segment, params, dt_hours);
        full_weather &= last_segment.is_full_weather();
    }

    // Never emit a non-finite prediction: if the relaxation produced inf/NaN
    // (degenerate params or pathological inputs), revert to "measured N ago".
    if !water_f.is_finite() {
        return ProjectedTemperature {
            predicted_f: anchor.temperature_f,
            uncertainty_f: uncertainty_for_gap(gap_hours),
            confidence: PredictionConfidence::None,
            basis: PredictionBasis::None,
            as_of_unix_ms: now_unix_ms,
        };
    }

    let basis = if full_weather {
        PredictionBasis::ProjectedWeather
    } else {
        PredictionBasis::ProjectedCoolingOnly
    };

    let confidence = match basis {
        PredictionBasis::ProjectedWeather if gap_hours <= 3.0 => PredictionConfidence::High,
        PredictionBasis::ProjectedWeather if gap_hours <= 6.0 => PredictionConfidence::Medium,
        PredictionBasis::ProjectedCoolingOnly if gap_hours <= 3.0 => PredictionConfidence::Medium,
        _ => PredictionConfidence::Low,
    };

    ProjectedTemperature {
        predicted_f: water_f,
        uncertainty_f: uncertainty_for_gap(gap_hours),
        confidence,
        basis,
        as_of_unix_ms: now_unix_ms,
    }
}

/// Air temperature (°F) covering the given instant, if any segment brackets it.
fn air_temp_at(weather: &[WeatherSegment], at_unix_ms: i64) -> Option<f64> {
    weather
        .iter()
        .find(|s| s.start_unix_ms <= at_unix_ms && at_unix_ms <= s.end_unix_ms)
        .map(|s| s.air_temp_f)
}

/// Golden-section minimization of a unimodal objective on `[lo, hi]`.
fn golden_section_min<F: Fn(f64) -> f64>(f: F, mut lo: f64, mut hi: f64) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_895; // 1/phi
    let mut c = hi - INV_PHI * (hi - lo);
    let mut d = lo + INV_PHI * (hi - lo);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..100 {
        if fc < fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - INV_PHI * (hi - lo);
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + INV_PHI * (hi - lo);
            fd = f(d);
        }
        if (hi - lo).abs() < 1.0e-9 {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// Fit the effective cooling constant `k0` from reliable cooling samples.
///
/// This is the *weak prior* seed: a tiny coarse-grid + golden-section fit over
/// the Newtonian relaxation model, holding the wind/evaporation/solar
/// coefficients at the seed. Sanity clamps reject `k <= 0` or `tau` outside
/// `[2h, 200h]`; any degenerate case falls back to `seed` with `Low` confidence
/// and never produces a NaN or negative `tau`.
pub fn fit_cooling_params(
    samples: &[ReliableSample],
    weather: &[WeatherSegment],
    seed: &CoolingParams,
) -> CoolingFit {
    let mut sorted: Vec<ReliableSample> = samples.to_vec();
    sorted.sort_by_key(|s| s.observed_at_unix_ms);

    // Build (T_start, T_end, dt_hours, air) pairs that have a known air temp.
    let mut pairs: Vec<(f64, f64, f64, f64)> = Vec::new();
    for window in sorted.windows(2) {
        let (a, b) = (window[0], window[1]);
        let dt_hours = (b.observed_at_unix_ms - a.observed_at_unix_ms) as f64 / MS_PER_HOUR;
        if dt_hours <= 0.0 {
            continue;
        }
        let mid = a.observed_at_unix_ms + (b.observed_at_unix_ms - a.observed_at_unix_ms) / 2;
        let Some(air) = air_temp_at(weather, mid) else {
            continue;
        };
        // Identifiability: a cooling pair must sit above air and not be flat.
        if (a.temperature_f - air).abs() < 1.0e-3 {
            continue;
        }
        pairs.push((a.temperature_f, b.temperature_f, dt_hours, air));
    }

    let degenerate = CoolingFit {
        params: *seed,
        confidence: PredictionConfidence::Low,
        residual_mae_f: f64::NAN,
        sample_count: pairs.len(),
    };

    if pairs.len() < 2 {
        return degenerate;
    }

    let sse = |k: f64| -> f64 {
        pairs
            .iter()
            .map(|&(t0, t1, dt, air)| {
                let pred = air + (t0 - air) * (-k * dt).exp();
                (pred - t1).powi(2)
            })
            .sum()
    };

    let k_min = 1.0 / TAU_MAX_HOURS;
    let k_max = 1.0 / TAU_MIN_HOURS;

    // Coarse grid (log-spaced) to bracket the basin, then golden refine.
    let grid = 64usize;
    let (mut best_k, mut best_sse) = (k_min, f64::INFINITY);
    for i in 0..=grid {
        let frac = i as f64 / grid as f64;
        let k = k_min * (k_max / k_min).powf(frac);
        let value = sse(k);
        if value < best_sse {
            best_sse = value;
            best_k = k;
        }
    }
    let lo = (best_k / 1.5).max(k_min);
    let hi = (best_k * 1.5).min(k_max);
    let k = golden_section_min(sse, lo, hi);

    let tau = 1.0 / k;
    if !k.is_finite() || k <= 0.0 || !(TAU_MIN_HOURS..=TAU_MAX_HOURS).contains(&tau) {
        return degenerate;
    }

    let mut params = *seed;
    params.k0_per_hour = k;

    let mae = pairs
        .iter()
        .map(|&(t0, t1, dt, air)| {
            let pred = air + (t0 - air) * (-k * dt).exp();
            (pred - t1).abs()
        })
        .sum::<f64>()
        / pairs.len() as f64;

    let confidence = if pairs.len() >= 5 && mae < 1.0 {
        PredictionConfidence::High
    } else if pairs.len() >= 3 {
        PredictionConfidence::Medium
    } else {
        PredictionConfidence::Low
    };

    CoolingFit {
        params,
        confidence,
        residual_mae_f: mae,
        sample_count: pairs.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 3_600_000;

    fn anchor(temp_f: f64, at_ms: i64) -> ReliableSample {
        ReliableSample {
            temperature_f: temp_f,
            observed_at_unix_ms: at_ms,
        }
    }

    fn cooling_only_segment(start: i64, end: i64, air: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: air,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: None,
        }
    }

    fn full_segment(start: i64, end: i64, air: f64, wind: f64, rh: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: air,
            wind_mph: Some(wind),
            humidity_fraction: Some(rh),
            cloud_fraction: Some(0.5),
        }
    }

    /// Conduction-only params (no wind/evap/solar), tau = 1/k0.
    fn conduction_params(k0: f64) -> CoolingParams {
        CoolingParams {
            k0_per_hour: k0,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.0,
            evap_b: 0.0,
            solar_gain_f: 0.0,
            max_projection_hours: 48.0,
        }
    }

    /// Fine forward-Euler reference integration of the no-evaporation ODE.
    fn euler_reference(t0: f64, air: f64, k_eff: f64, total_hours: f64) -> f64 {
        let step_h = 1.0 / 3600.0; // one-second steps
        let steps = (total_hours / step_h).round() as i64;
        let mut t = t0;
        for _ in 0..steps {
            t += -k_eff * (t - air) * step_h;
        }
        t
    }

    #[test]
    fn zero_gap_returns_anchor() {
        let params = conduction_params(0.1);
        let now = 1_000_000;
        let out = project_temperature(anchor(82.0, now), &[], &params, now);
        assert_eq!(out.predicted_f, 82.0);
        assert_eq!(out.basis, PredictionBasis::Measured);
        assert_eq!(out.confidence, PredictionConfidence::High);
        assert_eq!(out.as_of_unix_ms, now);
    }

    #[test]
    fn negative_gap_returns_anchor() {
        let params = conduction_params(0.1);
        let out = project_temperature(anchor(82.0, 2_000_000), &[], &params, 1_000_000);
        assert_eq!(out.predicted_f, 82.0);
        assert_eq!(out.basis, PredictionBasis::Measured);
    }

    #[test]
    fn constant_sub_air_matches_fine_euler() {
        let k0 = 0.1;
        let params = conduction_params(k0);
        let t0 = 0;
        let now = 3 * HOUR_MS; // 3-hour gap
        let air = 70.0;
        // Air present but no wind/humidity -> pure Newtonian cooling.
        let segment = cooling_only_segment(t0, now, air);
        let out = project_temperature(anchor(85.0, t0), &[segment], &params, now);

        let expected = euler_reference(85.0, air, k0, 3.0);
        assert!(
            (out.predicted_f - expected).abs() < 1.0e-3,
            "closed-form {} vs euler {}",
            out.predicted_f,
            expected
        );
        // And it really cooled toward air.
        assert!(out.predicted_f < 85.0 && out.predicted_f > air);
        assert_eq!(out.basis, PredictionBasis::ProjectedCoolingOnly);
    }

    #[test]
    fn evaporation_increases_loss_with_wind() {
        // No conductive contrast (water == air); only evaporation can cool.
        let params = CoolingParams {
            k0_per_hour: 0.01,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.1,
            evap_b: 0.1,
            solar_gain_f: 0.0,
            max_projection_hours: 48.0,
        };
        let now = 2 * HOUR_MS;
        let calm = full_segment(0, now, 80.0, 0.0, 0.3);
        let windy = full_segment(0, now, 80.0, 15.0, 0.3);

        let calm_out = project_temperature(anchor(80.0, 0), &[calm], &params, now);
        let windy_out = project_temperature(anchor(80.0, 0), &[windy], &params, now);

        assert!(calm_out.predicted_f < 80.0, "evaporation should cool");
        assert!(
            windy_out.predicted_f < calm_out.predicted_f,
            "more wind -> more evaporative loss: windy {} vs calm {}",
            windy_out.predicted_f,
            calm_out.predicted_f
        );
        assert_eq!(windy_out.basis, PredictionBasis::ProjectedWeather);
    }

    #[test]
    fn evaporation_increases_loss_with_low_humidity() {
        let params = CoolingParams {
            k0_per_hour: 0.01,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.1,
            evap_b: 0.1,
            solar_gain_f: 0.0,
            max_projection_hours: 48.0,
        };
        let now = 2 * HOUR_MS;
        let humid = full_segment(0, now, 80.0, 5.0, 0.9);
        let dry = full_segment(0, now, 80.0, 5.0, 0.1);

        let humid_out = project_temperature(anchor(80.0, 0), &[humid], &params, now);
        let dry_out = project_temperature(anchor(80.0, 0), &[dry], &params, now);

        assert!(
            dry_out.predicted_f < humid_out.predicted_f,
            "drier air -> more evaporative loss: dry {} vs humid {}",
            dry_out.predicted_f,
            humid_out.predicted_f
        );
    }

    #[test]
    fn multi_segment_chain_equals_single_when_params_equal() {
        let params = conduction_params(0.2);
        let air = 68.0;
        let now = 2 * HOUR_MS;

        let single = cooling_only_segment(0, now, air);
        let one = project_temperature(anchor(88.0, 0), &[single], &params, now);

        let seg_a = cooling_only_segment(0, HOUR_MS, air);
        let seg_b = cooling_only_segment(HOUR_MS, now, air);
        let two = project_temperature(anchor(88.0, 0), &[seg_a, seg_b], &params, now);

        assert!(
            (one.predicted_f - two.predicted_f).abs() < 1.0e-9,
            "single {} vs chained {}",
            one.predicted_f,
            two.predicted_f
        );
    }

    #[test]
    fn weather_absent_falls_back_to_cooling_only_basis() {
        let params = conduction_params(0.1);
        let now = 2 * HOUR_MS;
        let segment = cooling_only_segment(0, now, 70.0);
        let out = project_temperature(anchor(85.0, 0), &[segment], &params, now);
        assert_eq!(out.basis, PredictionBasis::ProjectedCoolingOnly);
        assert!(out.predicted_f < 85.0);
    }

    #[test]
    fn no_segments_yields_basis_none() {
        let params = conduction_params(0.1);
        let now = 2 * HOUR_MS;
        let out = project_temperature(anchor(85.0, 0), &[], &params, now);
        assert_eq!(out.basis, PredictionBasis::None);
        assert_eq!(out.confidence, PredictionConfidence::None);
        // Holds the anchor rather than fabricating.
        assert_eq!(out.predicted_f, 85.0);
    }

    #[test]
    fn gap_beyond_cutoff_yields_basis_none() {
        // tau = 50h -> 3*tau = 150h, but max_projection caps at 12h.
        let params = CoolingParams {
            max_projection_hours: 12.0,
            ..conduction_params(1.0 / 50.0)
        };
        let now = 20 * HOUR_MS; // 20h gap > 12h cutoff
        let segment = cooling_only_segment(0, now, 70.0);
        let out = project_temperature(anchor(85.0, 0), &[segment], &params, now);
        assert_eq!(out.basis, PredictionBasis::None);
        assert_eq!(out.confidence, PredictionConfidence::None);
        assert_eq!(out.predicted_f, 85.0);
        assert!(out.predicted_f.is_finite());
    }

    #[test]
    fn cutoff_uses_three_tau_when_smaller_than_max() {
        // tau = 1h -> 3*tau = 3h is the binding cutoff (< 12h).
        let params = CoolingParams {
            max_projection_hours: 12.0,
            ..conduction_params(1.0)
        };
        let now = 5 * HOUR_MS; // 5h > 3h cutoff
        let segment = cooling_only_segment(0, now, 70.0);
        let out = project_temperature(anchor(85.0, 0), &[segment], &params, now);
        assert_eq!(out.basis, PredictionBasis::None);
    }

    // ----- fit_cooling_params -----

    /// Generate noiseless Newtonian cooling samples toward `air` at known k.
    fn synthetic_cooling(
        start_temp: f64,
        air: f64,
        k: f64,
        count: usize,
        noise: &[f64],
    ) -> (Vec<ReliableSample>, Vec<WeatherSegment>) {
        let mut samples = Vec::new();
        let mut weather = Vec::new();
        for i in 0..count {
            let t_hours = i as f64;
            let clean = air + (start_temp - air) * (-k * t_hours).exp();
            let n = noise.get(i).copied().unwrap_or(0.0);
            samples.push(ReliableSample {
                temperature_f: clean + n,
                observed_at_unix_ms: (i as i64) * HOUR_MS,
            });
        }
        // One enclosing weather segment carrying the air temperature.
        weather.push(cooling_only_segment(
            -HOUR_MS,
            (count as i64) * HOUR_MS,
            air,
        ));
        (samples, weather)
    }

    #[test]
    fn fit_recovers_known_k_noiseless() {
        let k_true = 0.05;
        let seed = CoolingParams::seed();
        let (samples, weather) = synthetic_cooling(90.0, 60.0, k_true, 12, &[]);
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert!(
            (fit.params.k0_per_hour - k_true).abs() < 1.0e-3,
            "recovered k {} vs true {}",
            fit.params.k0_per_hour,
            k_true
        );
        assert!(fit.params.k0_per_hour > 0.0);
        let tau = 1.0 / fit.params.k0_per_hour;
        assert!((TAU_MIN_HOURS..=TAU_MAX_HOURS).contains(&tau));
        assert_eq!(fit.confidence, PredictionConfidence::High);
        // Untouched coefficients stay at the seed.
        assert_eq!(fit.params.evap_a, seed.evap_a);
    }

    #[test]
    fn fit_recovers_known_k_noisy() {
        let k_true = 0.05;
        let seed = CoolingParams::seed();
        // Small deterministic zero-ish-mean noise.
        let noise = [
            0.08, -0.06, 0.05, -0.04, 0.07, -0.05, 0.03, -0.02, 0.06, -0.07, 0.04, -0.03,
        ];
        let (samples, weather) = synthetic_cooling(90.0, 60.0, k_true, 12, &noise);
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert!(
            (fit.params.k0_per_hour - k_true).abs() < 1.0e-2,
            "recovered k {} vs true {}",
            fit.params.k0_per_hour,
            k_true
        );
        let tau = 1.0 / fit.params.k0_per_hour;
        assert!((TAU_MIN_HOURS..=TAU_MAX_HOURS).contains(&tau));
    }

    #[test]
    fn fit_degenerate_returns_seed_low_confidence() {
        let seed = CoolingParams::seed();
        // Single sample -> no pairs.
        let samples = vec![anchor(80.0, 0)];
        let weather = vec![cooling_only_segment(-HOUR_MS, HOUR_MS, 70.0)];
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert_eq!(fit.params, seed);
        assert_eq!(fit.confidence, PredictionConfidence::Low);
        let tau = 1.0 / fit.params.k0_per_hour;
        assert!(tau > 0.0 && tau.is_finite());
    }

    #[test]
    fn fit_without_weather_is_degenerate_not_nan_tau() {
        let seed = CoolingParams::seed();
        let samples = vec![
            anchor(90.0, 0),
            anchor(88.0, HOUR_MS),
            anchor(86.0, 2 * HOUR_MS),
        ];
        // No weather -> no air temperature -> no usable pairs.
        let fit = fit_cooling_params(&samples, &[], &seed);
        assert_eq!(fit.params, seed);
        assert_eq!(fit.confidence, PredictionConfidence::Low);
        let tau = 1.0 / fit.params.k0_per_hour;
        assert!(tau.is_finite() && tau > 0.0);
    }

    #[test]
    fn saturation_pressure_increases_with_temperature() {
        assert!(saturation_vapor_pressure_kpa(90.0) > saturation_vapor_pressure_kpa(60.0));
    }
}
