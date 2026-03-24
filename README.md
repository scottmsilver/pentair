# Pentair

Monorepo for a Pentair ScreenLogic-based pool controller stack.

This repo contains:
- a low-level ScreenLogic protocol implementation
- a client for talking to the Pentair adapter
- a daemon that exposes a semantic LAN API over HTTP/WebSocket
- mobile clients for iOS and Android
- a CLI for direct controller inspection and debugging

The current product direction is:
- daemon-centric semantics and discovery
- thin mobile clients over `/api/pool` and `/api/ws`
- server-side heating estimation and temperature-trust logic

## Repo Map

- `pentair-protocol`
  Raw protocol types, request builders, response parsing, and semantic model types.
- `pentair-client`
  Async client for the ScreenLogic adapter.
- `pentair-daemon`
  Long-running HTTP/WebSocket server, mDNS advertiser, heating estimator, and push-notification entry point.
- `pentair-cli`
  Direct CLI tools for status, schedules, raw packets, pump data, and controller history.
- `pentair-android`
  Android client. Modern UI is the default surface.
- `pentair-ios`
  SwiftUI iOS client.
- `docs`
  API notes, protocol notes, experiments, and design docs.

## Runtime Architecture

High level flow:

1. `pentair-client` connects to the ScreenLogic adapter.
2. `pentair-daemon` consumes raw controller state and builds a semantic `PoolSystem`.
3. The daemon advertises `_pentair._tcp` over mDNS/Bonjour.
4. Mobile clients discover the daemon and render `/api/pool`.
5. UI actions call semantic endpoints like `/api/spa/on` or `/api/lights/mode`.

More detail is in [ARCHITECTURE.md](/home/ssilver/development/pentair/ARCHITECTURE.md).

## Current Semantic Model

The daemon is the source of truth for:
- circuit/body/lights semantics
- pool/spa shared-pump behavior
- temperature trust
- stale-temperature fallback
- heat ETA availability and countdown behavior

Important current behavior:
- Pool/Spa temperature readings can be marked stale on shared-equipment systems.
- The daemon backfills last trusted temperatures from controller history.
- The daemon exposes `temperature_display` and `heat_estimate_display` so clients do not need to reinvent display-state rules.
- During shared-equipment warmup, the daemon can expose `available_in_seconds` so clients can say `Estimate in about 2 min`.

## Building

Rust workspace:

```bash
cargo build
```

Daemon:

```bash
cargo run -p pentair-daemon
```

CLI:

```bash
cargo run -p pentair-cli -- --help
```

Android:

```bash
cd pentair-android
./gradlew app:assembleDebug app:testDebugUnitTest
```

iOS:

Builds require macOS/Xcode. In this setup we typically sync to the Mac and run:

```bash
cd ~/development/pentair/pentair-ios
xcodebuild -project PentairIOS.xcodeproj -scheme PentairIOS -destination "platform=iOS Simulator,name=iPhone 17 Pro" build
```

## API

The primary client API is:
- `GET /api/pool`
- `GET /api/ws`

See [docs/api-spec.md](/home/ssilver/development/pentair/docs/api-spec.md).

## Heating Estimation

Heating ETA is server-side.

The daemon combines:
- configured heater output and body volume
- learned rates from prior sessions
- live observed session data
- shared-equipment sensor warmup rules

The daemon also owns when a body temperature is trusted, stale, or only last-known-good.

Related docs:
- [docs/designs/heat-up-estimation.md](/home/ssilver/development/pentair/docs/designs/heat-up-estimation.md)
- [docs/api-spec.md](/home/ssilver/development/pentair/docs/api-spec.md)

## Notes

- iOS and Android are intentionally thin. Server-side semantics are preferred when behavior needs to stay aligned across clients.
- The mobile apps still do optimistic UI updates, but stale/live temperature and ETA presentation should come from daemon display contracts whenever possible.
