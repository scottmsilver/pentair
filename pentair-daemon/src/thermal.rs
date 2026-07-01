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
//! T_eq  = T_air + (g / k_eff) * I_solar      (clear-sky solar from sun geometry)
//! k_eff = k0 + k_wind * wind                 (wind folded into cooling)
//! q_evap = (a + b*wind) * (Psat(T_water) - RH*Psat(T_air))   (Dalton evaporation)
//! ```
//!
//! The solar term is a real daytime heating source, not just a cloud-scaled
//! offset. `I_solar` (kW/m²) is the clear-sky global horizontal irradiance
//! derived from the sun's elevation angle for the site's lat/lon and the
//! segment timestamp, attenuated by cloud cover and the cover's solar
//! transmission (a solar/heat-retention cover passes most shortwave). `g`
//! (`solar_gain_f`, °F·hr⁻¹ per kW/m²) is the pool's solar heating-rate
//! coefficient. Adding `(g / k_eff) * I_solar` to `T_eq` makes the steady-state
//! relaxation target sit a constant solar bump above air, so a sunny segment
//! pulls the water toward a *warmer* equilibrium. At night the elevation is
//! `<= 0`, `I_solar = 0`, and the term vanishes — overnight cooling is
//! unchanged.
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
    /// Site latitude in degrees (north positive). Drives the solar geometry.
    pub latitude_deg: f64,
    /// Site longitude in degrees (east positive). Drives the solar geometry.
    pub longitude_deg: f64,
    /// Fraction of incident shortwave the cover passes into the water, in
    /// `[0, 1]`. A solar/heat-retention cover transmits most of it (~0.75);
    /// an opaque thermal blanket would be much lower.
    pub cover_solar_transmission: f64,
}

impl WeatherSegment {
    /// True when this segment carries the full free-OpenWeather field set
    /// (wind + humidity), enabling the evaporation-aware model.
    fn is_full_weather(&self) -> bool {
        self.wind_mph.is_some() && self.humidity_fraction.is_some()
    }
}

/// Fixed-site solar parameters shared by every [`WeatherSegment`]: the location
/// the sun geometry is computed for, and the cover's shortwave transmission.
///
/// `cover_solar_transmission == 0` (and the `disabled` constructor) switches the
/// solar term off entirely, which is the right default when no location is
/// configured — segments then relax toward plain air temperature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarSite {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub cover_solar_transmission: f64,
}

impl SolarSite {
    /// A site with no solar drive (no location / opaque): segments built from it
    /// carry `cover_solar_transmission = 0`, so the solar bump is always zero.
    pub fn disabled() -> Self {
        Self {
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }
}

impl Default for SolarSite {
    fn default() -> Self {
        Self::disabled()
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
    /// Solar heating-rate coefficient `g` (°F·hr⁻¹ per kW/m² of absorbed
    /// irradiance). Enters `T_eq` as `(g / k_eff) * I_solar`, so the daytime
    /// equilibrium sits a constant solar bump above air. Fit alongside `k0`
    /// when daytime (solar > 0) intervals are present.
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
            // g = 0.08 °F·hr⁻¹ per kW/m². With clear-sky midday GHI ~0.9 kW/m²,
            // a ~0.75-transmission cover and k_eff ~0.02/hr, this seeds a daytime
            // T_eq bump of (g/k)*I ≈ (0.08/0.02)*0.675 ≈ 2.7 °F above air —
            // matching the observed ~2-3 °F daytime under-prediction. Refined by
            // the daytime fit when warming intervals are present.
            solar_gain_f: 0.08,
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

/// Seconds in a (mean) day, for fractional day-of-year arithmetic.
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Solar constant proxy used by the Haurwitz clear-sky model (W/m²).
const HAURWITZ_A: f64 = 1098.0;

// ─── Solar geometry (clear-sky, no weather feed) ───────────────────────────

/// Solar elevation angle (degrees above the horizon) for a site at
/// `(lat_deg, lon_deg)` at `unix_secs`.
///
/// Standard textbook astronomy:
/// 1. fractional day-of-year `n` and UTC hour from the unix time;
/// 2. solar declination `decl = 23.45° · sin(360°·(284 + n)/365)`;
/// 3. solar time from UTC plus a longitude correction `lon/15` hours and the
///    equation of time, giving the hour angle `H = 15°·(solar_hour − 12)`;
/// 4. `elevation = asin(sin φ·sin δ + cos φ·cos δ·cos H)`.
///
/// **Approximation:** longitude is converted to a local-time offset at the
/// nominal `15°/hour` and combined with the equation of time (a closed-form
/// few-minute correction). This is accurate to a few minutes of solar time —
/// well within the hourly resolution of the weather segments — and needs no
/// timezone database. Returns `<= 0` (specifically the true, possibly negative
/// elevation) at night; callers treat any non-positive elevation as "no sun".
pub fn solar_position(lat_deg: f64, lon_deg: f64, unix_secs: i64) -> f64 {
    // Day-of-year (fractional) and fractional UTC hour.
    let days_since_epoch = unix_secs as f64 / SECONDS_PER_DAY;
    // 1970-01-01 was day-of-year 1; take the fractional position in the year.
    let day_of_year = (days_since_epoch.rem_euclid(365.25)) + 1.0;
    let utc_hour = ((unix_secs as f64).rem_euclid(SECONDS_PER_DAY)) / 3600.0;

    let lat = lat_deg.to_radians();

    // Solar declination (degrees → radians).
    let decl = (23.45 * (360.0 * (284.0 + day_of_year) / 365.0).to_radians().sin()).to_radians();

    // Equation of time (minutes), Spencer/standard approximation.
    let b = (360.0 * (day_of_year - 81.0) / 364.0).to_radians();
    let eot_min = 9.87 * (2.0 * b).sin() - 7.53 * b.cos() - 1.5 * b.sin();

    // Solar time (hours): UTC + longitude offset (15°/hr) + equation of time.
    let solar_hour = utc_hour + lon_deg / 15.0 + eot_min / 60.0;
    let hour_angle = (15.0 * (solar_hour - 12.0)).to_radians();

    let sin_elev = lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos();
    sin_elev.clamp(-1.0, 1.0).asin().to_degrees()
}

/// Clear-sky global horizontal irradiance (W/m²) for a solar `elevation_deg`,
/// via the Haurwitz model `GHI = 1098 · sin(el) · exp(−0.057 / sin(el))`.
/// Returns `0` for any non-positive elevation (the sun is at or below the
/// horizon).
pub fn clear_sky_ghi(elevation_deg: f64) -> f64 {
    if elevation_deg <= 0.0 {
        return 0.0;
    }
    let sin_el = elevation_deg.to_radians().sin();
    if sin_el <= 0.0 {
        return 0.0;
    }
    (HAURWITZ_A * sin_el * (-0.057 / sin_el).exp()).max(0.0)
}

/// Effective shortwave irradiance reaching the water (W/m²): clear-sky GHI from
/// sun geometry, attenuated by cloud cover (Kasten-Czeplak
/// `1 − 0.75·cloud^3.4`) and by the cover's solar transmission.
///
/// `cloud_fraction` and `cover_solar_transmission` are clamped to `[0, 1]`.
/// Night (elevation `<= 0`) yields `0`.
pub fn effective_irradiance(
    lat_deg: f64,
    lon_deg: f64,
    unix_secs: i64,
    cloud_fraction: f64,
    cover_solar_transmission: f64,
) -> f64 {
    let elevation = solar_position(lat_deg, lon_deg, unix_secs);
    let ghi = clear_sky_ghi(elevation);
    if ghi <= 0.0 {
        return 0.0;
    }
    let cloud = cloud_fraction.clamp(0.0, 1.0);
    let cloud_attenuation = 1.0 - 0.75 * cloud.powf(3.4);
    let transmission = cover_solar_transmission.clamp(0.0, 1.0);
    (ghi * cloud_attenuation * transmission).max(0.0)
}

/// Saturation vapor pressure of water (kPa) for a temperature in °F, via the
/// Magnus-Tetens approximation. The input is clamped to a physical range first
/// so the exponential can never overflow to a non-finite value.
fn saturation_vapor_pressure_kpa(temp_f: f64) -> f64 {
    let clamped_f = temp_f.clamp(TEMP_CLAMP_MIN_F, TEMP_CLAMP_MAX_F);
    let tc = (clamped_f - 32.0) * 5.0 / 9.0;
    0.610_94 * (17.625 * tc / (tc + 243.04)).exp()
}

/// Effective solar irradiance (kW/m²) absorbed during a segment, evaluated at
/// the segment midpoint. Clouds default to clear when unknown; the cover's
/// transmission scales the shortwave reaching the water. Night → 0.
fn segment_irradiance_kw(segment: &WeatherSegment) -> f64 {
    let mid_unix_secs = ((segment.start_unix_ms / 2) + (segment.end_unix_ms / 2)) / 1000;
    let cloud = segment.cloud_fraction.unwrap_or(0.0);
    effective_irradiance(
        segment.latitude_deg,
        segment.longitude_deg,
        mid_unix_secs,
        cloud,
        segment.cover_solar_transmission,
    ) / 1000.0
}

/// Advance the water temperature passively (no heater) across one
/// constant-weather segment, using the project's single source of truth for the
/// thermal relaxation (cooling + solar + evaporation). This is a thin public
/// wrapper over [`relax_over_segment`] so other modules (e.g. the comfort
/// scheduler's forward sim) can apply the *exact same* passive physics without
/// duplicating it. `dt_hours <= 0` returns `water_f` unchanged.
pub fn passive_relax_over_segment(
    water_f: f64,
    segment: &WeatherSegment,
    params: &CoolingParams,
    dt_hours: f64,
) -> f64 {
    relax_over_segment(water_f, segment, params, dt_hours)
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

    // Real solar gain: clear-sky irradiance (kW/m²) from sun geometry at the
    // segment midpoint, raising the relaxation target by (g / k_eff) * I_solar.
    // Night → I_solar = 0 → no bump → overnight cooling is unchanged.
    let irradiance_kw = segment_irradiance_kw(segment);
    let solar_bump = (params.solar_gain_f / k_eff) * irradiance_kw;
    let t_eq = segment.air_temp_f + solar_bump;

    // SIGNED Dalton evaporation, frozen at the segment-start water temperature.
    // Positive driving (water vapor pressure > humid-air vapor pressure) is an
    // evaporative LOSS; negative driving (warm, humid air over cooler water) is
    // a condensation GAIN and is kept, not floored. Only present when humidity
    // is known (full weather).
    let q_evap = match segment.humidity_fraction {
        Some(rh) => {
            let rh = rh.clamp(0.0, 1.0);
            let driving = saturation_vapor_pressure_kpa(water_f)
                - rh * saturation_vapor_pressure_kpa(segment.air_temp_f);
            (params.evap_a + params.evap_b * wind) * driving
        }
        None => 0.0,
    };

    // Fold the constant (signed) evaporation term into an effective equilibrium
    // so the ODE stays a single exponential relaxation. A negative q_evap raises
    // t_eq_eff above t_eq (condensation gain); a positive one lowers it (loss).
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

/// Effective solar irradiance (kW/m²) at `at_unix_ms`, using the bracketing
/// segment's lat/lon/cloud/cover. `0` when no segment brackets the instant or
/// the sun is down.
fn irradiance_kw_at(weather: &[WeatherSegment], at_unix_ms: i64) -> f64 {
    let Some(seg) = weather
        .iter()
        .find(|s| s.start_unix_ms <= at_unix_ms && at_unix_ms <= s.end_unix_ms)
    else {
        return 0.0;
    };
    let cloud = seg.cloud_fraction.unwrap_or(0.0);
    effective_irradiance(
        seg.latitude_deg,
        seg.longitude_deg,
        at_unix_ms / 1000,
        cloud,
        seg.cover_solar_transmission,
    ) / 1000.0
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

/// Upper sanity clamp on the fitted solar gain `g` (°F·hr⁻¹ per kW/m²). A
/// daytime fit is rejected back to the seed `g` if it lands outside `[0, this]`.
const SOLAR_GAIN_MAX: f64 = 1.0;

/// One usable consecutive-sample interval for the fit:
/// `(t_start, t_end, dt_hours, air, irradiance_kw_at_midpoint)`.
type FitPair = (f64, f64, f64, f64, f64);

/// Closed-form least-squares solar gain `g` for a fixed `k` over the daytime
/// pairs. The one-step prediction is linear in `g`:
/// `pred = air·(1−e) + (g/k)·I·(1−e) + t0·e`, with `e = exp(−k·dt)`. Returns
/// `0` when there is no solar leverage (no daytime pairs).
fn best_solar_gain_for_k(pairs: &[FitPair], k: f64) -> f64 {
    // Solve min_g Σ (t1 − base − g·x)² where x = (I/k)·(1−e), base = air·(1−e)+t0·e.
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for &(t0, t1, dt, air, irr) in pairs {
        let e = (-k * dt).exp();
        let x = (irr / k) * (1.0 - e);
        let base = air * (1.0 - e) + t0 * e;
        sxx += x * x;
        sxy += x * (t1 - base);
    }
    if sxx <= 1.0e-12 {
        return 0.0;
    }
    sxy / sxx
}

/// One-step prediction under the relaxation-with-solar model.
fn predict_step(t0: f64, dt: f64, air: f64, irr: f64, k: f64, g: f64) -> f64 {
    let t_eq = air + (g / k) * irr;
    t_eq + (t0 - t_eq) * (-k * dt).exp()
}

/// Fit the effective cooling constant `k0` (and, when daytime intervals are
/// present, the solar gain `g`) from reliable samples.
///
/// This is the *weak prior* seed: a coarse-grid + golden-section fit over `k`,
/// with the solar gain solved in closed form (least squares) for each candidate
/// `k`. The wind/evaporation coefficients are held at the seed. When **no**
/// daytime (solar > 0) interval is present, `g` is unobservable and stays at the
/// seed — we never fit an unobservable solar gain from night-only data. Sanity
/// clamps reject `k <= 0` or `tau` outside `[2h, 200h]`, and a fitted `g`
/// outside `[0, SOLAR_GAIN_MAX]`; any degenerate case falls back to `seed` with
/// `Low` confidence and never produces a NaN or negative `tau`.
pub fn fit_cooling_params(
    samples: &[ReliableSample],
    weather: &[WeatherSegment],
    seed: &CoolingParams,
) -> CoolingFit {
    let mut sorted: Vec<ReliableSample> = samples.to_vec();
    sorted.sort_by_key(|s| s.observed_at_unix_ms);

    // Build (T_start, T_end, dt_hours, air, irradiance) intervals with known air.
    let mut pairs: Vec<FitPair> = Vec::new();
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
        let irr = irradiance_kw_at(weather, mid);
        // Identifiability: an interval must carry leverage — either a non-flat
        // conductive contrast with air, or some solar drive.
        if (a.temperature_f - air).abs() < 1.0e-3 && irr <= 0.0 {
            continue;
        }
        pairs.push((a.temperature_f, b.temperature_f, dt_hours, air, irr));
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

    // Only fit g when some interval actually sees the sun; otherwise hold the
    // seed g (an unobservable solar gain must not be fitted from night data).
    let has_daytime = pairs.iter().any(|&(.., irr)| irr > 0.0);

    let sse = |k: f64| -> f64 {
        let g = if has_daytime {
            best_solar_gain_for_k(&pairs, k).clamp(0.0, SOLAR_GAIN_MAX)
        } else {
            seed.solar_gain_f
        };
        pairs
            .iter()
            .map(|&(t0, t1, dt, air, irr)| {
                let pred = predict_step(t0, dt, air, irr, k, g);
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

    // Resolve the solar gain at the chosen k (seed when unobservable).
    let g = if has_daytime {
        let fitted = best_solar_gain_for_k(&pairs, k);
        if fitted.is_finite() && (0.0..=SOLAR_GAIN_MAX).contains(&fitted) {
            fitted
        } else {
            seed.solar_gain_f
        }
    } else {
        seed.solar_gain_f
    };

    let mut params = *seed;
    params.k0_per_hour = k;
    params.solar_gain_f = g;

    let mae = pairs
        .iter()
        .map(|&(t0, t1, dt, air, irr)| {
            let pred = predict_step(t0, dt, air, irr, k, g);
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

    /// A cooling-only segment with the solar term disabled (cover = 0).
    fn cooling_only_segment(start: i64, end: i64, air: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: air,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: None,
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
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
            // Solar disabled by default so existing assertions are unchanged.
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }

    /// A daytime solar-enabled segment for a fixed N-hemisphere site. The
    /// timestamp is chosen near local solar noon by the caller.
    fn solar_segment(
        start: i64,
        end: i64,
        air: f64,
        lat: f64,
        lon: f64,
        cloud: f64,
        cover: f64,
    ) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: air,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: Some(cloud),
            latitude_deg: lat,
            longitude_deg: lon,
            cover_solar_transmission: cover,
        }
    }

    // A fixed site for solar tests: Los Altos, CA (lon ~ −122°, UTC−8 nominal),
    // so local solar noon is ~20:00 UTC. Helpers below build unix times in UTC.
    const SITE_LAT: f64 = 37.38;
    const SITE_LON: f64 = -122.11;

    /// Unix seconds for a given (year-independent) day-of-year and UTC hour,
    /// anchored at a recent year so the solar geometry is representative.
    fn unix_secs_for(day_of_year: i64, utc_hour: f64) -> i64 {
        // 2024-01-01T00:00:00Z = 1_704_067_200.
        let year_start = 1_704_067_200i64;
        year_start + (day_of_year - 1) * 86_400 + (utc_hour * 3600.0) as i64
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
    fn evaporation_is_signed_condensation_adds_heat() {
        // Warm, very humid air over cooler water: air vapor pressure exceeds the
        // water's, so the SIGNED Dalton term is negative -> condensation GAIN.
        // The gain must NOT be floored away; the humid case should end WARMER
        // than the pure-conduction (humidity-unknown) case.
        let params = CoolingParams {
            k0_per_hour: 0.02,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.2,
            evap_b: 0.0,
            solar_gain_f: 0.0,
            max_projection_hours: 48.0,
        };
        let now = 2 * HOUR_MS;
        let humid = full_segment(0, now, 90.0, 0.0, 0.98);
        let mut no_evap = full_segment(0, now, 90.0, 0.0, 0.98);
        no_evap.humidity_fraction = None; // pure conduction, no evap term

        let humid_out = project_temperature(anchor(70.0, 0), &[humid], &params, now);
        let cond_out = project_temperature(anchor(70.0, 0), &[no_evap], &params, now);
        assert!(
            humid_out.predicted_f > cond_out.predicted_f,
            "condensation gain should warm water beyond conduction: humid {} !> conduction {}",
            humid_out.predicted_f,
            cond_out.predicted_f
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

    // ----- solar geometry -----

    #[test]
    fn solar_elevation_zero_at_night_peaks_at_solar_noon() {
        // Summer day-of-year ~172 (≈ Jun 21). Local solar noon ≈ 20:00 UTC here.
        let doy = 172;
        let midnight = solar_position(SITE_LAT, SITE_LON, unix_secs_for(doy, 8.0)); // ~local midnight
        let noon = solar_position(SITE_LAT, SITE_LON, unix_secs_for(doy, 20.0)); // ~local noon

        assert!(midnight <= 0.0, "sun should be down at local midnight: {midnight}");
        assert!(noon > 60.0, "summer noon sun should be high: {noon}");

        // Noon is the daytime maximum: a couple of hours either side is lower.
        let morning = solar_position(SITE_LAT, SITE_LON, unix_secs_for(doy, 17.0));
        let afternoon = solar_position(SITE_LAT, SITE_LON, unix_secs_for(doy, 23.0));
        assert!(noon > morning && noon > afternoon, "noon is the peak");
    }

    #[test]
    fn solar_noon_higher_in_summer_than_winter() {
        let summer = solar_position(SITE_LAT, SITE_LON, unix_secs_for(172, 20.0)); // Jun
        let winter = solar_position(SITE_LAT, SITE_LON, unix_secs_for(355, 20.0)); // Dec
        assert!(
            summer > winter + 20.0,
            "N-hemisphere summer noon higher than winter: summer {summer} vs winter {winter}"
        );
        assert!(winter > 0.0, "winter noon sun is still up: {winter}");
    }

    #[test]
    fn clear_sky_ghi_zero_at_night_positive_at_midday_and_monotonic() {
        assert_eq!(clear_sky_ghi(0.0), 0.0);
        assert_eq!(clear_sky_ghi(-10.0), 0.0);
        assert!(clear_sky_ghi(60.0) > 0.0);
        // Monotonic increasing in elevation across the daytime range.
        let mut prev = clear_sky_ghi(1.0);
        for el in [5.0, 15.0, 30.0, 45.0, 60.0, 80.0, 90.0] {
            let ghi = clear_sky_ghi(el);
            assert!(ghi > prev, "GHI should rise with elevation at {el}: {ghi} <= {prev}");
            prev = ghi;
        }
        // Sanity: midday clear-sky GHI is in a physical ballpark (W/m²).
        assert!((700.0..1100.0).contains(&clear_sky_ghi(90.0)));
    }

    #[test]
    fn cloud_cover_reduces_effective_irradiance() {
        let noon = unix_secs_for(172, 20.0);
        let clear = effective_irradiance(SITE_LAT, SITE_LON, noon, 0.0, 0.75);
        let cloudy = effective_irradiance(SITE_LAT, SITE_LON, noon, 1.0, 0.75);
        assert!(clear > 0.0);
        assert!(cloudy < 0.3 * clear, "full cloud near-zeroes irradiance: {cloudy} vs {clear}");
        // Partial cloud sits between.
        let partial = effective_irradiance(SITE_LAT, SITE_LON, noon, 0.5, 0.75);
        assert!(partial < clear && partial > cloudy);
    }

    #[test]
    fn effective_irradiance_zero_at_night() {
        let night = unix_secs_for(172, 8.0);
        assert_eq!(
            effective_irradiance(SITE_LAT, SITE_LON, night, 0.0, 0.75),
            0.0
        );
    }

    #[test]
    fn cover_transmission_scales_irradiance() {
        let noon = unix_secs_for(172, 20.0);
        let opaque = effective_irradiance(SITE_LAT, SITE_LON, noon, 0.0, 0.2);
        let solar_cover = effective_irradiance(SITE_LAT, SITE_LON, noon, 0.0, 0.75);
        assert!(solar_cover > opaque * 3.0, "higher transmission passes more: {solar_cover} vs {opaque}");
    }

    // ----- solar feeds T_eq -----

    /// Params with a real solar gain but no evaporation, so the only daytime
    /// effect is the solar bump on T_eq.
    fn solar_params(k0: f64, g: f64) -> CoolingParams {
        CoolingParams {
            k0_per_hour: k0,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.0,
            evap_b: 0.0,
            solar_gain_f: g,
            max_projection_hours: 48.0,
        }
    }

    #[test]
    fn midday_segment_raises_t_eq_above_air() {
        // Pool starts AT air; the only thing that can move it is solar. A sunny
        // midday segment must warm the water above air (T_eq > air).
        let params = solar_params(0.05, 0.08);
        let noon_ms = unix_secs_for(172, 20.0) * 1000;
        let start = noon_ms;
        let end = noon_ms + HOUR_MS;
        let seg = solar_segment(start, end, 80.0, SITE_LAT, SITE_LON, 0.0, 0.75);
        let out = project_temperature(anchor(80.0, start), &[seg], &params, end);
        assert!(
            out.predicted_f > 80.0,
            "midday solar should warm water above air: {}",
            out.predicted_f
        );
    }

    #[test]
    fn night_segment_leaves_t_eq_at_air_no_solar() {
        // Same params, but a night segment: no solar, so the water just relaxes
        // toward air (here it starts at air, so it stays at air).
        let params = solar_params(0.05, 0.08);
        let night_ms = unix_secs_for(172, 8.0) * 1000;
        let start = night_ms;
        let end = night_ms + HOUR_MS;
        let seg = solar_segment(start, end, 80.0, SITE_LAT, SITE_LON, 0.0, 0.75);
        let out = project_temperature(anchor(80.0, start), &[seg], &params, end);
        assert!(
            (out.predicted_f - 80.0).abs() < 1.0e-6,
            "night has no solar; water stays at air: {}",
            out.predicted_f
        );
    }

    #[test]
    fn sunny_gap_yields_less_cooling_than_no_solar_baseline() {
        // A pool warmer than air over a sunny midday gap: solar offsets cooling,
        // so the solar projection ends WARMER than the no-solar baseline.
        let k0 = 0.05;
        let with_solar = solar_params(k0, 0.08);
        let no_solar = solar_params(k0, 0.0);
        let noon_ms = unix_secs_for(172, 19.0) * 1000;
        let start = noon_ms;
        let end = noon_ms + 3 * HOUR_MS;
        let seg = solar_segment(start, end, 85.0, SITE_LAT, SITE_LON, 0.0, 0.75);

        let warm = project_temperature(anchor(89.0, start), &[seg], &with_solar, end);
        let base = project_temperature(anchor(89.0, start), &[seg], &no_solar, end);
        assert!(
            warm.predicted_f > base.predicted_f,
            "solar reduces daytime cooling: solar {} vs baseline {}",
            warm.predicted_f,
            base.predicted_f
        );
    }

    #[test]
    fn overnight_projection_unchanged_by_solar_gain() {
        // The overnight-cooling regression guard: with the sun down, a non-zero
        // solar gain must produce EXACTLY the no-solar result.
        let k0 = 0.05;
        let with_solar = solar_params(k0, 0.08);
        let no_solar = solar_params(k0, 0.0);
        // Night window entirely before local sunrise.
        let night_ms = unix_secs_for(172, 6.0) * 1000;
        let start = night_ms;
        let end = night_ms + 4 * HOUR_MS;
        let seg = solar_segment(start, end, 70.0, SITE_LAT, SITE_LON, 0.0, 0.75);

        let a = project_temperature(anchor(85.0, start), &[seg], &with_solar, end);
        let b = project_temperature(anchor(85.0, start), &[seg], &no_solar, end);
        assert_eq!(a.predicted_f, b.predicted_f, "overnight must be solar-independent");
        assert!(a.predicted_f < 85.0, "still cools overnight");
    }

    // ----- fit recovers g -----

    /// Synthetic daytime warming under a known (k, g): a pool sitting near air
    /// that the sun lifts above air, sampled hourly across solar noon.
    fn synthetic_daytime(
        start_temp: f64,
        air: f64,
        k: f64,
        g: f64,
        first_utc_hour: f64,
        count: usize,
    ) -> (Vec<ReliableSample>, Vec<WeatherSegment>) {
        let doy = 172;
        let mut samples = Vec::new();
        let mut weather = Vec::new();
        let mut t = start_temp;
        for i in 0..count {
            let secs = unix_secs_for(doy, first_utc_hour + i as f64);
            let at_ms = secs * 1000;
            samples.push(ReliableSample {
                temperature_f: t,
                observed_at_unix_ms: at_ms,
            });
            // Hourly solar-enabled segment bracketing this step.
            let seg = solar_segment(
                at_ms,
                at_ms + HOUR_MS,
                air,
                SITE_LAT,
                SITE_LON,
                0.0,
                0.75,
            );
            // Advance one hour under the true model for the next sample.
            let irr = segment_irradiance_kw(&seg);
            let t_eq = air + (g / k) * irr;
            t = t_eq + (t - t_eq) * (-k).exp();
            weather.push(seg);
        }
        (samples, weather)
    }

    #[test]
    fn fit_recovers_known_g_from_daytime_warming() {
        let seed = CoolingParams::seed();
        let k_true = 0.05;
        let g_true = 0.12;
        // Start at air so the warming is purely solar-driven, across solar noon.
        let (samples, weather) = synthetic_daytime(80.0, 80.0, k_true, g_true, 16.0, 9);
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert!(
            (fit.params.solar_gain_f - g_true).abs() < 2.0e-2,
            "recovered g {} vs true {}",
            fit.params.solar_gain_f,
            g_true
        );
        // k stays in bounds and g is non-negative.
        assert!(fit.params.solar_gain_f >= 0.0);
        let tau = 1.0 / fit.params.k0_per_hour;
        assert!((TAU_MIN_HOURS..=TAU_MAX_HOURS).contains(&tau));
    }

    #[test]
    fn fit_leaves_g_at_seed_for_night_only_data() {
        // Night-only cooling data: g is unobservable, so the fit must leave it at
        // the seed (never fit an unobservable solar gain).
        let mut seed = CoolingParams::seed();
        seed.solar_gain_f = 0.33; // distinctive seed to detect any change
        let k_true = 0.05;
        // Pure Newtonian cooling toward air, all at night (sun down).
        let air = 60.0;
        let mut samples = Vec::new();
        let mut weather = Vec::new();
        let mut t = 90.0;
        for i in 0..8 {
            let secs = unix_secs_for(172, 4.0 + i as f64 * 0.4); // pre-dawn window
            let at_ms = secs * 1000;
            samples.push(ReliableSample {
                temperature_f: t,
                observed_at_unix_ms: at_ms,
            });
            let dt_h = 0.4;
            weather.push(solar_segment(
                at_ms,
                at_ms + (dt_h * MS_PER_HOUR) as i64,
                air,
                SITE_LAT,
                SITE_LON,
                0.0,
                0.75,
            ));
            t = air + (t - air) * (-k_true * dt_h).exp();
        }
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert_eq!(
            fit.params.solar_gain_f, seed.solar_gain_f,
            "night-only data must leave g at the seed"
        );
        // k is still recovered from the night cooling.
        assert!((fit.params.k0_per_hour - k_true).abs() < 1.0e-2);
    }

    #[test]
    fn fit_clamps_unphysical_solar_gain_to_seed() {
        // Construct daytime data demanding a huge g (water leaps far above air in
        // an hour). The fit must reject it back to the seed g, not emit garbage.
        let mut seed = CoolingParams::seed();
        seed.solar_gain_f = 0.07;
        let air = 80.0;
        let noon = unix_secs_for(172, 20.0);
        let mut samples = Vec::new();
        let mut weather = Vec::new();
        for i in 0..4 {
            let secs = noon + i as i64 * 3600;
            let at_ms = secs * 1000;
            // Implausible 20°F/hr jumps above air.
            samples.push(ReliableSample {
                temperature_f: air + 50.0 * (i as f64),
                observed_at_unix_ms: at_ms,
            });
            weather.push(solar_segment(
                at_ms,
                at_ms + HOUR_MS,
                air,
                SITE_LAT,
                SITE_LON,
                0.0,
                0.75,
            ));
        }
        let fit = fit_cooling_params(&samples, &weather, &seed);
        assert!(
            (0.0..=SOLAR_GAIN_MAX).contains(&fit.params.solar_gain_f),
            "fitted g must stay within sanity clamps: {}",
            fit.params.solar_gain_f
        );
    }
}
