# TODOs

## P1 — Blocks Phase 2

### Define daemon REST API contract
**What:** REST API spec for pentair-daemon — every endpoint, request/response JSON shapes, WebSocket event format.
**Why:** Unblocks parallel work on Android app + Google Home integration. Prevents API rework.
**Context:** Agreed during eng review (Issue 2). The spec should map CLI commands to REST endpoints, define JSON response shapes matching the protocol types, and document WebSocket subscription/push format.
**Effort:** S (CC: ~30 min)
**Depends on:** Design spec (done), protocol types (done). Build before starting Phase 2 consumers.

## P3 — Deferred features

### Pool party macros
**What:** Config-driven macros that execute multiple commands atomically (e.g., "pool party" = pool on + lights Caribbean + water feature on).
**Why:** One of the most common real-world pool actions. Accepted as a delight feature in CEO review but deferred from Phase 1.
**Context:** Requires daemon config system and circuit control working. Scene system could also serve Google Home Scene trait.
**Effort:** S (CC: ~15 min)
**Depends on:** Daemon + circuit control.

### Remote access / cloud relay
**What:** Lightweight cloud relay so Android app and Google Home work away from home WiFi.
**Why:** Currently LAN-only — VPN is the stopgap for remote access.
**Context:** Daemon connects outbound to Firebase Function or small VPS via WebSocket. App detects LAN vs remote and routes accordingly. Architecture supports this without changes — daemon's REST API is the same.
**Effort:** L (CC: ~2 hours)
**Depends on:** Daemon + REST API stable.
