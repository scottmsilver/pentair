# Pentair Daemon REST API Specification

Base URL: `http://<host>:8080`

No authentication — the daemon trusts all LAN clients (matches the ScreenLogic adapter's trust model).

---

## Semantic API

The primary API. Hides all protocol internals. Clients use semantic identifiers like "spa", "pool", "jets".

### GET /api/pool

Returns the complete pool system state in a single call. This is the only endpoint a UI needs to render.

**Response:**

```json
{
  "pool": {
    "on": false,
    "temperature": 99,
    "temperature_reliable": true,
    "last_reliable_temperature": 99,
    "last_reliable_temperature_at_unix_ms": 1774311705123,
    "setpoint": 59,
    "heat_mode": "heat-pump",
    "heating": "off",
    "temperature_display": {
      "value": 99,
      "is_stale": false,
      "last_reliable_at_unix_ms": 1774311705123
    },
    "heat_estimate_display": {
      "state": "unavailable",
      "reason": "not-heating",
      "target_temperature": 59
    },
    "heat_estimate": {
      "available": false,
      "minutes_remaining": null,
      "current_temperature": 99,
      "target_temperature": 59,
      "confidence": "none",
      "source": "none",
      "reason": "not-heating",
      "updated_at_unix_ms": 1774311705123
    }
  },
  "spa": {
    "on": false,
    "temperature": 103,
    "temperature_reliable": false,
    "temperature_reason": "inactive-shared-body",
    "last_reliable_temperature": 100,
    "last_reliable_temperature_at_unix_ms": 1774311105123,
    "setpoint": 104,
    "heat_mode": "heat-pump",
    "heating": "off",
    "temperature_display": {
      "value": 100,
      "is_stale": true,
      "stale_reason": "inactive-shared-body",
      "last_reliable_at_unix_ms": 1774311105123
    },
    "heat_estimate_display": {
      "state": "pending",
      "reason": "sensor-warmup",
      "available_in_seconds": 73,
      "target_temperature": 104
    },
    "heat_estimate": {
      "available": false,
      "minutes_remaining": null,
      "current_temperature": 103,
      "target_temperature": 104,
      "confidence": "none",
      "source": "none",
      "reason": "sensor-warmup",
      "updated_at_unix_ms": 1774311705123
    },
    "accessories": {
      "jets": false
    }
  },
  "lights": {
    "on": false,
    "mode": null,
    "available_modes": [
      "off", "on", "set", "sync", "swim", "party", "romantic",
      "caribbean", "american", "sunset", "royal", "blue", "green",
      "red", "white", "purple"
    ]
  },
  "auxiliaries": [
    { "id": "water_feature", "name": "Water Feature", "on": false },
    { "id": "floor_cleaner", "name": "Floor Cleaner", "on": false },
    { "id": "yard_light", "name": "Yard Light", "on": false }
  ],
  "pump": {
    "pump_type": "VS",
    "running": false,
    "watts": 0,
    "rpm": 0,
    "gpm": 0
  },
  "system": {
    "controller": "IntelliTouch",
    "firmware": "POOL: 5.2 Build 738.0 Rel",
    "temp_unit": "°F",
    "air_temperature": 69,
    "freeze_protection": false,
    "pool_spa_shared_pump": true
  }
}
```

**Field notes:**

| Field | Description |
|-------|-------------|
| `pool.on` | Pool circulation circuit is active (usually schedule-driven) |
| `spa.on` | Spa circulation is active (user-controlled) |
| `spa.accessories` | Map of accessory slug → on/off state. Auto-discovered from pump speed tables |
| `lights.mode` | Last known color mode. `null` if lights haven't been controlled via this daemon session (fire-and-forget — no protocol readback) |
| `lights.available_modes` | All supported IntelliBrite color modes |
| `auxiliaries[].id` | URL-safe slug for use in `/api/auxiliary/{id}/on` |
| `system.pool_spa_shared_pump` | Auto-detected from pump speed tables. If `true`, pool and spa are mutually exclusive |
| `pool.heat_mode` | One of: `off`, `solar`, `solar-preferred`, `heat-pump` |
| `pool.heating` | Current heater status: `off`, `solar`, `heater`, `both` |
| `pool.temperature_reliable` / `spa.temperature_reliable` | Whether the daemon currently trusts that body's displayed water temperature |
| `pool.temperature_reason` / `spa.temperature_reason` | Optional explanation when the body temperature is not currently trusted |
| `pool.last_reliable_temperature` / `spa.last_reliable_temperature` | Last known good water temperature for that body |
| `pool.last_reliable_temperature_at_unix_ms` / `spa.last_reliable_temperature_at_unix_ms` | Server timestamp for the last trusted temperature sample |
| `pool.temperature_display` / `spa.temperature_display` | UI-oriented temperature presentation contract. Clients should prefer this over rebuilding stale/live rules themselves. |
| `pool.heat_estimate_display` / `spa.heat_estimate_display` | UI-oriented heat-estimate presentation contract. Clients should prefer this over rebuilding pending/ready rules themselves. |
| `pool.heat_estimate` / `spa.heat_estimate` | Server-side estimate for time remaining to reach setpoint. Present when heating estimation is enabled in config. |

**`temperature_display` field notes:**

| Field | Description |
|-------|-------------|
| `value` | Temperature clients should display. When stale, this is usually the last trusted reading rather than the raw current body temperature |
| `is_stale` | Whether the displayed temperature is stale |
| `stale_reason` | Why the temperature is stale, e.g. `inactive-shared-body`, `waiting-for-flow`, `sensor-warmup` |
| `last_reliable_at_unix_ms` | Timestamp associated with the displayed temperature value |

**`heat_estimate_display` field notes:**

| Field | Description |
|-------|-------------|
| `state` | `ready`, `pending`, or `unavailable` |
| `reason` | Machine-readable reason for the display state |
| `available_in_seconds` | Present during fixed-duration pending states such as `sensor-warmup`; clients can render countdown copy from this |
| `minutes_remaining` | Rounded ETA when `state = ready` |
| `target_temperature` | Setpoint associated with the estimate |

**`heat_estimate` field notes:**

| Field | Description |
|-------|-------------|
| `available` | `true` when the daemon has enough information to provide an ETA |
| `minutes_remaining` | Rounded-up minutes left until the body reaches setpoint |
| `current_temperature` | Current body temperature in the system's configured unit |
| `target_temperature` | Current body setpoint in the system's configured unit |
| `confidence` | One of `none`, `low`, `medium`, `high` |
| `source` | One of `none`, `configured`, `learned`, `observed`, `blended` |
| `reason` | Why the estimate is or is not available: `estimating`, `at-temp`, `heat-off`, `not-heating`, `waiting-for-flow`, `sensor-warmup`, `inactive-shared-body`, `missing-config`, `insufficient-data` |
| `observed_rate_per_hour` | Live observed heating rate in the system's configured unit per hour, when enough session data exists |
| `learned_rate_per_hour` | Learned baseline heating rate from prior sessions, biased toward sessions in similar air temperatures |
| `configured_rate_per_hour` | Baseline configured heating rate in the system's configured unit per hour |
| `baseline_rate_per_hour` | The baseline rate actually used before current-session observation. This blends configured and learned rates when both exist. |
| `updated_at_unix_ms` | Server timestamp for the estimate calculation |

If the daemon hasn't connected to the adapter yet, returns:
```json
{"error": "pool data not yet available"}
```

---

### Pool Control

#### POST /api/pool/on

Turn on pool circulation. Optional body to set heat setpoint at the same time.

```json
{}
```
or
```json
{"setpoint": 82}
```

**Response:** `{"ok": true}` or `{"ok": false, "error": "..."}`

**Note:** If `pool_spa_shared_pump` is true, turning on pool will automatically turn off spa (controller enforced).

#### POST /api/pool/off

Turn off pool circulation.

**Response:** `{"ok": true}`

#### POST /api/pool/heat

Set pool heat setpoint and/or mode.

```json
{"setpoint": 82}
```
```json
{"mode": "heat-pump"}
```
```json
{"setpoint": 82, "mode": "solar-preferred"}
```

**Mode values:** `off`, `solar`, `solar-preferred`, `heat-pump`

**Response:** `{"ok": true}`

---

### Spa Control

#### POST /api/spa/on

Turn on spa circulation. Optional body to set heat setpoint.

```json
{}
```
or
```json
{"setpoint": 104}
```

**Response:** `{"ok": true}`

**Note:** Automatically turns off pool if shared pump.

#### POST /api/spa/off

Turn off spa and jets. Both are disabled — jets without spa is pointless.

**Response:** `{"ok": true}`

#### POST /api/spa/heat

Set spa heat setpoint and/or mode. Same format as pool heat.

```json
{"setpoint": 104, "mode": "heat-pump"}
```

**Response:** `{"ok": true}`

#### POST /api/spa/jets/on

Turn on jets. **Automatically turns on spa first** if it's not already on (jets need the spa valve open).

**Response:** `{"ok": true}`

#### POST /api/spa/jets/off

Turn off jets. Spa stays on.

**Response:** `{"ok": true}`

---

### Lights Control

#### POST /api/lights/on

Turn on lights (at whatever mode was last set).

**Response:** `{"ok": true}`

#### POST /api/lights/off

Turn off lights.

**Response:** `{"ok": true}`

#### POST /api/lights/mode

Set the light color mode. Also turns lights on if they're off.

```json
{"mode": "caribbean"}
```

**Mode values:** `swim`, `party`, `romantic`, `caribbean`, `american`, `sunset`, `royal`, `blue`, `green`, `red`, `white`, `purple`

**Response:** `{"ok": true}`

**Note:** Light mode is fire-and-forget — the protocol has no readback. The daemon tracks the last mode set during this session. Restarting the daemon resets `lights.mode` to `null`.

---

### Auxiliary Control

#### POST /api/auxiliary/{id}/on

Turn on an auxiliary device by its slug ID.

**Example:** `POST /api/auxiliary/water_feature/on`

Slug IDs come from `auxiliaries[].id` in the pool response.

**Response:** `{"ok": true}` or `{"ok": false, "error": "unknown device: foo"}`

#### POST /api/auxiliary/{id}/off

Turn off an auxiliary device.

**Response:** `{"ok": true}`

---

### System

#### POST /api/cancel-delay

Cancel all active delays (freeze protection, etc.).

**Response:** `{"ok": true}`

#### POST /api/refresh

Force a full data refresh from the adapter (status, config, chem, pumps).

**Response:** `{"ok": true}`

---

## WebSocket

### GET /api/ws → WebSocket upgrade

Subscribe to real-time push events. The daemon sends a JSON message whenever pool state changes.

**Event format:**

```json
{"type": "StatusChanged"}
```

**Event types:**
- `StatusChanged` — pool/spa/circuit state updated
- `ChemistryChanged` — chemistry data updated
- `ConfigChanged` — controller config updated

**Recommended usage:** On receiving any event, re-fetch `GET /api/pool` to get the latest state.

---

## Web UI

### GET /

Serves the embedded single-page web UI. No build step — it's a static HTML file compiled into the daemon binary.

---

## Smart Behaviors

The semantic API encodes physical relationships so clients don't have to:

| Action | What happens |
|--------|-------------|
| `POST /api/spa/jets/on` (spa is off) | Spa turns on first, waits 2s for valve, then enables jets |
| `POST /api/spa/off` | Jets turn off first, then spa turns off |
| `POST /api/spa/on` (pool is on, shared pump) | Controller auto-turns off pool (mutual exclusivity) |
| `POST /api/pool/on {"setpoint": 82}` | Sets heat setpoint first, then turns on pool |
| `POST /api/lights/mode {"mode": "party"}` | Sends light command, turns lights on if off, daemon tracks mode |

---

## Error Responses

All POST endpoints return the same shape:

```json
{"ok": true}
```
```json
{"ok": false, "error": "descriptive error message"}
```

Common errors:
- `"adapter disconnected"` — daemon lost connection to the ScreenLogic adapter
- `"unknown device: foo"` — unrecognized auxiliary slug ID
- `"unknown heat mode: foo"` — invalid heat mode string
- `"unknown light mode: foo"` — invalid light mode string

---

## Raw API (debugging)

Low-level endpoints that expose protocol-level data. Not recommended for app development.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/raw/status` | GET | Raw PoolStatus from protocol |
| `/api/raw/config` | GET | Raw ControllerConfig |
| `/api/raw/version` | GET | Firmware version string |
| `/api/raw/chem` | GET | Chemistry data (IntelliChem) |
| `/api/raw/chlor` | GET | Chlorinator config (SCG) |
| `/api/raw/pumps/{index}` | GET | Pump status (0-7) |
| `/api/raw/circuits/{id}` | POST | Set circuit by logical ID (`{"state": true}`) |
| `/api/raw/heat/setpoint` | POST | Set heat setpoint (`{"body_type": 0, "temperature": 82}`) |
| `/api/raw/heat/mode` | POST | Set heat mode (`{"body_type": 0, "mode": 3}`) |
| `/api/raw/heat/cool` | POST | Set cool setpoint (`{"body_type": 0, "temperature": 78}`) |
| `/api/raw/lights` | POST | Send light command (`{"command": 7}`) |
| `/api/raw/chlor/set` | POST | Set chlorinator output (`{"pool": 50, "spa": 0}`) |

---

## Configuration

The daemon reads `pentair.toml` (or path from `PENTAIR_CONFIG` env var):

```toml
# Adapter address. Empty = auto-discover via UDP broadcast.
adapter_host = "192.168.1.89"

# HTTP server bind address.
bind = "0.0.0.0:8080"

# Override spa accessory detection (default: name convention for "jets", "blower", etc.)
[associations]
spa = ["Bubbler", "Air Blower"]

# Optional server-side heat-up estimation.
[heating]
enabled = true
history_path = "~/.pentair/heat-estimator.json"
sample_window_minutes = 180
minimum_runtime_minutes = 10
minimum_temp_rise_f = 1.0
shared_equipment_temp_warmup_seconds = 120

[heating.heater]
kind = "gas"
output_btu_per_hr = 400000
efficiency = 0.84

[heating.pool]
volume_gallons = 16000

[heating.spa]
[heating.spa.dimensions]
length_ft = 8
width_ft = 8
depth_ft = 4
```

`[heating]` is optional. When enabled, the daemon combines configured heater/body sizes with observed heating sessions to estimate time remaining until the pool or spa reaches setpoint.

For each body, you can provide either:
- `volume_gallons`
- or `dimensions.length_ft`, `dimensions.width_ft`, and `dimensions.depth_ft` / `dimensions.average_depth_ft`

The daemon converts dimensions to gallons using:
`length_ft * width_ft * average_depth_ft * 7.48 * shape_factor`

`shape_factor` defaults to `1.0` for rectangular bodies and can be reduced for rounded/freeform shapes.
