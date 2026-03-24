# Heat-Up ETA Estimation

## Goal

Estimate time remaining for Pool and Spa to reach setpoint on the server side.

The estimate should:

- start with a useful first-pass ETA from configured physical characteristics
- improve during an active heating session from live observations
- persist learned behavior across daemon restarts
- expose a machine-readable ETA and confidence through the semantic API

## Non-Goals

- exact thermodynamic simulation
- weather-service integration
- solar production forecasting
- UI-only ETA logic

This should live in the daemon and ship through `/api/pool`.

## Existing Signals

Already available in the daemon:

- body `temperature`
- body `setpoint`
- body `on`
- body `active`
- body `heat_mode`
- body `heating`
- system `air_temperature`
- pump `running`, `rpm`, `watts`, `gpm`
- shared-pump topology
- 30 second refresh cadence in [`pentair-daemon/src/adapter.rs`](../../pentair-daemon/src/adapter.rs)

Not currently available:

- pool volume
- spa volume
- heater output / efficiency
- direct heater fire rate
- inlet / outlet water temperatures

So the initial model must be config-backed.

## Proposed Config

Extend `pentair.toml` with a `heating` section:

```toml
[heating]
enabled = true
history_path = "~/.pentair/heat-estimator.json"
sample_window_minutes = 180
minimum_runtime_minutes = 10
minimum_temp_rise_f = 1.0
shared_equipment_temp_warmup_seconds = 120

[heating.heater]
kind = "gas"            # gas | heat-pump | hybrid
output_btu_per_hr = 400000
efficiency = 0.84       # optional; default depends on kind

[heating.pool]
volume_gallons = 16000

[heating.spa]
[heating.spa.dimensions]
length_ft = 8
width_ft = 8
depth_ft = 4
```

Rust shape in [`pentair-daemon/src/config.rs`](../../pentair-daemon/src/config.rs):

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HeatingConfig {
    pub enabled: bool,
    pub history_path: String,
    pub sample_window_minutes: u64,
    pub minimum_runtime_minutes: u64,
    pub minimum_temp_rise_f: f32,
    pub shared_equipment_temp_warmup_seconds: u64,
    pub heater: HeaterConfig,
    pub pool: BodyHeatingConfig,
    pub spa: BodyHeatingConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HeaterConfig {
    pub kind: String,
    pub output_btu_per_hr: f64,
    pub efficiency: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BodyHeatingConfig {
    pub volume_gallons: Option<f64>,
    pub dimensions: Option<BodyDimensionsConfig>,
}

pub struct BodyDimensionsConfig {
    pub length_ft: Option<f64>,
    pub width_ft: Option<f64>,
    pub average_depth_ft: Option<f64>, // `depth_ft` accepted as a config alias
    pub shape_factor: f64,             // defaults to 1.0
}
```

The daemon should accept either an explicit `volume_gallons` or dimensions and derive gallons as:

```text
volume_gallons = length_ft * width_ft * average_depth_ft * 7.48 * shape_factor
```

## API Shape

Add an optional estimate object to both semantic bodies in [`pentair-protocol/src/semantic.rs`](../../pentair-protocol/src/semantic.rs):

```rust
pub struct HeatEstimate {
    pub available: bool,
    pub minutes_remaining: Option<u32>,
    pub current_temperature: i32,
    pub target_temperature: i32,
    pub confidence: String,          // none | low | medium | high
    pub source: String,              // configured | observed | blended
    pub reason: String,              // not-heating | sensor-warmup | inactive-shared-body | missing-config | insufficient-data | estimating
    pub observed_rate_f_per_hour: Option<f64>,
    pub configured_rate_f_per_hour: Option<f64>,
    pub updated_at_unix_ms: i64,
}
```

Attach as:

```rust
pub struct BodyState {
    ...
    pub temperature_reliable: bool,
    pub temperature_reason: Option<String>,
    pub heat_estimate: Option<HeatEstimate>,
}

pub struct SpaState {
    ...
    pub temperature_reliable: bool,
    pub temperature_reason: Option<String>,
    pub heat_estimate: Option<HeatEstimate>,
}
```

For shared-equipment systems, the inactive body temperature should be treated as low-confidence, and the newly active body should remain low-confidence until `shared_equipment_temp_warmup_seconds` has elapsed with flow active.

Example API result:

```json
"spa": {
  "on": true,
  "active": true,
  "temperature": 99,
  "setpoint": 104,
  "heat_mode": "heat-pump",
  "heating": "heater",
  "heat_estimate": {
    "available": true,
    "minutes_remaining": 37,
    "current_temperature": 99,
    "target_temperature": 104,
    "confidence": "medium",
    "source": "blended",
    "reason": "estimating",
    "observed_rate_f_per_hour": 5.2,
    "configured_rate_f_per_hour": 6.0,
    "updated_at_unix_ms": 1760000000000
  }
}
```

## Estimation Model

### 1. Baseline configured estimate

Use the configured body volume and heater output to derive a first-pass heating rate.

For Fahrenheit:

- water mass per gallon: `8.34 lb`
- BTU needed per 1°F rise: `volume_gallons * 8.34`
- effective heater output:
  - gas: `output_btu_per_hr * efficiency`
  - heat-pump: same shape initially, but optionally derated by air temperature later
  - hybrid: use configured nominal output initially

Configured rate:

```text
configured_rate_f_per_hour =
    effective_btu_per_hr / (volume_gallons * 8.34)
```

Configured ETA:

```text
delta_f = max(setpoint - current_temp, 0)
eta_hours = delta_f / configured_rate_f_per_hour
```

This gives a deterministic estimate immediately, even before enough live data has accumulated.

### 2. Live observed estimate

During a heating session, compute observed heating slope:

```text
observed_rate_f_per_hour =
    (current_temp - session_start_temp) / elapsed_hours
```

Use only after:

- runtime >= `minimum_runtime_minutes`
- observed temp rise >= `minimum_temp_rise_f`
- body is still on, active, and trying to heat

This avoids garbage ETAs during startup lag or integer-temperature plateaus.

### 3. Blended estimate

Blend configured and observed rates early in a session, then lean more heavily on observed rate as evidence increases.

Simple first-pass weighting:

```text
evidence = clamp(elapsed_minutes / 30.0, 0.0, 1.0)
blended_rate = configured_rate * (1 - evidence) + observed_rate * evidence
```

Remaining ETA:

```text
remaining_delta_f = max(setpoint - current_temp, 0)
eta_minutes = remaining_delta_f / blended_rate * 60
```

## Session Model

Create a new daemon module:

- [`pentair-daemon/src/heat.rs`](../../pentair-daemon/src/heat.rs)

Key runtime types:

```rust
pub enum HeatingBodyKind { Pool, Spa }

pub struct HeatingSample {
    pub at_unix_ms: i64,
    pub body: HeatingBodyKind,
    pub temperature_f: f64,
    pub setpoint_f: f64,
    pub air_temp_f: Option<f64>,
    pub heat_mode: String,
    pub heating: String,
    pub pump_rpm: Option<u32>,
    pub pump_watts: Option<u32>,
    pub active: bool,
}

pub struct HeatingSession {
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: Option<i64>,
    pub body: HeatingBodyKind,
    pub start_temp_f: f64,
    pub latest_temp_f: f64,
    pub target_temp_f: f64,
    pub heater_kind: String,
    pub samples: Vec<HeatingSample>,
}
```

Session state machine:

- idle
- candidate
- heating
- complete
- aborted

Session starts when all are true:

- body exists
- `on == true`
- `active == true`
- `setpoint > temperature`
- `heat_mode != "off"`
- `heating != "off"` or body has been in candidate state for 1-2 refreshes

Session ends when any are true:

- `temperature >= setpoint`
- body turns off
- opposite shared body takes over
- heating remains inactive for too long

## Persistence

Persist learned data separately from live state, similar to [`pentair-daemon/src/devices.rs`](../../pentair-daemon/src/devices.rs).

Suggested file:

- `~/.pentair/heat-estimator.json`

Suggested persisted shape:

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct HeatEstimatorStore {
    pub learned: LearnedRates,
    pub recent_sessions: Vec<CompletedHeatingSession>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct LearnedRates {
    pub pool_rate_f_per_hour: Option<f64>,
    pub spa_rate_f_per_hour: Option<f64>,
}
```

Keep it intentionally simple at first. Do not persist huge time-series blobs.

## Integration Points

### `pentair-daemon/src/config.rs`

- add `HeatingConfig`
- load defaults
- document `pentair.toml`

### `pentair-daemon/src/main.rs`

- construct `HeatEstimator`
- pass config and persistence path into shared state or adapter task

### `pentair-daemon/src/state.rs`

- add estimator runtime state and/or latest computed estimate snapshots
- include estimator config in `new_shared_state(...)`

### `pentair-daemon/src/adapter.rs`

- after each successful `refresh_status(...)`, call estimator update with current semantic state
- compute session transitions from the latest `PoolSystem`
- avoid heavy logic in HTTP handlers; do it in the adapter loop where telemetry arrives

### `pentair-protocol/src/semantic.rs`

- extend `BodyState` and `SpaState` with `heat_estimate`
- semantic builder should attach latest estimate snapshot

### `docs/api-spec.md`

- document new config
- document `heat_estimate` fields

## Confidence Rules

Use explicit confidence levels:

- `none`
  - no config
  - body not heating
  - insufficient samples
- `low`
  - configured-only estimate
- `medium`
  - blended estimate with some live evidence
- `high`
  - learned model plus current-session observation aligned within tolerance

This is better than pretending precision we do not have.

## Edge Cases

- Shared pool/spa pump:
  - only estimate for the active body
  - inactive opposite body gets no ETA
- Heat mode off:
  - no ETA
- Solar / solar-preferred:
  - first pass should either return low-confidence configured ETA only if explicitly configured, or `available=false`
- Temperature plateaus:
  - do not overreact to one unchanged integer reading
- Manual setpoint changes mid-session:
  - keep the session, update target, recompute ETA
- Reconnect / daemon restart:
  - persisted learned rate survives
  - active live session can restart from current state without perfect continuity

## Rollout Order

### Phase 1: Baseline estimate

- config schema
- `HeatEstimate` API type
- configured-only ETA
- no persistence

Ship this first.

### Phase 2: Live session tracking

- session detector
- observed rate
- blended ETA
- confidence/reason fields

### Phase 3: Persistence and learning

- persisted learned rates
- recent session summaries
- simple EWMA per body

### Phase 4: Better heuristics

- air-temp derating for heat pumps
- solar handling
- richer confidence scoring

## Test Plan

### Unit tests

- configured ETA math
- session start / end conditions
- shared-pump body selection
- blended-rate weighting
- confidence / reason classification
- IPv4/IPv6 / temp unit conversions if normalization helpers are added

### Integration tests

- semantic API includes estimate when config is present
- no estimate when heating is off
- estimate disappears when opposite shared body takes over
- observed estimate updates after several refresh ticks

### Manual validation

- spa heat-up from cold to setpoint
- pool heat-up with shared-pump spa off
- mid-session setpoint change
- body off before completion

## Recommendation

Implement Phase 1 and Phase 2 together if possible, but keep the internal model simple:

- deterministic configured estimate
- current-session observed rate
- blended ETA
- explicit confidence + reason

That will already feel smart to users without overbuilding the model.
