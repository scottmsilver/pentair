# TODOs

## Completed

### Define daemon REST API contract
**Completed:** v0.1.0 (2026-03-18)
**Spec:** `docs/api-spec.md`

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
