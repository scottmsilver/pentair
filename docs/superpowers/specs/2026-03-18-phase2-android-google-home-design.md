# Phase 2 Design: Android App + Google Home + Push Notifications

## Context

Phase 1 is complete: Rust workspace with protocol library, async TCP client, REST/WebSocket daemon with semantic API, CLI tool, and embedded web UI. The daemon's semantic API (`GET /api/pool`, `POST /api/spa/on`, etc.) is documented at `docs/api-spec.md` and tested against live hardware (IntelliTouch at 192.168.1.89:80).

Phase 2 adds three consumer applications on top of the existing daemon API.

## Architecture

```
                     YOUR PHONE
  ┌─────────────────┐  ┌──────────────────────┐
  │  Android App     │  │  Google Home App     │
  │  (Kotlin+Compose)│  │  (voice control)     │
  │  Pool visual UI  │  │  "warm up the spa"   │
  │  FCM push recv   │  └──────────┬───────────┘
  └────────┬─────────┘             │
           │ HTTP/WS (LAN)         │ Matter (LAN)
           ▼                       ▼
┌──────────────────────────────────────────────────┐
│              pentair-daemon                       │
│  Semantic API │ mDNS │ FCM push │ Matter bridge  │
└──────────────────────┬───────────────────────────┘
                       │ TCP (ScreenLogic)
                       ▼
              ScreenLogic Adapter
```

All three consumers use the same semantic REST API. The daemon gets additions: mDNS advertisement, FCM push, device token storage, and Matter bridge for voice assistants.

## Component 1: Android App

### Technology

- **Language:** Kotlin
- **UI:** Jetpack Compose
- **Min SDK:** 26 (Android 8.0)
- **HTTP:** Retrofit + Moshi (JSON)
- **WebSocket:** OkHttp WebSocket
- **Discovery:** Android NsdManager (mDNS/DNS-SD)
- **Notifications:** Firebase Cloud Messaging
- **DI:** Hilt
- **Architecture:** Single-activity, single-screen app. No navigation graph needed.

### UI Design

Mimics the existing web UI exactly:

- **Pool visual** — Custom Canvas composable drawing the pool shape with spa top-right
- **Spa area** — Temperature, setpoint (tappable to open bottom sheet), segmented Off/Spa/Jets control
- **Pool area** — Temperature, setpoint (tappable to open bottom sheet)
- **Lights** — Collapsible color swatch strip at bottom of pool. Selected mode shows as filled circle; tap to expand and pick a color.
- **Gear drawer** — settings icon top-right, slides out auxiliaries + system info
- **Dark theme** — matches web UI color palette (#0c1222 background, teal spa, blue pool)

### Screen: PoolScreen

Single screen rendering the entire `GET /api/pool` response:

```
┌─────────────────────────┐
│                       ⚙ │
│  ┌─────────┬──────┐     │
│  │         │108°  │     │
│  │         │set104│     │
│  │ 105°    │Off Sp│Jets │
│  │ set 59° │      │     │
│  │         └──────┘     │
│  │ ⏻ 🟢🔴🟣🔵          │
│  └─────────────────┘     │
│                          │
│                          │
└─────────────────────────┘
```

### Data Flow

1. App starts -> NsdManager discovers `_pentair._tcp.local` -> gets daemon IP:port
2. App calls `GET /api/pool` to get initial state
3. App opens WebSocket at `/api/ws`
4. On WebSocket `StatusChanged` event -> re-fetch `GET /api/pool`
5. User taps control -> POST to semantic endpoint -> optimistic UI update -> verify on next fetch
6. On first launch, app registers FCM token with daemon via `POST /api/devices/register`

### Android Lifecycle

- **Foreground:** WebSocket connected, live updates via StatusChanged events
- **Background:** WebSocket disconnected (battery). Push notifications via FCM handle alerts.
- **Resume:** Reconnect WebSocket + full `GET /api/pool` refresh
- **PoolRepository** ties into `ProcessLifecycleOwner` to manage WebSocket lifecycle

### File Structure

```
pentair-android/
  app/src/main/
    java/com/ssilver/pentair/
      PoolApp.kt                    -- Application, Hilt setup
      MainActivity.kt               -- Single activity, sets Compose content

      data/
        PoolApiClient.kt            -- Retrofit interface for semantic API
        PoolRepository.kt           -- HTTP + WebSocket + lifecycle, exposes StateFlow<PoolSystem>
        PoolSystem.kt               -- Data classes for /api/pool response
        DeviceTokenManager.kt       -- FCM token registration with daemon

      di/
        NetworkModule.kt            -- Hilt @Module: Retrofit, OkHttp, base URL from discovery

      discovery/
        DaemonDiscovery.kt          -- NsdManager wrapper, finds _pentair._tcp.local

      ui/
        PoolScreen.kt               -- Main screen composable
        PoolVisualCanvas.kt         -- Custom Canvas: pool shape, spa, water effects
        SpaSegmentedControl.kt      -- Off | Spa | Jets segmented control
        LightPicker.kt              -- Collapsible color swatch row
        SetpointBottomSheet.kt      -- Temperature +/- picker
        SettingsDrawer.kt           -- Gear icon -> auxiliary toggles + system info
        theme/
          Theme.kt                  -- Colors, typography matching web UI
          Color.kt                  -- Pool blue, spa teal, deck gray, etc.

      notifications/
        PoolFcmService.kt           -- FirebaseMessagingService subclass
        NotificationHelper.kt       -- Builds and shows local notifications

    res/
      values/colors.xml             -- Color palette
      drawable/                     -- App icon
```

### Key Implementation Details

**Pool Visual (Canvas):**
- Compose `Canvas` composable with `drawRoundRect` for pool and spa shapes
- Spa drawn as separate rounded rect top-right with 5px gap (deck)
- Water gradient fills: `Brush.linearGradient` for pool blue, spa teal
- Caustic shimmer via animated alpha on radial gradient overlays
- Text drawn with `drawText` for temperatures

**mDNS Discovery:**
- Use `NsdManager.discoverServices("_pentair._tcp", NsdManager.PROTOCOL_DNS_SD, listener)`
- On service found -> `NsdManager.resolveService` -> get host + port
- Cache discovered address in SharedPreferences as fallback
- If discovery fails, fall back to cached address or manual entry in settings

**WebSocket:**
- OkHttp `WebSocket` connecting to `ws://{daemon}/api/ws`
- On message received -> trigger repository refresh
- Auto-reconnect with exponential backoff on disconnect
- Disconnect on app background, reconnect on foreground (via ProcessLifecycleOwner)

**Optimistic Updates:**
- When user taps a control, update the local UI state immediately
- POST to daemon in background
- On next `GET /api/pool`, the real state overwrites the optimistic state
- If the POST fails, revert the optimistic update

**Hilt DI:**
- `NetworkModule` provides Retrofit + OkHttp scoped to the discovered daemon address
- Base URL is dynamic (discovered via mDNS), so Retrofit instance is created after discovery
- Repository is `@Singleton`, UI observes its `StateFlow<PoolSystem?>`

## Component 2: Google Home / Voice Assistant Integration

### Approach: Matter Bridge

Google's Local Home SDK is deprecated. The current standard for local smart home integration is **Matter** -- the industry protocol supported by Google Home, Apple HomeKit, and Amazon Alexa.

The daemon acts as a **Matter bridge**, exposing pool devices as Matter-compatible endpoints on the LAN. Google Home (and Apple Home, Alexa) discovers them automatically.

### Matter Device Types

| Device | Matter Type | Clusters | Maps to |
|--------|-------------|----------|---------|
| Pool Spa | Thermostat | OnOff, Thermostat | `/api/spa/on`, `/api/spa/heat` |
| Pool Jets | On/Off Plug-in Unit | OnOff | `/api/spa/jets/on`, `/api/spa/jets/off` |
| Pool Lights | Extended Color Light | OnOff, ColorControl | `/api/lights/on`, `/api/lights/mode` |
| Pool | On/Off Plug-in Unit | OnOff | `/api/pool/on`, `/api/pool/off` |

### Implementation

Use the `rs-matter` Rust crate to implement a Matter bridge in the daemon. The bridge:

1. Advertises via mDNS (`_matter._tcp`)
2. Handles Matter fabric commissioning (one-time pairing via QR code or setup code)
3. Translates Matter cluster commands to semantic API calls internally
4. Reports device state back via Matter subscriptions

### Voice Commands

| What you say | Matter cluster | Daemon action |
|---|---|---|
| "Turn on the spa" | OnOff.On(spa) | `/api/spa/on` |
| "Turn off the spa" | OnOff.Off(spa) | `/api/spa/off` |
| "Set spa to 104" | Thermostat.SetpointRaise(spa, 104) | `/api/spa/heat {"setpoint":104}` |
| "Turn on the jets" | OnOff.On(jets) | `/api/spa/jets/on` |
| "Turn off the jets" | OnOff.Off(jets) | `/api/spa/jets/off` |
| "Turn on pool lights" | OnOff.On(lights) | `/api/lights/on` |
| "Set lights to caribbean" | ColorControl(lights, caribbean) | `/api/lights/mode {"mode":"caribbean"}` |
| "Turn on the pool" | OnOff.On(pool) | `/api/pool/on` |

### Bonus

Matter support gives us Apple HomeKit and Amazon Alexa for free -- same protocol, same bridge. No additional work per platform.

## Component 3: FCM Push Notifications

### Daemon Side

**New dependency:** `reqwest` for outbound HTTPS POST to FCM API.

**New endpoint:** `POST /api/devices/register`

```json
{"token": "fcm-device-token-string"}
```

Daemon stores tokens in a JSON file (`~/.pentair/devices.json`) so they survive restarts. Tokens are deduplicated on register. Invalid tokens (FCM returns 404/410) are automatically removed on failed push attempts.

**Previous state tracking:** The daemon's adapter task stores the last-known `PoolSystem` snapshot. On each refresh, it compares current vs previous state to detect transitions:

| Event | Detection | Notification |
|---|---|---|
| Spa ready | `spa.temperature >= spa.setpoint` transitions from false to true while `spa.on` | "Spa is ready -- 104F" |
| Freeze protection | `system.freeze_protection` transitions to true | "Freeze warning -- protection active" |
| Heater started | `spa.heating` or `pool.heating` transitions from "off" to non-"off" | "Spa heater started -- currently 98F, heating to 104F" |
| Connection lost | Adapter TCP connection drops | "Pool controller disconnected" |

**FCM API call:**
```
POST https://fcm.googleapis.com/v1/projects/{project_id}/messages:send
Authorization: Bearer {access_token}

{
  "message": {
    "token": "{device_token}",
    "notification": {
      "title": "Spa is ready",
      "body": "104F -- time to get in!"
    }
  }
}
```

**FCM Authentication:** Use a Firebase service account JSON file. The daemon uses the `jsonwebtoken` crate to sign a JWT with the service account's private key (RS256), exchanges it with Google's OAuth2 token endpoint for a short-lived access token, and refreshes before expiry (tokens last 1 hour). Config in `pentair.toml`:

```toml
[fcm]
service_account = "/path/to/firebase-service-account.json"
project_id = "your-firebase-project-id"
```

**FCM error handling:**
- 401 (bad auth) -> refresh OAuth2 token and retry once
- 404/410 (invalid token) -> remove token from devices.json
- 429 (rate limited) -> backoff and retry
- 500 (server error) -> log and skip

### Android Side

**FirebaseMessagingService subclass** receives pushes and shows notifications:

```kotlin
class PoolFcmService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        NotificationHelper.show(this, message.notification)
    }
    override fun onNewToken(token: String) {
        DeviceTokenManager.register(token)
    }
}
```

**Notification channel:** "Pool Alerts" with high importance for spa-ready, default for others.

## Component 4: Daemon Additions

### mDNS Advertisement

Add `mdns-sd` crate dependency. Advertise `_pentair._tcp.local` for app discovery:

```rust
let mdns = mdns_sd::ServiceDaemon::new()?;
let service = mdns_sd::ServiceInfo::new(
    "_pentair._tcp.local.",
    "Pentair Pool",
    &hostname,
    "",
    config.bind_port,
    None,
)?;
mdns.register(service)?;
```

### New Dependencies

- `mdns-sd` -- mDNS service advertisement and discovery
- `reqwest` -- outbound HTTP for FCM push (add to daemon, already in CLI)
- `jsonwebtoken` -- JWT signing for FCM OAuth2
- `rs-matter` -- Matter protocol bridge (for Google Home / HomeKit / Alexa)

### New Endpoints

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/devices/register` | POST | Store FCM device token |

### New Config (pentair.toml additions)

```toml
[fcm]
service_account = "/path/to/service-account.json"
project_id = "your-firebase-project-id"
```

### Device Token Storage

JSON file at `~/.pentair/devices.json`:
```json
{"tokens": ["token1", "token2"]}
```

Loaded at startup, updated on register (deduplicated), persisted on change. Concurrent writes protected by the existing SharedState RwLock.

## Build Order

1. **Daemon additions** (mDNS + device registration + FCM push) -- enables app and notifications
2. **Android app** -- core UI + discovery + API client + notifications
3. **Matter bridge** -- voice assistant integration

Steps 2 and 3 are independent after step 1.

## What's NOT in Phase 2

- Remote access / cloud relay (deferred)
- iOS app (Phase 2.5 -- same design, SwiftUI)
- Pool party macros / scenes
- Temperature history / analytics
- Conversational AI (Phase 3)
