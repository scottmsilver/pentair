# Spa Heating Notification State Machine

## Problem

When the spa reaches its target temperature, the heater turns off. As the water cools, the heater kicks back on, creating a reheat cycle. The current notification system treats each reheat cycle as a new heating session, resetting all milestone flags and generating a full notification sequence (HeatingStarted → EstimateReady → Halfway → AlmostReady → AtTemp) every time. This is noisy — the user only cares about the initial heat-up.

## Solution

Replace the flat boolean flags in `SpaHeatNotificationState` with a three-phase state machine. Only the initial heat-up toward the setpoint generates notifications. Maintenance reheat cycles are silent.

## State Machine

```
Idle ──→ Heating ──→ Maintaining ──→ Idle
            ↑              │
            └──────────────┘
           (setpoint raised above current temp)
```

### Phases

| Phase | Entry condition | Notifications | Exit |
|---|---|---|---|
| **Idle** | `spa_on == false` | None | Spa turns on with heat demand |
| **Heating** | First heat-up or setpoint raised | All milestones fire (HeatingStarted, EstimateReady, Halfway, AlmostReady, AtTemp) | AtTemp fires → Maintaining |
| **Maintaining** | Reached setpoint | Silent — heater cycles ignored | Spa off → Idle; setpoint raised → Heating |

### Transition Rules

1. **Any phase + `!spa_on`** → Idle (reset everything)
2. **Idle + `spa_on && heating_active`** → Heating (fire HeatingStarted)
3. **Heating + AtTemp fires** → Maintaining (capture current setpoint)
4. **Maintaining + `setpoint > state.setpoint`** → Heating (new intent, fire HeatingStarted)
5. **Maintaining + heater cycles on/off** → stay Maintaining (silent)
6. **Maintaining + setpoint lowered** → stay Maintaining (no heating work needed)

## Changes

### `pentair-daemon/src/spa_notifications.rs` — logic changes

Replace `SpaHeatNotificationState` flat struct with enum:

```rust
enum NotificationPhase {
    Idle,
    Heating {
        heating_started_sent: bool,
        estimate_ready_sent: bool,
        halfway_sent: bool,
        almost_ready_sent: bool,
        at_temp_sent: bool,
        trusted_session_id: Option<i64>,
    },
    Maintaining {
        setpoint: i32,
    },
}
```

`evaluate_spa_heat_notifications` matches on phase:
- Idle: check for spa_on + heating_active to transition to Heating
- Heating: existing milestone logic (identical to current implementation)
- Maintaining: check for spa_off (→ Idle) or setpoint raise (→ Heating), otherwise no-op

Add `spa_on: bool` to `SpaHeatNotificationInput`.

### `pentair-daemon/src/heat.rs` — one field addition

Pass `spa.on` into `SpaHeatNotificationInput` as `spa_on`.

### No other changes

Notification text, event struct, FCM/APNs delivery, mobile clients — all untouched.

## Tests

Update existing tests to pass `spa_on: true`. Add new tests:

- **Reheat cycle silence**: Heating → AtTemp → Maintaining, then heating_active cycles off/on → no notifications
- **Setpoint raise re-enters Heating**: In Maintaining at setpoint 102, raise to 106 → fires HeatingStarted
- **Setpoint lower stays Maintaining**: In Maintaining at 104, lower to 100 → no notifications
- **Spa off resets**: Maintaining → spa_on=false → Idle, then spa_on=true + heating → fresh Heating sequence
