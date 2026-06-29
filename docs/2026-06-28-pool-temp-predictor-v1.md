# Pool Temperature Predictor — Lean v1 Spec

**Goal:** Replace "72°F · measured 4h ago" with an honest *current* estimate —
"74°F · predicted ±2" — by projecting the last reliable water temperature forward
across the sensing gap using weather, and reverting to the measured value when we
can't predict well.

**Status:** design + adversarial critique complete (workflow `wf_a7a5f750-8c8`).
This is the lean-v1 implementation scope.

## Locked decisions
- **Cover:** covered-when-idle (idle ≡ covered). No separate schedule.
- **Scope:** lean v1 — fit ONE effective covered-idle cooling constant; defer
  cover_factor/solar_transmission/surface-area separation.
- **Weather:** OpenWeather **free Current 2.5** (gives air temp, wind, humidity,
  clouds). No paid One Call / timemachine in v1.
- **Location:** `[weather].latitude/longitude` in **gitignored** `pentair.toml`
  (Los Altos ZIP centroid). Never committed.
- **Key:** read ONLY from env `OPENWEATHER_API_KEY`. Never in config/code/logs.

## Model (lumped-capacitance, single node)
Project last reliable temp `T0 @ t0` to now over hourly weather segments:

    dT/dt = -k_eff·(T − T_eq) − q_evap
    T_eq  = T_air + solar_bump            (solar_bump small in v1; clouds-derived)
    k_eff = k0 + k_wind·wind              (wind folded into cooling)
    q_evap ∝ (a + b·wind)·(Psat(T_water) − RH·Psat(T_air))   (Dalton evaporation)

Closed-form per constant-weather segment, chained hour to hour:

    T(t1) = T_eq + (T(t0) − T_eq)·exp(−k_eff·Δt)

**Evaporation is the dominant uncovered-pool loss and is explicit** (critique
finding). Wind+humidity come free from OW 2.5.

## Calibration (closed-loop primary)
- **Primary:** each time the pump next cycles on and yields a fresh post-warmup
  reliable reading, compare it to what the predictor said for that instant during
  the gap. Feed the delta into a rolling MAE → drives `confidence` + uncertainty,
  and adjusts `k0` (and effective evaporation coeff) by gradient/secant step.
- **Weak prior:** history-interval fit from controller 48h `HistoryData`
  (heater-off cooling intervals) only seeds initial params; NOT trusted alone
  (train-on-pump-on / predict-on-pump-off regime mismatch — critique finding).
- **Persist** fitted params + rolling MAE into the existing heat-estimator JSON
  store (`HeatingConfig.history_path`) so they survive restarts.
- **Sanity clamps:** reject k≤0 or τ outside [2h, 200h]; fall back to physics seed.

## Guardrails
- **Max-gap cutoff:** beyond `min(3·τ, 12h)` set `basis=none`, `confidence=none`,
  revert UI to "measured N ago" — never fabricate a long-gap number.
- **Heater-on subsegments:** REPLACE with existing rate model
  (`configured_/learned_rate_f_per_hour`); do NOT superimpose cooling (avoids
  double-counting losses — critique finding).
- **Degradation order:** full model → cooling+evap only (clouds/solar=0 if weather
  stale >2h) → controller air temp → measured-N-ago. Surfaced via `basis`.

## Code changes (extend, don't reinvent)
- **NEW** `pentair-daemon/src/thermal.rs` — pure functions:
  `project_temperature(anchor, &[WeatherSegment], &CoolingParams, now) -> Projected`
  and `fit_cooling_params(...)`. No I/O → unit-testable like existing rate math.
- `heat.rs` `apply_to_system` (~336): when `temperature_trust_for_body` is NOT
  reliable AND a `last_reliable_temperature` exists, call `thermal::project_*` and
  write the new fields. Reuse the existing anchor accessor + trust gate verbatim.
- `heat.rs` calibration: generalize the `latest_history_observation_for_body`
  filter predicate (1139) into a shared helper returning ALL reliable (T,t)
  samples; reuse `controller_history_time_to_unix_ms` (1203).
- `adapter.rs` (~121): add a weather-refresh tokio interval (~15 min) updating an
  in-memory `WeatherCache` on `CachedState`; extend `backfill_last_reliable_from_history`
  (393) to also seed cooling params (no extra controller round-trips). reqwest is
  already a dep.
- `state.rs` `CachedState`: add `weather: WeatherCache`; pass into `apply_to_system`.
- `semantic.rs` BodyState/SpaState + TemperatureDisplay: new optional fields below.

## New serialized fields (additive; old clients ignore)
- `predicted_temperature: Option<i32>`, `predicted_temperature_f_precise: Option<f64>`
- `prediction_confidence: 'high'|'medium'|'low'|'none'`
- `prediction_uncertainty_f: Option<f64>`  (the ± band)
- `prediction_as_of_unix_ms: i64`
- `prediction_basis: 'measured'|'projected-weather'|'projected-cooling-only'|'none'`
- `TemperatureDisplay.is_predicted: bool` drives the home UI swap.

## Config additions (`pentair.toml`, gitignored)
    [weather]
    enabled = true            # opt-in; missing key/disabled never breaks startup
    latitude = <ZIP centroid> # not committed
    longitude = <ZIP centroid>
    poll_interval_seconds = 900
    # api key: env OPENWEATHER_API_KEY only

    [heating.cooling]
    # tau_covered_hours / evap coeffs optional seeds; calibration fills + persists.
    max_projection_hours = 12

Cover = covered-when-idle is implicit (no schedule needed in v1).

## Tests (mirror live_heat_estimate.rs)
- **Pure `thermal.rs` unit tests:** zero-gap returns anchor; constant sub-air →
  exp decay matches fine Euler; evaporation raises loss with wind/low-humidity;
  multi-segment chain == single when params equal; `fit_cooling_params` recovers
  known k from synthetic (noiseless + noisy); sparse/degenerate → seed + 'low',
  never NaN/negative τ; weather-absent → cooling-only + correct basis;
  gap > max → basis 'none'.
- **`tests/live_thermal_prediction.rs`:** loopback daemon vs real controller +
  mock-weather flag; idle body with stale reading → `predicted_temperature`
  present, basis 'projected-weather'; |predicted − next reliable| within MAE band.
- **Fixture replay:** recorded 48h `HistoryData` → `fit_cooling_params` → assert
  physically-plausible τ and hold-out MAE under threshold.

## UI (home gateway — presentational only)
When `temperature_display.is_predicted`, show predicted value as primary with a
"· predicted ±N" qualifier; keep measured + timestamp on tap. No transport change.

## Open risks (carried)
Stratification of an unmixed idle pool; sun-biased controller air sensor (prefer
OW air); cover state unobservable (covered-when-idle assumption + closed-loop
residual cross-check); fresh-water top-offs / rain not modeled (closed-loop MAE
auto-drops confidence). All surfaced via uncertainty + basis, never false precision.
