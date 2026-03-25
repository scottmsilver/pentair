# Pentair Pool Controller

A complete smart pool control platform built on the Pentair ScreenLogic IP protocol. Control your pool, spa, lights, heater, and chlorinator from your phone, Google Home, or command line.

**What it does:** Replaces the Pentair ScreenLogic app with a self-hosted system — a Rust daemon on your LAN that talks to the ScreenLogic adapter, plus native Android and iOS apps with real-time status, one-tap controls, and push notifications when your spa is ready.

**Tested on:** IntelliTouch controller, IntelliFlow VS pump, IntelliBrite lights, firmware 5.2 Build 738.0.

## Architecture

```
  ScreenLogic Adapter (192.168.1.x:80)
         │
         │  TCP (ScreenLogic protocol)
         │
  pentair-daemon (Rust, runs on your LAN)
    ├── REST API + WebSocket
    ├── mDNS discovery (_pentair._tcp)
    ├── State cache + push subscriptions
    ├── Heating ETA estimation
    └── FCM push notifications
         │
    ┌────┼────────────┐
    │    │             │
  CLI  Android       iOS
       (Kotlin)    (SwiftUI)
```

## Repo Structure

| Directory | Description |
|-----------|-------------|
| `pentair-protocol/` | Wire protocol: types, encode/decode, semantic model (no IO) |
| `pentair-client/` | Async TCP/UDP client (tokio) |
| `pentair-daemon/` | Long-running service: REST API, WebSocket, web UI, heating estimator, push notifications |
| `pentair-cli/` | Command-line tool (direct to adapter or via daemon) |
| `pentair-android/` | Android app (Kotlin + Jetpack Compose) |
| `pentair-ios/` | iOS app (SwiftUI) |
| `docs/` | Protocol reference, API spec, design docs |
| `test-fixtures/` | 24 binary captures from live hardware |

## Quick Start

### 1. Build and run the daemon

```bash
cargo build --release -p pentair-daemon
cargo run -p pentair-daemon  # auto-discovers adapter on LAN
```

The daemon advertises itself via mDNS. Mobile apps find it automatically.

### 2. CLI

```bash
# Direct mode (talks to adapter, no daemon needed)
cargo run -p pentair-cli -- --direct --host 192.168.1.89 status

# Daemon mode (default, talks to daemon HTTP API)
cargo run -p pentair-cli -- status
cargo run -p pentair-cli -- circuit on "Pool"
cargo run -p pentair-cli -- heat set spa 102
cargo run -p pentair-cli -- light party
```

### 3. Mobile apps

**Android:**
```bash
cd pentair-android
./gradlew app:assembleDebug app:testDebugUnitTest
```

**iOS** (requires macOS + Xcode):
```bash
cd pentair-ios
xcodebuild -project PentairIOS.xcodeproj -scheme PentairIOS \
  -destination "platform=iOS Simulator,name=iPhone 17 Pro" build
```

## Setup After Cloning

### Firebase config (required for mobile apps)

Download Firebase config files from the [Firebase Console](https://console.firebase.google.com) -> Project Settings -> Your Apps:

- **Android**: Download `google-services.json` -> place at `pentair-android/app/google-services.json`
- **iOS**: Download `GoogleService-Info.plist` -> place at `pentair-ios/PentairIOS/GoogleService-Info.plist`

These files are gitignored to keep API keys out of the public repo.

### Daemon FCM key (required for push notifications)

1. Firebase Console -> Project Settings -> Service Accounts -> Generate New Private Key
2. Save to `~/.pentair/firebase/<project-id>-pentair-daemon-fcm.json`
3. Reference in your daemon config:
   ```toml
   [fcm]
   project_id = "your-project-id"
   service_account = "~/.pentair/firebase/your-project-id-pentair-daemon-fcm.json"
   ```

## Semantic API

The daemon exposes a semantic pool API at `GET /api/pool` that auto-discovers pool topology from pump speed tables and circuit function codes. The response is human-friendly JSON (pool, spa, lights, auxiliaries, pump, system) with no protocol internals.

Write endpoints use semantic identifiers:

```
POST /api/spa/on
POST /api/spa/off
POST /api/spa/heat          {"setpoint": 104}
POST /api/spa/jets/on
POST /api/pool/on
POST /api/pool/off
POST /api/lights/mode       {"mode": "caribbean"}
POST /api/devices/register  {"token": "fcm-token"}
GET  /api/ws                WebSocket for real-time state push
```

Smart behaviors: jets auto-enables spa, spa-off disables jets, light mode tracked by daemon. Pool and spa include `active: bool` (circuit on AND pump running with RPM > 0).

See [docs/api-spec.md](docs/api-spec.md) for the full API reference.

## Push Notifications

The daemon sends FCM push notifications for spa heating milestones:

- **Heating Started** -- spa heater engaged
- **Estimate Ready** -- ETA calculated (e.g., "ready in about 18 min")
- **Halfway** -- 50% of the way to target temperature
- **Almost Ready** -- 90% of the way
- **At Temperature** -- spa has reached the setpoint

Heating ETA is computed server-side by combining configured heater specs, learned rates from prior sessions, and live observed data. The daemon also manages temperature trust for shared-equipment systems where sensor readings can be stale.

## Testing

```bash
cargo test --workspace                # All unit tests (81 daemon + 22 protocol + 2 client)

# Live hardware tests (require adapter on LAN)
PENTAIR_HOST=192.168.1.89 cargo test --test live_read -p pentair-client -- --ignored --test-threads=1
PENTAIR_HOST=192.168.1.89 cargo test --test live_write -p pentair-client -- --ignored --test-threads=1
```

Live write tests save/restore state automatically. If restoration fails, a loud panic shows what to fix manually.

## Documentation

- [Protocol Reference](docs/protocol-reference.md) -- byte-level wire format with verification status
- [API Spec](docs/api-spec.md) -- REST and WebSocket API
- [Architecture](ARCHITECTURE.md) -- system design details
- [Smart Pool Platform Vision](docs/designs/smart-pool-platform.md) -- product roadmap
- [Heat-Up Estimation](docs/designs/heat-up-estimation.md) -- how ETA is computed

## Design Principles

- **Daemon is the source of truth** for semantics, temperature trust, heating estimates, and display state. Mobile apps are intentionally thin.
- **No hardcoded server URLs** in any client code. Daemon discovered via mDNS/Bonjour.
- **Protocol library has zero IO dependencies** -- testable with byte slices, reusable in embedded/WASM.
- **Mutating live tests use snapshot/restore** -- read state before, restore after, Drop guard on panic.
