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
```

## Building & Testing

```bash
cargo build --workspace              # Build everything
cargo test --workspace               # Run unit tests (80 protocol + 2 client)
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
```

## Semantic API

The daemon exposes a semantic pool API at `GET /api/pool` that auto-discovers the pool topology from pump speed tables and circuit function codes. The response is a human-friendly JSON object (pool, spa, lights, auxiliaries, pump, system) with no protocol internals.

Write endpoints use semantic identifiers:
- `POST /api/spa/on`, `/api/spa/off`, `/api/spa/jets/on`
- `POST /api/pool/on`, `/api/pool/off`
- `POST /api/lights/mode {"mode": "caribbean"}`
- `POST /api/spa/heat {"setpoint": 104}`

Smart behaviors: jets auto-enables spa, spa-off disables jets, light mode tracked by daemon.

## Key Files

- `docs/protocol-reference.md` — byte-level protocol documentation
- `test-fixtures/` — 24 binary captures from live hardware
- `pentair-protocol/src/semantic.rs` — topology discovery and semantic model
- `pentair-daemon/static/index.html` — embedded web UI
- `pentair-client/tests/live_write.rs` — stateful hardware tests with save/restore
