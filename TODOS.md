# TODOs

## Completed

### Define daemon REST API contract
**Completed:** v0.1.0 (2026-03-18)
**Spec:** `docs/api-spec.md`

## P2 — Google Home Matter Bridge v2 (deferred from v1 by eng review)

### Timed heating ("have spa ready at 7pm")
**What:** Daemon-level scheduler that calculates optimal start time from the ETA engine, starts heating automatically, and notifies on completion. New command: `ScheduleHeat { ready_by, setpoint }`. Persistent timers survive restarts. REST endpoint `POST /api/spa/heat-at` for mobile apps. Also exposed via Matter bridge.
**Why:** Accepted in CEO review (2026-03-25). Highest-delight feature — turns pool from dumb switch to predictive system. Deferred to v2 by eng review to ship core bridge first.
**Context:** ETA engine exists in `heat.rs`. Scheduler calculates `ready_by - estimated_minutes`. Edge case: "too late" → start immediately + warning. Persist to `~/.pentair/scheduled-heat.json`. Codex noted controller-native schedules exist — consider using those instead of daemon-local scheduler.
**Effort:** M (CC: ~2 hours)
**Depends on:** Matter bridge v1 working.

### Scene orchestration (pool party macros)
**What:** Config-driven scenes that execute multiple commands (e.g., "pool party" = spa on + jets on + lights Caribbean). Exposed as Matter Scenes cluster for Google Home voice control ("Hey Google, pool party").
**Why:** Accepted in CEO review (2026-03-25). The demo moment. Deferred to v2 by eng review to ship core bridge first.
**Context:** TOML config defines scenes with target + command pairs. Need serialization, cancellation, idempotency, and rules for live user commands landing mid-scene. More complex than "sequential commands."
**Effort:** M (CC: ~2 hours)
**Depends on:** Matter bridge v1 working.

### QR code commissioning in web UI
**What:** Generate Matter QR code server-side (`qrcode` crate), serve as PNG at `GET /api/matter/qr`, display in daemon web UI for easy commissioning.
**Why:** Accepted in CEO review (2026-03-25). Polishes first-time setup. Deferred to v2 by eng review.
**Context:** Matter QR code format encodes discriminator, passcode, vendor ID (spec section 5.1.3). Consider auth implications — daemon web UI is currently unauthenticated on LAN.
**Effort:** S (CC: ~15 min)
**Depends on:** Matter bridge v1 working.

## P3 — Deferred features

### Remote access / cloud relay
**What:** Lightweight cloud relay so Android app and Google Home work away from home WiFi.
**Why:** Currently LAN-only — VPN is the stopgap for remote access.
**Context:** Daemon connects outbound to Firebase Function or small VPS via WebSocket. App detects LAN vs remote and routes accordingly. Architecture supports this without changes — daemon's REST API is the same.
**Effort:** L (CC: ~2 hours)
**Depends on:** Daemon + REST API stable.
