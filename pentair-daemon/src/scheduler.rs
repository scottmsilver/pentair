//! Pure model primitives for the **predictive pool comfort scheduler**
//! (advisory / evaluation only — see `docs/2026-06-29-pool-comfort-scheduler-v1.md`).
//!
//! # HARD CONSTRAINT — EVALUATION / ADVISORY ONLY
//!
//! Nothing in this module actuates. It **computes and reports** a recommended
//! heating plan plus its projected energy/cost; it never POSTs `/api/*/heat` or
//! `/on`, never writes a setpoint, and never issues a pump/heater command. There
//! is no actuation code path here. A future `[comfort].actuate` flag is
//! *reserved* (default `false`) but is **inert** — no code in v1 reads it to act.
//! This module's only outputs are read-only data structures.
//!
//! # Purity
//!
//! Like `thermal.rs`, every function here is pure: the clock, weather, params,
//! gas/rate tables, and the UTC offset are all *injected*. There is no I/O, no
//! network, and no `Local::now()` — so the logic unit-tests deterministically.
//! "Local-time aware" rate periods and comfort windows are resolved against an
//! injected `utc_offset_seconds` (e.g. `-8 * 3600` for PST) rather than a
//! timezone database, keeping the module a pure function of its inputs.
//!
//! # Equipment model
//!
//! The pool is heated by **one gas heater** (ScreenLogic heat-source 3). A gas
//! heater has a *constant* delivered thermal output and a flat combustion
//! efficiency — there is no COP curve and no dependence on air/water temperature.
//! Its fuel is natural gas, billed at a **flat `$/therm`** (no intraday TOU). The
//! only legitimate intraday time-shift lever is the *circulation pump's*
//! electricity draw while heating, which IS time-of-use (so the electricity
//! [`RateSchedule`] is kept — but it now costs only the pump, never the heat).
//!
//! # Reuse
//!
//! The passive (no-heater) temperature step delegates to
//! [`thermal::passive_relax_over_segment`], the project's single source of truth
//! for the cooling + solar + evaporation relaxation. A heat-on slot adds a
//! `delta_f` computed from the gas heater's BTU output and the pool's thermal
//! mass. We never re-derive the passive physics here.
//
// Phase 1 lands the pure primitives + tests; the optimizer/baseline/report and
// the read-only API/CLI surfaces arrive in later phases. Until then the public
// surface is exercised by the in-module unit tests, so suppress dead-code.
#![allow(dead_code)]

use crate::thermal::{self, CoolingParams, WeatherSegment};

/// Pounds of water per gallon — matches `heat.rs`'s `WATER_LB_PER_GALLON`. With
/// water's specific heat of 1 BTU/(lb·°F), the pool's thermal mass in BTU/°F is
/// `volume_gallons * WATER_LB_PER_GALLON`.
pub const WATER_LB_PER_GALLON: f64 = 8.34;

const SECONDS_PER_DAY: i64 = 86_400;

// ─── Grid-sizing guard rails ────────────────────────────────────────────────
//
// A pathological config (`slot_hours = 0`, `1e-9`, or negative; an enormous
// `horizon_hours`) must never blow up the discretized grid: a tiny slot makes
// `num_slots = ceil(horizon/slot)` astronomically large, which both allocates a
// huge `Vec<bool>` and overflows the `i64` timestamp math in [`SlotGrid`]. We
// clamp the slot length to a sane floor and the slot count to a hard ceiling so
// the advisory plan stays bounded regardless of config.

/// Minimum slot length (hours) — 3 minutes. A smaller (or non-positive,
/// NaN/inf) configured slot is raised to this floor.
pub const MIN_SLOT_HOURS: f64 = 0.05;
/// Default slot length (hours) when the configured value is non-finite or
/// non-positive — mirrors the daemon's `default_comfort_slot_hours`.
pub const DEFAULT_SLOT_HOURS: f64 = 0.5;
/// Hard ceiling on the number of slots in a grid. Bounds the `Vec` allocation
/// and keeps `anchor + num_slots * slot_secs` far from `i64` overflow.
pub const MAX_NUM_SLOTS: usize = 4096;

/// Sanitize a configured `slot_hours`: a non-finite or non-positive value falls
/// back to [`DEFAULT_SLOT_HOURS`], and anything below [`MIN_SLOT_HOURS`] is
/// raised to that floor. The result is always finite and `>= MIN_SLOT_HOURS`.
pub fn sanitize_slot_hours(slot_hours: f64) -> f64 {
    if !slot_hours.is_finite() || slot_hours <= 0.0 {
        DEFAULT_SLOT_HOURS
    } else {
        slot_hours.max(MIN_SLOT_HOURS)
    }
}

/// Compute a bounded slot count for a horizon: `ceil(horizon / slot)` clamped to
/// `[1, MAX_NUM_SLOTS]`. Assumes `slot_hours` has already been sanitized; guards
/// against a non-finite `horizon_hours` too. Never panics, never overflows.
pub fn bounded_num_slots(horizon_hours: f64, slot_hours: f64) -> usize {
    let slot = sanitize_slot_hours(slot_hours);
    let horizon = if horizon_hours.is_finite() {
        horizon_hours.max(slot)
    } else {
        slot
    };
    let raw = (horizon / slot).ceil();
    // Map the (possibly overflowing) ratio onto `[1, MAX_NUM_SLOTS]`. A non-
    // finite or huge ratio saturates at the ceiling; anything sub-1 floors at 1.
    let n = if !raw.is_finite() || raw >= MAX_NUM_SLOTS as f64 {
        MAX_NUM_SLOTS
    } else if raw >= 1.0 {
        // Safe `as usize` cast: `raw` is finite and `< MAX_NUM_SLOTS` here.
        raw as usize
    } else {
        1
    };
    n.clamp(1, MAX_NUM_SLOTS)
}

// ─── GasHeaterModel ────────────────────────────────────────────────────────

/// A single **gas** pool heater (ScreenLogic heat-source 3). Config-driven, pure.
///
/// Unlike a heat pump, a gas heater has a *constant* delivered thermal output
/// and a flat combustion efficiency — there is no COP curve and no dependence on
/// air or water temperature. [`heat_output`](Self::heat_output) returns the
/// constant delivered BTU/hr and the gas it burns to produce it (therms/hr);
/// [`delta_f_per_slot`](Self::delta_f_per_slot) converts that thermal energy into
/// a temperature rise given the pool's thermal mass. The circulation pump's
/// electrical draw while heating is a separate field ([`pump_kw`](Self::pump_kw)),
/// costed by the (time-of-use) electricity rate — NOT part of the heat output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasHeaterModel {
    /// Delivered thermal output (BTU/hr), e.g. 250_000. Constant while heating.
    pub rated_btu_per_hr: f64,
    /// Combustion efficiency = delivered BTU ÷ gas BTU consumed, e.g. 0.82.
    pub thermal_efficiency: f64,
    /// Circulation-pump electrical draw while heating (kW), e.g. 0.75.
    pub pump_kw: f64,
}

impl GasHeaterModel {
    /// BTU of gas INPUT in one therm.
    const BTU_PER_THERM: f64 = 100_000.0;

    /// A reasonable default for the real equipment: a 250k BTU/hr gas heater at
    /// 82 % combustion efficiency with a 0.75 kW circulation pump.
    pub fn spec_default() -> Self {
        Self {
            rated_btu_per_hr: 250_000.0,
            thermal_efficiency: 0.82,
            pump_kw: 0.75,
        }
    }

    /// Combustion efficiency clamped into `(0.0, 1.0]` with a floor so the
    /// gas-input math never divides by zero or runs away on a degenerate config.
    fn efficiency_clamped(&self) -> f64 {
        if self.thermal_efficiency.is_finite() {
            self.thermal_efficiency.clamp(0.5, 1.0)
        } else {
            0.82
        }
    }

    /// Gas INPUT consumed per hour while heating, in therms/hr:
    /// `(delivered_btu_per_hr / efficiency) / 100_000`. Constant — independent of
    /// any temperatures.
    pub fn gas_therms_per_hr(&self) -> f64 {
        (self.rated_btu_per_hr.max(0.0) / self.efficiency_clamped()) / Self::BTU_PER_THERM
    }

    /// Heater output while heating: `(thermal_btu_per_hr, gas_therms_per_hr)`.
    /// A gas heater's output is CONSTANT — it takes no air/water arguments. The
    /// pump draw is the separate [`pump_kw`](Self::pump_kw) field, not returned
    /// here.
    pub fn heat_output(&self) -> (f64, f64) {
        (self.rated_btu_per_hr.max(0.0), self.gas_therms_per_hr())
    }

    /// Temperature rise (°F) the heater delivers over a slot of `slot_hours` into
    /// a body with `thermal_mass_btu_per_f` BTU/°F. `delta = BTU / (m*c)`. A
    /// non-positive slot or thermal mass yields `0`.
    pub fn delta_f_per_slot(
        &self,
        slot_hours: f64,
        thermal_mass_btu_per_f: f64,
    ) -> f64 {
        if slot_hours <= 0.0 || thermal_mass_btu_per_f <= 0.0 {
            return 0.0;
        }
        let btu = self.rated_btu_per_hr.max(0.0) * slot_hours;
        btu / thermal_mass_btu_per_f
    }
}

/// Pool thermal mass (BTU/°F) from a volume in gallons. `0` for a non-positive
/// volume.
pub fn thermal_mass_btu_per_f(volume_gallons: f64) -> f64 {
    (volume_gallons.max(0.0)) * WATER_LB_PER_GALLON
}

// ─── Local-time helpers (injected UTC offset; no timezone DB) ───────────────

/// Day of week, Monday = 0 … Sunday = 6. 1970-01-01 (unix 0) was a Thursday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    /// Parse a 3-letter (case-insensitive) weekday abbreviation.
    pub fn parse(s: &str) -> Option<Weekday> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mon" => Some(Weekday::Mon),
            "tue" => Some(Weekday::Tue),
            "wed" => Some(Weekday::Wed),
            "thu" => Some(Weekday::Thu),
            "fri" => Some(Weekday::Fri),
            "sat" => Some(Weekday::Sat),
            "sun" => Some(Weekday::Sun),
            _ => None,
        }
    }
}

/// Local wall-clock breakdown of an instant, given an injected UTC offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalTime {
    weekday: Weekday,
    /// Minutes since local midnight, in `[0, 1440)`.
    minute_of_day: i64,
}

/// Decompose `unix_secs` into a local weekday + minute-of-day using
/// `utc_offset_seconds` (e.g. `-8*3600` for PST). Pure: no timezone database.
fn local_time(unix_secs: i64, utc_offset_seconds: i64) -> LocalTime {
    let local = unix_secs + utc_offset_seconds;
    // Days since the unix epoch in local time (floored toward negative infinity).
    let day_index = local.div_euclid(SECONDS_PER_DAY);
    let secs_into_day = local.rem_euclid(SECONDS_PER_DAY);
    // 1970-01-01 was a Thursday => Thu has weekday index 3 in Mon=0 numbering.
    let weekday = match (day_index + 3).rem_euclid(7) {
        0 => Weekday::Mon,
        1 => Weekday::Tue,
        2 => Weekday::Wed,
        3 => Weekday::Thu,
        4 => Weekday::Fri,
        5 => Weekday::Sat,
        _ => Weekday::Sun,
    };
    LocalTime {
        weekday,
        minute_of_day: secs_into_day / 60,
    }
}

/// A wall-clock time of day as minutes since midnight, in `[0, 1440]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HhMm {
    pub minute_of_day: i64,
}

impl HhMm {
    /// Build from hour/minute, clamping into a valid range.
    pub fn new(hour: i64, minute: i64) -> Self {
        let m = (hour.clamp(0, 24) * 60 + minute.clamp(0, 59)).clamp(0, 1440);
        Self { minute_of_day: m }
    }

    /// Parse `"HH:MM"`. `"24:00"` is accepted as end-of-day (1440).
    pub fn parse(s: &str) -> Option<Self> {
        let (h, m) = s.split_once(':')?;
        let hour: i64 = h.trim().parse().ok()?;
        let minute: i64 = m.trim().parse().ok()?;
        if !(0..=24).contains(&hour) || !(0..=59).contains(&minute) {
            return None;
        }
        Some(Self::new(hour, minute))
    }
}

/// True when `minute_of_day` falls in the half-open `[start, end)` window.
/// A window with `end <= start` is treated as empty (we do not wrap past
/// midnight in v1 — split such a span into two windows if ever needed).
fn minute_in_window(minute_of_day: i64, start: HhMm, end: HhMm) -> bool {
    start.minute_of_day < end.minute_of_day
        && minute_of_day >= start.minute_of_day
        && minute_of_day < end.minute_of_day
}

// ─── RateSchedule ──────────────────────────────────────────────────────────

/// One time-of-use rate period: applies on `days`, during `[start, end)` local
/// wall-clock, at `usd_per_kwh`.
#[derive(Debug, Clone, PartialEq)]
pub struct RatePeriod {
    pub days: Vec<Weekday>,
    pub start: HhMm,
    pub end: HhMm,
    pub usd_per_kwh: f64,
}

impl RatePeriod {
    fn matches(&self, local: LocalTime) -> bool {
        self.days.contains(&local.weekday) && minute_in_window(local.minute_of_day, self.start, self.end)
    }
}

/// Time-of-use electricity rate schedule with a flat default fallback.
///
/// The first matching period (in declaration order) wins; when none match the
/// `default_usd_per_kwh` flat rate applies. Local-time aware via the injected
/// UTC offset — no `Local::now()`.
#[derive(Debug, Clone, PartialEq)]
pub struct RateSchedule {
    pub periods: Vec<RatePeriod>,
    pub default_usd_per_kwh: f64,
    /// Injected UTC offset in seconds (e.g. `-8*3600` for PST). Keeps the
    /// schedule a pure function of its inputs.
    pub utc_offset_seconds: i64,
}

impl RateSchedule {
    /// A flat-rate schedule (no TOU periods).
    pub fn flat(usd_per_kwh: f64, utc_offset_seconds: i64) -> Self {
        Self {
            periods: Vec::new(),
            default_usd_per_kwh: usd_per_kwh,
            utc_offset_seconds,
        }
    }

    /// USD per kWh in effect at `unix_secs`. Any non-finite rate (from a
    /// directly-constructed schedule that bypassed [`Self::from_config`]) is
    /// treated as `0` so it can't propagate NaN/inf into the cost math.
    pub fn rate(&self, unix_secs: i64) -> f64 {
        let local = local_time(unix_secs, self.utc_offset_seconds);
        let raw = self
            .periods
            .iter()
            .find(|p| p.matches(local))
            .map(|p| p.usd_per_kwh)
            .unwrap_or(self.default_usd_per_kwh);
        finite_nonneg_or(raw, 0.0)
    }
}

// ─── GasRate ───────────────────────────────────────────────────────────────

/// Flat natural-gas price. Gas has no intraday time-of-use, so this is a single
/// `$/therm` figure (no schedule, no UTC offset). The pump's electricity is the
/// only intraday time-shift lever and is costed separately via [`RateSchedule`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasRate {
    pub usd_per_therm: f64,
}

impl GasRate {
    /// A flat gas rate.
    pub fn flat(usd_per_therm: f64) -> Self {
        Self { usd_per_therm }
    }

    /// USD per therm of gas. Any non-finite or negative rate (from a directly-
    /// constructed value) is treated as `0` so it can't poison the cost math.
    pub fn cost(&self) -> f64 {
        finite_nonneg_or(self.usd_per_therm, 0.0)
    }
}

// ─── ComfortWindow ─────────────────────────────────────────────────────────

/// A comfort target: on `days`, during `[start, end)` local wall-clock, the
/// pool should be at least `target_f`. An empty set of windows means the
/// feature is off (no comfort target, hence no recommended heating).
#[derive(Debug, Clone, PartialEq)]
pub struct ComfortWindow {
    pub days: Vec<Weekday>,
    pub start: HhMm,
    pub end: HhMm,
    pub target_f: f64,
}

impl ComfortWindow {
    /// True when `unix_secs` falls inside this window (in local time).
    pub fn contains(&self, unix_secs: i64, utc_offset_seconds: i64) -> bool {
        let local = local_time(unix_secs, utc_offset_seconds);
        self.days.contains(&local.weekday)
            && minute_in_window(local.minute_of_day, self.start, self.end)
    }

    /// Unix seconds of the next start-of-window strictly after `after_unix_secs`,
    /// scanning forward day by day. Returns `None` if no window matches within
    /// `horizon_days` (bounded so the scan always terminates).
    pub fn next_start_after(
        &self,
        after_unix_secs: i64,
        utc_offset_seconds: i64,
        horizon_days: i64,
    ) -> Option<i64> {
        if self.days.is_empty() || self.end.minute_of_day <= self.start.minute_of_day {
            return None;
        }
        // Local midnight (unix secs) of the day containing `after`.
        let local = after_unix_secs + utc_offset_seconds;
        let day_start_local = local.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY;
        for day in 0..=horizon_days.max(0) {
            let midnight_local = day_start_local + day * SECONDS_PER_DAY;
            // Convert this local midnight back to a unix instant.
            let midnight_unix = midnight_local - utc_offset_seconds;
            let start_unix = midnight_unix + self.start.minute_of_day * 60;
            if start_unix <= after_unix_secs {
                continue;
            }
            let lt = local_time(start_unix, utc_offset_seconds);
            if self.days.contains(&lt.weekday) {
                return Some(start_unix);
            }
        }
        None
    }
}

// ─── Forward simulation with heating ───────────────────────────────────────

/// Result of a heated forward simulation over a slot grid.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatedTrajectory {
    /// `(unix_secs, water_temp_f)` at each slot boundary, starting at
    /// `(anchor, start_temp)` and including one point per slot end.
    pub points: Vec<(i64, f64)>,
    /// Circulation-pump electrical energy drawn per slot (kWh); length == number
    /// of slots. `pump_kw * slot_hours` on heated slots, `0` on off slots.
    pub pump_kwh: Vec<f64>,
    /// Gas INPUT burned per slot (therms); length == number of slots.
    /// `gas_therms_per_hr * slot_hours` on heated slots, `0` on off slots.
    pub gas_therms: Vec<f64>,
}

/// Step the thermal model across `heat_on.len()` slots of `slot_hours` each,
/// starting at `start_temp` at `anchor_unix` (seconds).
///
/// Each slot's passive temperature change comes from
/// [`thermal::passive_relax_over_segment`] — the **same** relaxation `thermal.rs`
/// uses — applied with the weather segment covering the slot midpoint. When
/// `heat_on[i]` is true, the gas heater's `delta_f` for the slot is added on top
/// and the slot's gas (therms) and pump electricity (kWh) are accumulated; an off
/// slot consumes nothing. Pure: no I/O.
// Every argument is an independent injected input (state, clock, weather,
// params, schedule, heater, mass, slot length) — exactly the "pure, fully
// injected" shape the spec mandates so this unit-tests like `thermal.rs`.
#[allow(clippy::too_many_arguments)]
pub fn forward_sim_with_heat(
    start_temp: f64,
    anchor_unix: i64,
    segments: &[WeatherSegment],
    params: &CoolingParams,
    heat_on: &[bool],
    heater: &GasHeaterModel,
    thermal_mass_btu_per_f: f64,
    slot_hours: f64,
) -> HeatedTrajectory {
    let slot_ms = (slot_hours * 3_600_000.0) as i64;
    let mut points = Vec::with_capacity(heat_on.len() + 1);
    let mut pump_kwh = Vec::with_capacity(heat_on.len());
    let mut gas_therms = Vec::with_capacity(heat_on.len());
    let mut water_f = start_temp;
    let mut cursor_unix = anchor_unix;
    points.push((cursor_unix, water_f));

    for &on in heat_on {
        let slot_start_unix = cursor_unix;
        let slot_end_unix = cursor_unix + slot_ms / 1000;
        let mid_unix_ms = (slot_start_unix * 1000 + slot_end_unix * 1000) / 2;

        // Passive step: relax across the segment covering this slot's midpoint,
        // using the project's shared thermal physics. If no segment brackets the
        // midpoint, fall back to the nearest segment by start time; if there are
        // no segments at all, the passive step is a no-op (temp unchanged).
        let seg = segment_for(segments, mid_unix_ms);
        water_f = match seg {
            Some(s) => thermal::passive_relax_over_segment(water_f, s, params, slot_hours),
            None => water_f,
        };

        // Heated slot: add delta_f and account the gas burned + pump electricity.
        let mut slot_gas_therms = 0.0;
        let mut slot_pump_kwh = 0.0;
        if on {
            let delta = heater.delta_f_per_slot(slot_hours, thermal_mass_btu_per_f);
            water_f += delta;
            let (_btu, gas_per_hr) = heater.heat_output();
            slot_gas_therms = gas_per_hr * slot_hours;
            slot_pump_kwh = heater.pump_kw.max(0.0) * slot_hours;
        }
        gas_therms.push(slot_gas_therms);
        pump_kwh.push(slot_pump_kwh);

        cursor_unix = slot_end_unix;
        points.push((cursor_unix, water_f));
    }

    HeatedTrajectory {
        points,
        pump_kwh,
        gas_therms,
    }
}

/// The weather segment bracketing `at_unix_ms`, else the nearest by start time.
fn segment_for(segments: &[WeatherSegment], at_unix_ms: i64) -> Option<&WeatherSegment> {
    if segments.is_empty() {
        return None;
    }
    if let Some(s) = segments
        .iter()
        .find(|s| s.start_unix_ms <= at_unix_ms && at_unix_ms < s.end_unix_ms)
    {
        return Some(s);
    }
    segments
        .iter()
        .min_by_key(|s| (s.start_unix_ms - at_unix_ms).abs())
}

// ─── Slot grid + cost accounting ───────────────────────────────────────────

/// The discretized horizon the optimizer and baseline both reason over: a fixed
/// grid of `num_slots` slots, each `slot_hours` long, starting at `anchor_unix`.
///
/// Slot `i` covers `[anchor_unix + i*slot_secs, anchor_unix + (i+1)*slot_secs)`;
/// its *start* instant is `slot_start_unix(i)`. The grid is the single place
/// timestamps are derived, so the optimizer, baseline, and forward sim stay in
/// lock-step. Pure data; no clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotGrid {
    pub anchor_unix: i64,
    pub slot_hours: f64,
    pub num_slots: usize,
}

impl SlotGrid {
    pub fn new(anchor_unix: i64, slot_hours: f64, num_slots: usize) -> Self {
        Self {
            anchor_unix,
            slot_hours,
            num_slots,
        }
    }

    fn slot_secs(&self) -> i64 {
        // `slot_hours` is sanitized upstream, but guard against a non-finite or
        // absurd direct construction so the cast never produces garbage. At least
        // one second per slot keeps the grid monotonic.
        let secs = if self.slot_hours.is_finite() {
            self.slot_hours * 3600.0
        } else {
            DEFAULT_SLOT_HOURS * 3600.0
        };
        (secs.clamp(1.0, SECONDS_PER_DAY as f64)) as i64
    }

    /// Unix seconds of the start of slot `i`. Saturating arithmetic so an
    /// out-of-range `i` (or an absurd grid) can never overflow `i64` and panic.
    pub fn slot_start_unix(&self, i: usize) -> i64 {
        let offset = (i as i64).saturating_mul(self.slot_secs());
        self.anchor_unix.saturating_add(offset)
    }

    /// Index of the slot whose half-open span contains `unix_secs`, if any.
    fn slot_index_for(&self, unix_secs: i64) -> Option<usize> {
        if unix_secs < self.anchor_unix {
            return None;
        }
        let secs = self.slot_secs();
        if secs <= 0 {
            return None;
        }
        let idx = ((unix_secs - self.anchor_unix) / secs) as usize;
        if idx < self.num_slots {
            Some(idx)
        } else {
            None
        }
    }
}

/// Total gas (therms), pump electricity (kWh), and cost (USD) of a per-slot
/// schedule. Gas is billed at the flat [`GasRate`]; the pump electricity is
/// billed against the (local-time aware) [`RateSchedule`] at each slot's *start*
/// instant — the one legitimate intraday time-shift lever. Returns
/// `(total_gas_therms, total_pump_kwh, total_usd)`.
fn cost_of(
    grid: &SlotGrid,
    traj: &HeatedTrajectory,
    gas_rate: &GasRate,
    elec_rates: &RateSchedule,
) -> (f64, f64, f64) {
    let gas_cost = gas_rate.cost();
    let mut total_gas_therms = 0.0;
    let mut total_pump_kwh = 0.0;
    let mut total_usd = 0.0;
    for i in 0..traj.gas_therms.len() {
        let therms = traj.gas_therms[i];
        let kwh = traj.pump_kwh[i];
        total_gas_therms += therms;
        total_pump_kwh += kwh;
        total_usd += therms * gas_cost + kwh * elec_rates.rate(grid.slot_start_unix(i));
    }
    (total_gas_therms, total_pump_kwh, total_usd)
}

// ─── Optimizer + baseline + report ─────────────────────────────────────────

/// A recommended heating plan plus the energy/cost evaluation that justifies it.
///
/// **Advisory only.** This is the module's headline output: a per-slot on/off
/// schedule the user *could* run, the temperature trajectory it would produce,
/// and the projected kWh/$ for the optimizer vs the dumb constant-setpoint
/// baseline. Nothing here actuates — the schedule is data, never a command.
#[derive(Debug, Clone, PartialEq)]
pub struct SavingsReport {
    /// `(slot_start_unix, heat_on)` for every slot in the horizon.
    pub schedule: Vec<(i64, bool)>,
    /// Projected water-temperature trajectory under `schedule`
    /// (`(unix, temp_f)`, one point per slot boundary plus the start).
    pub trajectory: Vec<(i64, f64)>,
    /// Optimizer projected gas (therms), pump electricity (kWh), and cost (USD).
    pub optimizer_gas_therms: f64,
    pub optimizer_pump_kwh: f64,
    pub optimizer_usd: f64,
    /// Constant-setpoint baseline projected gas (therms), pump electricity (kWh),
    /// and cost (USD).
    pub baseline_gas_therms: f64,
    pub baseline_pump_kwh: f64,
    pub baseline_usd: f64,
    /// `baseline_usd − optimizer_usd` (positive == the plan is cheaper).
    pub savings_usd: f64,
    /// Savings as a fraction of the baseline cost in `[..]` (0 when baseline is
    /// free). Multiply by 100 for a percentage.
    pub savings_pct: f64,
    /// For each configured comfort window that starts within the horizon:
    /// `(window_start_unix, target_f, met)` where `met` is true iff the
    /// optimizer's projected temperature is at/above `target_f` across the whole
    /// window.
    pub comfort_met: Vec<ComfortOutcome>,
    /// A short human-readable summary of the plan (advisory).
    pub summary: String,
}

/// Per-window comfort outcome under the optimizer's plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComfortOutcome {
    pub window_start_unix: i64,
    pub target_f: f64,
    pub met: bool,
}

impl SavingsReport {
    /// True iff every evaluated comfort window was met.
    pub fn all_comfort_met(&self) -> bool {
        self.comfort_met.iter().all(|o| o.met)
    }
}

/// Inputs shared by [`optimize`] and [`baseline_constant_setpoint`]: the physical
/// model and the horizon. Grouping them keeps the optimizer signature readable
/// and guarantees the two evaluations run on identical assumptions.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    pub grid: SlotGrid,
    pub segments: &'a [WeatherSegment],
    pub params: &'a CoolingParams,
    pub heater: &'a GasHeaterModel,
    pub gas_rate: &'a GasRate,
    /// Time-of-use electricity rates — used ONLY to cost the circulation pump.
    pub rates: &'a RateSchedule,
    pub thermal_mass_btu_per_f: f64,
    pub start_temp: f64,
}

impl PlanContext<'_> {
    /// Simulate a given on/off schedule and return its trajectory + per-slot gas
    /// (therms) and pump electricity (kWh).
    fn simulate(&self, heat_on: &[bool]) -> HeatedTrajectory {
        forward_sim_with_heat(
            self.start_temp,
            self.grid.anchor_unix,
            self.segments,
            self.params,
            heat_on,
            self.heater,
            self.thermal_mass_btu_per_f,
            self.grid.slot_hours,
        )
    }

    /// The water temperature at the *start* of slot `i` under `heat_on`, read off
    /// the simulated trajectory (`points[i]` is the slot-`i` start boundary).
    fn temp_at_slot_start(&self, traj: &HeatedTrajectory, i: usize) -> f64 {
        traj.points
            .get(i)
            .map(|&(_, t)| t)
            .unwrap_or(self.start_temp)
    }

}

/// Greedy, explainable optimizer (spec §5).
///
/// For each comfort window whose start falls within the horizon:
/// 1. Project the pool **passively** (heater off everywhere) — this already
///    includes free solar gain, so the window-start temperature is net of sun.
/// 2. `deficit = target_f − passive_temp_at_window_start`. If `<= 0`, the sun and
///    ambient already carry the pool to target: **heat nothing** for this window.
/// 3. Otherwise rank the candidate **lead-time** slots (those strictly before the
///    window start) by *cost per °F delivered* —
///    `(gas_therms*gas_rate + pump_kwh*elec_rate(slot)) / delta_f` — and switch
///    on the cheapest first. The gas term is constant across slots (the heater's
///    output is fixed), so the ranking is driven by the pump's TOU electricity
///    and by solar (which the passive projection already folds into each slot's
///    deficit), while respecting thermal lead time (only slots before the window
///    can pre-heat it). Stop once the projected temperature meets the target
///    across the whole window.
///
/// **No heating is ever scheduled outside a comfort window** — the pool drifts
/// when nobody is using it. Returns the on/off schedule (length `num_slots`).
pub fn optimize(ctx: &PlanContext, windows: &[ComfortWindow]) -> Vec<bool> {
    let n = ctx.grid.num_slots;
    let mut heat_on = vec![false; n];
    if n == 0 {
        return heat_on;
    }

    for window in windows {
        // Treat each CONTIGUOUS run of in-horizon window slots as its own
        // occurrence (e.g. Sat and Sun of a `["Sat","Sun"]` window are two), so
        // each occurrence only ever pre-heats with its own lead-time slots —
        // those after the PREVIOUS occurrence's end and before THIS one's start.
        let mut prev_end: Option<usize> = None;
        for occ in window_occurrences(ctx, window) {
            let lead_start = prev_end.map(|e| e + 1).unwrap_or(0);
            prev_end = occ.last().copied();
            heat_for_occurrence(ctx, window, &occ, lead_start, &mut heat_on);
        }
    }

    heat_on
}

/// A single contiguous in-horizon occurrence of a comfort window: the sorted run
/// of slot indices whose start instants fall inside the window with no gap. The
/// next occurrence begins only after a slot that is *not* in the window.
fn window_occurrences(ctx: &PlanContext, window: &ComfortWindow) -> Vec<Vec<usize>> {
    let grid = &ctx.grid;
    let offset = ctx.rates.utc_offset_seconds;
    let mut occurrences: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for i in 0..grid.num_slots {
        if window.contains(grid.slot_start_unix(i), offset) {
            current.push(i);
        } else if !current.is_empty() {
            occurrences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        occurrences.push(current);
    }
    occurrences
}

/// Allocate heating (mutating `heat_on`) so the projected temperature meets
/// `window.target_f` across the slots of ONE window occurrence. Greedy by
/// cost-per-°F over that occurrence's lead-time slots — those strictly before the
/// occurrence's first slot and after the previous occurrence's end — never
/// touching slots inside or after the occurrence, and never un-setting a slot
/// another occurrence/window already claimed.
fn heat_for_occurrence(
    ctx: &PlanContext,
    window: &ComfortWindow,
    window_slots: &[usize],
    lead_start: usize,
    heat_on: &mut [bool],
) {
    let grid = &ctx.grid;

    if window_slots.is_empty() {
        return;
    }
    let first_window_slot = window_slots[0];

    // Candidate lead-time slots: in `[lead_start, first_window_slot)` — i.e.
    // strictly before THIS occurrence's first slot and strictly after the
    // previous occurrence's end — and not already on. This is what stops a later
    // occurrence (e.g. Sunday) from borrowing an earlier occurrence's lead-time
    // (Saturday's) as its own pre-heat boundary. The window-membership filter is
    // belt-and-suspenders: the contiguous grouping already excludes window slots.
    let mut candidates: Vec<usize> = (lead_start..first_window_slot)
        .filter(|&i| !heat_on[i] && !window.contains(grid.slot_start_unix(i), ctx.rates.utc_offset_seconds))
        .collect();

    // Rank by cost-per-°F = (gas_therms*gas_rate + pump_kwh*elec_rate) / delta_f,
    // cheapest first. Both delta_f and the gas term are constant across slots (a
    // gas heater's output is fixed), so the ranking is driven by the pump's TOU
    // electricity rate(slot); solar enters via each slot's passive deficit in the
    // comfort re-sim below, not via the per-slot heating cost.
    let delta_f = ctx.heater.delta_f_per_slot(grid.slot_hours, ctx.thermal_mass_btu_per_f);
    if delta_f <= 0.0 {
        return;
    }
    // `total_cmp` is a total order over all f64 (including any residual NaN/inf),
    // so the sort can never panic the way `partial_cmp().unwrap()` would. The
    // cost helper already maps non-finite costs to `+inf` so they rank last.
    candidates.sort_by(|&a, &b| {
        cost_per_degf(ctx, a, delta_f).total_cmp(&cost_per_degf(ctx, b, delta_f))
    });

    // Turn on the cheapest candidate slots until the target holds across the
    // window, re-simulating after each to capture the (slow) thermal response.
    for slot in candidates {
        if comfort_met_across(ctx, heat_on, window_slots, window.target_f) {
            break;
        }
        heat_on[slot] = true;
    }
}

/// Cost per °F delivered by heating slot `i`:
/// `(gas_therms*gas_rate + pump_kwh*elec_rate(slot)) / delta_f`. The gas term is
/// constant across slots (the heater's output is fixed), so the per-slot ranking
/// is driven by the pump's TOU electricity at this slot. Lower is better. Returns
/// `+inf` for a degenerate (non-positive) `delta_f`.
fn cost_per_degf(ctx: &PlanContext, i: usize, delta_f: f64) -> f64 {
    if !delta_f.is_finite() || delta_f <= 0.0 {
        return f64::INFINITY;
    }
    let slot_hours = ctx.grid.slot_hours;
    let (_btu, gas_per_hr) = ctx.heater.heat_output();
    let gas_therms = gas_per_hr * slot_hours;
    let pump_kwh = ctx.heater.pump_kw.max(0.0) * slot_hours;
    let elec_rate = ctx.rates.rate(ctx.grid.slot_start_unix(i));
    let cost = (gas_therms * ctx.gas_rate.cost() + pump_kwh * elec_rate) / delta_f;
    // A non-finite cost (NaN/inf from a degenerate model/rate) must sort LAST so
    // such a slot is never chosen — and so the sort itself stays a total order.
    if cost.is_finite() {
        cost
    } else {
        f64::INFINITY
    }
}

/// True iff, simulating `heat_on`, the projected temperature is `>= target_f` at
/// the start of every slot in `window_slots`.
fn comfort_met_across(
    ctx: &PlanContext,
    heat_on: &[bool],
    window_slots: &[usize],
    target_f: f64,
) -> bool {
    let traj = ctx.simulate(heat_on);
    window_slots
        .iter()
        .all(|&i| ctx.temp_at_slot_start(&traj, i) >= target_f - TARGET_EPSILON_F)
}

/// Tolerance (°F) on meeting a comfort target, absorbing floating-point slop so
/// a temperature that lands a hair under target isn't reported as a miss.
const TARGET_EPSILON_F: f64 = 1.0e-6;

/// Constant-setpoint baseline (spec §6): today's dumb behavior. Heat in any slot
/// whose *entry* temperature is below `setpoint_f`, across the whole horizon
/// (no comfort-window awareness, no drift). Returns the on/off schedule.
///
/// The decision is causal: each slot looks at the temperature it would start at
/// given the schedule built so far, so it never peeks at the future.
pub fn baseline_constant_setpoint(ctx: &PlanContext, setpoint_f: f64) -> Vec<bool> {
    let n = ctx.grid.num_slots;
    let mut heat_on = vec![false; n];
    for i in 0..n {
        // Simulate the schedule decided so far to read this slot's entry temp.
        let traj = ctx.simulate(&heat_on);
        if ctx.temp_at_slot_start(&traj, i) < setpoint_f - TARGET_EPSILON_F {
            heat_on[i] = true;
        }
    }
    heat_on
}

/// Build the full [`SavingsReport`]: run the optimizer and the constant-setpoint
/// baseline over the same context, cost both, evaluate comfort per window, and
/// assemble the advisory summary.
///
/// `baseline_setpoint_f` is the constant setpoint the baseline holds (typically
/// the max comfort target — what a naive "just keep it hot" user would set).
pub fn build_report(
    ctx: &PlanContext,
    windows: &[ComfortWindow],
    baseline_setpoint_f: f64,
) -> SavingsReport {
    let grid = &ctx.grid;

    let opt_schedule = optimize(ctx, windows);
    let opt_traj = ctx.simulate(&opt_schedule);
    let (optimizer_gas_therms, optimizer_pump_kwh, optimizer_usd) =
        cost_of(grid, &opt_traj, ctx.gas_rate, ctx.rates);

    let base_schedule = baseline_constant_setpoint(ctx, baseline_setpoint_f);
    let base_traj = ctx.simulate(&base_schedule);
    let (baseline_gas_therms, baseline_pump_kwh, baseline_usd) =
        cost_of(grid, &base_traj, ctx.gas_rate, ctx.rates);

    let savings_usd = baseline_usd - optimizer_usd;
    let savings_pct = if baseline_usd > 0.0 {
        savings_usd / baseline_usd
    } else {
        0.0
    };

    // Per-OCCURRENCE comfort under the optimizer's plan: each contiguous in-
    // horizon run of window slots is reported separately (so e.g. Sat and Sun of
    // a `["Sat","Sun"]` window are two outcomes), matching how `optimize` plans
    // them. The occurrence's start instant is its first slot's start.
    let mut comfort_met = Vec::new();
    for window in windows {
        for window_slots in window_occurrences(ctx, window) {
            let win_start = grid.slot_start_unix(window_slots[0]);
            let met = window_slots
                .iter()
                .all(|&i| ctx.temp_at_slot_start(&opt_traj, i) >= window.target_f - TARGET_EPSILON_F);
            comfort_met.push(ComfortOutcome {
                window_start_unix: win_start,
                target_f: window.target_f,
                met,
            });
        }
    }

    let schedule: Vec<(i64, bool)> = opt_schedule
        .iter()
        .enumerate()
        .map(|(i, &on)| (grid.slot_start_unix(i), on))
        .collect();
    let opt_on_slots = opt_schedule.iter().filter(|&&on| on).count();
    let all_met = comfort_met.iter().all(|o| o.met);

    let summary = format!(
        "Advisory plan (no actuation): {opt_on_slots} of {} slots heated. \
         Projected {optimizer_gas_therms:.1} therms + {optimizer_pump_kwh:.1} kWh pump / \
         ${optimizer_usd:.2} vs constant-{baseline_setpoint_f:.0}°F baseline \
         {baseline_gas_therms:.1} therms + {baseline_pump_kwh:.1} kWh pump / ${baseline_usd:.2} \
         \u{2192} saves ${savings_usd:.2} ({:.0}%). Comfort {}.",
        grid.num_slots,
        savings_pct * 100.0,
        if all_met { "met" } else { "NOT met" },
    );

    SavingsReport {
        schedule,
        trajectory: opt_traj.points,
        optimizer_gas_therms,
        optimizer_pump_kwh,
        optimizer_usd,
        baseline_gas_therms,
        baseline_pump_kwh,
        baseline_usd,
        savings_usd,
        savings_pct,
        comfort_met,
        summary,
    }
}

// ─── Config bridge (pure) ───────────────────────────────────────────────────
//
// Translate the daemon's `[comfort]`/`[gasheater]`/`[gas]`/`[rates]` config into
// the pure scheduler types. These are pure functions of config — no I/O, no clock — so
// they unit-test like the rest of the module. A malformed `HH:MM` or weekday is
// skipped/clamped rather than panicking, keeping a bad config from breaking the
// read-only plan endpoint.

/// Replace a non-finite (NaN/inf) float with `fallback`; pass finite values
/// through unchanged. Used to keep bad config or model params from poisoning the
/// advisory math (NaN propagation, NaN-breaks-ordering in the cost sort).
fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// Like [`finite_or`] but also clamps the (finite) result to be `>= 0.0` — for
/// quantities (rates, BTU ratings, gains) where a negative value is meaningless.
fn finite_nonneg_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        fallback
    }
}

impl GasHeaterModel {
    /// Build from `[gasheater]` config, sanitizing every field so a NaN/inf (or a
    /// negative-where-invalid) config value can't propagate into the advisory
    /// output. `rated_btu_per_hr` and `pump_kw` are clamped non-negative; the
    /// efficiency is clamped into `(0.5, 1.0]` after a finite check (falling back
    /// to the spec default when non-finite) so the gas-input math never divides
    /// by zero.
    pub fn from_config(c: &crate::config::GasHeaterConfig) -> Self {
        let def = Self::spec_default();
        let efficiency = if c.thermal_efficiency.is_finite() {
            c.thermal_efficiency.clamp(0.5, 1.0)
        } else {
            def.thermal_efficiency
        };
        Self {
            rated_btu_per_hr: finite_nonneg_or(c.rated_btu_per_hr, def.rated_btu_per_hr),
            thermal_efficiency: efficiency,
            pump_kw: finite_nonneg_or(c.pump_kw, def.pump_kw),
        }
    }
}

impl GasRate {
    /// Build from `[gas]` config, sanitizing the rate to a finite, non-negative
    /// `$/therm` so a bad config can't poison the cost math.
    pub fn from_config(c: &crate::config::GasConfig) -> Self {
        Self {
            usd_per_therm: finite_nonneg_or(c.usd_per_therm, 1.80),
        }
    }
}

/// Sanitize [`CoolingParams`] (fitted model params or config-seeded) so any
/// non-finite field is replaced with the physically-plausible seed value and
/// the inherently-non-negative coefficients are floored at zero. Advisory-only:
/// keeps a degenerate fit from producing NaN savings or panicking the sort.
pub fn sanitize_cooling_params(p: &CoolingParams) -> CoolingParams {
    let seed = CoolingParams::seed();
    CoolingParams {
        k0_per_hour: finite_nonneg_or(p.k0_per_hour, seed.k0_per_hour),
        k_wind_per_hour_per_mph: finite_nonneg_or(
            p.k_wind_per_hour_per_mph,
            seed.k_wind_per_hour_per_mph,
        ),
        evap_a: finite_nonneg_or(p.evap_a, seed.evap_a),
        evap_b: finite_nonneg_or(p.evap_b, seed.evap_b),
        solar_gain_f: finite_nonneg_or(p.solar_gain_f, seed.solar_gain_f),
        max_projection_hours: finite_nonneg_or(p.max_projection_hours, seed.max_projection_hours),
    }
}

/// Parse a list of 3-letter weekday abbreviations, dropping any that don't parse.
fn parse_days(days: &[String]) -> Vec<Weekday> {
    days.iter().filter_map(|d| Weekday::parse(d)).collect()
}

impl RateSchedule {
    /// Build a [`RateSchedule`] from `[rates]` config + the injected UTC offset.
    /// Periods whose `start`/`end` fail to parse are skipped (the flat default
    /// then applies for those instants).
    pub fn from_config(c: &crate::config::RatesConfig, utc_offset_seconds: i64) -> Self {
        // A non-finite or negative rate is meaningless and would poison the
        // cost-per-°F math (and its sort); clamp every rate to a finite `>= 0`.
        let periods = c
            .periods
            .iter()
            .filter_map(|p| {
                Some(RatePeriod {
                    days: parse_days(&p.days),
                    start: HhMm::parse(&p.start)?,
                    end: HhMm::parse(&p.end)?,
                    usd_per_kwh: finite_nonneg_or(p.usd_per_kwh, 0.0),
                })
            })
            .collect();
        Self {
            periods,
            default_usd_per_kwh: finite_nonneg_or(c.default_usd_per_kwh, 0.0),
            utc_offset_seconds,
        }
    }
}

impl ComfortWindow {
    /// Build a [`ComfortWindow`] from a `[[comfort.windows]]` entry, or `None`
    /// when its `start`/`end` fail to parse.
    pub fn from_config(c: &crate::config::ComfortWindowConfig) -> Option<Self> {
        Some(Self {
            days: parse_days(&c.days),
            start: HhMm::parse(&c.start)?,
            end: HhMm::parse(&c.end)?,
            // Sanitize: a NaN/inf target from TOML would make comfort comparisons
            // always-false/always-true and poison the savings math.
            target_f: finite_or(c.target_f, 80.0),
        })
    }
}

/// Build the comfort windows from config, dropping any that fail to parse.
pub fn comfort_windows_from_config(c: &crate::config::ComfortConfig) -> Vec<ComfortWindow> {
    c.windows.iter().filter_map(ComfortWindow::from_config).collect()
}

// ─── Evaluation harness (backtest) ──────────────────────────────────────────
//
// The POINT of v1 (spec §"Evaluation"): replay recorded weather *as if it were
// the forecast*, run the optimizer vs a constant-setpoint baseline over the
// horizon using the FITTED `(k, g)` (or the physics seed if unfit), and report
// projected energy + cost for each. There is NO real energy meter, so every
// kilowatt-hour and dollar here is **MODEL-PROJECTED** — the report carries that
// label so it can never be mistaken for a metered measurement. Still advisory:
// the backtest computes and reports; it actuates nothing.

/// Inputs to a backtest run. Every field is *injected* (weather, params, heater,
/// gas/elec rates, horizon, comfort windows) so the run is a pure function of its
/// inputs and unit-tests deterministically — the same purity contract as
/// `thermal.rs`.
#[derive(Debug, Clone)]
pub struct BacktestInput<'a> {
    /// Recorded weather replayed as the forecast: piecewise-constant segments
    /// (e.g. one per logged sample) covering the horizon. Built from the monitor
    /// log by the caller; the scheduler never reads a file.
    pub segments: &'a [WeatherSegment],
    /// Fitted cooling params `(k, g, …)` — or [`CoolingParams::seed`] if unfit.
    pub params: &'a CoolingParams,
    /// Gas heater model (from `[gasheater]`).
    pub heater: &'a GasHeaterModel,
    /// Flat gas price (from `[gas]`).
    pub gas_rate: &'a GasRate,
    /// Time-of-use electricity rates (from `[rates]`) — costs the pump only.
    pub rates: &'a RateSchedule,
    /// Comfort targets (from `[comfort].windows`).
    pub windows: &'a [ComfortWindow],
    /// Pool thermal mass (BTU/°F) from pool volume.
    pub thermal_mass_btu_per_f: f64,
    /// Starting water temperature (°F) at the horizon anchor.
    pub start_temp_f: f64,
    /// Horizon anchor (unix seconds) — typically the first replayed sample.
    pub anchor_unix: i64,
    /// Slot length (hours), e.g. `0.5`.
    pub slot_hours: f64,
    /// Horizon length (hours), e.g. `48.0`.
    pub horizon_hours: f64,
    /// Constant setpoint the baseline holds (°F). Typically the max comfort
    /// target — what a naive "just keep it hot" user would dial in.
    pub baseline_setpoint_f: f64,
}

/// A model-projected backtest result: the underlying [`SavingsReport`] plus the
/// horizon framing, explicitly flagged as projected-not-metered energy.
///
/// **Advisory / evaluation only.** Nothing here is a command; the schedule is
/// data describing what the optimizer *would* have done.
#[derive(Debug, Clone, PartialEq)]
pub struct BacktestReport {
    /// Optimizer-vs-baseline plan + projected kWh/$ + comfort outcomes.
    pub report: SavingsReport,
    /// Horizon anchor (unix seconds).
    pub anchor_unix: i64,
    /// Slot length (hours) and number of slots simulated.
    pub slot_hours: f64,
    pub num_slots: usize,
    /// Always `true`: the energy figures are MODEL-PROJECTED (no real meter).
    pub energy_is_model_projected: bool,
}

impl BacktestReport {
    /// True iff every evaluated comfort window was met by the optimizer.
    pub fn all_comfort_met(&self) -> bool {
        self.report.all_comfort_met()
    }
}

/// Run a backtest: replay the recorded weather as the forecast and evaluate the
/// optimizer against the constant-setpoint baseline. Pure — no I/O, no clock.
///
/// The number of slots is `ceil(horizon_hours / slot_hours)` (at least one); the
/// grid anchors at `anchor_unix`. The forward sim, optimizer, and baseline all
/// share the injected segments + fitted params, so the energy/cost comparison is
/// apples-to-apples. Energy is MODEL-PROJECTED (flagged on the report).
pub fn run_backtest(input: &BacktestInput) -> BacktestReport {
    let slot_hours = sanitize_slot_hours(input.slot_hours);
    let num_slots = bounded_num_slots(input.horizon_hours, slot_hours);

    let ctx = PlanContext {
        grid: SlotGrid::new(input.anchor_unix, slot_hours, num_slots),
        segments: input.segments,
        params: input.params,
        heater: input.heater,
        gas_rate: input.gas_rate,
        rates: input.rates,
        thermal_mass_btu_per_f: input.thermal_mass_btu_per_f,
        start_temp: input.start_temp_f,
    };

    let report = build_report(&ctx, input.windows, input.baseline_setpoint_f);

    BacktestReport {
        report,
        anchor_unix: input.anchor_unix,
        slot_hours,
        num_slots,
        energy_is_model_projected: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 3_600_000;

    fn cooling_only_segment(start_ms: i64, end_ms: i64, air: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start_ms,
            end_unix_ms: end_ms,
            air_temp_f: air,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: None,
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }

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

    // ----- GasHeaterModel -----

    #[test]
    fn gas_therms_matches_btu_over_efficiency() {
        let heater = GasHeaterModel::spec_default(); // 250k BTU/hr @ 0.82
        // gas input/hr = delivered / efficiency; therms/hr = that / 100_000.
        let expected = (250_000.0 / 0.82) / 100_000.0;
        assert!((heater.gas_therms_per_hr() - expected).abs() < 1e-9);
        let (btu, gas) = heater.heat_output();
        assert_eq!(btu, 250_000.0, "delivered thermal output is the rated BTU/hr");
        assert!((gas - expected).abs() < 1e-9);
        // A lower efficiency burns MORE gas for the same delivered heat.
        let inefficient = GasHeaterModel {
            thermal_efficiency: 0.60,
            ..heater
        };
        assert!(inefficient.gas_therms_per_hr() > heater.gas_therms_per_hr());
        // Efficiency is clamped into (0.5, 1.0] so a 0.0/NaN can't divide-by-zero.
        let degenerate = GasHeaterModel {
            thermal_efficiency: 0.0,
            ..heater
        };
        assert!(degenerate.gas_therms_per_hr().is_finite());
        assert!(degenerate.gas_therms_per_hr() > 0.0);
    }

    #[test]
    fn heat_output_is_constant_regardless_of_temp() {
        // A gas heater's delivered output and gas burn are CONSTANT — there is no
        // air/water dependence at all (heat_output takes no temperature args).
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let (btu, gas) = heater.heat_output();
        // Re-reading yields identical values; nothing varies it.
        assert_eq!(heater.heat_output(), (btu, gas));
        assert_eq!(btu, heater.rated_btu_per_hr);
        assert!(gas > 0.0);
    }

    #[test]
    fn delta_f_per_slot_matches_btu_over_mass() {
        let heater = GasHeaterModel::spec_default(); // 250k BTU/hr
        let mass = thermal_mass_btu_per_f(16_000.0); // gallons -> BTU/°F
        let delta = heater.delta_f_per_slot(0.5, mass); // half-hour slot
        let expected = (250_000.0 * 0.5) / mass;
        assert!((delta - expected).abs() < 1e-9);
        assert!(delta > 0.0);
        // Degenerate inputs are zero, not NaN.
        assert_eq!(heater.delta_f_per_slot(0.0, mass), 0.0);
        assert_eq!(heater.delta_f_per_slot(0.5, 0.0), 0.0);
    }

    #[test]
    fn cost_of_sums_gas_and_pump_terms() {
        // total_usd = Σ gas_therms*gas_rate + Σ pump_kwh*elec_rate(slot).
        // Anchor Mon 00:00 PST, hourly slots; flat $0.30/kWh electricity and
        // $1.80/therm gas. Heat two slots so both terms are exercised.
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 70.0, 4);
        let params = conduction_params(0.02);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = pool_mass();
        let heat_on = [true, false, true, false];
        let traj = forward_sim_with_heat(
            80.0, anchor, &segments, &params, &heat_on, &heater, mass, 1.0,
        );
        let grid = SlotGrid::new(anchor, 1.0, 4);
        let rates = RateSchedule::flat(0.30, pst());
        let (total_gas, total_pump, total_usd) = cost_of(&grid, &traj, &gas_rate, &rates);

        // Two heated 1h slots: gas = 2 * therms/hr, pump = 2 * pump_kw.
        let expected_gas = 2.0 * heater.gas_therms_per_hr();
        let expected_pump = 2.0 * heater.pump_kw;
        assert!((total_gas - expected_gas).abs() < 1e-9);
        assert!((total_pump - expected_pump).abs() < 1e-9);
        let expected_usd = expected_gas * 1.80 + expected_pump * 0.30;
        assert!((total_usd - expected_usd).abs() < 1e-9, "usd {total_usd} vs {expected_usd}");
    }

    // ----- RateSchedule -----

    fn pst() -> i64 {
        -8 * 3600
    }

    /// A unix instant for a known local weekday/time. 2024-01-01 was a Monday;
    /// PST midnight that day is unix 1_704_096_000.
    fn local_unix(days_after_mon: i64, hour: i64, minute: i64) -> i64 {
        // 2024-01-01T00:00:00 PST = 2024-01-01T08:00:00Z = 1_704_096_000.
        let mon_midnight_pst = 1_704_096_000i64;
        mon_midnight_pst + days_after_mon * SECONDS_PER_DAY + hour * 3600 + minute * 60
    }

    #[test]
    fn rate_selects_peak_offpeak_and_flat_default() {
        let schedule = RateSchedule {
            periods: vec![RatePeriod {
                days: vec![
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                start: HhMm::new(16, 0),
                end: HhMm::new(21, 0),
                usd_per_kwh: 0.55,
            }],
            default_usd_per_kwh: 0.30,
            utc_offset_seconds: pst(),
        };

        // Monday 17:00 local -> peak.
        assert_eq!(schedule.rate(local_unix(0, 17, 0)), 0.55);
        // Monday 10:00 local -> off-peak default.
        assert_eq!(schedule.rate(local_unix(0, 10, 0)), 0.30);
        // Saturday 17:00 local -> weekday peak does not apply -> default.
        assert_eq!(schedule.rate(local_unix(5, 17, 0)), 0.30);
    }

    #[test]
    fn rate_flat_default_when_no_periods() {
        let schedule = RateSchedule::flat(0.42, pst());
        assert_eq!(schedule.rate(local_unix(0, 3, 0)), 0.42);
        assert_eq!(schedule.rate(local_unix(5, 18, 0)), 0.42);
    }

    #[test]
    fn rate_boundaries_are_half_open() {
        let schedule = RateSchedule {
            periods: vec![RatePeriod {
                days: vec![Weekday::Mon],
                start: HhMm::new(16, 0),
                end: HhMm::new(21, 0),
                usd_per_kwh: 0.55,
            }],
            default_usd_per_kwh: 0.30,
            utc_offset_seconds: pst(),
        };
        // 16:00 included, 21:00 excluded.
        assert_eq!(schedule.rate(local_unix(0, 16, 0)), 0.55);
        assert_eq!(schedule.rate(local_unix(0, 21, 0)), 0.30);
        assert_eq!(schedule.rate(local_unix(0, 20, 59)), 0.55);
    }

    // ----- Weekday / local time -----

    #[test]
    fn local_time_weekday_is_correct() {
        // 2024-01-01 was a Monday (local PST).
        assert_eq!(local_time(local_unix(0, 12, 0), pst()).weekday, Weekday::Mon);
        assert_eq!(local_time(local_unix(5, 12, 0), pst()).weekday, Weekday::Sat);
        assert_eq!(local_time(local_unix(6, 12, 0), pst()).weekday, Weekday::Sun);
    }

    #[test]
    fn weekday_parse_roundtrips() {
        assert_eq!(Weekday::parse("Sat"), Some(Weekday::Sat));
        assert_eq!(Weekday::parse("sun"), Some(Weekday::Sun));
        assert_eq!(Weekday::parse("FRI"), Some(Weekday::Fri));
        assert_eq!(Weekday::parse("xyz"), None);
    }

    #[test]
    fn hhmm_parse_valid_and_invalid() {
        assert_eq!(HhMm::parse("16:00").unwrap().minute_of_day, 16 * 60);
        assert_eq!(HhMm::parse("24:00").unwrap().minute_of_day, 1440);
        assert!(HhMm::parse("25:00").is_none());
        assert!(HhMm::parse("noon").is_none());
    }

    // ----- ComfortWindow -----

    fn weekend_window() -> ComfortWindow {
        ComfortWindow {
            days: vec![Weekday::Sat, Weekday::Sun],
            start: HhMm::new(15, 0),
            end: HhMm::new(20, 0),
            target_f: 88.0,
        }
    }

    #[test]
    fn comfort_window_membership() {
        let w = weekend_window();
        // Saturday 16:00 local -> inside.
        assert!(w.contains(local_unix(5, 16, 0), pst()));
        // Saturday 21:00 local -> outside (after end).
        assert!(!w.contains(local_unix(5, 21, 0), pst()));
        // Monday 16:00 local -> wrong day.
        assert!(!w.contains(local_unix(0, 16, 0), pst()));
        // Sunday 15:00 local -> inside (start inclusive).
        assert!(w.contains(local_unix(6, 15, 0), pst()));
    }

    #[test]
    fn comfort_window_next_start_after() {
        let w = weekend_window();
        // From Monday noon, the next window start is the upcoming Saturday 15:00.
        let from = local_unix(0, 12, 0);
        let next = w.next_start_after(from, pst(), 14).expect("a window within 2 weeks");
        let lt = local_time(next, pst());
        assert_eq!(lt.weekday, Weekday::Sat);
        assert_eq!(lt.minute_of_day, 15 * 60);
        assert!(next > from);
    }

    #[test]
    fn comfort_window_empty_days_has_no_next() {
        let w = ComfortWindow {
            days: vec![],
            start: HhMm::new(15, 0),
            end: HhMm::new(20, 0),
            target_f: 88.0,
        };
        assert_eq!(w.next_start_after(local_unix(0, 12, 0), pst(), 14), None);
    }

    // ----- forward_sim_with_heat -----

    #[test]
    fn passive_slot_matches_thermal_rs_exactly() {
        // With heat_on = [false], the forward sim's single passive slot must
        // reproduce thermal.rs's relaxation for the same segment/params/dt.
        let params = conduction_params(0.1);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(16_000.0);
        let anchor_unix = 1_704_096_000; // arbitrary; segment brackets the slot
        let slot_hours = 1.0;
        let seg = cooling_only_segment(
            anchor_unix * 1000,
            anchor_unix * 1000 + HOUR_MS,
            70.0,
        );
        let traj = forward_sim_with_heat(
            85.0,
            anchor_unix,
            std::slice::from_ref(&seg),
            &params,
            &[false],
            &heater,
            mass,
            slot_hours,
        );
        // Reference straight from thermal.rs's shared relaxation.
        let expected = thermal::passive_relax_over_segment(85.0, &seg, &params, slot_hours);
        let (_t, got) = traj.points.last().copied().unwrap();
        assert!((got - expected).abs() < 1e-12, "passive {got} vs thermal {expected}");
        // Passive slot draws no electricity.
        assert_eq!(traj.pump_kwh, vec![0.0]);
        // And it actually cooled toward air.
        assert!(got < 85.0 && got > 70.0);
    }

    #[test]
    fn heated_slot_raises_temp_by_about_delta_f() {
        // A heated slot should be warmer than the same slot passive, by ~delta_f
        // (passive change is tiny here so the rise is dominated by the heater).
        let params = conduction_params(0.02);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(16_000.0);
        let anchor_unix = 1_704_096_000;
        let slot_hours = 0.5;
        let seg = cooling_only_segment(
            anchor_unix * 1000,
            anchor_unix * 1000 + HOUR_MS,
            60.0,
        );

        let passive = forward_sim_with_heat(
            82.0, anchor_unix, std::slice::from_ref(&seg), &params, &[false], &heater, mass, slot_hours,
        );
        let heated = forward_sim_with_heat(
            82.0, anchor_unix, std::slice::from_ref(&seg), &params, &[true], &heater, mass, slot_hours,
        );

        let passive_end = passive.points.last().unwrap().1;
        let heated_end = heated.points.last().unwrap().1;
        let delta = heater.delta_f_per_slot(slot_hours, mass);
        assert!((heated_end - (passive_end + delta)).abs() < 1e-9);
        assert!(heated_end > passive_end, "heating raises temperature");
        // Heated slot draws positive pump electricity.
        assert!(heated.pump_kwh[0] > 0.0);
    }

    #[test]
    fn trajectory_has_one_point_per_slot_plus_start() {
        let params = conduction_params(0.05);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(16_000.0);
        let anchor_unix = 1_704_096_000;
        let slot_hours = 0.5;
        let mut segs = Vec::new();
        for i in 0..4 {
            let s = anchor_unix * 1000 + i * HOUR_MS;
            segs.push(cooling_only_segment(s, s + HOUR_MS, 65.0));
        }
        let heat_on = [true, false, true, false];
        let traj = forward_sim_with_heat(
            80.0, anchor_unix, &segs, &params, &heat_on, &heater, mass, slot_hours,
        );
        assert_eq!(traj.points.len(), heat_on.len() + 1);
        assert_eq!(traj.pump_kwh.len(), heat_on.len());
        // Off slots draw nothing; on slots draw something.
        assert_eq!(traj.pump_kwh[1], 0.0);
        assert_eq!(traj.pump_kwh[3], 0.0);
        assert!(traj.pump_kwh[0] > 0.0 && traj.pump_kwh[2] > 0.0);
    }

    // ----- Phase 2: optimizer / baseline / report -----

    // A fixed N-hemisphere site (Los Altos, CA) so solar-enabled horizons see
    // the sun near local solar noon (~20:00 UTC).
    const SITE_LAT: f64 = 37.38;
    const SITE_LON: f64 = -122.11;

    /// One hourly solar-enabled segment for the test site, given cloud + cover.
    fn solar_hour_segment(start_unix: i64, air: f64, cloud: f64, cover: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start_unix * 1000,
            end_unix_ms: (start_unix + 3600) * 1000,
            air_temp_f: air,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: Some(cloud),
            latitude_deg: SITE_LAT,
            longitude_deg: SITE_LON,
            cover_solar_transmission: cover,
        }
    }

    /// Build `n` consecutive hourly cooling-only segments from `anchor_unix`.
    fn hourly_cooling_horizon(anchor_unix: i64, air: f64, n: usize) -> Vec<WeatherSegment> {
        (0..n)
            .map(|i| {
                let s = anchor_unix + (i as i64) * 3600;
                cooling_only_segment(s * 1000, (s + 3600) * 1000, air)
            })
            .collect()
    }

    fn flat_rates() -> RateSchedule {
        RateSchedule::flat(0.30, pst())
    }

    /// Standard pool mass used across the Phase 2 tests (~16k gal).
    fn pool_mass() -> f64 {
        thermal_mass_btu_per_f(16_000.0)
    }

    #[test]
    fn optimizer_meets_a_comfort_target() {
        // Anchor at Mon 00:00 PST; a comfort window Mon 06:00–08:00 @ 85°F, with
        // hourly slots. The pool starts at 78°F and air is cool (70°F), so it
        // needs pre-heating to reach target.
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 70.0, 12);
        let params = conduction_params(0.02);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        // Smaller pool so the heater can move a few °F over the lead time.
        let mass = thermal_mass_btu_per_f(5_000.0);
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: mass,
            start_temp: 80.0,
        };
        let window = ComfortWindow {
            days: vec![Weekday::Mon],
            start: HhMm::new(6, 0),
            end: HhMm::new(8, 0),
            target_f: 84.0,
        };
        let schedule = optimize(&ctx, std::slice::from_ref(&window));
        let traj = ctx.simulate(&schedule);

        // Every slot inside the window must be at/above the window's target.
        for i in 0..12 {
            if window.contains(ctx.grid.slot_start_unix(i), pst()) {
                let t = ctx.temp_at_slot_start(&traj, i);
                assert!(
                    t >= window.target_f - 1e-6,
                    "window slot {i} below target {}: {t}",
                    window.target_f
                );
            }
        }
        // It actually had to heat something.
        assert!(schedule.iter().any(|&on| on), "should have scheduled heating");
    }

    #[test]
    fn optimizer_heats_zero_outside_windows() {
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 70.0, 12);
        let params = conduction_params(0.05);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: pool_mass(),
            start_temp: 78.0,
        };
        let window = ComfortWindow {
            days: vec![Weekday::Mon],
            start: HhMm::new(6, 0),
            end: HhMm::new(8, 0),
            target_f: 85.0,
        };
        let schedule = optimize(&ctx, std::slice::from_ref(&window));
        // No slot AT or AFTER the window start may be heated: pre-heat only.
        let win_start = window.next_start_after(anchor - 1, pst(), 14).unwrap();
        for (i, &on) in schedule.iter().enumerate() {
            if ctx.grid.slot_start_unix(i) >= win_start {
                assert!(!on, "slot {i} at/after window start must not heat");
            }
        }
        // And with NO windows at all, nothing is heated anywhere.
        let empty = optimize(&ctx, &[]);
        assert!(empty.iter().all(|&on| !on), "no windows => no heating (drift)");
    }

    #[test]
    fn sunny_forecast_skips_or_minimizes_heating_and_saves() {
        // A warm, sunny daytime horizon: solar carries the pool to target for
        // free, so the optimizer should heat far less than a constant setpoint
        // (and may heat nothing). Anchor near local sunrise so the window sits in
        // strong midday sun. 2024-06-20 is day-of-year ~172.
        // 2024-06-20T13:00:00Z = 1_718_888_400 (≈ 06:00 local solar-ish morning).
        let anchor = 1_718_884_800i64; // 2024-06-20T12:00:00Z
        let air = 82.0;
        let segments: Vec<WeatherSegment> = (0..12)
            .map(|i| solar_hour_segment(anchor + i * 3600, air, 0.0, 0.75))
            .collect();
        let params = solar_params_local(0.03, 0.30);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = pool_mass();

        // Window in strong midday sun (~5–7h after anchor => ~local noon).
        let win_start_unix = anchor + 5 * 3600;
        let win_local = local_time(win_start_unix, pst());
        let window = ComfortWindow {
            days: vec![win_local.weekday],
            start: HhMm {
                minute_of_day: win_local.minute_of_day,
            },
            end: HhMm {
                minute_of_day: win_local.minute_of_day + 120,
            },
            target_f: 84.0,
        };

        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: mass,
            start_temp: 83.0,
        };

        let report = build_report(&ctx, std::slice::from_ref(&window), 88.0);
        // Solar does the work: the optimizer heats fewer slots than baseline and
        // costs strictly less, while still meeting comfort.
        let opt_on = report.schedule.iter().filter(|(_, on)| *on).count();
        assert!(report.all_comfort_met(), "solar-assisted comfort should be met");
        assert!(
            report.optimizer_usd < report.baseline_usd,
            "optimizer must beat constant-setpoint on a sunny day: opt {} vs base {}",
            report.optimizer_usd,
            report.baseline_usd
        );
        assert!(report.savings_usd > 0.0 && report.savings_pct > 0.0);
        // Baseline holds 88°F all day; optimizer rides the sun to a 84°F target.
        let base_on = baseline_constant_setpoint(&ctx, 88.0)
            .iter()
            .filter(|&&on| on)
            .count();
        assert!(opt_on < base_on, "optimizer heats fewer slots: {opt_on} vs {base_on}");
    }

    #[test]
    fn optimizer_cost_leq_baseline_with_comfort_met() {
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 72.0, 12);
        let params = conduction_params(0.02);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(5_000.0);
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: mass,
            start_temp: 80.0,
        };
        let window = ComfortWindow {
            days: vec![Weekday::Mon],
            start: HhMm::new(8, 0),
            end: HhMm::new(10, 0),
            target_f: 84.0,
        };
        // Realistic baseline: a naive user holds a constant setpoint at/above the
        // comfort target the whole horizon, doing redundant work outside the
        // window. The window-only optimizer must not cost more.
        let report = build_report(&ctx, std::slice::from_ref(&window), 86.0);
        assert!(report.all_comfort_met(), "comfort must be met");
        assert!(
            report.optimizer_usd <= report.baseline_usd + 1e-9,
            "optimizer cost must not exceed baseline: opt {} vs base {}",
            report.optimizer_usd,
            report.baseline_usd
        );
        // The summary is advisory and self-describes (no actuation language).
        assert!(report.summary.contains("no actuation"));
    }

    #[test]
    fn cost_per_degf_ranking_prefers_cheap_offpeak_slots() {
        // Two pre-heat slots available before a window; one falls in an expensive
        // peak rate, the other off-peak. With equal COP (same air), the optimizer
        // must pick the cheaper off-peak slot first.
        //
        // Window Mon 18:00–19:00; lead slots at 16:00 (peak) and 17:00 (peak) vs
        // earlier off-peak hours. We make 17:00–21:00 a steep peak so the slots
        // right before the window are the EXPENSIVE ones, forcing the optimizer
        // to reach back to a cheaper earlier slot when one heat slot suffices.
        let anchor = local_unix(0, 12, 0); // Mon 12:00 PST, hourly slots
        let segments = hourly_cooling_horizon(anchor, 60.0, 12);
        let params = conduction_params(0.005); // very slow drift -> pre-heat holds
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let rates = RateSchedule {
            periods: vec![RatePeriod {
                days: vec![Weekday::Mon],
                start: HhMm::new(16, 0),
                end: HhMm::new(21, 0),
                usd_per_kwh: 5.00, // steep peak
            }],
            default_usd_per_kwh: 0.10,
            utc_offset_seconds: pst(),
        };
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &rates,
            thermal_mass_btu_per_f: pool_mass(),
            start_temp: 84.7, // just shy of target; one slot of heat suffices
        };
        let window = ComfortWindow {
            days: vec![Weekday::Mon],
            start: HhMm::new(18, 0),
            end: HhMm::new(19, 0),
            target_f: 85.0,
        };
        let schedule = optimize(&ctx, std::slice::from_ref(&window));
        // The chosen heated slot(s) must be off-peak (before 16:00 local), not
        // the cheap-to-reach-but-expensive peak slots right before the window.
        for (i, &on) in schedule.iter().enumerate() {
            if on {
                let lt = local_time(ctx.grid.slot_start_unix(i), pst());
                assert!(
                    lt.minute_of_day < 16 * 60,
                    "optimizer picked an expensive peak slot at minute {} instead of off-peak",
                    lt.minute_of_day
                );
            }
        }
        assert!(schedule.iter().any(|&on| on), "should heat exactly the cheap slot");
        // And comfort is still met.
        let traj = ctx.simulate(&schedule);
        for i in 0..12 {
            if window.contains(ctx.grid.slot_start_unix(i), pst()) {
                assert!(ctx.temp_at_slot_start(&traj, i) >= 85.0 - 1e-6);
            }
        }
    }

    #[test]
    fn baseline_heats_whenever_below_setpoint() {
        // Cold air, no solar, starting below setpoint: the baseline must heat the
        // early slots (it holds the setpoint), unlike the window-only optimizer.
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 55.0, 6);
        let params = conduction_params(0.1);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 6),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: pool_mass(),
            start_temp: 70.0,
        };
        let base = baseline_constant_setpoint(&ctx, 85.0);
        assert!(base[0], "starts below setpoint -> first slot heats");
        assert!(base.iter().filter(|&&on| on).count() >= 1);
    }

    /// Solar params for the local test module (mirrors thermal.rs's helper).
    fn solar_params_local(k0: f64, g: f64) -> CoolingParams {
        CoolingParams {
            k0_per_hour: k0,
            k_wind_per_hour_per_mph: 0.0,
            evap_a: 0.0,
            evap_b: 0.0,
            solar_gain_f: g,
            max_projection_hours: 48.0,
        }
    }

    // ----- Phase 3: config bridge -----

    #[test]
    fn gas_heater_model_from_config_maps_fields() {
        let c = crate::config::GasHeaterConfig {
            rated_btu_per_hr: 300_000.0,
            thermal_efficiency: 0.85,
            pump_kw: 1.1,
        };
        let heater = GasHeaterModel::from_config(&c);
        assert_eq!(heater.rated_btu_per_hr, 300_000.0);
        assert_eq!(heater.thermal_efficiency, 0.85);
        assert_eq!(heater.pump_kw, 1.1);

        // The flat gas rate maps straight through (sanitized non-negative).
        let gc = crate::config::GasConfig { usd_per_therm: 2.05 };
        assert_eq!(GasRate::from_config(&gc).cost(), 2.05);
    }

    #[test]
    fn rate_schedule_from_config_parses_periods_and_skips_bad() {
        let c = crate::config::RatesConfig {
            periods: vec![
                crate::config::RatePeriodConfig {
                    days: vec!["Mon".into(), "Fri".into()],
                    start: "16:00".into(),
                    end: "21:00".into(),
                    usd_per_kwh: 0.55,
                },
                // Unparseable start => skipped.
                crate::config::RatePeriodConfig {
                    days: vec!["Sat".into()],
                    start: "nope".into(),
                    end: "21:00".into(),
                    usd_per_kwh: 9.99,
                },
            ],
            default_usd_per_kwh: 0.30,
        };
        let sched = RateSchedule::from_config(&c, pst());
        assert_eq!(sched.periods.len(), 1, "bad period dropped");
        assert_eq!(sched.default_usd_per_kwh, 0.30);
        assert_eq!(sched.utc_offset_seconds, pst());
        // Monday 17:00 local hits the parsed peak.
        assert_eq!(sched.rate(local_unix(0, 17, 0)), 0.55);
        // Off-peak falls back to default.
        assert_eq!(sched.rate(local_unix(0, 10, 0)), 0.30);
    }

    #[test]
    fn comfort_windows_from_config_parses_and_skips_bad() {
        let c = crate::config::ComfortConfig {
            actuate: false,
            windows: vec![
                crate::config::ComfortWindowConfig {
                    days: vec!["Sat".into(), "Sun".into()],
                    start: "15:00".into(),
                    end: "20:00".into(),
                    target_f: 88.0,
                },
                // Bad end => dropped.
                crate::config::ComfortWindowConfig {
                    days: vec!["Mon".into()],
                    start: "10:00".into(),
                    end: "??:??".into(),
                    target_f: 80.0,
                },
            ],
            utc_offset_seconds: pst(),
            slot_hours: 0.5,
            horizon_hours: 48.0,
            baseline_setpoint_f: None,
        };
        let windows = comfort_windows_from_config(&c);
        assert_eq!(windows.len(), 1, "unparseable window dropped");
        assert_eq!(windows[0].target_f, 88.0);
        assert_eq!(windows[0].days, vec![Weekday::Sat, Weekday::Sun]);
        assert_eq!(windows[0].start.minute_of_day, 15 * 60);
    }

    // ----- Phase 4: evaluation harness (backtest) -----

    /// Build a 48 h horizon of hourly solar-enabled segments for the test site,
    /// starting at `anchor_unix`, with a fixed warm air temp and clear skies.
    /// Clear (`cloud = 0`) + a typical solar cover (`cover = 0.75`) makes it a
    /// genuinely "sunny" forecast that drives the solar term.
    fn sunny_48h_horizon(anchor_unix: i64, air_f: f64, n: usize) -> Vec<WeatherSegment> {
        (0..n)
            .map(|i| solar_hour_segment(anchor_unix + (i as i64) * 3600, air_f, 0.0, 0.75))
            .collect()
    }

    #[test]
    fn backtest_sunny_weekend_meets_88f_cheaper_than_holding_setpoint() {
        // SYNTHETIC-BUT-REALISTIC deterministic scenario (spec §Tests):
        //   * A warm, sunny weekend: 2024-06-22 is a Saturday; anchor at local
        //     midnight PST (unix 1_719_043_200), 48 hourly slots of clear-sky sun.
        //   * A Sat 15:00–20:00 comfort window targeting 88 °F.
        //   * A PG&E-like time-of-use rate: peak 16:00–21:00 @ $0.55/kWh, else
        //     $0.30/kWh off-peak — so heating during/just-before the window is
        //     expensive, and the cheap hours are the sunny pre-window morning.
        //
        // Assertions:
        //   (a) the optimizer MEETS 88 °F across the whole window,
        //   (b) it costs strictly LESS than holding 88 °F all 48 h,
        //   (c) it does its heating in the cheap/warm pre-window hours (or skips
        //       when solar suffices) — never in the peak window itself.
        let anchor = 1_719_043_200i64; // 2024-06-22 00:00 PST (a Saturday)
        let air = 85.0; // warm summer day
        let segments = sunny_48h_horizon(anchor, air, 48);

        // Fitted-style params: a slow covered pool (k ≈ seed) with real solar
        // gain `g`. Using the seed here stands in for "the fitted (k, g)".
        let params = solar_params_local(0.02, 0.30);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(20_000.0);

        let window = ComfortWindow {
            days: vec![Weekday::Sat],
            start: HhMm::new(15, 0),
            end: HhMm::new(20, 0),
            target_f: 88.0,
        };
        // PG&E-like peak 16:00–21:00 every day; off-peak default otherwise.
        let rates = RateSchedule {
            periods: vec![RatePeriod {
                days: vec![
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                    Weekday::Sat,
                    Weekday::Sun,
                ],
                start: HhMm::new(16, 0),
                end: HhMm::new(21, 0),
                usd_per_kwh: 0.55,
            }],
            default_usd_per_kwh: 0.30,
            utc_offset_seconds: pst(),
        };

        let input = BacktestInput {
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &rates,
            windows: std::slice::from_ref(&window),
            thermal_mass_btu_per_f: mass,
            start_temp_f: 80.0, // starts a few °F below target
            anchor_unix: anchor,
            slot_hours: 1.0,
            horizon_hours: 48.0,
            baseline_setpoint_f: 88.0, // naive user holds 88 °F all 48 h
        };

        let bt = run_backtest(&input);
        let report = &bt.report;

        // Energy is honestly labeled as projected, not metered.
        assert!(bt.energy_is_model_projected);
        assert_eq!(bt.num_slots, 48);

        // (a) 88 °F is met across the whole window.
        assert!(
            bt.all_comfort_met(),
            "optimizer must meet 88°F across the window; outcomes: {:?}",
            report.comfort_met
        );

        // (b) strictly cheaper than holding the setpoint all 48 h.
        assert!(
            report.optimizer_usd < report.baseline_usd,
            "optimizer (${:.2}) must cost strictly less than the constant-88°F \
             baseline (${:.2})",
            report.optimizer_usd,
            report.baseline_usd
        );
        assert!(report.savings_usd > 0.0 && report.savings_pct > 0.0);

        // (c) no heating inside the peak comfort window — the optimizer pre-heats
        // in cheap/sunny earlier hours (or coasts on solar), never during peak.
        let grid = SlotGrid::new(anchor, 1.0, 48);
        for &(unix, on) in &report.schedule {
            if on {
                assert!(
                    !window.contains(unix, pst()),
                    "optimizer must not heat inside the comfort window (peak): {unix}"
                );
                // Any heating it does is off-peak (before the 16:00 peak start),
                // i.e. the cheap warm pre-window hours.
                assert_eq!(
                    rates.rate(unix),
                    0.30,
                    "optimizer should heat only in off-peak hours, not peak"
                );
            }
        }

        // The optimizer heats no more slots than the constant-setpoint baseline.
        let base = baseline_constant_setpoint(
            &PlanContext {
                grid,
                segments: &segments,
                params: &params,
                heater: &heater,
                gas_rate: &gas_rate,
                rates: &rates,
                thermal_mass_btu_per_f: mass,
                start_temp: 80.0,
            },
            88.0,
        );
        let opt_on = report.schedule.iter().filter(|(_, on)| *on).count();
        let base_on = base.iter().filter(|&&on| on).count();
        assert!(
            opt_on <= base_on,
            "optimizer heats no more slots than baseline: {opt_on} vs {base_on}"
        );

        // Surface the example SavingsReport numbers this deterministic scenario
        // produces (visible with `cargo test -- --nocapture`).
        println!(
            "[backtest example] optimizer {:.1} therms + {:.1} kWh pump / ${:.2}  vs  \
             baseline {:.1} therms + {:.1} kWh pump / ${:.2}  \
             => saves ${:.2} ({:.1}%), {} of {} slots heated, comfort {}",
            report.optimizer_gas_therms,
            report.optimizer_pump_kwh,
            report.optimizer_usd,
            report.baseline_gas_therms,
            report.baseline_pump_kwh,
            report.baseline_usd,
            report.savings_usd,
            report.savings_pct * 100.0,
            opt_on,
            bt.num_slots,
            if bt.all_comfort_met() { "MET" } else { "NOT MET" },
        );
    }

    #[test]
    fn backtest_horizon_slot_count_and_projected_flag() {
        // ceil(horizon/slot) slots, at least one; energy flagged projected.
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 70.0, 6);
        let params = conduction_params(0.05);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let input = BacktestInput {
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            windows: &[],
            thermal_mass_btu_per_f: pool_mass(),
            start_temp_f: 78.0,
            anchor_unix: anchor,
            slot_hours: 0.5,
            horizon_hours: 3.0,
            baseline_setpoint_f: 85.0,
        };
        let bt = run_backtest(&input);
        assert_eq!(bt.num_slots, 6); // 3h / 0.5h
        assert!(bt.energy_is_model_projected);
        // No windows => optimizer heats nothing (drift), so it can only be cheaper.
        assert!(bt.report.optimizer_usd <= bt.report.baseline_usd + 1e-9);
        assert!(bt.report.schedule.iter().all(|(_, on)| !*on));
    }

    // ----- Robustness fixes (P2) -----

    #[test]
    fn absurd_slot_config_yields_bounded_grid_no_panic() {
        // FIX 1: a pathological `slot_hours` (0.0 or 1e-9) with a 48h horizon must
        // never produce an enormous grid or overflow the timestamp math. The
        // helpers clamp the slot to the floor and the count to the ceiling, and
        // the (saturating) `SlotGrid` math stays finite.
        let anchor = local_unix(0, 0, 0);

        // slot_hours = 0.0 -> default (0.5h) -> 96 slots over 48h.
        assert_eq!(sanitize_slot_hours(0.0), DEFAULT_SLOT_HOURS);
        assert_eq!(bounded_num_slots(48.0, 0.0), 96);

        // slot_hours = 1e-9 -> floored to MIN_SLOT_HOURS (0.05h); 48h/0.05h = 960
        // slots — bounded, well under the ceiling.
        let slot = sanitize_slot_hours(1e-9);
        assert_eq!(slot, MIN_SLOT_HOURS);
        let n = bounded_num_slots(48.0, 1e-9);
        assert_eq!(n, 960);
        assert!(n <= MAX_NUM_SLOTS);

        // A horizon long enough to exceed the ceiling at the floor slot length is
        // hard-capped (0.05h * 4096 = 204.8h, so 10_000h must clamp to the cap).
        assert_eq!(bounded_num_slots(10_000.0, 1e-9), MAX_NUM_SLOTS);
        assert_eq!(bounded_num_slots(f64::MAX, 0.5), MAX_NUM_SLOTS);

        // NaN / inf slot or horizon all stay bounded.
        assert_eq!(sanitize_slot_hours(f64::NAN), DEFAULT_SLOT_HOURS);
        assert_eq!(sanitize_slot_hours(f64::INFINITY), DEFAULT_SLOT_HOURS);
        assert!((1..=MAX_NUM_SLOTS).contains(&bounded_num_slots(f64::INFINITY, 0.5)));
        assert!((1..=MAX_NUM_SLOTS).contains(&bounded_num_slots(f64::NAN, 1e-9)));

        // The grid built from the capped count never overflows i64 timestamps:
        // the last slot start is finite and monotonic, no panic.
        let grid = SlotGrid::new(anchor, slot, n);
        let first = grid.slot_start_unix(0);
        let last = grid.slot_start_unix(n - 1);
        assert_eq!(first, anchor);
        assert!(last >= first, "timestamps stay monotonic and finite");

        // And a full backtest with the absurd config runs without panicking and
        // reports a bounded slot count.
        let segments = hourly_cooling_horizon(anchor, 70.0, 6);
        let params = conduction_params(0.05);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let input = BacktestInput {
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            windows: &[],
            thermal_mass_btu_per_f: pool_mass(),
            start_temp_f: 78.0,
            anchor_unix: anchor,
            slot_hours: 1e-9,
            horizon_hours: 48.0,
            baseline_setpoint_f: 85.0,
        };
        let bt = run_backtest(&input);
        assert!(bt.num_slots <= MAX_NUM_SLOTS && bt.num_slots >= 1);
        assert!(bt.slot_hours >= MIN_SLOT_HOURS);
    }

    #[test]
    fn nonfinite_rate_and_heater_do_not_panic_or_produce_nan() {
        // A NaN default rate and a non-finite gas-heater config must be sanitized
        // so the advisory savings stay finite and the cost-per-°F sort never
        // panics.
        let anchor = local_unix(0, 0, 0);
        let segments = hourly_cooling_horizon(anchor, 70.0, 12);
        let params = conduction_params(0.02);

        // A gas-heater config riddled with non-finite values -> spec defaults
        // (and a degenerate efficiency clamps rather than divides by zero).
        let bad_heater_cfg = crate::config::GasHeaterConfig {
            rated_btu_per_hr: f64::INFINITY,
            thermal_efficiency: f64::NAN,
            pump_kw: f64::INFINITY,
        };
        let heater = GasHeaterModel::from_config(&bad_heater_cfg);
        let def = GasHeaterModel::spec_default();
        assert_eq!(heater.rated_btu_per_hr, def.rated_btu_per_hr);
        assert_eq!(heater.thermal_efficiency, def.thermal_efficiency);
        assert_eq!(heater.pump_kw, def.pump_kw);
        // Gas burn is finite and positive for any config now.
        let (_btu, gas) = heater.heat_output();
        assert!(gas.is_finite() && gas > 0.0);

        // A non-finite gas price is sanitized to a finite, non-negative rate.
        let bad_gas_cfg = crate::config::GasConfig {
            usd_per_therm: f64::NAN,
        };
        let gas_rate = GasRate::from_config(&bad_gas_cfg);
        assert!(gas_rate.cost().is_finite());

        // A rates config with a NaN default and an inf period rate -> sanitized.
        let bad_rates_cfg = crate::config::RatesConfig {
            periods: vec![crate::config::RatePeriodConfig {
                days: vec!["Mon".into()],
                start: "16:00".into(),
                end: "21:00".into(),
                usd_per_kwh: f64::INFINITY,
            }],
            default_usd_per_kwh: f64::NAN,
        };
        let rates = RateSchedule::from_config(&bad_rates_cfg, pst());
        assert!(rates.rate(local_unix(0, 17, 0)).is_finite());
        assert!(rates.rate(local_unix(0, 10, 0)).is_finite());

        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 12),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &rates,
            thermal_mass_btu_per_f: thermal_mass_btu_per_f(5_000.0),
            start_temp: 80.0,
        };
        let window = ComfortWindow {
            days: vec![Weekday::Mon],
            start: HhMm::new(6, 0),
            end: HhMm::new(8, 0),
            target_f: 84.0,
        };
        // The optimizer sort must not panic, and the report's numbers stay finite.
        let report = build_report(&ctx, std::slice::from_ref(&window), 86.0);
        assert!(report.optimizer_usd.is_finite());
        assert!(report.baseline_usd.is_finite());
        assert!(report.savings_usd.is_finite(), "savings must not be NaN/inf");
        assert!(report.savings_pct.is_finite());

        // FIX 2 also covers a directly-constructed NaN-laden cooling params: the
        // sanitizer replaces every non-finite field with the seed.
        let bad_params = CoolingParams {
            k0_per_hour: f64::NAN,
            k_wind_per_hour_per_mph: f64::INFINITY,
            evap_a: f64::NAN,
            evap_b: -1.0,
            solar_gain_f: f64::NAN,
            max_projection_hours: f64::INFINITY,
        };
        let clean = sanitize_cooling_params(&bad_params);
        let seed = CoolingParams::seed();
        assert_eq!(clean.k0_per_hour, seed.k0_per_hour);
        assert_eq!(clean.solar_gain_f, seed.solar_gain_f);
        assert_eq!(clean.max_projection_hours, seed.max_projection_hours);
        assert_eq!(clean.evap_b, 0.0, "negative coefficient floored at zero");
    }

    #[test]
    fn multi_day_window_groups_into_independent_occurrences() {
        // FIX 3: a 48h horizon with a Sat 15:00–20:00 + Sun 15:00–20:00 @ 88°F
        // window must yield TWO occurrences, and Sunday's pre-heating must use
        // only Sunday lead-time slots — never Saturday's (which would let the
        // Saturday-first-slot serve as Sunday's lead-time boundary, the bug).
        let anchor = 1_719_043_200i64; // 2024-06-22 00:00 PST (a Saturday)
        // Cool, no-solar air so the pool actually needs pre-heating both days.
        let segments = hourly_cooling_horizon(anchor, 70.0, 48);
        let params = conduction_params(0.02);
        let heater = GasHeaterModel::spec_default();
        let gas_rate = GasRate::flat(1.80);
        let mass = thermal_mass_btu_per_f(5_000.0);
        let ctx = PlanContext {
            grid: SlotGrid::new(anchor, 1.0, 48),
            segments: &segments,
            params: &params,
            heater: &heater,
            gas_rate: &gas_rate,
            rates: &flat_rates(),
            thermal_mass_btu_per_f: mass,
            start_temp: 80.0,
        };
        let window = ComfortWindow {
            days: vec![Weekday::Sat, Weekday::Sun],
            start: HhMm::new(15, 0),
            end: HhMm::new(20, 0),
            target_f: 88.0,
        };

        // Two contiguous occurrences (Sat run, Sun run), each 5 slots wide.
        let occs = window_occurrences(&ctx, &window);
        assert_eq!(occs.len(), 2, "Sat + Sun must be two occurrences");
        let sat_first = occs[0][0];
        let sun_first = occs[1][0];
        // Sunday's first slot is ~24h after Saturday's: a real gap between runs.
        assert!(sun_first > sat_first + 5, "occurrences are separated by a gap");

        let schedule = optimize(&ctx, std::slice::from_ref(&window));

        // Every ON slot must fall in EXACTLY one valid lead-time band:
        //   * Saturday's lead-time `[0, sat_first)`, or
        //   * Sunday's lead-time `(sat_last, sun_first)` — strictly after the
        //     Saturday occurrence ends and before the Sunday occurrence starts.
        // A slot in `[sat_first, sat_last]` or `>= sun_first` would mean heating
        // inside a window; a Sunday pre-heat slot `<= sat_last` would be the bug
        // (Sunday borrowing a Saturday/earlier lead-time slot).
        let sat_last = *occs[0].last().unwrap();
        let mut sun_lead_used = false;
        for (i, &on) in schedule.iter().enumerate() {
            if !on {
                continue;
            }
            let in_sat_lead = i < sat_first;
            let in_sun_lead = i > sat_last && i < sun_first;
            assert!(
                in_sat_lead || in_sun_lead,
                "ON slot {i} is not in a valid lead-time band \
                 (sat_lead [0,{sat_first}), sun_lead ({sat_last},{sun_first}))"
            );
            sun_lead_used |= in_sun_lead;
        }
        assert!(
            sun_lead_used,
            "Sunday must pre-heat using its own lead-time (slots after the Saturday window)"
        );

        // Both occurrences are reported and met.
        let report = build_report(&ctx, std::slice::from_ref(&window), 88.0);
        assert_eq!(report.comfort_met.len(), 2, "two occurrences reported separately");
        assert!(
            report.all_comfort_met(),
            "both Saturday and Sunday comfort must be met: {:?}",
            report.comfort_met
        );
        // The two reported window starts are a day apart.
        let starts: Vec<i64> = report.comfort_met.iter().map(|o| o.window_start_unix).collect();
        assert!(starts[1] - starts[0] >= SECONDS_PER_DAY - 3600, "occurrences ~a day apart");
    }

    #[test]
    fn no_actuation_symbols_in_module_source() {
        // SAFETY guard (spec §"Safety / review focus"): this module must contain
        // no actuation surface — no HTTP POST, no `/heat` or `/on` endpoint, no
        // setpoint write, no network client. We scan only *real code* lines,
        // skipping `//` doc/comment lines (which legitimately describe what the
        // module does NOT do) and this test's own block. Needles are assembled
        // from fragments so the assertions themselves can't self-match.
        let post = format!("{}{}", "PO", "ST");
        let heat_ep = format!("{}{}", "/he", "at");
        let on_ep = format!("{}{}", "/o", "n");
        let setpoint_write = format!("{}{}", "set_set", "point");
        let http_client = format!("{}{}", "reqw", "est");
        let needles = [
            post.as_str(),
            heat_ep.as_str(),
            on_ep.as_str(),
            setpoint_write.as_str(),
            http_client.as_str(),
        ];

        let src = include_str!("scheduler.rs");
        let mut in_test_module = false;
        for line in src.lines() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                in_test_module = true;
            }
            if in_test_module {
                continue; // skip the test module (it names the forbidden tokens)
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // doc/comment lines are descriptive, not code
            }
            for needle in needles {
                assert!(
                    !line.contains(needle),
                    "scheduler.rs code line must not contain actuation token {needle:?}: {line}"
                );
            }
        }
    }
}
