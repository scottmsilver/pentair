# Spa Heat Push Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add spa-only push notifications for heating-started, first-real-estimate, halfway, almost-ready, and at-temperature heat milestones from the daemon with minimal client work.

**Architecture:** Extend the daemon config with spa-heat notification settings, teach the heat estimator to track per-session milestone state using trusted spa temperature, and emit structured spa heat notification events during adapter updates. The adapter will map those events to FCM pushes using the existing device-token and FCM sender plumbing. Android/iOS clients remain unchanged in V1 because the daemon supplies title/body text.

**Tech Stack:** Rust, serde/toml config parsing, existing `pentair-daemon` heat estimator and FCM sender, Cargo tests

---

## File Structure

- Modify: `pentair-daemon/src/config.rs`
  - Add notification config types and defaults for spa-heat progress notifications.
- Modify: `pentair-daemon/src/heat.rs`
  - Add session milestone state, milestone event calculation, and unit tests.
- Modify: `pentair-daemon/src/adapter.rs`
  - Request milestone events from the heat estimator and dispatch FCM pushes.
- Modify: `pentair-daemon/src/fcm.rs`
  - Keep the current sender API or add a tiny structured helper if needed, but avoid client-facing payload churn in V1.
- Modify: `docs/api-spec.md` or daemon config docs only if runtime/config surface changes need documentation.
- Optionally modify: `/tmp/pentair-daemon-8080.toml`
  - Only for local manual verification, not for committed repo changes.

### Task 1: Add Config Surface

**Files:**
- Modify: `pentair-daemon/src/config.rs`

- [ ] **Step 1: Write the failing config tests**

Add tests in `pentair-daemon/src/config.rs` that deserialize:
- default config with notifications absent
- explicit `[notifications.spa_heat]` values

Assertions:
- defaults are `enabled=true`, `halfway=true`, `almost_ready=true`, `at_temp=true`, `minimum_delta_f=4.0`
- explicit values override defaults

- [ ] **Step 2: Run config-focused test to verify it fails**

Run:

```bash
cargo test -p pentair-daemon config::
```

Expected: FAIL because notification config types/fields do not exist yet.

- [ ] **Step 3: Write minimal config implementation**

Add:
- `NotificationsConfig`
- `SpaHeatNotificationsConfig`
- `#[serde(default)] pub notifications: NotificationsConfig` on `Config`
- default helpers for the spa-heat notification settings

Keep the shape:

```toml
[notifications.spa_heat]
enabled = true
halfway = true
almost_ready = true
at_temp = true
minimum_delta_f = 4.0
```

- [ ] **Step 4: Run config tests to verify they pass**

Run:

```bash
cargo test -p pentair-daemon config::
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/config.rs
git commit -m "feat: add spa heat notification config"
```

### Task 2: Add Failing Milestone Tests

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

- [ ] **Step 1: Write failing daemon tests for milestone behavior**

Add tests covering:
- heating-started fires once when spa heating genuinely begins
- estimate-ready fires once when the first trustworthy ETA becomes available
- halfway fires once when progress crosses 50%
- almost-ready fires once when progress crosses 90%
- at-temp fires once when trusted temperature reaches setpoint
- stale shared-body temps during warmup fire nothing
- runs below `minimum_delta_f` skip halfway/almost-ready
- duplicate updates do not re-fire milestones

Test via a pure helper API in `heat.rs`, not through FCM transport.

- [ ] **Step 2: Run the focused heat tests to verify they fail**

Run:

```bash
cargo test -p pentair-daemon heat::tests::spa_
```

Expected: FAIL because milestone state/event helpers do not exist yet.

- [ ] **Step 3: Add the minimal milestone model**

In `heat.rs`, add:
- `SpaHeatMilestone` enum
- `SpaHeatNotificationEvent` struct
- per-session fired flags on the active spa heating session

Do not wire adapter/FCM yet. Only add enough model state for the tests.

- [ ] **Step 4: Re-run focused tests**

Run:

```bash
cargo test -p pentair-daemon heat::tests::spa_
```

Expected: still FAIL, but now on missing logic rather than missing types.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "test: add spa heat notification milestone regressions"
```

### Task 3: Implement Milestone Engine

**Files:**
- Modify: `pentair-daemon/src/heat.rs`

- [ ] **Step 1: Implement milestone progress calculation**

Use trusted spa session data to calculate:

```rust
progress = (current_temp_f - start_temp_f) / (target_temp_f - start_temp_f)
```

Rules:
- only use trusted spa temperature
- skip `halfway`/`almost_ready` if target delta `< minimum_delta_f`
- `at_temp` allowed for any valid run

- [ ] **Step 2: Implement per-session deduplication**

Store fired flags on the active spa session:
- `halfway_sent`
- `almost_ready_sent`
- `at_temp_sent`

Ensure repeated updates only emit each event once per session.

- [ ] **Step 3: Implement a pure event builder**

Add a `HeatEstimator` method that returns zero or more `SpaHeatNotificationEvent`s for the current update, for example:

```rust
pub fn update(&mut self, system: &PoolSystem) -> Vec<SpaHeatNotificationEvent>
```

or a similarly narrow helper if keeping `update()` unchanged is cleaner.

The event should include:
- milestone kind
- current temp
- target temp
- minutes remaining when known

- [ ] **Step 4: Run the milestone tests**

Run:

```bash
cargo test -p pentair-daemon heat::tests::spa_
```

Expected: PASS

- [ ] **Step 5: Run the full daemon test suite**

Run:

```bash
cargo test -p pentair-daemon
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add pentair-daemon/src/heat.rs
git commit -m "feat: add spa heat milestone event engine"
```

### Task 4: Wire Milestones to Push Delivery

**Files:**
- Modify: `pentair-daemon/src/adapter.rs`
- Modify: `pentair-daemon/src/fcm.rs` (only if a small helper improves clarity)

- [ ] **Step 1: Write failing adapter-level tests or focused assertions**

If adapter tests are practical, add a small test around notification-text formatting. If not, write a pure formatting helper with tests in `adapter.rs` or `fcm.rs`:
- heating-started => `Spa heating started` / `Heating to 104°`
- estimate-ready => `Spa ready in about 42 min` / `Current temperature 92°`
- halfway => `Spa warming up` / `About halfway to 104°`
- almost-ready => `Spa almost ready` / `About 10% left to 104°`
- at-temp => `Spa ready` / `Spa has reached 104°`

- [ ] **Step 2: Run the focused formatting test to verify it fails**

Run:

```bash
cargo test -p pentair-daemon notification
```

Expected: FAIL because the formatter/helper does not exist yet.

- [ ] **Step 3: Wire adapter updates to milestone pushes**

In `adapter.rs`:
- collect milestone events from the heat estimator during status refresh/update
- if FCM is configured, convert each event to title/body text
- send through existing `FcmSender`

Keep:
- existing freeze/heater/connection-lost pushes untouched unless cleanup is obviously beneficial
- V1 limited to spa events only

- [ ] **Step 4: Re-run the formatting test**

Run:

```bash
cargo test -p pentair-daemon notification
```

Expected: PASS

- [ ] **Step 5: Re-run the full daemon test suite**

Run:

```bash
cargo test -p pentair-daemon
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add pentair-daemon/src/adapter.rs pentair-daemon/src/fcm.rs
git commit -m "feat: send spa heat progress push notifications"
```

### Task 5: Document and Manually Verify

**Files:**
- Modify: `docs/api-spec.md` only if config/runtime docs need it
- Modify: `README.md` only if user-facing daemon capability docs need it

- [ ] **Step 1: Update docs/config notes**

Document the new config block and the fact that spa-only heat progress notifications exist server-side.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo test -p pentair-daemon
cargo build -p pentair-daemon --release
```

Expected: PASS

- [ ] **Step 3: Manual local verification**

Use the local daemon config on a non-default port if needed, start a real spa heating run, and confirm:
- no pushes during stale warmup
- halfway push once
- almost-ready push once
- at-temp push once

- [ ] **Step 4: Commit**

```bash
git add docs/api-spec.md README.md
git commit -m "docs: describe spa heat progress notifications"
```
