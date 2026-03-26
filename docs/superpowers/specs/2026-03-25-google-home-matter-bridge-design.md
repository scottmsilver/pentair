# Google Home Matter Bridge — Design Spec

## Summary

A standalone Rust binary (`pentair-matter`) that acts as a Matter bridge, exposing the Pentair pool system to Google Home (and potentially Apple HomeKit / Amazon Alexa) via the Matter protocol. The sidecar talks to `pentair-daemon`'s existing REST API and WebSocket — zero changes to the daemon.

## Architecture

```
  ┌─────────────────────────────────────┐
  │     pentair-daemon (UNCHANGED)       │
  │                                      │
  │  REST API (localhost:8080)           │
  │  WebSocket (/api/ws)                 │
  │  mDNS (_pentair._tcp)               │
  └──────────┬──────────────────────────┘
             │ HTTP + WS (localhost)
             │
  ┌──────────▼──────────────────────────┐
  │     pentair-matter (NEW SIDECAR)     │
  │                                      │
  │  rs-matter + embassy runtime          │
  │  _matter._tcp mDNS (own stack)       │
  │  Fabric persistence (~/.pentair/)    │
  │                                      │
  │  Endpoints (stable IDs):             │
  │    1: Spa    — Thermostat            │
  │    2: Jets   — OnOff Plug-in Unit    │
  │    3: Lights — OnOff + ModeSelect    │
  └──────────┬──────────────────────────┘
             │ Matter (LAN, encrypted)
             │
  ┌──────────▼──────────────────────────┐
  │  Google Home Hub / Nest Speaker      │
  └─────────────────────────────────────┘
```

### Why sidecar (not in-process)?

1. **No embassy↔tokio bridging.** rs-matter uses embassy async; the daemon uses tokio. A sidecar runs its own runtime — no thread bridging, no channel plumbing.
2. **No mDNS conflicts.** Sidecar owns `_matter._tcp` independently. Daemon keeps `_pentair._tcp` untouched.
3. **Zero daemon changes.** The daemon's REST API is already the consumer boundary (Android, iOS, CLI all use it). The sidecar is just another consumer.
4. **Isolation.** rs-matter API instability can't crash the daemon. If the sidecar dies, pool control via apps and web UI continues unaffected.

## Crate Structure

```
pentair-matter/
  Cargo.toml           — rs-matter, reqwest, serde, tokio (for HTTP client), clap
  src/
    main.rs            — CLI args, config loading, start Matter stack
    bridge.rs          — Matter stack init, mDNS, commissioning, fabric persistence
    endpoints/
      mod.rs           — endpoint registry, stable ID assignment
      spa.rs           — Thermostat (OnOff + Thermostat clusters)
      pool.rs          — OnOff Plug-in Unit
      jets.rs          — OnOff Plug-in Unit
      lights.rs        — OnOff + ModeSelect clusters
    daemon_client.rs   — HTTP client for pentair-daemon REST API + WS subscriber
    state.rs           — Cached pool state from daemon, feeds Matter attribute reports
    config.rs          — CLI args / config file parsing
```

## Device Endpoint Mappings

### Endpoint 1: Spa (Thermostat)

| Matter cluster | Attribute/Command | Daemon API | Notes |
|---|---|---|---|
| OnOff | OnOff (read) | `GET /api/pool` → `spa.active` | `active` = circuit on AND pump running (truthful) |
| OnOff | On | `POST /api/spa/on` | Turns on spa circulation |
| OnOff | Off | `POST /api/spa/off` | Turns off spa (auto-disables jets) |
| Thermostat | LocalTemperature (read) | `GET /api/pool` → `spa.temperature` | Convert °F → 0.01°C |
| Thermostat | OccupiedHeatingSetpoint (r/w) | `GET /api/pool` → `spa.setpoint` / `POST /api/spa/heat` | Convert °F ↔ 0.01°C |
| Thermostat | SystemMode (r/w) | `GET /api/pool` → `spa.heat_mode` / `POST /api/spa/heat` | Off→Off, Heat→configured mode |

**Temperature conversion:** Matter uses 0.01°C internally. Pentair uses °F.
- Read: `(fahrenheit - 32.0) * 5.0 / 9.0 * 100.0` → i16
- Write: `(matter_value as f64 / 100.0) * 9.0 / 5.0 + 32.0` → round to nearest °F

**SystemMode mapping:**
- Matter `Off` → Pentair heat mode `"off"`
- Matter `Heat` → Pentair's currently configured heat mode (solar, heat-pump, etc.)
- Matter `Cool` / `Auto` → not supported, reject

### Endpoint 2: Jets (OnOff Plug-in Unit)

| Matter cluster | Attribute/Command | Daemon API |
|---|---|---|
| OnOff | OnOff (read) | `GET /api/pool` → `spa.accessories.jets` |
| OnOff | On | `POST /api/spa/jets/on` |
| OnOff | Off | `POST /api/spa/jets/off` |

Note: `jets/on` auto-enables spa via the daemon's existing smart behavior.

### Endpoint 3: Lights (OnOff + ModeSelect)

| Matter cluster | Attribute/Command | Daemon API |
|---|---|---|
| OnOff | OnOff (read) | `GET /api/pool` → `lights.on` |
| OnOff | On | `POST /api/lights/on` |
| OnOff | Off | `POST /api/lights/off` |
| ModeSelect | ChangeToMode(N) | `POST /api/lights/mode {"mode": "<name>"}` |
| ModeSelect | SupportedModes (read) | `GET /api/pool` → `lights.available_modes` |

**Why ModeSelect, not ColorControl:** IntelliBrite light "modes" (Caribbean, Party, Romance, etc.) are color-cycling programs, not static colors. ModeSelect truthfully represents what the hardware does. Google Home shows a mode picker, not a color wheel.

**Mode index mapping:** The daemon returns `lights.available_modes` which includes non-selectable entries (`off`, `on`, `set`, `sync`). The sidecar filters these out and assigns stable numeric indices to the remaining user-selectable modes (e.g., 0=party, 1=romance, 2=caribbean, 3=american, ...). The mapping is built on startup from the daemon's response and refreshes if the list changes.

**Null mode handling:** After a daemon restart, `lights.mode` is `null` (fire-and-forget — the protocol has no readback for light mode). The sidecar reports ModeSelect.CurrentMode as "unknown" (index 255 or a sentinel). Google Home may show "unknown mode" which is truthful.

## Stable Endpoint IDs

Matter endpoint IDs must stay constant across restarts and topology changes. The daemon auto-discovers pool topology from controller config, which can shift.

**Solution:** Hardcode endpoint IDs by role:
- Endpoint 0 = Root node (required by Matter spec)
- Endpoint 1 = Aggregator (required for bridge topology)
- Endpoint 2 = Spa (Thermostat + OnOff)
- Endpoint 3 = Jets (OnOff)
- Endpoint 4 = Lights (OnOff + ModeSelect)

Pool is excluded — pool circulation is schedule-managed and not suitable for voice on/off control.

If a device is not detected (e.g., no spa configured), the endpoint is still registered but marked as unavailable via the `Reachable` attribute on the Bridged Device Basic Information cluster (set to `false`). Google Home shows "device unavailable" rather than the device disappearing and reappearing.

## State Synchronization

### Daemon → Google Home (state updates)

1. Sidecar connects to daemon WebSocket at `ws://{daemon}/api/ws`
2. On connect: receives full `PoolSystem` JSON snapshot
3. On each push: receives updated `PoolSystem` JSON snapshot (raw JSON, no event wrapper — same shape as `GET /api/pool`)
4. Sidecar updates internal state cache
5. Changed attributes are pushed to Matter subscribers (Google Home sees updated temperature, on/off state, etc.)

**Worst-case latency:** Daemon refreshes from controller every 30 seconds. Google Home sees the update on the next Matter subscription report after that. Total: ~30-35 seconds from physical state change to Google Home UI.

### Google Home → Daemon (commands)

1. Google Home sends Matter cluster command to sidecar
2. Sidecar maps cluster command → daemon REST API call
3. Sidecar calls `POST /api/spa/on` (or equivalent) via HTTP
4. Daemon processes command, sends to ScreenLogic adapter
5. On success: daemon broadcasts `StatusChanged`, sidecar receives via WS, updates Matter attributes
6. On failure: sidecar reports error to Matter, Google Home shows failure

**Error handling:**
- Daemon unreachable → retry with exponential backoff (1s, 2s, 4s, max 30s). Report endpoint as "offline" to Matter after 3 failures.
- Daemon returns error (400/500) → log error, report command failure to Google Home
- WebSocket disconnects → reconnect with backoff, full state refresh on reconnect

## Commissioning

### First-time pairing

1. Sidecar starts, generates pairing code from config (discriminator + passcode)
2. Prints QR code as ASCII art to terminal + prints manual pairing code
3. User opens Google Home app → "Set up device" → "Matter device" → scans QR / enters code
4. Google Home pairs via Matter CASE protocol using "on-network" (IP-only) commissioning — sidecar is already on the LAN, no BLE or soft-AP needed
5. Sidecar persists fabric credentials to `~/.pentair/matter-fabrics/`
6. All 4 endpoints appear in Google Home

### Restart behavior

1. Sidecar starts, loads persisted fabric credentials
2. Advertises via `_matter._tcp` mDNS
3. Google Home recognizes the bridge (same fabric) → no re-pairing needed
4. Endpoints report current state from daemon

### Fabric persistence

rs-matter provides a `PersistStorage` trait for persisting fabric credentials (NOC, fabric keys, ACLs). The sidecar implements this trait, writing to `~/.pentair/matter-fabrics/`. The file format is implementation-defined by rs-matter's trait — likely binary blobs, not JSON. On corrupt/missing files, log warning → user must re-commission.

## Configuration

### CLI args (minimal for v1)

```
pentair-matter --daemon-url http://localhost:8080 --discriminator 3840 --passcode 20202021
```

### Optional config file (`~/.pentair/matter.toml`)

```toml
daemon_url = "http://localhost:8080"
discriminator = 3840
passcode = 20202021  # WARNING: These are well-known Matter test defaults. Anyone on
                     # your LAN could pair with these. Change for real deployments.
fabric_path = "~/.pentair/matter-fabrics"
```

CLI args override config file. `--daemon-url` is never hardcoded — always configurable.

## Graceful Degradation

| Failure | Behavior |
|---|---|
| Daemon unreachable at startup | Sidecar starts, retries connection. Endpoints report "unavailable." |
| Daemon goes down while running | WebSocket reconnects with backoff. Commands fail with "unavailable." |
| Sidecar crashes | Daemon unaffected. Apps, web UI, push notifications all continue. |
| rs-matter panics | Sidecar exits. Systemd/supervisor restarts it. Daemon unaffected. |
| Fabric credentials corrupt | Log warning. User must re-commission from Google Home app. |

## v1 Scope

**In scope:**
- Sidecar binary with rs-matter
- 3 device endpoints (Spa Thermostat, Jets OnOff, Lights ModeSelect) — Pool excluded (schedule-managed)
- Bidirectional state sync (WS subscribe + REST commands)
- Commissioning with pairing code (terminal output)
- Fabric persistence
- Stable endpoint IDs
- Temperature °F↔°C conversion
- Light mode index mapping
- Error handling + graceful degradation

**NOT in scope (deferred to v2):**
- Timed heating ("have spa ready at 7pm")
- Scene orchestration ("pool party")
- QR code in daemon web UI
- Energy dashboard (pump watts)
- Freeze protection announcements
- Apple HomeKit / Alexa validation
- Matter certification
- Remote access / cloud relay

## Testing Strategy

**Unit tests (testable without Matter hardware):**
- Temperature conversion: °F → 0.01°C and back (32°F, 104°F, 0°F, 212°F, negative)
- SystemMode mapping: Matter modes → Pentair heat modes and back
- Light mode index mapping: mode names → stable indices
- Config parsing: CLI args, TOML, defaults, overrides
- State cache: parse PoolSystem JSON, detect changes, identify which attributes changed
- Error handling: daemon unreachable, 400/500 responses, WS disconnect

**Integration tests (mock daemon):**
- HTTP client: mock daemon REST responses, verify correct API calls for each endpoint command
- WebSocket client: mock state change events, verify attribute updates
- Fabric persistence: write/read/corrupt round-trip

**Manual tests (require Google Home hardware):**
- Commission via pairing code
- "Hey Google, turn on the spa" / "set spa to 104" / "turn on the jets" / "set lights to caribbean"
- State sync: change from Android app → verify Google Home shows update
- Daemon restart → verify sidecar reconnects
- Sidecar restart → verify Google Home still sees devices (no re-pair)

## Dependencies

```toml
[dependencies]
rs-matter = "*"           # Matter protocol (embassy async)
reqwest = { version = "0.12", features = ["json"] }  # HTTP client for daemon API
tokio = { version = "1", features = ["full"] }        # Async runtime (for reqwest + WS)
tokio-tungstenite = "0.21"  # WebSocket client
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }       # CLI args
toml = "0.8"              # Config file
tracing = "0.1"           # Structured logging
tracing-subscriber = "0.3"
dirs = "5"                # Home directory for config/fabrics
```

### Runtime architecture

The sidecar runs two async runtimes in the same process:

```
  main() — tokio runtime
    ├── tokio::spawn(daemon_ws_subscriber)     # WebSocket state sync
    ├── tokio::spawn(http_command_handler)      # REST API calls to daemon
    └── std::thread::spawn(matter_thread)       # Dedicated OS thread
              └── embassy executor
                    ├── Matter protocol stack
                    ├── mDNS (_matter._tcp)
                    └── Cluster handlers
```

Communication between runtimes: `std::sync::mpsc` channels (thread-safe, no async runtime dependency). The Matter thread sends commands to tokio tasks ("user said turn on spa") and receives state updates ("spa temperature is now 103°F").

This works because rs-matter's embassy executor owns one thread and tokio owns the rest. They share nothing except the mpsc channels.

## Risks

1. **rs-matter Linux maturity.** rs-matter primarily targets embedded (ESP32). Running as a Linux bridge is plausible but may hit gaps: mDNS implementation on Linux, bridge device type support, ModeSelect cluster implementation. The spike must validate all of these.
2. **embassy on Linux.** embassy-executor is designed for `#[no_std]` embedded. It can run on std Linux but it's not the primary platform. Monitor for edge cases.
3. **ModeSelect cluster support.** ModeSelect is uncommon in Matter. Verify both rs-matter and Google Home support it for bridged devices. If not supported, fall back to OnOff-only for lights.
4. **Google Home on-network commissioning.** Verify that IP-only commissioning (no BLE) works from the Google Home app for a Linux-based Matter bridge. This is the standard path for bridges but should be tested early.

All 4 risks are addressed by the spike (Milestone 1). The spike's go/no-go criteria: rs-matter compiles on Linux, bridge device type works, mDNS advertises, commissioning succeeds from Google Home, at least OnOff cluster works end-to-end.

## Supersedes

This spec supersedes the "Google Home / Voice Assistant Integration" section (Component 2) of `docs/superpowers/specs/2026-03-18-phase2-android-google-home-design.md`, which proposed an in-process Matter bridge using `rs-matter` inside `pentair-daemon`. The sidecar approach was chosen during eng review based on Codex's architectural challenge.
