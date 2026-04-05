# Pentair ScreenLogic CLI

Rust workspace for controlling a Pentair pool system over the ScreenLogic IP protocol. Includes a protocol library, async TCP client, REST/WebSocket daemon with web UI, and CLI tool.

## gstack

Use the `/browse` skill from gstack for all web browsing. Never use `mcp__chrome-devtools__*` tools.

Available skills: `/plan-ceo-review`, `/plan-eng-review`, `/plan-design-review`, `/design-consultation`, `/review`, `/ship`, `/browse`, `/qa`, `/qa-only`, `/qa-design-review`, `/setup-browser-cookies`, `/retro`, `/document-release`.

## Git

- Do not commit or push without explicit permission from the user.
- Never hard code a server URL in code.

## Crate Structure

```
pentair-protocol/    Wire protocol: types, encode, decode, semantic model (no IO)
pentair-client/      Async TCP/UDP client (tokio)
pentair-daemon/      Long-running service: REST API + WebSocket + web UI + state cache
pentair-cli/         Command-line tool (talks to daemon or direct to adapter)
pentair-matter/      Matter bridge sidecar — exposes pool to Google Home via rs-matter
```

## Building & Testing

```bash
cargo build --workspace              # Build everything
cargo test --workspace               # Run all unit + integration tests
```

### Live hardware tests (require adapter at PENTAIR_HOST)

```bash
PENTAIR_HOST=192.168.1.89 cargo test --test live_read -p pentair-client -- --ignored --test-threads=1 --nocapture
PENTAIR_HOST=192.168.1.89 cargo test --test live_write -p pentair-client -- --ignored --test-threads=1 --nocapture
```

Live write tests save/restore state automatically. If restoration fails, a loud panic shows what to fix manually.

## Running

```bash
# CLI direct mode (TCP to adapter)
cargo run -p pentair-cli -- --direct --host 192.168.1.89 status

# Daemon (auto-discovers adapter, serves web UI at http://localhost:8080)
cargo run -p pentair-daemon

# CLI daemon mode (default, talks to daemon HTTP API)
cargo run -p pentair-cli -- status

# Matter bridge sidecar (requires daemon running, exposes to Google Home)
cargo run -p pentair-matter -- --daemon-url http://localhost:8080
```

## Semantic API

The daemon exposes a semantic pool API at `GET /api/pool` that auto-discovers the pool topology from pump speed tables and circuit function codes. The response is a human-friendly JSON object (pool, spa, lights, auxiliaries, pump, system) with no protocol internals.

Write endpoints use semantic identifiers:
- `POST /api/spa/on`, `/api/spa/off`, `/api/spa/jets/on`
- `POST /api/pool/on`, `/api/pool/off`
- `POST /api/lights/mode {"mode": "caribbean"}`
- `POST /api/spa/heat {"setpoint": 104}`

Smart behaviors: jets auto-enables spa, spa-off disables jets, light mode tracked by daemon.

Pool and spa bodies include `active: bool` — true when the circuit is on AND the pump is running with RPM > 0. Use `on` for what the user commanded, `active` for whether water is actually flowing.

## Mobile Architecture — Layering Rules

iOS and Android clients should use the same layering for the same feature. When implementing a feature on both platforms, the logic should live at the same architectural layer. Cross-platform consistency makes the codebase predictable — anyone reading one platform can infer how the other works.

**Layers (from bottom to top):**

1. **Data layer** (Repository / data classes) — networking, caching, persistence. Transforms wire data into domain models. No UI logic, no presentation decisions.
2. **ViewModel / presentation layer** — state machines, transition detection, "when to show what." Decides when to start/stop/update UI elements based on domain state changes. This is where feature lifecycle logic belongs.
3. **View layer** (Compose / SwiftUI) — pure rendering. Takes state, produces pixels. No business logic.

**Rules:**

- Feature lifecycle logic (detecting state transitions, deciding when to show/hide a widget or notification) belongs in the **ViewModel**, not the data layer. The data layer provides the state; the ViewModel reacts to it.
- Each platform feature should have **one manager/controller instance**, not duplicates. If both a push handler and a foreground path need to drive the same UI, they should share one instance (via DI singleton or equivalent).
- Push message handlers (FCM Service, APNs delegate) should be thin — parse the payload and forward to the shared manager. Don't duplicate state machine logic that already exists in the ViewModel.
- When the daemon provides display-ready contracts (`temperature_display`, `heat_estimate_display`), use them. Don't recompute server-side logic on the client.

## Matter Bridge

`pentair-matter` is a sidecar that exposes the pool to Google Home (and any Matter controller) via the Matter protocol. It talks to the daemon's REST API and WebSocket — zero daemon changes needed.

**Endpoints:**
- Endpoint 2: Spa — Thermostat + ThermostatUI (Fahrenheit) + OnOff
- Endpoint 3: Pool — Thermostat + ThermostatUI (Fahrenheit) + OnOff
- Endpoint 4: Jets — OnOff (On/Off Plug, auto-enables spa via daemon smart behavior)
- Endpoint 5: Lights — Extended Color Light (OnOff + LevelControl + ColorControl + ModeSelect + Identify + Groups)
- Endpoint 6: Goodnight — OnOff (On/Off Plug, momentary)

**Google Home Integration:**
- Thermostat endpoints show temperature in Fahrenheit via ThermostatUserInterfaceConfiguration cluster (0x0204)
- Light endpoint uses ColorControl (HS+XY+CT) to map IntelliBrite modes to a color wheel — Google Home shows this on Nest Hub displays (phone app only shows on/off)
- ModeSelect is also on the light endpoint but Google Home doesn't support it; chip-tool and Apple Home can use it
- Each bridged endpoint has ProductName + NodeLabel + UniqueID for proper naming in controllers
- mDNS responder includes IPv4-mapped IPv6 address fix (`Ipv4MappedFixSocket`) required for Google Home discovery

**QR Code Pairing Page:** `http://localhost:8080/matter` — scannable QR code + manual pairing code for Google Home setup.

**Architecture:** rs-matter runs on a dedicated OS thread (embassy async). Tokio handles daemon HTTP/WS. Communication via `std::sync::mpsc` channels and `Arc<Mutex<MatterState>>`.

**Testing with chip-tool:**
```bash
# Requires: sudo snap install chip-tool && sudo snap connect chip-tool:avahi-observe
# Requires: daemon running on localhost:8080

# Automated e2e test (starts bridge, commissions, tests all endpoints):
./pentair-matter/tests/chip_tool_e2e.sh

# Manual:
chip-tool pairing onnetwork 1 20202021
chip-tool thermostat read local-temperature 1 2
chip-tool thermostat write occupied-heating-setpoint 4000 1 2
chip-tool onoff on 1 4                              # jets
chip-tool colorcontrol move-to-hue-and-saturation 170 254 0 0 0 1 5  # blue
chip-tool modeselect change-to-mode 3 1 5           # caribbean
chip-tool modeselect read supported-modes 1 5
```

**Commissioning:** Pairing code `3497-0112-332` (test defaults: discriminator 3840, passcode 20202021). Fabric persisted to `~/.pentair/matter-fabrics.bin`.

## Key Files

- `docs/protocol-reference.md` — byte-level protocol documentation
- `test-fixtures/` — 24 binary captures from live hardware
- `pentair-protocol/src/semantic.rs` — topology discovery and semantic model
- `pentair-daemon/static/index.html` — embedded web UI
- `pentair-client/tests/live_write.rs` — stateful hardware tests with save/restore
- `pentair-matter/src/matter_bridge.rs` — Matter bridge topology, OnOff hooks, mDNS IPv4 fix, endpoint wiring
- `pentair-matter/src/thermostat_handler.rs` — Thermostat cluster for spa and pool (temp/setpoint/mode)
- `pentair-matter/src/thermostat_ui_handler.rs` — ThermostatUserInterfaceConfiguration (Fahrenheit display)
- `pentair-matter/src/color_control_handler.rs` — ColorControl cluster (HS+XY+CT → IntelliBrite mode mapping)
- `pentair-matter/src/level_control_handler.rs` — Fixed-brightness LevelControl (pool lights don't dim)
- `pentair-matter/src/identify_handler.rs` — Stub Identify cluster (required by Extended Color Light)
- `pentair-matter/src/groups_handler.rs` — Stub Groups cluster (required by Extended Color Light)
- `pentair-matter/src/mode_select_handler.rs` — ModeSelect cluster (IntelliBrite lights, not used by Google Home)
- `pentair-daemon/static/matter.html` — Matter QR code pairing page
- `pentair-matter/tests/chip_tool_e2e.sh` — end-to-end test with chip-tool

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:
- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
