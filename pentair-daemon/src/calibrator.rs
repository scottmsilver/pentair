//! Continuous thermal calibrator — pure core (spec:
//! docs/2026-07-01-thermal-calibrator-v1.md, Phases 1+2).
//!
//! Everything here is pure: no I/O, no clock, no store access. `heat.rs` owns
//! capture/persist/orchestration and calls into this module, mirroring how the
//! scheduler consumes `thermal.rs`.
//
// Tasks 1-3 land the pure primitives + tests; heat.rs wires them in from
// Task 6. Until then the binary target sees the public surface as unused, so
// suppress dead-code (same pattern scheduler.rs used during its phase-in).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::thermal::{self, CoolingParams, SolarSite, WeatherSegment};

/// Hard cap on weather buckets stored per interval (spec §11): hourly buckets,
/// widened (never truncated) when the gap exceeds 24h, so the store stays small
/// while the whole gap remains covered.
pub const MAX_BUCKETS_PER_INTERVAL: usize = 24;

const MS_PER_HOUR: i64 = 3_600_000;

/// One summarized slice of the weather an interval experienced. Same fields as
/// a `WeatherSegment` minus the site (the site is global, re-attached on read).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherBucket {
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    pub air_temp_f: f64,
    pub wind_mph: Option<f64>,
    pub humidity_fraction: Option<f64>,
    pub cloud_fraction: Option<f64>,
}

/// Classification of a stored interval (spec §3). Heating / contaminated
/// intervals are never stored, so only these two regimes exist on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntervalRegime {
    IdleCovered,
    ExcludedAnomalous,
}

/// A self-contained cooling interval (spec §11): two settled reliable reads
/// bounding an idle gap, plus the weather that spanned it. Self-contained so
/// the fit never depends on the 48h `WeatherCache` retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingInterval {
    pub t0_unix_ms: i64,
    pub t1_unix_ms: i64,
    pub temp0_f: f64,
    pub temp1_f: f64,
    pub regime: IntervalRegime,
    pub weather: Vec<WeatherBucket>,
}

/// Summarize `segments` over `[t0, t1]` into at most [`MAX_BUCKETS_PER_INTERVAL`]
/// equal buckets (hourly when the gap is <= 24h). Each bucket copies the weather
/// of the segment covering its midpoint (nearest by start when none covers it).
/// Empty input -> empty output (the caller then skips capture).
pub fn bucket_weather(segments: &[WeatherSegment], t0_unix_ms: i64, t1_unix_ms: i64) -> Vec<WeatherBucket> {
    if segments.is_empty() || t1_unix_ms <= t0_unix_ms {
        return Vec::new();
    }
    let gap_ms = t1_unix_ms - t0_unix_ms;
    let n = ((gap_ms + MS_PER_HOUR - 1) / MS_PER_HOUR).clamp(1, MAX_BUCKETS_PER_INTERVAL as i64) as usize;
    let mut buckets = Vec::with_capacity(n);
    for i in 0..n {
        let start = t0_unix_ms + gap_ms * i as i64 / n as i64;
        let end = t0_unix_ms + gap_ms * (i as i64 + 1) / n as i64;
        let mid = start + (end - start) / 2;
        let seg = segments
            .iter()
            .find(|s| s.start_unix_ms <= mid && mid < s.end_unix_ms)
            .or_else(|| segments.iter().min_by_key(|s| (s.start_unix_ms - mid).abs()));
        let Some(seg) = seg else { continue };
        buckets.push(WeatherBucket {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: seg.air_temp_f,
            wind_mph: seg.wind_mph,
            humidity_fraction: seg.humidity_fraction,
            cloud_fraction: seg.cloud_fraction,
        });
    }
    buckets
}

/// Re-attach the (global) solar site to a stored interval's buckets, yielding
/// segments the thermal relaxation can consume.
pub fn interval_segments(interval: &CoolingInterval, site: SolarSite) -> Vec<WeatherSegment> {
    interval
        .weather
        .iter()
        .map(|b| WeatherSegment {
            start_unix_ms: b.start_unix_ms,
            end_unix_ms: b.end_unix_ms,
            air_temp_f: b.air_temp_f,
            wind_mph: b.wind_mph,
            humidity_fraction: b.humidity_fraction,
            cloud_fraction: b.cloud_fraction,
            latitude_deg: site.latitude_deg,
            longitude_deg: site.longitude_deg,
            cover_solar_transmission: site.cover_solar_transmission,
        })
        .collect()
}

/// Tau clamps for the interval fit, matching thermal.rs's documented [2h, 200h].
const FIT_TAU_MIN_HOURS: f64 = 2.0;
const FIT_TAU_MAX_HOURS: f64 = 200.0;
/// Solar-gain grid ceiling (°F/h per kW/m²) when g is observable.
const FIT_G_MAX: f64 = 1.5;
/// End-temp shift (°F) that makes solar observable in a window (spec §6).
const SOLAR_OBSERVABLE_DELTA_F: f64 = 0.2;

/// Relax the interval's start temperature across its own weather buckets.
pub fn predict_interval_end(interval: &CoolingInterval, params: &CoolingParams, site: SolarSite) -> f64 {
    let mut water = interval.temp0_f;
    for seg in interval_segments(interval, site) {
        let dt_hours = (seg.end_unix_ms - seg.start_unix_ms) as f64 / MS_PER_HOUR as f64;
        if dt_hours > 0.0 {
            water = thermal::passive_relax_over_segment(water, &seg, params, dt_hours);
        }
    }
    water
}

/// Mean absolute end-temperature error of `params` across `intervals`.
pub fn score_params(intervals: &[&CoolingInterval], params: &CoolingParams, site: SolarSite) -> f64 {
    if intervals.is_empty() {
        return f64::NAN;
    }
    let sum: f64 = intervals
        .iter()
        .map(|i| (predict_interval_end(i, params, site) - i.temp1_f).abs())
        .sum();
    sum / intervals.len() as f64
}

/// True when the window can identify `solar_gain_f`: bumping g materially moves
/// at least one interval's predicted end temp (i.e., the window contains real
/// daytime irradiance). Uses only public thermal API — no geometry duplicated.
pub fn solar_observable(intervals: &[&CoolingInterval], seed: &CoolingParams, site: SolarSite) -> bool {
    let bumped = CoolingParams { solar_gain_f: seed.solar_gain_f + 0.5, ..*seed };
    intervals.iter().any(|i| {
        (predict_interval_end(i, &bumped, site) - predict_interval_end(i, seed, site)).abs()
            > SOLAR_OBSERVABLE_DELTA_F
    })
}

/// Grid-fit `k0` (and `g` when observable) minimizing summed squared end-temp
/// error. Evaporation coefficients are HELD at the seed (spec §6: covered-idle
/// evap stays pulled toward its current value — re-fitting it here is how the
/// double-count bug happened). Returns the seed untouched when `intervals` is
/// empty. Never returns non-finite params.
pub fn fit_intervals(intervals: &[&CoolingInterval], seed: &CoolingParams, site: SolarSite) -> CoolingParams {
    if intervals.is_empty() {
        return *seed;
    }
    let fit_g = solar_observable(intervals, seed, site);
    let g_grid: Vec<f64> = if fit_g {
        (0..=15).map(|i| i as f64 * FIT_G_MAX / 15.0).collect()
    } else {
        vec![seed.solar_gain_f]
    };
    // 60 log-spaced k0 candidates across tau [2h, 200h].
    let (k_lo, k_hi) = (1.0 / FIT_TAU_MAX_HOURS, 1.0 / FIT_TAU_MIN_HOURS);
    let k_grid: Vec<f64> = (0..60)
        .map(|i| k_lo * (k_hi / k_lo).powf(i as f64 / 59.0))
        .collect();

    let mut best = *seed;
    let mut best_sse = f64::INFINITY;
    for &k0 in &k_grid {
        for &g in &g_grid {
            let candidate = CoolingParams { k0_per_hour: k0, solar_gain_f: g, ..*seed };
            let sse: f64 = intervals
                .iter()
                .map(|i| {
                    let e = predict_interval_end(i, &candidate, site) - i.temp1_f;
                    e * e
                })
                .sum();
            if sse.is_finite() && sse < best_sse {
                best_sse = sse;
                best = candidate;
            }
        }
    }
    best
}

/// Deterministic holdout: every 3rd interval (index % 3 == 0) is held out.
pub fn holdout_split(intervals: &[CoolingInterval]) -> (Vec<&CoolingInterval>, Vec<&CoolingInterval>) {
    let mut fit_set = Vec::new();
    let mut holdout = Vec::new();
    for (i, interval) in intervals.iter().enumerate() {
        if i % 3 == 0 {
            holdout.push(interval);
        } else {
            fit_set.push(interval);
        }
    }
    (fit_set, holdout)
}

/// The accept gate (spec §6). Scores the CURRENT params and the DAMPED BLEND of
/// current+candidate on the SAME holdout intervals (the blend is what actually
/// gets applied, so the blend is what gets validated). Returns the blend on
/// accept; `None` on reject. Evap coefficients always come from `current`.
pub fn evaluate_candidate(
    current: &CoolingParams,
    candidate: &CoolingParams,
    holdout: &[&CoolingInterval],
    site: SolarSite,
    damping_alpha: f64,
    tolerance_f: f64,
) -> Option<CoolingParams> {
    if holdout.is_empty() {
        return None;
    }
    let a = damping_alpha.clamp(0.0, 1.0);
    let blend = CoolingParams {
        k0_per_hour: current.k0_per_hour * (1.0 - a) + candidate.k0_per_hour * a,
        k_wind_per_hour_per_mph: current.k_wind_per_hour_per_mph * (1.0 - a)
            + candidate.k_wind_per_hour_per_mph * a,
        solar_gain_f: current.solar_gain_f * (1.0 - a) + candidate.solar_gain_f * a,
        evap_a: current.evap_a,
        evap_b: current.evap_b,
        max_projection_hours: current.max_projection_hours,
    };
    // Physical clamps: reject rather than silently repair (spec §6).
    let tau = 1.0 / blend.k0_per_hour;
    let physical = blend.k0_per_hour.is_finite()
        && blend.solar_gain_f.is_finite()
        && blend.k_wind_per_hour_per_mph.is_finite()
        && (FIT_TAU_MIN_HOURS..=FIT_TAU_MAX_HOURS).contains(&tau)
        && (0.0..=FIT_G_MAX).contains(&blend.solar_gain_f);
    if !physical {
        return None;
    }
    let blend_mae = score_params(holdout, &blend, site);
    let current_mae = score_params(holdout, current, site);
    (blend_mae.is_finite() && current_mae.is_finite() && blend_mae <= current_mae + tolerance_f)
        .then_some(blend)
}

/// Fraction of intervals ending at/after `window_start_unix_ms` that were
/// excluded as anomalous — the §9 exclusion-deadlock detector's input.
pub fn exclusion_rate(intervals: &[CoolingInterval], window_start_unix_ms: i64) -> f64 {
    let in_window: Vec<&CoolingInterval> = intervals
        .iter()
        .filter(|i| i.t1_unix_ms >= window_start_unix_ms)
        .collect();
    if in_window.is_empty() {
        return 0.0;
    }
    in_window
        .iter()
        .filter(|i| i.regime == IntervalRegime::ExcludedAnomalous)
        .count() as f64
        / in_window.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::{SolarSite, WeatherSegment};

    fn seg(start_h: i64, end_h: i64, air: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start_h * 3_600_000,
            end_unix_ms: end_h * 3_600_000,
            air_temp_f: air,
            wind_mph: Some(4.0),
            humidity_fraction: Some(0.6),
            cloud_fraction: Some(0.1),
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }

    #[test]
    fn buckets_are_hourly_and_capped_at_24() {
        // 10h gap -> 10 buckets; 40h gap -> capped at 24 wider buckets.
        let segs = vec![seg(0, 48, 60.0)];
        let ten = bucket_weather(&segs, 0, 10 * 3_600_000);
        assert_eq!(ten.len(), 10);
        assert_eq!(ten[0].start_unix_ms, 0);
        assert_eq!(ten[9].end_unix_ms, 10 * 3_600_000);
        let forty = bucket_weather(&segs, 0, 40 * 3_600_000);
        assert_eq!(forty.len(), MAX_BUCKETS_PER_INTERVAL);
        assert_eq!(forty.last().unwrap().end_unix_ms, 40 * 3_600_000);
    }

    #[test]
    fn buckets_take_weather_from_covering_segment() {
        // Air steps 60 -> 50 at hour 2; buckets on each side see their side.
        let segs = vec![seg(0, 2, 60.0), seg(2, 6, 50.0)];
        let buckets = bucket_weather(&segs, 0, 4 * 3_600_000);
        assert_eq!(buckets[0].air_temp_f, 60.0);
        assert_eq!(buckets[3].air_temp_f, 50.0);
    }

    #[test]
    fn empty_segments_yield_no_buckets() {
        assert!(bucket_weather(&[], 0, 3_600_000).is_empty());
    }

    #[test]
    fn interval_segments_round_trip_site() {
        let segs = vec![seg(0, 6, 55.0)];
        let interval = CoolingInterval {
            t0_unix_ms: 0,
            t1_unix_ms: 4 * 3_600_000,
            temp0_f: 90.0,
            temp1_f: 89.0,
            regime: IntervalRegime::IdleCovered,
            weather: bucket_weather(&segs, 0, 4 * 3_600_000),
        };
        let site = SolarSite { latitude_deg: 37.35, longitude_deg: -122.09, cover_solar_transmission: 0.75 };
        let out = interval_segments(&interval, site);
        assert_eq!(out.len(), interval.weather.len());
        assert_eq!(out[0].latitude_deg, 37.35);
        assert_eq!(out[0].air_temp_f, 55.0);
    }

    use crate::thermal::CoolingParams;

    fn params(k0: f64, g: f64) -> CoolingParams {
        CoolingParams {
            k0_per_hour: k0,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.0,
            evap_b: 0.0,
            solar_gain_f: g,
            max_projection_hours: 48.0,
        }
    }

    /// Build a synthetic interval whose end temp comes from the true params.
    fn synth_interval(t0_h: i64, hours: i64, temp0: f64, air: f64, truth: &CoolingParams) -> CoolingInterval {
        let segs = vec![seg(t0_h, t0_h + hours, air)];
        let weather = bucket_weather(&segs, t0_h * 3_600_000, (t0_h + hours) * 3_600_000);
        let mut interval = CoolingInterval {
            t0_unix_ms: t0_h * 3_600_000,
            t1_unix_ms: (t0_h + hours) * 3_600_000,
            temp0_f: temp0,
            temp1_f: 0.0,
            regime: IntervalRegime::IdleCovered,
            weather,
        };
        interval.temp1_f = predict_interval_end(&interval, truth, SolarSite::disabled());
        interval
    }

    #[test]
    fn fit_recovers_known_k0_from_synthetic_intervals() {
        let truth = params(1.0 / 96.0, 0.0);
        let intervals: Vec<CoolingInterval> = (0..8)
            .map(|i| synth_interval(i * 12, 10, 95.0 - i as f64, 60.0, &truth))
            .collect();
        let refs: Vec<&CoolingInterval> = intervals.iter().collect();
        let seed = params(1.0 / 50.0, 0.0);
        let fit = fit_intervals(&refs, &seed, SolarSite::disabled());
        let tau = 1.0 / fit.k0_per_hour;
        assert!((tau - 96.0).abs() < 12.0, "recovered tau {tau} should be near 96h");
        // evap held at seed, never fit.
        assert_eq!(fit.evap_a, seed.evap_a);
        assert_eq!(fit.evap_b, seed.evap_b);
    }

    #[test]
    fn night_only_windows_hold_solar_gain_at_seed() {
        // Disabled site -> zero irradiance everywhere -> g unobservable.
        let truth = params(1.0 / 96.0, 0.0);
        let intervals: Vec<CoolingInterval> =
            (0..4).map(|i| synth_interval(i * 12, 10, 95.0, 60.0, &truth)).collect();
        let refs: Vec<&CoolingInterval> = intervals.iter().collect();
        let seed = params(1.0 / 50.0, 0.9);
        assert!(!solar_observable(&refs, &seed, SolarSite::disabled()));
        let fit = fit_intervals(&refs, &seed, SolarSite::disabled());
        assert_eq!(fit.solar_gain_f, seed.solar_gain_f, "g must stay at seed when unobservable");
    }

    #[test]
    fn score_params_is_mae_over_endpoints() {
        let truth = params(1.0 / 96.0, 0.0);
        let interval = synth_interval(0, 10, 95.0, 60.0, &truth);
        let refs = [&interval];
        assert!(score_params(&refs, &truth, SolarSite::disabled()) < 1e-9);
        let wrong = params(1.0 / 10.0, 0.0);
        assert!(score_params(&refs, &wrong, SolarSite::disabled()) > 0.5);
    }

    #[test]
    fn holdout_split_is_deterministic_and_nonempty() {
        let truth = params(1.0 / 96.0, 0.0);
        let intervals: Vec<CoolingInterval> =
            (0..6).map(|i| synth_interval(i * 12, 10, 95.0, 60.0, &truth)).collect();
        let (fit_set, holdout) = holdout_split(&intervals);
        assert_eq!(fit_set.len(), 4);
        assert_eq!(holdout.len(), 2); // indices 0 and 3
        let (fit2, hold2) = holdout_split(&intervals);
        assert_eq!(fit_set.len(), fit2.len());
        assert_eq!(holdout.len(), hold2.len());
    }

    #[test]
    fn candidate_better_on_same_holdout_is_accepted_and_blended() {
        let truth = params(1.0 / 96.0, 0.0);
        let intervals: Vec<CoolingInterval> =
            (0..6).map(|i| synth_interval(i * 12, 10, 95.0, 60.0, &truth)).collect();
        let (_, holdout) = holdout_split(&intervals);
        let current = params(1.0 / 30.0, 0.0); // badly wrong
        let blended = evaluate_candidate(&current, &truth, &holdout, SolarSite::disabled(), 0.3, 0.15)
            .expect("better candidate must be accepted");
        // Damped: strictly between current and candidate.
        assert!(blended.k0_per_hour < current.k0_per_hour);
        assert!(blended.k0_per_hour > truth.k0_per_hour);
    }

    #[test]
    fn worse_candidate_is_rejected() {
        let truth = params(1.0 / 96.0, 0.0);
        let intervals: Vec<CoolingInterval> =
            (0..6).map(|i| synth_interval(i * 12, 10, 95.0, 60.0, &truth)).collect();
        let (_, holdout) = holdout_split(&intervals);
        let awful = params(1.0 / 2.0, 0.0);
        assert!(evaluate_candidate(&truth, &awful, &holdout, SolarSite::disabled(), 0.3, 0.15).is_none());
    }

    #[test]
    fn exclusion_rate_counts_only_window() {
        let truth = params(1.0 / 96.0, 0.0);
        let mut intervals: Vec<CoolingInterval> =
            (0..4).map(|i| synth_interval(i * 12, 10, 95.0, 60.0, &truth)).collect();
        intervals[2].regime = IntervalRegime::ExcludedAnomalous;
        intervals[3].regime = IntervalRegime::ExcludedAnomalous;
        // Window covering only the last two intervals -> 100% excluded.
        let window_start = intervals[2].t1_unix_ms - 1;
        assert_eq!(exclusion_rate(&intervals, window_start), 1.0);
        // Whole history -> 50%.
        assert_eq!(exclusion_rate(&intervals, 0), 0.5);
        assert_eq!(exclusion_rate(&[], 0), 0.0);
    }
}
