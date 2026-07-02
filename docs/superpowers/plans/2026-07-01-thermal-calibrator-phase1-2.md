# Thermal Calibrator Phases 1+2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Continuous, non-actuating thermal calibration for the pentair daemon — persisted cooling-interval capture + interval-based re-fit with a same-holdout accept gate and damped silent auto-apply, plus a read-only `GET /api/pool/calibration` endpoint.

**Architecture:** A new pure module `calibrator.rs` (types, hourly weather bucketing, interval fit, holdout/accept/blend logic — no I/O, no clock, mirrors `thermal.rs` purity) is driven by `HeatEstimator` in `heat.rs`, which captures classified cooling intervals into the (already atomically-persisted) `HeatEstimatorStore`, re-fits on gated triggers, and learns the heater's outlet-vs-bulk offset from settled post-heat reads. Spec: `docs/2026-07-01-thermal-calibrator-v1.md` (§Phases 1+2 only).

**Tech Stack:** Rust, pentair-daemon crate. No new dependencies.

## Global Constraints

- **Phases 3 (characterization campaign) and 4 (active probes) are OUT of scope** — nothing in this plan actuates anything (no setpoint writes, no heat/on commands, no `cmd_tx.send`).
- **Silent auto-apply:** accepted fits update the store only; no event-log/change-history entries anywhere. `tracing` debug lines are fine.
- Never hard-code a server URL.
- No new crate dependencies.
- Commits go on the current branch `pool-temp-predictor`. **Never `git push`** (user pushes explicitly).
- Every task ends with `cargo test -p pentair-daemon --quiet` green (and the final task runs the whole workspace).
- Back-compat: the deployed `/root/.pentair/heat-estimator.json` must still deserialize — every new store field takes `#[serde(default)]`.

## What already exists (build on it, do NOT rebuild)

| Spec item | Existing code |
|---|---|
| Per-reading MAE + damped k0 nudge | `heat.rs` `calibrate_body` (EWMA MAE + secant step) |
| Heater-overlap gap guard | `heat.rs` `heating_overlapped_gap` |
| Shared-sensor attribution | `heat.rs` `temperature_trust_for_body` (`inactive-shared-body`) |
| Heater sessions + learned °F/h | `HeatEstimatorStore.recent_sessions`, `*_learned_rate_f_per_hour` |
| Atomic persist (tmp+rename) | `heat.rs` `write_json_off_lock` |
| Single-writer | everything mutates via `HeatEstimator` |
| Passive relaxation physics | `thermal::passive_relax_over_segment` (pub), `thermal::CoolingParams` |
| Pairwise sample fit | `thermal::fit_cooling_params` (NOT reusable over the interval buffer — consecutive-sample pairing would create bogus cross-interval pairs; Task 2 adds an interval-aware fit in `calibrator.rs`) |

Key existing signatures the code below relies on (verified against source):

```rust
// thermal.rs (all pub)
pub struct CoolingParams { pub k0_per_hour: f64, pub k_wind_per_hour_per_mph: f64,
    pub evap_a: f64, pub evap_b: f64, pub solar_gain_f: f64, pub max_projection_hours: f64 }
pub struct WeatherSegment { pub start_unix_ms: i64, pub end_unix_ms: i64, pub air_temp_f: f64,
    pub wind_mph: Option<f64>, pub humidity_fraction: Option<f64>, pub cloud_fraction: Option<f64>,
    pub latitude_deg: f64, pub longitude_deg: f64, pub cover_solar_transmission: f64 }
pub struct SolarSite { pub latitude_deg: f64, pub longitude_deg: f64, pub cover_solar_transmission: f64 }
impl SolarSite { pub fn disabled() -> Self }
pub fn passive_relax_over_segment(water_f: f64, segment: &WeatherSegment,
    params: &CoolingParams, dt_hours: f64) -> f64;

// heat.rs (private to the module; new code lives inside heat.rs / is called from it)
enum HeatingBodyKind { Pool, Spa }              // Copy, Serialize, Deserialize
struct ReliableTemperatureObservation { temperature: i32, observed_at_unix_ms: i64 }
struct BodyTelemetry { on: bool, active: bool, pool_spa_shared_pump: bool, temperature: i32,
    setpoint: i32, temperature_f: f64, setpoint_f: f64, heat_mode: String, heating: String,
    air_temp_f: Option<f64> }
fn unix_time_ms() -> i64;
impl HeatEstimator {
    fn solar_site(&self) -> thermal::SolarSite;
    fn cooling_params(&self, body: HeatingBodyKind) -> CoolingParams;
    fn cooling_params_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Option<CoolingParams>;
    fn prediction_mae(&self, body: HeatingBodyKind) -> Option<f64>;
    fn heating_overlapped_gap(&self, body: HeatingBodyKind, gap_start_unix_ms: i64, gap_end_unix_ms: i64) -> bool;
    fn persist(&self);
    #[cfg(test)] pub fn load(config: HeatingConfig, path: PathBuf) -> Self;
}

// weather.rs
impl WeatherCache { pub fn to_segments(&self, site: SolarSite) -> Vec<WeatherSegment> }
```

Module registration: `calibrator` must be declared in **both** `src/main.rs` and `src/lib.rs`, exactly the way `scheduler` is declared in each (the CLI links the lib).

---

### Task 1: Pure calibrator types + hourly weather bucketing

**Files:**
- Create: `pentair-daemon/src/calibrator.rs`
- Modify: `pentair-daemon/src/main.rs` (module decl, next to `mod scheduler;`)
- Modify: `pentair-daemon/src/lib.rs` (module decl, next to `pub mod scheduler;`)
- Test: in-file `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `thermal::{WeatherSegment, SolarSite}`.
- Produces (later tasks rely on these exact names):
  - `pub struct WeatherBucket { pub start_unix_ms: i64, pub end_unix_ms: i64, pub air_temp_f: f64, pub wind_mph: Option<f64>, pub humidity_fraction: Option<f64>, pub cloud_fraction: Option<f64> }` (Serialize/Deserialize/Clone/Debug/PartialEq)
  - `pub enum IntervalRegime { IdleCovered, ExcludedAnomalous }` (Serialize/Deserialize/Clone/Copy/Debug/PartialEq/Eq)
  - `pub struct CoolingInterval { pub t0_unix_ms: i64, pub t1_unix_ms: i64, pub temp0_f: f64, pub temp1_f: f64, pub regime: IntervalRegime, pub weather: Vec<WeatherBucket> }` (Serialize/Deserialize/Clone/Debug/PartialEq)
  - `pub const MAX_BUCKETS_PER_INTERVAL: usize = 24;`
  - `pub fn bucket_weather(segments: &[WeatherSegment], t0_unix_ms: i64, t1_unix_ms: i64) -> Vec<WeatherBucket>`
  - `pub fn interval_segments(interval: &CoolingInterval, site: SolarSite) -> Vec<WeatherSegment>`

- [ ] **Step 1: Write the failing tests**

Create `pentair-daemon/src/calibrator.rs` containing ONLY the module doc + the test module (types/functions come in Step 3, so the tests fail to compile first — that is the "failing test" signal for a new module):

```rust
//! Continuous thermal calibrator — pure core (spec:
//! docs/2026-07-01-thermal-calibrator-v1.md, Phases 1+2).
//!
//! Everything here is pure: no I/O, no clock, no store access. `heat.rs` owns
//! capture/persist/orchestration and calls into this module, mirroring how the
//! scheduler consumes `thermal.rs`.

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
}
```

Also add the module declarations now:
- In `pentair-daemon/src/main.rs`, next to the existing `mod scheduler;` line: `mod calibrator;`
- In `pentair-daemon/src/lib.rs`, next to the existing `pub mod scheduler;` line: `pub mod calibrator;`

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/ssilver/development/pentair && cargo test -p pentair-daemon --quiet calibrator 2>&1 | head -20`
Expected: compile error — `bucket_weather`/`CoolingInterval` etc. not found.

- [ ] **Step 3: Implement the types + functions**

Insert above the test module in `calibrator.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::thermal::{SolarSite, WeatherSegment};

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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pentair-daemon --quiet calibrator`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/calibrator.rs pentair-daemon/src/main.rs pentair-daemon/src/lib.rs
git commit -m "feat(calibrator): pure interval types + hourly weather bucketing"
```

---

### Task 2: Interval-based cooling fit

**Files:**
- Modify: `pentair-daemon/src/calibrator.rs`

**Interfaces:**
- Consumes: Task 1 types; `thermal::{passive_relax_over_segment, CoolingParams}`.
- Produces:
  - `pub fn predict_interval_end(interval: &CoolingInterval, params: &CoolingParams, site: SolarSite) -> f64`
  - `pub fn score_params(intervals: &[&CoolingInterval], params: &CoolingParams, site: SolarSite) -> f64` (MAE over interval endpoints; `f64::NAN` when empty)
  - `pub fn solar_observable(intervals: &[&CoolingInterval], seed: &CoolingParams, site: SolarSite) -> bool`
  - `pub fn fit_intervals(intervals: &[&CoolingInterval], seed: &CoolingParams, site: SolarSite) -> CoolingParams` — grid-fits `k0` (τ ∈ [2h, 200h], log-spaced) and, only when solar is observable, `g` ∈ [0, 1.5]; **`evap_a`/`evap_b` are held at the seed's values** (spec §6 prior: covered evap stays pulled to current/0, never re-fit here).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet calibrator 2>&1 | head -5`
Expected: compile error — `fit_intervals` etc. not found.

- [ ] **Step 3: Implement**

Insert above the test module:

```rust
use crate::thermal::{self, CoolingParams};

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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pentair-daemon --quiet calibrator`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/calibrator.rs
git commit -m "feat(calibrator): interval-based cooling fit (k0 + observable-only g, evap held)"
```

---

### Task 3: Holdout split, same-holdout accept gate, damped validated blend, exclusion stats

**Files:**
- Modify: `pentair-daemon/src/calibrator.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces:
  - `pub fn holdout_split<'a>(intervals: &'a [CoolingInterval]) -> (Vec<&'a CoolingInterval>, Vec<&'a CoolingInterval>)` — deterministic (every 3rd interval, by index, to holdout); both halves non-empty when `len >= 3`.
  - `pub fn evaluate_candidate(current: &CoolingParams, candidate: &CoolingParams, holdout: &[&CoolingInterval], site: SolarSite, damping_alpha: f64, tolerance_f: f64) -> Option<CoolingParams>` — returns the **validated damped blend** or `None`. Blend: `new = (1−α)·current + α·candidate` per field (evap fields copied from current). Accept iff blend's holdout MAE is finite and `<= current's holdout MAE + tolerance_f`, and every blend param is finite with τ within [2h, 200h], `g ∈ [0, FIT_G_MAX]`.
  - `pub fn exclusion_rate(intervals: &[CoolingInterval], window_start_unix_ms: i64) -> f64` — fraction of intervals with `t1_unix_ms >= window_start` that are `ExcludedAnomalous` (0.0 when none in window).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet calibrator 2>&1 | head -5`
Expected: compile error — `holdout_split` etc. not found.

- [ ] **Step 3: Implement**

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pentair-daemon --quiet calibrator`
Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/calibrator.rs
git commit -m "feat(calibrator): same-holdout accept gate with validated damped blend + exclusion stats"
```

---

### Task 4: `[heating.calibration]` config

**Files:**
- Modify: `pentair-daemon/src/config.rs` (add `CalibrationConfig`; add field to `HeatingConfig`)

**Interfaces:**
- Produces: `pub struct CalibrationConfig` with these exact fields/defaults (later tasks read them via `self.config.calibration.*` inside `HeatEstimator`):
  - `enabled: bool = true`
  - `window_days: f64 = 14.0`
  - `min_new_intervals: usize = 4`
  - `refit_min_hours: f64 = 24.0`
  - `damping_alpha: f64 = 0.3`
  - `accept_tolerance_f: f64 = 0.15`
  - `mae_drift_f: f64 = 1.5`
  - `exclusion_rate_threshold: f64 = 0.5`
  - `exclusion_window_days: f64 = 5.0`
  - `offset_settle_window_hours: f64 = 6.0`
- `HeatingConfig` gains `#[serde(default)] pub calibration: CalibrationConfig`.

- [ ] **Step 1: Write the failing test**

In `config.rs`'s existing `#[cfg(test)] mod tests`, add (modeled on the existing `[gasheater]` parse tests):

```rust
    #[test]
    fn calibration_config_defaults_and_parse() {
        // Absent section -> defaults.
        let config: Config = toml::from_str("adapter_host = \"h\"").expect("parse");
        let cal = &config.heating.calibration;
        assert!(cal.enabled);
        assert_eq!(cal.window_days, 14.0);
        assert_eq!(cal.min_new_intervals, 4);
        assert_eq!(cal.refit_min_hours, 24.0);
        assert_eq!(cal.damping_alpha, 0.3);
        assert_eq!(cal.accept_tolerance_f, 0.15);
        assert_eq!(cal.mae_drift_f, 1.5);
        assert_eq!(cal.exclusion_rate_threshold, 0.5);
        assert_eq!(cal.exclusion_window_days, 5.0);
        assert_eq!(cal.offset_settle_window_hours, 6.0);

        // Explicit section overrides.
        let config: Config = toml::from_str(
            "adapter_host = \"h\"\n[heating.calibration]\nenabled = false\nwindow_days = 7.0\n",
        )
        .expect("parse");
        assert!(!config.heating.calibration.enabled);
        assert_eq!(config.heating.calibration.window_days, 7.0);
    }
```

(If the existing tests build `Config` differently — e.g. a helper or a different required root field — follow the file's local pattern; the assertions stay the same.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pentair-daemon --quiet calibration_config 2>&1 | head -5`
Expected: compile error — no `calibration` field.

- [ ] **Step 3: Implement**

In `config.rs`, next to `CoolingConfig`:

```rust
/// `[heating.calibration]` — continuous thermal calibrator (spec
/// docs/2026-07-01-thermal-calibrator-v1.md §12). Advisory only; nothing here
/// actuates.
#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Rolling fit window (days of stored cooling intervals).
    #[serde(default = "default_calibration_window_days")]
    pub window_days: f64,
    /// Re-fit trigger: at least this many fresh intervals since the last fit.
    #[serde(default = "default_min_new_intervals")]
    pub min_new_intervals: usize,
    /// Re-fit rate limit: at most one fit per body per this many hours.
    #[serde(default = "default_refit_min_hours")]
    pub refit_min_hours: f64,
    /// Damping alpha for accepted fits: new = (1-a)*old + a*fit.
    #[serde(default = "default_damping_alpha")]
    pub damping_alpha: f64,
    /// Accept tolerance (°F) on the same-holdout MAE comparison.
    #[serde(default = "default_accept_tolerance_f")]
    pub accept_tolerance_f: f64,
    /// Re-fit trigger: rolling prediction MAE above this (°F) forces a fit.
    #[serde(default = "default_mae_drift_f")]
    pub mae_drift_f: f64,
    /// Exclusion-deadlock escape hatch (spec §9): trip when the exclusion rate
    /// over `exclusion_window_days` exceeds this fraction.
    #[serde(default = "default_exclusion_rate_threshold")]
    pub exclusion_rate_threshold: f64,
    #[serde(default = "default_exclusion_window_days")]
    pub exclusion_window_days: f64,
    /// Outlet-offset learning: a settled read must land within this many hours
    /// of a completed heating session to pair with it.
    #[serde(default = "default_offset_settle_window_hours")]
    pub offset_settle_window_hours: f64,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_days: default_calibration_window_days(),
            min_new_intervals: default_min_new_intervals(),
            refit_min_hours: default_refit_min_hours(),
            damping_alpha: default_damping_alpha(),
            accept_tolerance_f: default_accept_tolerance_f(),
            mae_drift_f: default_mae_drift_f(),
            exclusion_rate_threshold: default_exclusion_rate_threshold(),
            exclusion_window_days: default_exclusion_window_days(),
            offset_settle_window_hours: default_offset_settle_window_hours(),
        }
    }
}

fn default_calibration_window_days() -> f64 { 14.0 }
fn default_min_new_intervals() -> usize { 4 }
fn default_refit_min_hours() -> f64 { 24.0 }
fn default_damping_alpha() -> f64 { 0.3 }
fn default_accept_tolerance_f() -> f64 { 0.15 }
fn default_mae_drift_f() -> f64 { 1.5 }
fn default_exclusion_rate_threshold() -> f64 { 0.5 }
fn default_exclusion_window_days() -> f64 { 5.0 }
fn default_offset_settle_window_hours() -> f64 { 6.0 }
```

(If `config.rs` has no `default_true` helper, add `fn default_true() -> bool { true }`; if one exists under another name, use it.)

And in `HeatingConfig`, after the `cooling` field:

```rust
    /// Continuous thermal calibrator (advisory; spec §12).
    #[serde(default)]
    pub calibration: CalibrationConfig,
```

Update `HeatingConfig`'s `Default` impl (if it has an explicit one) with `calibration: Default::default()`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p pentair-daemon --quiet config`
Expected: all config tests pass including the new one.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/config.rs
git commit -m "feat(config): [heating.calibration] section with spec defaults"
```

---

### Task 5: Store extension + body-activity windows

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

**Interfaces:**
- Consumes: Task 1's `CoolingInterval` (`crate::calibrator::CoolingInterval`).
- Produces (all inside `heat.rs`; used by Tasks 6–9):
  - `HeatEstimatorStore` gains (every one `#[serde(default)]` for back-compat with the deployed file):
    ```rust
    pool_cooling_intervals: Vec<crate::calibrator::CoolingInterval>,
    spa_cooling_intervals: Vec<crate::calibrator::CoolingInterval>,
    activity_windows: Vec<ActivityWindow>,
    mae_history: Vec<MaePoint>,
    pool_outlet_offset_f: Option<f64>,
    spa_outlet_offset_f: Option<f64>,
    pool_bulk_rate_f_per_hour: Option<f64>,
    spa_bulk_rate_f_per_hour: Option<f64>,
    pool_last_refit_unix_ms: Option<i64>,
    spa_last_refit_unix_ms: Option<i64>,
    pool_offset_done_session_end_ms: Option<i64>,
    spa_offset_done_session_end_ms: Option<i64>,
    ```
  - `struct ActivityWindow { body: HeatingBodyKind, start_unix_ms: i64, end_unix_ms: i64 }` (Serialize/Deserialize/Clone/Copy/Debug)
  - `struct MaePoint { body: HeatingBodyKind, at_unix_ms: i64, mae_f: f64 }` (same derives)
  - Consts: `const MAX_INTERVALS_PER_BODY: usize = 200; const MAX_ACTIVITY_WINDOWS: usize = 200; const MAX_MAE_POINTS: usize = 100;`
  - `fn activity_overlapped_gap(&self, body: HeatingBodyKind, gap_start_unix_ms: i64, gap_end_unix_ms: i64) -> bool` — true when any recorded activity window for `body` overlaps the open gap `(start, end)` (same overlap arithmetic as `heating_overlapped_gap`).
  - Activity recording: in `update_active_since_for_body`, on the **true→false transition** (the `Some(_) if !telemetry.active` arm where `*last_active_observed == Some(true)`), push `ActivityWindow { body, start_unix_ms: active_since (or now), end_unix_ms: now }` and truncate the vec front to `MAX_ACTIVITY_WINDOWS`.
  - `fn intervals_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Vec<crate::calibrator::CoolingInterval>` and `fn intervals(&self, body: HeatingBodyKind) -> &[crate::calibrator::CoolingInterval]`.

- [ ] **Step 1: Write the failing tests**

In `heat.rs`'s `#[cfg(test)] mod tests` (follow the file's existing test-construction helpers — it already builds `HeatEstimator::load(config, temp_path)` style fixtures; reuse whatever helper pattern the module's existing tests use to get an estimator with a temp store path):

```rust
    #[test]
    fn store_new_fields_default_and_round_trip_old_file() {
        // An old-format store (missing every new field) must deserialize.
        let old = r#"{
            "pool_learned_rate_f_per_hour": 2.0,
            "spa_learned_rate_f_per_hour": null,
            "pool_last_reliable_temperature": null,
            "spa_last_reliable_temperature": null,
            "recent_sessions": []
        }"#;
        let store: HeatEstimatorStore = serde_json::from_str(old).expect("back-compat");
        assert!(store.pool_cooling_intervals.is_empty());
        assert!(store.activity_windows.is_empty());
        assert!(store.mae_history.is_empty());
        assert!(store.pool_outlet_offset_f.is_none());
        assert!(store.pool_last_refit_unix_ms.is_none());
        // And the extended store round-trips.
        let json = serde_json::to_string(&store).expect("serialize");
        let _: HeatEstimatorStore = serde_json::from_str(&json).expect("round-trip");
    }

    #[test]
    fn activity_windows_recorded_on_body_off_transition_and_bounded() {
        let (mut estimator, _dir) = test_estimator(); // module's existing fixture helper
        for i in 0..(MAX_ACTIVITY_WINDOWS + 10) {
            let t_on = (i as i64) * 100_000;
            estimator.test_note_activity(HeatingBodyKind::Spa, true, t_on);
            estimator.test_note_activity(HeatingBodyKind::Spa, false, t_on + 50_000);
        }
        assert_eq!(estimator.store.activity_windows.len(), MAX_ACTIVITY_WINDOWS);
        // Overlap detection: last window overlaps a gap spanning it.
        let last = *estimator.store.activity_windows.last().unwrap();
        assert!(estimator.activity_overlapped_gap(
            HeatingBodyKind::Spa, last.start_unix_ms - 10, last.end_unix_ms + 10));
        assert!(!estimator.activity_overlapped_gap(
            HeatingBodyKind::Spa, last.end_unix_ms + 10, last.end_unix_ms + 20));
        assert!(!estimator.activity_overlapped_gap(
            HeatingBodyKind::Pool, last.start_unix_ms - 10, last.end_unix_ms + 10));
    }
```

Where `test_note_activity` is a small `#[cfg(test)]` helper on `HeatEstimator` that drives `update_active_since_for_body` with a minimal `BodyTelemetry` (`active` set as given, everything else zero/empty) at the given timestamp — add it next to the other `#[cfg(test)]` helpers. If the module has no `test_estimator()` fixture, create one that calls `HeatEstimator::load(HeatingConfig::default(), tempdir-path)` per the pattern of the module's existing tests.

**Note on timestamps in `update_active_since_for_body`:** the production fn takes `now_unix_ms` already — the helper just forwards a synthetic clock; no `unix_time_ms()` call is added.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet store_new_fields 2>&1 | head -5`
Expected: compile error — fields not found.

- [ ] **Step 3: Implement**

1. Add the three consts near the other `heat.rs` consts.
2. Add `ActivityWindow` + `MaePoint` structs next to `ReliableTemperatureObservation`.
3. Add the twelve new fields to `HeatEstimatorStore`, each with `#[serde(default)]`.
4. In `update_active_since_for_body`, the `Some(_)` (inactive) arm currently does `*slot = None; *last_active_observed = Some(false);`. Change it to capture the window first:

```rust
            Some(_) => {
                if *last_active_observed == Some(true) {
                    // Body just turned off: record the activity window so the
                    // calibrator can reject cooling intervals it overlaps
                    // (spec §3: idle across the WHOLE gap, not just endpoints).
                    let start = slot.unwrap_or(now_unix_ms);
                    self_activity_push = Some(ActivityWindow {
                        body,
                        start_unix_ms: start,
                        end_unix_ms: now_unix_ms,
                    });
                }
                *slot = None;
                *last_active_observed = Some(false);
            }
```

Borrow note: `slot`/`last_active_observed` are `&mut` into `self`, so push via a local `let mut self_activity_push: Option<ActivityWindow> = None;` declared before the `match`, and after the `match` (borrows ended):

```rust
        if let Some(window) = self_activity_push {
            self.store.activity_windows.push(window);
            let len = self.store.activity_windows.len();
            if len > MAX_ACTIVITY_WINDOWS {
                self.store.activity_windows.drain(..len - MAX_ACTIVITY_WINDOWS);
            }
        }
```

5. Add the accessors + overlap check:

```rust
    fn intervals(&self, body: HeatingBodyKind) -> &[crate::calibrator::CoolingInterval] {
        match body {
            HeatingBodyKind::Pool => &self.store.pool_cooling_intervals,
            HeatingBodyKind::Spa => &self.store.spa_cooling_intervals,
        }
    }

    fn intervals_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Vec<crate::calibrator::CoolingInterval> {
        match body {
            HeatingBodyKind::Pool => &mut self.store.pool_cooling_intervals,
            HeatingBodyKind::Spa => &mut self.store.spa_cooling_intervals,
        }
    }

    /// True when any recorded pump/body activity window for `body` overlaps the
    /// open gap. Complements `heating_overlapped_gap` (heater sessions) so a
    /// cooling interval is idle across its WHOLE span (spec §3).
    fn activity_overlapped_gap(
        &self,
        body: HeatingBodyKind,
        gap_start_unix_ms: i64,
        gap_end_unix_ms: i64,
    ) -> bool {
        self.store.activity_windows.iter().any(|w| {
            w.body == body
                && w.start_unix_ms < gap_end_unix_ms
                && w.end_unix_ms > gap_start_unix_ms
        })
    }
```

6. Add the `#[cfg(test)]` helper:

```rust
    #[cfg(test)]
    fn test_note_activity(&mut self, body: HeatingBodyKind, active: bool, now_unix_ms: i64) {
        let telemetry = BodyTelemetry {
            on: active,
            active,
            pool_spa_shared_pump: true,
            temperature: 80,
            setpoint: 80,
            temperature_f: 80.0,
            setpoint_f: 80.0,
            heat_mode: "off".to_string(),
            heating: "off".to_string(),
            air_temp_f: None,
        };
        self.update_active_since_for_body(body, Some(&telemetry), now_unix_ms);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p pentair-daemon --quiet`
Expected: all pass (including the two new ones and every pre-existing store test).

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "feat(heat): store cooling intervals, activity windows, offset/rate/refit slots (back-compat serde)"
```

---

### Task 6: Interval capture + anomaly classification in `calibrate_body`

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

**Interfaces:**
- Consumes: Tasks 1, 5; existing `calibrate_body` locals (`anchor`, `params`, `segments`, `projected`, `actual_f`, `error_f`, `now_unix_ms`).
- Produces: `fn capture_cooling_interval(&mut self, body: HeatingBodyKind, anchor: thermal::ReliableSample, actual_f: f64, error_f: f64, segments: &[thermal::WeatherSegment], now_unix_ms: i64)` called from `calibrate_body` right after the rolling-MAE update (before the secant step). Consts: `const ANOMALY_SIGMA: f64 = 3.0; const ANOMALY_MAE_FLOOR_F: f64 = 0.75;`

Classification rule (spec §3): the gap is already known heater-free (`heating_overlapped_gap` returned false earlier in `calibrate_body`). Additionally require `!activity_overlapped_gap(...)` — if activity overlapped, **discard** (no interval stored; the gap wasn't idle). Otherwise store with regime `ExcludedAnomalous` when `error_f.abs() > ANOMALY_SIGMA * max(rolling_mae, ANOMALY_MAE_FLOOR_F)`, else `IdleCovered`. Empty `bucket_weather` result → skip capture entirely.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn capture_stores_idle_interval_and_tags_anomalies() {
        let (mut estimator, _dir) = test_estimator();
        let segs = vec![test_weather_segment(0, 20 * 3_600_000, 60.0)];
        let anchor = thermal::ReliableSample { temperature_f: 90.0, observed_at_unix_ms: 0 };
        // Normal residual -> IdleCovered.
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa, anchor, 89.0, 0.3, &segs, 10 * 3_600_000);
        assert_eq!(estimator.intervals(HeatingBodyKind::Spa).len(), 1);
        assert_eq!(
            estimator.intervals(HeatingBodyKind::Spa)[0].regime,
            crate::calibrator::IntervalRegime::IdleCovered
        );
        // Huge residual (way past 3 * max(mae, 0.75)) -> ExcludedAnomalous.
        let anchor2 = thermal::ReliableSample { temperature_f: 89.0, observed_at_unix_ms: 10 * 3_600_000 };
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa, anchor2, 80.0, -9.0, &segs, 20 * 3_600_000);
        assert_eq!(
            estimator.intervals(HeatingBodyKind::Spa)[1].regime,
            crate::calibrator::IntervalRegime::ExcludedAnomalous
        );
    }

    #[test]
    fn capture_discards_gap_overlapping_body_activity() {
        let (mut estimator, _dir) = test_estimator();
        // Body ran 4h..5h inside the 0..10h gap.
        estimator.test_note_activity(HeatingBodyKind::Spa, true, 4 * 3_600_000);
        estimator.test_note_activity(HeatingBodyKind::Spa, false, 5 * 3_600_000);
        let segs = vec![test_weather_segment(0, 20 * 3_600_000, 60.0)];
        let anchor = thermal::ReliableSample { temperature_f: 90.0, observed_at_unix_ms: 0 };
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa, anchor, 89.0, 0.3, &segs, 10 * 3_600_000);
        assert!(estimator.intervals(HeatingBodyKind::Spa).is_empty());
    }

    #[test]
    fn interval_buffer_is_bounded() {
        let (mut estimator, _dir) = test_estimator();
        let segs = vec![test_weather_segment(0, i64::MAX / 2, 60.0)];
        for i in 0..(MAX_INTERVALS_PER_BODY + 10) {
            let t0 = i as i64 * 10 * 3_600_000;
            let anchor = thermal::ReliableSample { temperature_f: 90.0, observed_at_unix_ms: t0 };
            estimator.capture_cooling_interval(
                HeatingBodyKind::Pool, anchor, 89.5, 0.1, &segs, t0 + 8 * 3_600_000);
        }
        assert_eq!(estimator.intervals(HeatingBodyKind::Pool).len(), MAX_INTERVALS_PER_BODY);
    }
```

Where `test_weather_segment(start_ms, end_ms, air)` is a tiny `#[cfg(test)]` helper building a full-weather `WeatherSegment` with disabled site fields (lat/lon 0.0, transmission 0.0), `wind Some(0.0)`, `humidity Some(0.6)`, `clouds Some(0.0)`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet capture_ 2>&1 | head -5`
Expected: compile error — `capture_cooling_interval` not found.

- [ ] **Step 3: Implement**

Consts near the other calibration consts:

```rust
/// Residual beyond ANOMALY_SIGMA * max(rolling MAE, floor) tags the interval
/// ExcludedAnomalous (spec §3 uncovered/in-use heuristic).
const ANOMALY_SIGMA: f64 = 3.0;
const ANOMALY_MAE_FLOOR_F: f64 = 0.75;
```

Method on `HeatEstimator`:

```rust
    /// Capture one classified cooling interval (spec §3, §11). The caller has
    /// already established the gap is heater-free and the endpoint reading is
    /// trusted; this adds the whole-gap pump/body-activity check, classifies
    /// the residual, and stores a bounded, self-contained interval.
    fn capture_cooling_interval(
        &mut self,
        body: HeatingBodyKind,
        anchor: thermal::ReliableSample,
        actual_f: f64,
        error_f: f64,
        segments: &[thermal::WeatherSegment],
        now_unix_ms: i64,
    ) {
        if !self.config.calibration.enabled {
            return;
        }
        if self.activity_overlapped_gap(body, anchor.observed_at_unix_ms, now_unix_ms) {
            return; // the gap wasn't idle end-to-end — not a cooling interval
        }
        let weather =
            crate::calibrator::bucket_weather(segments, anchor.observed_at_unix_ms, now_unix_ms);
        if weather.is_empty() {
            return; // no weather spanned the gap — interval is unusable
        }
        let scale = self.prediction_mae(body).unwrap_or(0.0).max(ANOMALY_MAE_FLOOR_F);
        let regime = if error_f.abs() > ANOMALY_SIGMA * scale {
            crate::calibrator::IntervalRegime::ExcludedAnomalous
        } else {
            crate::calibrator::IntervalRegime::IdleCovered
        };
        let interval = crate::calibrator::CoolingInterval {
            t0_unix_ms: anchor.observed_at_unix_ms,
            t1_unix_ms: now_unix_ms,
            temp0_f: anchor.temperature_f,
            temp1_f: actual_f,
            regime,
            weather,
        };
        let buffer = self.intervals_slot_mut(body);
        buffer.push(interval);
        let len = buffer.len();
        if len > MAX_INTERVALS_PER_BODY {
            buffer.drain(..len - MAX_INTERVALS_PER_BODY);
        }
    }
```

Call site — in `calibrate_body`, immediately after the rolling-MAE block (`*mae_slot = Some(...)`) and before the secant-step block:

```rust
        // Persist the classified interval for the slow re-fit loop (spec §4.2).
        self.capture_cooling_interval(body, anchor, actual_f, error_f, &segments, now_unix_ms);
```

(`calibrate_body` already ends with `self.persist()`, which now also persists the captured interval — no extra persist call.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p pentair-daemon --quiet`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "feat(heat): capture classified cooling intervals with whole-gap idleness check"
```

---

### Task 7: Gated re-fit loop with silent auto-apply

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

**Interfaces:**
- Consumes: Tasks 2–6.
- Produces: `fn maybe_refit(&mut self, body: HeatingBodyKind, now_unix_ms: i64)` called for both bodies at the end of `calibrate_predictions` (piggybacks on the existing update cadence — no new tokio task; spec §4.2's triggers are time/count/drift-gated so cadence source is irrelevant). Also appends to `mae_history` (bounded `MAX_MAE_POINTS`) after each refit attempt, and sets `*_last_refit_unix_ms`.

Trigger (all of): calibration enabled; `>= refit_min_hours` since last refit (or never); AND (`>= min_new_intervals` IdleCovered intervals newer than last refit OR rolling MAE `> mae_drift_f`). Escape hatch (spec §9): when `exclusion_rate(window) > exclusion_rate_threshold`, run the fit over **all** window intervals (excluded included) instead of only IdleCovered.

- [ ] **Step 1: Write the failing tests**

```rust
    /// Fill the estimator with synthetic idle intervals generated from `truth`.
    #[cfg(test)]
    fn seed_synthetic_intervals(
        estimator: &mut HeatEstimator,
        body: HeatingBodyKind,
        truth: &CoolingParams,
        count: usize,
    ) {
        for i in 0..count {
            let t0 = i as i64 * 12 * 3_600_000;
            let t1 = t0 + 10 * 3_600_000;
            let segs = vec![test_weather_segment(t0, t1, 60.0)];
            let weather = crate::calibrator::bucket_weather(&segs, t0, t1);
            let mut interval = crate::calibrator::CoolingInterval {
                t0_unix_ms: t0, t1_unix_ms: t1, temp0_f: 95.0, temp1_f: 0.0,
                regime: crate::calibrator::IntervalRegime::IdleCovered, weather,
            };
            interval.temp1_f = crate::calibrator::predict_interval_end(
                &interval, truth, thermal::SolarSite::disabled());
            estimator.intervals_slot_mut(body).push(interval);
        }
    }

    #[test]
    fn refit_applies_validated_blend_silently() {
        let (mut estimator, _dir) = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        // Current params badly wrong.
        *estimator.cooling_params_slot_mut(HeatingBodyKind::Spa) =
            Some(CoolingParams { k0_per_hour: 1.0 / 30.0, ..CoolingParams::seed() });
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 8);
        let now = 100 * 12 * 3_600_000;
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        let after = estimator.cooling_params(HeatingBodyKind::Spa);
        // Moved toward truth (damped: between old 1/30 and truth 1/96).
        assert!(after.k0_per_hour < 1.0 / 30.0);
        assert!(after.k0_per_hour > 1.0 / 96.0);
        assert_eq!(estimator.store.spa_last_refit_unix_ms, Some(now));
        assert!(!estimator.store.mae_history.is_empty());
    }

    #[test]
    fn refit_rate_limited_by_refit_min_hours() {
        let (mut estimator, _dir) = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 8);
        let now = 100 * 12 * 3_600_000;
        estimator.store.spa_last_refit_unix_ms = Some(now - 3_600_000); // 1h ago < 24h
        let before = estimator.cooling_params(HeatingBodyKind::Spa);
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert_eq!(estimator.cooling_params(HeatingBodyKind::Spa), before);
        assert_eq!(estimator.store.spa_last_refit_unix_ms, Some(now - 3_600_000));
    }

    #[test]
    fn refit_needs_min_new_intervals_unless_mae_drifts() {
        let (mut estimator, _dir) = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 2); // < 4
        let now = 100 * 12 * 3_600_000;
        let before = estimator.cooling_params(HeatingBodyKind::Spa);
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert_eq!(estimator.cooling_params(HeatingBodyKind::Spa), before, "too few intervals, no drift");
        // Drifted MAE forces the fit even with few intervals... but the accept
        // gate needs a holdout, so seed enough for a split first.
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 4);
        *estimator.prediction_mae_slot_mut(HeatingBodyKind::Spa) = Some(5.0); // > mae_drift_f
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert!(estimator.store.spa_last_refit_unix_ms.is_some(), "drift trigger fired");
    }
```

(`CoolingParams::seed()` exists in `thermal.rs`; `PartialEq` on `CoolingParams` exists — it derives `PartialEq`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet refit_ 2>&1 | head -5`
Expected: compile error — `maybe_refit` not found.

- [ ] **Step 3: Implement**

```rust
    /// The slow re-fit loop (spec §4.2, §6, §7): gated, holdout-validated,
    /// damped, and SILENT (store-only; tracing debug; no event-log anywhere).
    fn maybe_refit(&mut self, body: HeatingBodyKind, now_unix_ms: i64) {
        let cal = self.config.calibration.clone();
        if !cal.enabled {
            return;
        }
        let last_refit = match body {
            HeatingBodyKind::Pool => self.store.pool_last_refit_unix_ms,
            HeatingBodyKind::Spa => self.store.spa_last_refit_unix_ms,
        };
        let min_gap_ms = (cal.refit_min_hours * 3_600_000.0) as i64;
        if let Some(last) = last_refit {
            if now_unix_ms - last < min_gap_ms {
                return;
            }
        }
        let window_start = now_unix_ms - (cal.window_days * 24.0 * 3_600_000.0) as i64;
        let since_fit = last_refit.unwrap_or(i64::MIN);
        let intervals = self.intervals(body);
        let fresh_clean = intervals
            .iter()
            .filter(|i| {
                i.t1_unix_ms > since_fit
                    && i.regime == crate::calibrator::IntervalRegime::IdleCovered
            })
            .count();
        let mae_drifted = self.prediction_mae(body).is_some_and(|m| m > cal.mae_drift_f);
        if fresh_clean < cal.min_new_intervals && !mae_drifted {
            return;
        }

        // Escape hatch (spec §9): a persistently-high exclusion rate means the
        // model, not the water, is wrong — fit over EVERYTHING in the window.
        let exclusion_window_start =
            now_unix_ms - (cal.exclusion_window_days * 24.0 * 3_600_000.0) as i64;
        let escape = crate::calibrator::exclusion_rate(intervals, exclusion_window_start)
            > cal.exclusion_rate_threshold;
        let window: Vec<crate::calibrator::CoolingInterval> = intervals
            .iter()
            .filter(|i| {
                i.t1_unix_ms >= window_start
                    && (escape || i.regime == crate::calibrator::IntervalRegime::IdleCovered)
            })
            .cloned()
            .collect();
        if window.len() < 3 {
            return; // holdout_split needs both halves populated
        }

        let site = self.solar_site();
        let current = self.cooling_params(body);
        let (fit_set, holdout) = crate::calibrator::holdout_split(&window);
        let candidate = crate::calibrator::fit_intervals(&fit_set, &current, site);
        let accepted = crate::calibrator::evaluate_candidate(
            &current, &candidate, &holdout, site, cal.damping_alpha, cal.accept_tolerance_f,
        );
        match accepted {
            Some(blend) => {
                tracing::debug!(?body, tau_hours = 1.0 / blend.k0_per_hour, escape,
                    "calibrator: refit accepted (silent auto-apply)");
                *self.cooling_params_slot_mut(body) = Some(blend);
            }
            None => {
                tracing::debug!(?body, escape, "calibrator: refit rejected, keeping current");
            }
        }

        // Bookkeeping either way: rate-limit stamp + MAE trend point.
        match body {
            HeatingBodyKind::Pool => self.store.pool_last_refit_unix_ms = Some(now_unix_ms),
            HeatingBodyKind::Spa => self.store.spa_last_refit_unix_ms = Some(now_unix_ms),
        }
        if let Some(mae) = self.prediction_mae(body) {
            self.store.mae_history.push(MaePoint { body, at_unix_ms: now_unix_ms, mae_f: mae });
            let len = self.store.mae_history.len();
            if len > MAX_MAE_POINTS {
                self.store.mae_history.drain(..len - MAX_MAE_POINTS);
            }
        }
        self.persist();
    }
```

Call site — at the end of `calibrate_predictions`, after both body blocks:

```rust
        self.maybe_refit(HeatingBodyKind::Pool, now_unix_ms);
        self.maybe_refit(HeatingBodyKind::Spa, now_unix_ms);
```

(If `heat.rs` imports `warn!`-style macros from `tracing` at the top, follow the same import style for `debug!` instead of the fully-qualified path.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p pentair-daemon --quiet`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "feat(heat): gated holdout-validated re-fit with damped silent auto-apply + escape hatch"
```

---

### Task 8: Outlet-vs-bulk offset + bulk heater rate from settled post-heat reads

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

**Interfaces:**
- Consumes: `store.recent_sessions` (`CompletedHeatingSession { body, started_at_unix_ms, ended_at_unix_ms, start_temp_f, end_temp_f, .. }`), Task 5 store slots.
- Produces: `fn learn_outlet_offset(&mut self, body: HeatingBodyKind, settled_bulk_f: f64, now_unix_ms: i64)` — called from `calibrate_body` right after the trust check succeeds (every fresh settled read), BEFORE the gap-length early-return (a settled read minutes after a session must still pair). Logic (spec §8):
  - Find the most recent completed session for `body` with `ended_at_unix_ms <= now_unix_ms` within `offset_settle_window_hours`.
  - Skip if that session's `ended_at_unix_ms` equals the body's `*_offset_done_session_end_ms` (already paired).
  - `offset_sample = session.end_temp_f - settled_bulk_f` (positive: outlet reads hot). `bulk_rate_sample = (settled_bulk_f - session.start_temp_f) / heating_hours` where `heating_hours = (ended - started)/3.6e6`; skip non-finite / non-positive-duration samples; clamp `offset` into `[-5.0, 20.0]` and `bulk_rate` into `(0.0, 60.0]` — outside means a mis-pair, skip.
  - EWMA into the store slots with alpha 0.3 (`new = old*(1-0.3) + sample*0.3`, or the sample itself when the slot is `None`); mark `*_offset_done_session_end_ms = Some(session.ended_at_unix_ms)`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn outlet_offset_and_bulk_rate_learned_from_settled_post_heat_read() {
        let (mut estimator, _dir) = test_estimator();
        // 30-min spa session: 91F -> outlet said 104F at cutoff.
        estimator.store.recent_sessions.push(test_completed_session(
            HeatingBodyKind::Spa, 0, 30 * 60_000, 91.0, 104.0));
        // Settled mixed read 20 min later: true bulk 94F.
        estimator.learn_outlet_offset(HeatingBodyKind::Spa, 94.0, 50 * 60_000);
        let offset = estimator.store.spa_outlet_offset_f.expect("offset learned");
        assert!((offset - 10.0).abs() < 1e-9); // 104 - 94
        let rate = estimator.store.spa_bulk_rate_f_per_hour.expect("bulk rate learned");
        assert!((rate - 6.0).abs() < 1e-9); // (94 - 91) / 0.5h
        // Second call for the same session is a no-op (already paired).
        estimator.learn_outlet_offset(HeatingBodyKind::Spa, 93.0, 60 * 60_000);
        assert!((estimator.store.spa_outlet_offset_f.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn outlet_offset_ignores_reads_outside_settle_window() {
        let (mut estimator, _dir) = test_estimator();
        estimator.store.recent_sessions.push(test_completed_session(
            HeatingBodyKind::Spa, 0, 30 * 60_000, 91.0, 104.0));
        // 10 hours later (> 6h window): stale, don't pair.
        estimator.learn_outlet_offset(HeatingBodyKind::Spa, 92.0, 10 * 3_600_000);
        assert!(estimator.store.spa_outlet_offset_f.is_none());
    }
```

Where `test_completed_session(body, started_ms, ended_ms, start_f, end_f)` is a `#[cfg(test)]` helper constructing a `CompletedHeatingSession` with the remaining fields at their simplest valid values (copy the field list from the struct definition; set target/rate fields to `end_f`/`None`-equivalents per the struct's actual fields).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pentair-daemon --quiet outlet_offset 2>&1 | head -5`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
    /// Learn the heater-return sensor's outlet-vs-bulk offset and the true BULK
    /// heating rate from the first settled read after a completed session
    /// (spec §8): during firing the sensor reads ~6-10F above the mixed bulk,
    /// so the session's end reading is outlet-biased; the settled read is truth.
    fn learn_outlet_offset(&mut self, body: HeatingBodyKind, settled_bulk_f: f64, now_unix_ms: i64) {
        if !self.config.calibration.enabled {
            return;
        }
        let window_ms = (self.config.calibration.offset_settle_window_hours * 3_600_000.0) as i64;
        let done_slot_value = match body {
            HeatingBodyKind::Pool => self.store.pool_offset_done_session_end_ms,
            HeatingBodyKind::Spa => self.store.spa_offset_done_session_end_ms,
        };
        let Some(session) = self
            .store
            .recent_sessions
            .iter()
            .filter(|s| {
                s.body == body
                    && s.ended_at_unix_ms <= now_unix_ms
                    && now_unix_ms - s.ended_at_unix_ms <= window_ms
            })
            .max_by_key(|s| s.ended_at_unix_ms)
            .cloned()
        else {
            return;
        };
        if done_slot_value == Some(session.ended_at_unix_ms) {
            return; // this session already produced its one offset sample
        }
        let heating_hours =
            (session.ended_at_unix_ms - session.started_at_unix_ms) as f64 / 3_600_000.0;
        if heating_hours <= 0.0 {
            return;
        }
        let offset = session.end_temp_f - settled_bulk_f;
        let bulk_rate = (settled_bulk_f - session.start_temp_f) / heating_hours;
        if !offset.is_finite() || !bulk_rate.is_finite() {
            return;
        }
        if !(-5.0..=20.0).contains(&offset) || !(0.0..=60.0).contains(&bulk_rate) || bulk_rate == 0.0 {
            return; // implausible pairing — skip rather than poison the EWMA
        }
        const EWMA_ALPHA: f64 = 0.3;
        let (offset_slot, rate_slot, done_slot) = match body {
            HeatingBodyKind::Pool => (
                &mut self.store.pool_outlet_offset_f,
                &mut self.store.pool_bulk_rate_f_per_hour,
                &mut self.store.pool_offset_done_session_end_ms,
            ),
            HeatingBodyKind::Spa => (
                &mut self.store.spa_outlet_offset_f,
                &mut self.store.spa_bulk_rate_f_per_hour,
                &mut self.store.spa_offset_done_session_end_ms,
            ),
        };
        *offset_slot = Some(match *offset_slot {
            Some(prev) => prev * (1.0 - EWMA_ALPHA) + offset * EWMA_ALPHA,
            None => offset,
        });
        *rate_slot = Some(match *rate_slot {
            Some(prev) => prev * (1.0 - EWMA_ALPHA) + bulk_rate * EWMA_ALPHA,
            None => bulk_rate,
        });
        *done_slot = Some(session.ended_at_unix_ms);
        self.persist();
    }
```

Call site — in `calibrate_body`, immediately after the trust check succeeds (right after the `if !self.temperature_trust_for_body(...).reliable { return; }` block, before the anchor/gap logic):

```rust
        // A fresh settled read may be the post-mix truth for a just-finished
        // heating session — learn the outlet offset + bulk rate (spec §8).
        self.learn_outlet_offset(body, telemetry.temperature_f, now_unix_ms);
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p pentair-daemon --quiet`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "feat(heat): learn outlet-vs-bulk offset + bulk heater rate from settled post-heat reads"
```

---

### Task 9: `GET /api/pool/calibration` (Phase 2)

**Files:**
- Modify: `pentair-daemon/src/heat.rs` (public snapshot method)
- Modify: `pentair-daemon/src/api/routes.rs` (route + handler)

**Interfaces:**
- Produces:
  - `impl HeatEstimator { pub fn calibration_snapshot(&self, now_unix_ms: i64) -> serde_json::Value }` — current-state-only (spec §10), per body: `params {tau_hours, k0_per_hour, k_wind_per_hour_per_mph, evap_a, evap_b, solar_gain_f}`, `rolling_mae_f`, `last_refit_unix_ms`, `outlet_offset_f`, `bulk_rate_f_per_hour`, `interval_counts {idle_covered, excluded_anomalous}`, `exclusion_rate_recent`, plus top-level `enabled`, `mae_trend` (the ring, as `[{body, at_unix_ms, mae_f}]`), and `as_of_unix_ms`.
  - Route `GET /api/pool/calibration` → `Json<serde_json::Value>`, registered next to `/api/pool/heat-plan`. **Strictly read-only** — the handler takes `State`, locks shared state for read, calls the snapshot; no `cmd_tx`, no writes.

- [ ] **Step 1: Write the failing test**

In `heat.rs` tests:

```rust
    #[test]
    fn calibration_snapshot_shape() {
        let (mut estimator, _dir) = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 3);
        let snap = estimator.calibration_snapshot(1_000_000);
        assert_eq!(snap["as_of_unix_ms"], 1_000_000);
        assert!(snap["enabled"].as_bool().unwrap());
        let spa = &snap["spa"];
        assert!(spa["params"]["tau_hours"].as_f64().unwrap() > 0.0);
        assert_eq!(spa["interval_counts"]["idle_covered"], 3);
        assert_eq!(spa["interval_counts"]["excluded_anomalous"], 0);
        assert!(snap["pool"].is_object());
        assert!(snap["mae_trend"].is_array());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pentair-daemon --quiet calibration_snapshot 2>&1 | head -5`
Expected: compile error.

- [ ] **Step 3: Implement**

In `heat.rs`:

```rust
    /// Read-only calibration state for GET /api/pool/calibration (spec §10).
    /// Current state only — no change history exists by design (spec §7).
    pub fn calibration_snapshot(&self, now_unix_ms: i64) -> serde_json::Value {
        let body_json = |body: HeatingBodyKind| {
            let params = self.cooling_params(body);
            let intervals = self.intervals(body);
            let idle = intervals
                .iter()
                .filter(|i| i.regime == crate::calibrator::IntervalRegime::IdleCovered)
                .count();
            let excluded = intervals.len() - idle;
            let exclusion_window_start = now_unix_ms
                - (self.config.calibration.exclusion_window_days * 24.0 * 3_600_000.0) as i64;
            let (last_refit, offset, bulk_rate) = match body {
                HeatingBodyKind::Pool => (
                    self.store.pool_last_refit_unix_ms,
                    self.store.pool_outlet_offset_f,
                    self.store.pool_bulk_rate_f_per_hour,
                ),
                HeatingBodyKind::Spa => (
                    self.store.spa_last_refit_unix_ms,
                    self.store.spa_outlet_offset_f,
                    self.store.spa_bulk_rate_f_per_hour,
                ),
            };
            serde_json::json!({
                "params": {
                    "tau_hours": 1.0 / params.k0_per_hour,
                    "k0_per_hour": params.k0_per_hour,
                    "k_wind_per_hour_per_mph": params.k_wind_per_hour_per_mph,
                    "evap_a": params.evap_a,
                    "evap_b": params.evap_b,
                    "solar_gain_f": params.solar_gain_f,
                },
                "rolling_mae_f": self.prediction_mae(body),
                "last_refit_unix_ms": last_refit,
                "outlet_offset_f": offset,
                "bulk_rate_f_per_hour": bulk_rate,
                "interval_counts": { "idle_covered": idle, "excluded_anomalous": excluded },
                "exclusion_rate_recent":
                    crate::calibrator::exclusion_rate(intervals, exclusion_window_start),
            })
        };
        serde_json::json!({
            "as_of_unix_ms": now_unix_ms,
            "enabled": self.config.calibration.enabled,
            "pool": body_json(HeatingBodyKind::Pool),
            "spa": body_json(HeatingBodyKind::Spa),
            "mae_trend": self.store.mae_history.iter().map(|p| serde_json::json!({
                "body": p.body, "at_unix_ms": p.at_unix_ms, "mae_f": p.mae_f,
            })).collect::<Vec<_>>(),
        })
    }
```

In `routes.rs`, next to the heat-plan route registration:

```rust
        .route("/api/pool/calibration", get(get_calibration))
```

and the handler, modeled exactly on `get_heat_plan`'s state access (same lock pattern the file already uses — read lock, no commands):

```rust
/// GET /api/pool/calibration — read-only calibrator state (advisory; spec §10).
/// Actuates nothing: read lock + snapshot, no command channel access.
async fn get_calibration(State(state): State<AppState>) -> Json<serde_json::Value> {
    let shared = state.shared.read().await;
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Json(shared.heat.calibration_snapshot(now_unix_ms))
}
```

**Access note:** `CachedState.heat` — check its visibility in `state.rs`; `routes.rs`'s `get_heat_plan` already reaches the same shared state, so mirror however it accesses `heat`/config there (if `heat` is private to `state.rs`, add `pub` to the field or a `pub fn heat(&self) -> &HeatEstimator` accessor, whichever matches the file's existing style).

- [ ] **Step 4: Run tests + build**

Run: `cargo test -p pentair-daemon --quiet && cargo build -p pentair-daemon --quiet`
Expected: all tests pass; clean build.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs pentair-daemon/src/api/routes.rs pentair-daemon/src/state.rs
git commit -m "feat(api): read-only GET /api/pool/calibration snapshot"
```

---

### Task 10: Synthetic end-to-end regression + full verification

**Files:**
- Modify: `pentair-daemon/src/heat.rs` (one test)

**Interfaces:** none new — this task proves the assembled loop per spec §13.

- [ ] **Step 1: Write the end-to-end test**

```rust
    #[test]
    fn end_to_end_capture_then_refit_recovers_truth_and_excludes_anomaly() {
        let (mut estimator, _dir) = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        *estimator.cooling_params_slot_mut(HeatingBodyKind::Spa) =
            Some(CoolingParams { k0_per_hour: 1.0 / 40.0, ..CoolingParams::seed() });

        // Capture 8 clean intervals THROUGH the real capture path.
        for i in 0..8 {
            let t0 = i as i64 * 12 * 3_600_000;
            let t1 = t0 + 10 * 3_600_000;
            let segs = vec![test_weather_segment(t0, t1, 60.0)];
            let anchor = thermal::ReliableSample { temperature_f: 95.0, observed_at_unix_ms: t0 };
            let probe = crate::calibrator::CoolingInterval {
                t0_unix_ms: t0, t1_unix_ms: t1, temp0_f: 95.0, temp1_f: 0.0,
                regime: crate::calibrator::IntervalRegime::IdleCovered,
                weather: crate::calibrator::bucket_weather(&segs, t0, t1),
            };
            let actual = crate::calibrator::predict_interval_end(
                &probe, &truth, thermal::SolarSite::disabled());
            // error vs CURRENT (wrong) params is small enough to stay IdleCovered
            // because rolling MAE is None -> floor 0.75*3 = 2.25F... so use the
            // real error the capture path would compute: pass a modest error.
            estimator.capture_cooling_interval(
                HeatingBodyKind::Spa, anchor, actual, 1.0, &segs, t1);
        }
        // One wild swim-loss interval: tagged anomalous, excluded from the fit.
        let t0 = 8 * 12 * 3_600_000;
        let segs = vec![test_weather_segment(t0, t0 + 10 * 3_600_000, 60.0)];
        let anchor = thermal::ReliableSample { temperature_f: 95.0, observed_at_unix_ms: t0 };
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa, anchor, 80.0, -12.0, &segs, t0 + 10 * 3_600_000);

        let counts = estimator.intervals(HeatingBodyKind::Spa);
        assert_eq!(counts.len(), 9);
        assert_eq!(counts.iter().filter(|i|
            i.regime == crate::calibrator::IntervalRegime::ExcludedAnomalous).count(), 1);

        // Refit: moves tau from 40h toward 96h (damped single step).
        let before = estimator.cooling_params(HeatingBodyKind::Spa).k0_per_hour;
        estimator.maybe_refit(HeatingBodyKind::Spa, t0 + 11 * 3_600_000);
        let after = estimator.cooling_params(HeatingBodyKind::Spa).k0_per_hour;
        assert!(after < before, "tau must move toward the (slower) truth");
    }
```

- [ ] **Step 2: Run the new test**

Run: `cargo test -p pentair-daemon --quiet end_to_end_capture`
Expected: PASS.

- [ ] **Step 3: Full verification**

Run: `cd /home/ssilver/development/pentair && cargo test --workspace --quiet 2>&1 | grep -E "test result" && cargo clippy -p pentair-daemon --quiet 2>&1 | grep -cE "^warning|^error" || true`
Expected: every suite `ok`, 0 failed (baseline was 412 passing + new tests); clippy no new warnings.

- [ ] **Step 4: Manual smoke (documentation only — do not deploy in this plan)**

After a later deploy, `curl -s http://127.0.0.1:8080/api/pool/calibration | python3 -m json.tool` should show both bodies' params (pool τ≈120h, spa τ≈96h from the seeded store), zero intervals initially, and `enabled: true`.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "test(heat): end-to-end calibrator capture->classify->refit regression"
```

---

## Self-review notes (spec → task mapping)

- §3 classification (whole-gap idleness, shared-sensor, anomaly): Tasks 5–6 (shared-sensor attribution is the existing trust check `calibrate_body` already gates on; activity windows add the whole-gap rule).
- §4 loops: per-reading = existing `calibrate_body` (+capture, Task 6); slow re-fit = Task 7. Probe loop = Phase 4, out of scope.
- §5: passive/opportunistic = existing cadence; probes/campaign OUT (Global Constraints).
- §6 fit/gate/priors: Tasks 2–3, 7 (evap held; g held when unobservable; same-holdout; blend validated; clamps reject).
- §7 silent auto-apply: Task 7 (store-only + tracing debug; no event-log code exists or is added).
- §8 outlet offset + bulk rate: Task 8.
- §9 escape hatch: Task 7; crash-mid-probe is Phase 4.
- §10 endpoint: Task 9.
- §11 persistence: Task 5 (serde defaults; bounded; existing atomic tmp+rename writer is reused via `persist()`; single-writer holds — everything goes through `HeatEstimator`).
- §12 config: Task 4.
- §13 testing: unit tests per task; synthetic end-to-end = Task 10; the "replay recorded soak logs" regression is intentionally NOT a repo test (the logs live in `~/.config`, not the repo) — the synthetic recovery test covers the same property.
