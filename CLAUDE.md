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

## Matter Bridge

`pentair-matter` is a sidecar that exposes the pool to Google Home (and any Matter controller) via the Matter protocol. It talks to the daemon's REST API and WebSocket — zero daemon changes needed.

**Endpoints:**
- Endpoint 2: Spa — Thermostat (temperature, setpoint, heat mode) + OnOff
- Endpoint 3: Jets — OnOff (auto-enables spa via daemon smart behavior)
- Endpoint 4: Lights — OnOff + ModeSelect (12 IntelliBrite modes)

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
chip-tool onoff on 1 3                              # jets
chip-tool modeselect change-to-mode 3 1 4           # caribbean
chip-tool modeselect read supported-modes 1 4
```

**Commissioning:** Pairing code `3497-0112-332` (test defaults: discriminator 3840, passcode 20202021). Fabric persisted to `~/.pentair/matter-fabrics.bin`.

## Key Files

- `docs/protocol-reference.md` — byte-level protocol documentation
- `test-fixtures/` — 24 binary captures from live hardware
- `pentair-protocol/src/semantic.rs` — topology discovery and semantic model
- `pentair-daemon/static/index.html` — embedded web UI
- `pentair-client/tests/live_write.rs` — stateful hardware tests with save/restore
- `pentair-matter/src/matter_bridge.rs` — Matter bridge topology + OnOff hooks + mDNS
- `pentair-matter/src/thermostat_handler.rs` — Thermostat cluster (spa temp/setpoint/mode)
- `pentair-matter/src/mode_select_handler.rs` — ModeSelect cluster (IntelliBrite lights)
- `pentair-matter/tests/chip_tool_e2e.sh` — end-to-end test with chip-tool
