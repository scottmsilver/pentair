# Spa Heat Live Activities & Live Updates — Design Spec

## Overview

Replace the 5 separate push notifications for spa heating milestones with a single persistent live widget on both iOS (Live Activity + Dynamic Island) and Android (Live Update with ProgressStyle). **Milestone alerts (haptic/sound) are preserved** — each milestone update triggers an alert sound so the user still gets buzzed at key moments (HeatingStarted, Halfway, AlmostReady, AtTemp).

## Approach

Dual-channel delivery:
- **iOS**: APNs `liveactivity` push type → updates the Widget Extension directly, works even when app is killed
- **Android**: FCM data messages → app code posts/updates ProgressStyle notification

**Important**: Live Activities must be started locally by the app (not remotely via push). The app starts the activity when it detects spa heating via WebSocket, then registers the per-session APNs push token with the daemon for subsequent remote updates.

## Daemon Changes

### APNs configuration

New `[apns]` section in daemon TOML config:

```toml
[apns]
key_id = "CMMDZ3CMW8"
team_id = "M8B368H9T5"
key_path = "~/.pentair/firebase/AuthKey_CMMDZ3CMW8.p8"
bundle_id = "com.ssilver.pentair.ios"
```

New `apns.rs` module: HTTP/2 client using `reqwest` with HTTP/2 support, ES256 JWT signing via `jsonwebtoken` crate with `ring` P-256 backend. JWT cached for 55 minutes (APNs tokens expire at 60 min). Same pattern as `fcm.rs` token caching.

### APNs error handling

| APNs Status | Meaning | Action |
|---|---|---|
| 200 | Success | Continue |
| 400 | Bad request | Log, don't retry |
| 403 | Auth failure | Clear cached JWT, retry once |
| 410 | Token expired | Remove Live Activity token, fall back to FCM |
| 429 | Rate limited | Back off, skip this update |
| 500/503 | APNs down | Retry with backoff, max 2 retries |

### Structured data payloads

`spa_notifications.rs` currently calls `fcm.send(title, body)` with plain text. Extend `SpaHeatNotificationEvent` with new fields:

```rust
pub struct SpaHeatNotificationEvent {
    pub milestone: SpaHeatMilestone,
    pub current_temp_f: Option<f64>,
    pub target_temp_f: Option<f64>,
    pub start_temp_f: Option<f64>,       // NEW
    pub progress_pct: Option<u8>,         // NEW (0-100)
    pub minutes_remaining: Option<u32>,
    pub session_id: String,               // NEW (ISO 8601 timestamp of session start)
}
```

### Milestone-to-state mapping

The existing 5 milestones map to 3 Live Activity states:

| Milestone | Live Activity State | Alert Sound? |
|---|---|---|
| HeatingStarted | STARTED | Yes — "Spa heating started" |
| EstimateReady | TRACKING | No — silent update (progress bar appears) |
| Halfway | TRACKING | Yes — "Halfway there" buzz |
| AlmostReady | TRACKING | Yes — "Almost ready" buzz |
| AtTemp | REACHED | Yes — "Spa is ready!" full alert |

Intermediate 30s refresh updates (no milestone change) are **silent** — they update the progress bar and ETA but don't buzz.

### FCM data message format

For Android, send as FCM **data+notification** message. The `data` payload drives the Live Update UI; the `notification` payload provides the alert sound for milestone events and backward compatibility for older app versions:

```json
{
  "message": {
    "token": "...",
    "notification": {
      "title": "Spa heating: halfway there",
      "body": "96°F → 102°F, about 8 min remaining"
    },
    "data": {
      "kind": "spa_heat",
      "milestone": "halfway",
      "current_temp_f": "96",
      "target_temp_f": "102",
      "start_temp_f": "86",
      "progress_pct": "62",
      "minutes_remaining": "8",
      "session_id": "2026-03-25T04:38:00Z"
    }
  }
}
```

For silent progress updates (no milestone), omit the `notification` block and send data-only.

### APNs Live Activity payload

```json
{
  "aps": {
    "timestamp": 1711339086,
    "event": "update",
    "sound": "default",
    "content-state": {
      "currentTempF": 96,
      "targetTempF": 102,
      "startTempF": 86,
      "progressPct": 62,
      "minutesRemaining": 8,
      "phase": "tracking",
      "milestone": "halfway"
    }
  }
}
```

Note: APNs uses `camelCase` (Swift conventions); FCM data uses `snake_case`. The daemon produces both serializations from the same `SpaHeatNotificationEvent`.

For silent progress updates: omit `"sound"` field.
For end: `"event": "end"`, `"dismissal-date"` set to now + 30 seconds.

### Device token registration extension

Extend `/api/devices/register` to accept platform and Live Activity tokens:

```json
{
  "token": "fcm-token-here",
  "platform": "ios",
  "live_activity_token": "apns-live-activity-push-token"
}
```

The `live_activity_token` is per-session — it changes each time a new Live Activity is started. The daemon replaces the previous Live Activity token for that device.

### Device storage schema change

`devices.json` changes from a flat token list to per-device records:

```json
{
  "devices": [
    {
      "fcm_token": "abc...",
      "platform": "android"
    },
    {
      "fcm_token": "def...",
      "platform": "ios",
      "live_activity_token": "ghi..."
    }
  ]
}
```

Migration: on startup, if `devices.json` has the old `{"tokens": [...]}` format, auto-migrate to the new format (assume platform "unknown", no live_activity_token). Re-registration from the apps will populate platform and tokens correctly.

### Token delivery race condition

After the iOS app starts a Live Activity, there's a brief window before the push token arrives at the daemon. During this window:
- Daemon sends FCM data messages (as usual)
- iOS app updates the Live Activity locally from WebSocket data
- Once the Live Activity push token is registered, daemon begins APNs updates

This is a natural handoff — no special handling needed.

### Update frequency

Continue using the existing 30-second refresh cycle in `adapter.rs` for milestone evaluation. For smooth ETA countdown between server updates, clients use device-local countdown timers:
- iOS: `Text(timerInterval:)` in the widget
- Android: `setChronometerCountdown(true)` + `setWhen()` on the notification

### 8-hour limit (iOS)

Apple automatically ends Live Activities after 8 hours. Spa heating typically completes in under 1 hour. If a session somehow exceeds 8 hours (unlikely), the Live Activity will end automatically and the user will still receive the AtTemp push notification via FCM.

## iOS Implementation

### New Widget Extension target

A new WidgetKit extension target `PentairLiveActivity` (bundle ID: `com.ssilver.pentair.ios.live-activity`) containing:

- `SpaHeatAttributes.swift` — shared `ActivityAttributes`:
  ```swift
  struct SpaHeatAttributes: ActivityAttributes {
      struct ContentState: Codable, Hashable {
          var currentTempF: Int
          var targetTempF: Int
          var startTempF: Int
          var progressPct: Int
          var minutesRemaining: Int?
          var phase: String      // "started", "tracking", "reached"
          var milestone: String? // "heating_started", "halfway", etc.
      }
      var spaName: String
  }
  ```
- `SpaHeatLiveActivity.swift` — `ActivityConfiguration` with views for:
  - Lock Screen: progress bar + current temp + target temp + ETA
  - Dynamic Island compact: current temp (leading) + ETA (trailing)
  - Dynamic Island expanded: full progress bar + temps + ETA + phase label
  - Dynamic Island minimal: temperature only

### Info.plist additions

- `NSSupportsLiveActivities = YES`
- `NSSupportsLiveActivitiesFrequentUpdates = YES`

### App-side lifecycle

In `PoolViewModel.swift`:

1. **Start**: When WebSocket state shows spa heating started → call `Activity.request(attributes:content:pushType: .token)`
2. **Observe push token**: `for await tokenData in activity.pushTokenUpdates` → send token to daemon via POST `/api/devices/register` with `live_activity_token`
3. **Local update**: On WebSocket state change while app is foreground → call `activity.update()`
4. **End**: When spa reaches temperature or spa turned off → call `activity.end(content, dismissalPolicy: .after(Date().addingTimeInterval(30)))`

Remote updates via APNs happen automatically when daemon sends to the Live Activity push token.

### Fallback

If `ActivityAuthorizationInfo().areActivitiesEnabled == false`, fall back to standard push notifications (existing behavior). No code change needed — the existing FCM notification path still works.

## Android Implementation

### New class: `SpaHeatLiveUpdate.kt`

Builds and manages the ProgressStyle notification:

```kotlin
class SpaHeatLiveUpdate(private val context: Context) {

    fun start(data: SpaHeatData) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) {
            postProgressStyle(data)
        } else {
            postFallbackNotification(data)
        }
    }

    fun update(data: SpaHeatData) { /* re-post with same notification ID */ }

    fun end(finalTemp: Int) { /* post "Ready!" then cancel after 30s delay */ }
}
```

### ProgressStyle configuration (API 36+)

```kotlin
Notification.ProgressStyle()
    .setProgress(data.progressPct)  // 0-100
    .setProgressSegments(listOf(
        Segment(data.progressPct).setColor(ORANGE),   // heated portion
        Segment(100 - data.progressPct).setColor(GRAY) // remaining
    ))
```

Status bar chip: fire icon + ETA countdown via `setChronometerCountdown(true)` + `setWhen(targetTimeMs)`.

### AndroidManifest.xml

Add: `<uses-permission android:name="android.permission.POST_PROMOTED_NOTIFICATIONS" />`

This is a normal (not dangerous) permission — auto-granted at install, no runtime prompt needed. User can disable Live Updates for the app in system Settings.

### FCM data message handling

In `PoolFcmService.onMessageReceived()`:
- If `data["kind"] == "spa_heat"`, route to `SpaHeatLiveUpdate`
- Otherwise, display as standard notification (existing behavior)

### Fallback (pre-Android 16)

Standard ongoing notification with `setProgress(100, pct, false)` and `BigTextStyle`:
```
Spa Heating: 96°F → 102°F (~8 min remaining)
████████████░░░░░░░░░  62%
```

## State Machine

```
     spa heat on + pump active
              │
    ┌─────────▼──────────┐
    │   STARTED          │  Start Live Activity/Update
    │   No ETA yet       │  Show "Heating..." + indeterminate
    │   BUZZES: yes      │  Alert: "Spa heating started"
    └─────────┬──────────┘
              │ minutes_remaining != nil
    ┌─────────▼──────────┐
    │   TRACKING         │◄── update every 30s (silent)
    │   temp + progress  │    milestone events buzz:
    │   + ETA countdown  │    Halfway, AlmostReady
    └─────────┬──────────┘
              │ current_temp >= target_temp
    ┌─────────▼──────────┐
    │   REACHED          │  End Live Activity/Update
    │   "Spa is ready!"  │  BUZZES: yes (full alert)
    │   Auto-dismiss 30s │
    └────────────────────┘

  At any point:
    spa turned off    → End with "Cancelled" (silent)
    adapter disconn.  → Freeze last state (client shows stale)
    app killed (iOS)  → APNs continues updating widget directly
    app killed (Andr) → Notification persists with last state
    8h limit (iOS)    → System ends activity; AtTemp push still sent via FCM
```

## Visual Design

### Information Hierarchy (all surfaces)

Priority order — what the user sees first:
1. **Current temperature** — largest/boldest number. The thing you care about.
2. **Progress bar** — instant visual "how far along." Full width.
3. **ETA** — "when will it be ready?" Orange accent to stand out.
4. **Target temperature** — context. Secondary weight.
5. **Milestone label** — flavor text. Smallest, gray.

### Layouts

```
iOS LOCK SCREEN (160pt tall):
┌─────────────────────────────────────────┐
│  🔥 Spa Heating              ~14 min    │  ← title (15pt semibold) + ETA (15pt, orange)
│                                          │
│    92°F ───────────────▶ 102°F          │  ← temps (28pt bold) are the hero
│    ████████████░░░░░░░░░░  62%          │  ← progress bar (full width, 8pt tall)
│                                          │
│    Halfway there                         │  ← milestone label (13pt, secondary gray)
└─────────────────────────────────────────┘

iOS DYNAMIC ISLAND COMPACT (36pt tall):
  Leading: current temp (17pt bold, white)
  Center: mini progress bar (no text)
  Trailing: minutes remaining (17pt, orange)

iOS DYNAMIC ISLAND EXPANDED (~160pt):
┌──────────────────────────────────────────┐
│  🔥  Spa Heating                         │
│     92°F                    102°F        │
│  ████████████████░░░░░░░░░░░░░          │
│  Halfway there        ~14 min remaining  │
└──────────────────────────────────────────┘

iOS DYNAMIC ISLAND MINIMAL (36pt circle):
  Current temp only (15pt bold)

ANDROID NOTIFICATION SHADE:
┌─────────────────────────────────────────┐
│  🔥 Spa Heating             ~14 min     │
│  ████████████░░░░░░░░░  92°F → 102°F  │
│  Halfway there                          │
└─────────────────────────────────────────┘

ANDROID STATUS BAR CHIP (96dp max):
  🔥 14m  (icon + chronometer countdown)
```

### Progress Bar Color Semantics

| Progress | Color | Hex | Rationale |
|---|---|---|---|
| 0-30% | Deep orange | #FF6D00 | Cold → heating |
| 30-70% | Orange | #FF9100 | Mid-heat |
| 70-90% | Amber | #FFC107 | Getting close |
| 90-100% | Green | #4CAF50 | At/near temperature |

The bar "warms up" in color as the spa heats — not just a generic fill.

### Visual State Table

| State | Lock Screen | Dynamic Island | Android Shade | Android Chip |
|---|---|---|---|---|
| STARTED (no ETA) | "Spa Heating" + animated pulse bar + temp | temp + "..." + pulse | Indeterminate bar + temp | 🔥 icon only |
| TRACKING (ETA) | Full layout: temp + bar + ETA + milestone | temp + bar + ETA | Full layout + chronometer | 🔥 14m countdown |
| TRACKING (milestone buzz) | Briefly highlights milestone text in orange | Brief expand animation | Heads-up peek | Same |
| REACHED | "Spa is ready! 102°F" + ✓ + green + bar 100% | ✓ + "Ready!" green | "Ready!" + green bar 100% | ✓ green |
| CANCELLED | "Cancelled" gray, dismiss in 5s | Dismiss immediately | Cancel notification, auto-dismiss 5s | Dismiss |
| STALE (adapter down) | Last state + "Updated 2m ago" gray | Last state, no animation | Last state + "Connection lost" | Icon dims |

### Tap Action

Tapping the Live Activity (iOS) or notification (Android) opens the app to the main dashboard, which already shows the spa section with current temp, setpoint, heat status, and controls. No deep-linking needed — the dashboard IS the detail view.

### Accessibility

- **VoiceOver**: "Spa heating, currently 92 degrees, target 102 degrees, approximately 14 minutes remaining, 62 percent progress"
- **TalkBack**: contentDescription with same information
- **Dynamic Type**: Lock screen title/milestone respect Dynamic Type. Temps use fixed size (must fit constraints).
- **Color contrast**: All text meets WCAG AA (4.5:1). Orange on dark passes. Green on dark passes.
- **Reduced Motion**: Skip pulse animation in STARTED state — show static indeterminate bar instead.

## Testing

- **Unit**: Data payload builder produces correct JSON for all 5 milestones
- **Unit**: APNs JWT signing produces valid ES256 token
- **Unit**: APNs payload has correct camelCase keys; FCM has snake_case
- **Unit**: ProgressStyle builder handles 0%, 50%, 100% progress
- **Unit**: Fallback detection (API < 36, Live Activities disabled)
- **Unit**: devices.json migration from old flat format to new per-device format
- **Unit**: Sound/alert included only for milestone updates, not silent progress
- **Integration**: Start spa heat → verify Live Activity starts on iOS device
- **Integration**: Progress updates reach lock screen while app is backgrounded
- **Integration**: AtTemp → Live Activity ends and dismisses with alert sound
- **Integration**: Kill iOS app → verify APNs updates still reach lock screen
- **Manual**: Dynamic Island compact + expanded rendering
- **Manual**: Android status bar chip with countdown
- **Manual**: Pre-Android-16 fallback notification renders correctly

## New Rust dependencies (daemon)

- `jsonwebtoken` — ES256 JWT signing for APNs
- No new HTTP dependency — `reqwest` already supports HTTP/2

## Files changed

### Daemon (Rust)
- `pentair-daemon/src/apns.rs` — NEW: APNs HTTP/2 sender with JWT auth
- `pentair-daemon/src/config.rs` — Add `[apns]` config section
- `pentair-daemon/src/devices.rs` — New per-device storage schema with migration
- `pentair-daemon/src/spa_notifications.rs` — Extend event struct with progress/session fields
- `pentair-daemon/src/adapter.rs` — Route milestone events to APNs + FCM
- `pentair-daemon/src/fcm.rs` — Send data+notification payloads
- `pentair-daemon/src/api/routes.rs` — Extend `/api/devices/register` for platform + LA token

### iOS (Swift)
- `pentair-ios/PentairLiveActivity/` — NEW Widget Extension target (2-3 files)
- `pentair-ios/PentairIOS/SpaHeatAttributes.swift` — NEW shared attributes
- `pentair-ios/PentairIOS/PoolViewModel.swift` — Start/end Live Activity
- `pentair-ios/PentairIOS/Info.plist` — Add Live Activity keys
- `pentair-ios/PentairIOS.xcodeproj/project.pbxproj` — Add extension target

### Android (Kotlin)
- `pentair-android/.../notifications/SpaHeatLiveUpdate.kt` — NEW
- `pentair-android/.../notifications/PoolFcmService.kt` — Route spa_heat data messages
- `pentair-android/app/src/main/AndroidManifest.xml` — Add permission
