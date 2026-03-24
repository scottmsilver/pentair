# Architecture

## Overview

This system is a daemon-first Pentair controller stack.

Primary layers:

1. `pentair-protocol`
2. `pentair-client`
3. `pentair-daemon`
4. mobile/CLI consumers

The core design choice is that ScreenLogic protocol knowledge and pool/spa semantics live on the server, not in the apps.

## Component Boundaries

### `pentair-protocol`

Owns:
- raw protocol enums and wire types
- request builders
- response parsing
- semantic top-level types shared across the stack

Does not own:
- network I/O
- adapter sessions
- HTTP
- UI concerns

### `pentair-client`

Owns:
- connection/session with the ScreenLogic adapter
- request/response exchange
- push-event handling for status/config/history-style data

Does not own:
- semantic HTTP API
- app presentation logic

### `pentair-daemon`

Owns:
- long-running adapter connection
- semantic API over HTTP and WebSocket
- mDNS/Bonjour advertisement
- temperature trust rules
- last-known-reliable temperature backfill from Pentair history
- server-side heat estimation
- display contracts used by clients

This is the main product boundary. If behavior must stay identical across iOS and Android, it should usually move here.

### Mobile Apps

Own:
- local rendering
- optimistic UI for immediate feedback
- platform-native interaction models
- relative-time formatting and final wording

Do not own:
- Pentair semantics
- discovery rules
- temperature trust rules
- ETA readiness rules

## Data Flow

Normal read path:

1. Pentair controller pushes/serves raw status through the ScreenLogic adapter.
2. `pentair-client` reads raw protocol frames.
3. `pentair-daemon` converts them into semantic `PoolSystem` state.
4. `pentair-daemon` enriches that state with:
   - temperature trust
   - stale temperature snapshots
   - heat estimate
   - display contracts
5. clients fetch `/api/pool` for bootstrap/fallback and subscribe to `/api/ws`
6. `/api/ws` delivers full semantic `PoolSystem` snapshots, not event stubs

Normal write path:

1. user taps a semantic control in a client
2. client applies optimistic local state
3. client calls daemon semantic endpoint, e.g. `/api/spa/on`
4. daemon maps semantic action to controller action
5. later daemon refresh/push reconciles the optimistic state

## Discovery

The daemon advertises `_pentair._tcp` via mDNS/Bonjour.

Expected client behavior:
- discover automatically on LAN
- prefer semantic daemon API, not direct adapter access
- only fall back to manual address entry when discovery fails

Android emulator is a special case and may need host-loopback fallback. Real devices should use normal discovery/LAN routing.

## Semantic Model

The semantic API intentionally hides:
- raw circuit IDs
- body indices
- protocol action codes
- controller-specific packet shape

Clients consume:
- `pool`
- `spa`
- `lights`
- `auxiliaries`
- `pump`
- `system`

The daemon also exposes raw/debug endpoints for investigation, but product clients should stay on the semantic API.

## Temperature Trust

Pool and spa temperatures are not always equally trustworthy.

Important rule:
- on shared-equipment systems, a body temperature can be stale when that body is off

The daemon maintains:
- `temperature_reliable`
- `temperature_reason`
- `last_reliable_temperature`
- `last_reliable_temperature_at_unix_ms`

Backfill strategy:
- on startup, the daemon queries Pentair history
- it intersects body run windows with body temperature samples
- only samples that occurred while that body was actually running are eligible as last-known-reliable readings

This avoids treating random stale controller temp samples as live truth.

## Heating Estimation

Heating ETA is server-side.

The estimator combines:
- configured heater output
- configured body volume or derived volume from dimensions
- learned rates from prior sessions
- current-session observed rate
- air-temperature weighting

It also tracks heating sessions so ETA updates happen within one coherent heat-up run instead of mixing unrelated samples.

### Warmup and ETA readiness

On shared-equipment systems, when a body first becomes active:
- the temperature can remain stale during a warmup window
- ETA is pending during that window

The daemon currently exposes:
- raw `heat_estimate`
- `heat_estimate_display`

`heat_estimate_display` is the client-facing contract for:
- `ready`
- `pending`
- `unavailable`
- `available_in_seconds` for fixed warmup countdowns

## Display Contract Split

The intended split is:

- daemon owns meaning/state:
  - whether a temperature is stale
  - why ETA is pending
  - whether a countdown is known
- clients own wording/rendering:
  - `Estimate in about 2 min`
  - `Learning estimate`
  - `1h ago`
  - typography and layout

This keeps client UI thin without pushing literal presentation strings into the API.

## Optimistic UI

Clients do optimistic mutations for controls like:
- spa mode
- light mode
- setpoint changes
- auxiliaries

Important constraint:
- optimistic updates must clear stale daemon display contracts when local state changes would invalidate them

Otherwise a client can keep showing an old ETA/countdown/staleness line until the next server refresh.

## Documentation

Key docs:
- [README.md](/home/ssilver/development/pentair/README.md)
- [docs/api-spec.md](/home/ssilver/development/pentair/docs/api-spec.md)
- [docs/designs/heat-up-estimation.md](/home/ssilver/development/pentair/docs/designs/heat-up-estimation.md)
- [docs/protocol-reference.md](/home/ssilver/development/pentair/docs/protocol-reference.md)
