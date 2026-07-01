# Pool Comfort Scheduler (predictive heating) — v1 Spec — EVALUATION ONLY

**Goal:** A predictive ("MPC-lite") heat-pump scheduler that, given the calibrated
thermal model + weather forecast + comfort targets + heat-pump COP + time-of-use
rates, computes the **cheapest heating schedule that lands the pool at the target
temperature for the times you actually use it** — exploiting free solar gain,
warm-hour heat-pump efficiency, and off-peak rates, and letting the pool **drift
when nobody's using it** instead of holding a setpoint 24/7.

**HARD CONSTRAINT — ADVISORY/EVALUATION ONLY (this is the #1 rule):**
- This module **NEVER actuates**. It does **not** POST `/api/*/heat`, never changes
  the setpoint, never turns on the pump/heater. It only *computes and reports* a
  recommended plan and its projected energy/cost vs the current behavior.
- No actuation code path is wired in v1. A future `[comfort].actuate` flag is
  reserved (default **false**) but is explicitly OUT OF SCOPE — not implemented.
- The deliverable is an **evaluation**: numbers showing whether the optimizer
  would meet comfort targets at lower cost than a constant setpoint.

## Reuse (extend, don't reinvent)
- `thermal.rs` forward model: `project_temperature` (cooling `k`, evaporation,
  solar `g`, clear-sky irradiance), `CoolingParams`, `WeatherSegment`, `SolarSite`.
- `weather.rs`: `WeatherClient::fetch_forecast` (already exists) + `WeatherCache`
  for the 48 h air/wind/humidity/cloud forecast; clear-sky solar from geometry.
- The calibrated `(k, g)` from the soak fit.

## New module: `pentair-daemon/src/scheduler.rs` (PURE — unit-testable, no I/O)
1. **HeatPumpModel** — heat-pump pool heater. Config-driven:
   - `rated_btu_per_hr` (e.g. 100k), and a COP curve rising with air temp
     (air-source heat pumps are more efficient when warm): `cop(air_f, water_f)`
     e.g. linear `cop = clamp(cop_at_50 + slope*(air_f-50), 1.5, cop_max)`; also
     derate when `water_f` is high. Returns `(heat_btu_per_hr, elec_kw)` for a slot.
   - `delta_f_per_slot` from `heat_btu / (m*c)` over the slot length, given the
     pool thermal mass from pool params.
2. **RateSchedule** — time-of-use electricity. Config: list of `{days, start_hh_mm,
   end_hh_mm, usd_per_kwh}` (e.g. PG&E peak 16:00–21:00 high, else low); default
   flat. `rate(unix) -> usd_per_kwh`.
3. **ComfortWindow** — `{days, start_hh_mm, end_hh_mm, target_f}` (e.g. Sat/Sun
   15:00–20:00 → 88 °F). Empty = feature off.
4. **Forward sim with heating:** step the thermal model over discretized slots
   (e.g. 30 min) across the horizon (next 48 h). Passive dT comes from
   `thermal` (cooling + solar + evap); a heating slot adds `delta_f_per_slot`.
   Returns the temp trajectory + per-slot elec kWh + cost.
5. **Optimizer (greedy, explainable v1):** for each comfort window, compute the
   **deficit** = `target_f − passive_projected_temp_at_window_start` (passive
   projection already includes free solar, so the deficit is net of sun). Allocate
   heating to the **cheapest effective slots** in the preceding lead-time window —
   ranked by `rate/COP` (cost per °F delivered), respecting the thermal lead time
   (start early enough given the slow response) — until the projected temp meets
   the target across the window. Outside comfort windows: **no heating** (drift).
   (DP/MILP is a future refinement; greedy cost-per-°F is sound for v1 and easy to
   explain/evaluate.)
6. **Baseline:** constant-setpoint sim = heat whenever `pool < setpoint` across the
   whole horizon (today's dumb behavior). Sum its energy + cost.
7. **`HeatPlan` / `SavingsReport`:** recommended on/off schedule, projected
   trajectory, optimizer kWh + $, baseline kWh + $, **savings (% and $)**, and a
   `comfort_met: bool` per window. Plus a human-readable plan summary.

## Config additions (`pentair.toml`, gitignored)
    [comfort]                       # empty => feature off
    actuate = false                 # RESERVED, NOT IMPLEMENTED in v1
    # windows = [{ days=["Sat","Sun"], start="15:00", end="20:00", target_f=88 }]

    [heatpump]
    rated_btu_per_hr = 100000
    cop_at_50f = 3.0
    cop_per_f  = 0.06               # COP rises ~0.06 per °F of air temp
    cop_max    = 6.0

    [rates]                         # default flat if omitted
    # periods = [{ days=["Mon".."Fri"], start="16:00", end="21:00", usd_per_kwh=0.55 }]
    default_usd_per_kwh = 0.30

## Evaluation (the point of v1)
- **Forecast simulation:** fetch the 48 h forecast, run optimizer vs baseline,
  output the `SavingsReport` (kWh, $, % saved, comfort met) + the plan.
- **Backtest:** replay the soak + controller history (`~/.config/pool-temp/logs`,
  48 h `pentair-cli history`) as the "forecast" and show what the optimizer *would*
  have done vs constant-setpoint — projected savings, using the **fitted** `(k,g)`.
  Honest: no real energy meter, so this is model-projected energy, clearly labeled.
- Surface via a **read-only** `GET /api/pool/heat-plan` (returns the plan + savings,
  actuates nothing) and a `pentair-cli heat-plan [--backtest]` command.

## Safety / review focus
- Grep-confirm: scheduler/API/CLI contain **no** `/heat`/`/on` POST, no setpoint
  write, no daemon command. Advisory output only.
- The `[comfort].actuate` flag must be inert (no code reads it to act in v1).
- Pure `scheduler.rs` (clock/weather/params injected) → fully unit-tested.

## Tests
- HeatPumpModel: COP rises with air temp, clamps; delta_f sane.
- RateSchedule: peak/off-peak/day selection; flat default.
- Forward-sim-with-heat: a heated slot raises temp by ~delta_f; passive matches
  thermal.rs.
- Optimizer: meets a comfort target; prefers cheap/warm/solar-assisted slots;
  heats **zero** outside comfort windows; on a sunny forecast it **skips heating**
  and coasts on solar (the 13%-savings behavior).
- Baseline vs optimizer: optimizer cost ≤ baseline while meeting comfort.
- SAFETY test: a fuzz/grep test asserting the module emits no actuation calls.
