# Spa Heat Push Notifications Design

Date: 2026-03-24

## Goal

Add cross-platform push notifications for spa heating progress, with server-side logic in the daemon and platform-native delivery on Android now and iOS later.

V1 is spa-only. Pool notifications are explicitly out of scope.

## Context

The daemon already has:
- device token storage in `pentair-daemon/src/devices.rs`
- FCM send support in `pentair-daemon/src/fcm.rs`
- transition detection hooks in `pentair-daemon/src/adapter.rs`
- server-side heat estimation and heating-session state in `pentair-daemon/src/heat.rs`

Android already has FCM receive/display plumbing. iOS does not yet have push plumbing, but the daemon contract should be platform-neutral so iOS can adopt it later without changing server semantics.

## User Outcome

When the spa is heating toward a target temperature, the user can receive:
- a halfway notification
- an almost-ready notification
- an at-temperature notification

These notifications should be based on trusted spa temperature, not stale shared-equipment readings.

## Scope

In scope:
- daemon-side milestone detection for spa heat sessions
- deduplicated per-session milestone firing
- push payload contract for progress notifications
- Android compatibility with current FCM notification receiving
- config for enabling/disabling spa heat progress notifications

Out of scope for V1:
- pool progress notifications
- iOS APNs implementation
- user-configurable milestone percentages in the mobile UI
- historical notification log

## Approaches Considered

### 1. Daemon milestones from heating-session progress

Use the daemon's heating-session model and trusted temperature rules to compute session progress and fire milestone notifications.

Pros:
- one source of truth across Android and iOS
- works when apps are backgrounded or disconnected
- reuses the daemon's existing trust and ETA logic
- easiest to keep consistent with heat estimation

Cons:
- requires daemon state tracking for fired milestones
- slightly more stateful than simple transition detection

### 2. Daemon notifications from raw temperature thresholds

Send notifications when the spa crosses specific thresholds such as halfway to target or within 2 degrees.

Pros:
- simpler implementation
- less session bookkeeping

Cons:
- less consistent across different target deltas
- awkward on small runs
- less aligned with the existing heat-session model

### 3. Client-side local milestone notifications

Have the mobile apps infer progress and schedule notifications themselves.

Pros:
- less daemon work

Cons:
- wrong architecture for this product
- fails when the app is backgrounded, killed, or disconnected
- duplicates logic across platforms

## Recommendation

Use approach 1.

The daemon should own spa heat progress milestones using the existing heating-session/trusted-temperature model. Clients should only receive and render the notification payload.

## Functional Design

### Session model

Reuse the daemon's existing spa heating session concept from `heat.rs`.

Each session needs:
- trusted start temperature
- target setpoint for the run
- current trusted temperature
- a set of milestones already fired

Progress notifications are only valid once:
- the spa session exists
- spa temperature is trustworthy
- the session has a meaningful trusted delta from start to target

### Milestones

V1 milestones:
- `halfway` at 50%
- `almost_ready` at 90%
- `at_temp` at 100%

Milestones are based on progress across the run:

`progress = (current_trusted_temp - trusted_start_temp) / (target_temp - trusted_start_temp)`

Rules:
- do not fire progress notifications for very small runs
- require minimum trusted delta of `4°F` for `halfway` and `almost_ready`
- `at_temp` still fires for any valid heating run

### Trust requirements

Milestones must use the same trust model as heat estimation:
- stale shared-equipment spa temperatures do not count
- warmup period does not count
- only trusted temperatures can advance milestone progress

This avoids false notifications caused by latched off-state spa readings.

### Deduplication

Each spa heating session tracks fired milestones:
- `halfway_sent`
- `almost_ready_sent`
- `at_temp_sent`

These flags must prevent duplicate notifications:
- during repeated periodic refreshes
- across websocket/poll churn
- after temporary adapter disconnects

V1 does not need persistence across daemon restarts. If the daemon restarts mid-session, missed/duplicate progress notifications are acceptable, as long as `at_temp` still behaves correctly after restart.

If restart durability becomes important later, milestone state can be folded into the persisted heat estimator store.

## Push payload contract

The daemon should send structured notification data internally, then map it to platform text.

Recommended internal event shape:

```json
{
  "kind": "spa_heat_progress",
  "milestone": "halfway",
  "current_temp": 97,
  "target_temp": 104,
  "minutes_remaining": 23
}
```

Milestone values:
- `halfway`
- `almost_ready`
- `at_temp`

Text defaults:
- halfway: `Spa warming up`
  - body: `About halfway to 104°`
- almost_ready: `Spa almost ready`
  - body: `About 10% left to 104°`
- at_temp: `Spa ready`
  - body: `Spa has reached 104°`

If `minutes_remaining` exists for `halfway` or `almost_ready`, Android/iOS may later use it for richer local formatting, but V1 server text should stay simple.

## Config

Add daemon config under notifications:

```toml
[notifications.spa_heat]
enabled = true
halfway = true
almost_ready = true
at_temp = true
minimum_delta_f = 4.0
```

Defaults:
- enabled: `true`
- halfway: `true`
- almost_ready: `true`
- at_temp: `true`
- minimum_delta_f: `4.0`

This is intentionally small. No per-user or per-device settings in V1.

## Android / iOS delivery strategy

### Android

Current Android FCM receive/display path can consume the push immediately.

V1 can continue using notification title/body strings from the daemon, with optional structured extras added later.

### iOS

iOS push delivery is not implemented yet.

The daemon contract should still be designed so that iOS can later consume the same event semantics through APNs or an FCM-for-iOS bridge without changing milestone logic.

## Implementation plan shape

### Daemon

1. Extend config for `notifications.spa_heat`
2. Add milestone state to the active spa heating session in `heat.rs`
3. Add a helper that inspects current trusted session progress and returns zero or more notification events
4. Call that helper from adapter transition/update flow
5. Send notifications through `FcmSender`

### Android

No mandatory code changes for V1 if daemon-generated title/body text is sufficient.

Optional follow-up:
- inspect data payload
- map milestone types to local notification channels or actions

### iOS

No implementation in V1.

Follow-up will need:
- APNs or Firebase Messaging client plumbing
- device token registration endpoint usage
- notification presentation handling

## Failure handling

- If FCM is not configured, daemon should still compute milestones but simply not send pushes.
- If no registered devices exist, do nothing.
- If notification send fails for a token, existing invalid-token handling should continue to prune dead tokens.
- Milestone detection must not block normal adapter refresh work.

## Testing

### Daemon unit tests

Add tests for:
- halfway fires once when progress crosses 50%
- almost_ready fires once when progress crosses 90%
- at_temp fires once when trusted temperature reaches setpoint
- stale/warmup temperatures do not fire milestones
- runs below `minimum_delta_f` skip halfway/almost_ready but still allow at_temp
- duplicate refreshes do not re-fire the same milestone

### Integration tests

Add daemon-side tests around the notification event builder, independent of actual FCM transport.

### Manual verification

Real spa heating run:
- start below setpoint
- observe halfway notification
- observe almost-ready notification
- observe at-temp notification
- verify no duplicates on repeated refreshes

## Risks

- Integer temperature readings make progress coarse, especially on small deltas.
- Restarting the daemon mid-session can lose in-memory milestone flags in V1.
- If the trusted start temperature is captured too late, progress percentages will be skewed.

## Decision

Implement spa-only daemon-side progress notifications based on trusted heating-session progress, with milestones at 50%, 90%, and at temperature.
