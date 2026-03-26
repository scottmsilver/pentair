---
name: layering-review
description: |
  Cross-platform layering consistency review. Checks that the same feature uses
  the same architectural layer on iOS and Android, flags duplicated business logic
  that should live in the daemon, and catches multiple instances of the same manager.
  Use when adding or modifying features that span daemon + mobile clients.
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
  - AskUserQuestion
  - Agent
---

# /layering-review — Cross-Platform Layering Consistency

## When to use

Run this after implementing a feature that touches both iOS and Android (or daemon + any client). Catches architectural drift before it becomes technical debt.

## Step 1: Read the rules

Read `CLAUDE.md` and find the "Mobile Architecture — Layering Rules" section. These are the project's stated layering conventions. All findings are calibrated against them.

If no layering rules exist in CLAUDE.md, use these universal defaults:
- Data layer: networking, caching, persistence. No UI logic.
- ViewModel: state machines, transition detection, feature lifecycle.
- View: pure rendering from state. No business logic.

## Step 2: Identify the feature scope

Ask: what feature or change is being reviewed? If not obvious from context, check:
```bash
git diff origin/main --stat
git log origin/main..HEAD --oneline
```

Identify all files related to the feature across:
- Daemon (Rust)
- Android (Kotlin)
- iOS (Swift)

## Step 3: Layer mapping

For each platform, classify where feature logic lives:

```
FEATURE: {name}
──────────────────────────────────────────────────────────
              DAEMON          ANDROID           iOS
──────────────────────────────────────────────────────────
Detection:    {where}         {where}           {where}
State track:  {where}         {where}           {where}
Computation:  {where}         {where}           {where}
Lifecycle:    {where}         {where}           {where}
Rendering:    n/a             {where}           {where}
──────────────────────────────────────────────────────────
```

For each row, check:
- **Same layer across platforms?** If Android puts detection in Repository but iOS puts it in ViewModel, flag it.
- **Could it be in the daemon instead?** If both clients compute the same thing, the daemon should do it once and serve a display contract.
- **Is there a display contract?** Check if the daemon already serves a `*_display` or `*_progress` field that the client could use instead of computing locally.

## Step 4: Instance audit

For each feature component (managers, controllers, notification handlers):
- **How many instances exist?** Search for class instantiation across the codebase.
- **Should there be one?** If a manager posts to a fixed notification ID or manages a singleton resource (Live Activity, foreground service), there must be exactly one instance.
- **Is it shared via DI?** Check if Hilt `@Singleton` (Android) or shared instance pattern (iOS) is used.

```bash
# Find all instantiations of a class
grep -rn "SpaHeatLiveUpdate(" pentair-android/app/src/main/
grep -rn "Activity.request(" pentair-ios/PentairIOS/
```

## Step 5: Push handler audit

For each push/notification handler (FCM Service, APNs delegate):
- **Is it thin?** Should only parse payload and forward to a shared manager.
- **Does it duplicate state machine logic?** If the handler has `if/else` chains that mirror the ViewModel's state detection, flag it.
- **Does it create its own manager instance?** Should inject the shared singleton instead.

## Step 6: Display contract audit

For each piece of data the clients use:
- **Is the daemon already computing it?** Check the `/api/pool` response.
- **Are clients recomputing it locally?** Search for the same calculation in both platforms.
- **Could it be a display contract?** If both clients do `(current - start) / (target - start)`, the daemon should serve `progress_pct` directly.

Pattern to look for:
```
Daemon computes X but doesn't expose it
  → Client A computes X from raw fields
  → Client B computes X from raw fields (same formula)
  → FIX: Daemon exposes X, clients read it
```

## Step 7: Output

```
LAYERING REVIEW: {feature name}
═══════════════════════════════════════════════════

LAYER CONSISTENCY:
  [OK]    Detection logic: ViewModel on both platforms
  [DRIFT] Progress calc: Android=ViewModel, iOS=View — should be ViewModel
  [PUSH]  Phase detection: both clients compute locally — daemon should serve it

INSTANCE AUDIT:
  [OK]    SpaHeatLiveUpdate: 1 Hilt singleton, shared by ViewModel + FCM
  [ISSUE] Two Activity.request() call sites — consolidate

PUSH HANDLER AUDIT:
  [OK]    PoolFcmService: thin forwarder, no state machine
  [ISSUE] PoolAppDelegate: computes phase locally — should read from contract

DISPLAY CONTRACT OPPORTUNITIES:
  [DONE]  spa_heat_progress served by daemon, used by both clients
  [MISS]  filter_cycle_progress: both clients compute locally from schedule data

VERDICT: {CLEAN / N issues found}
═══════════════════════════════════════════════════
```

## Severity

- **DRIFT** (layer mismatch across platforms): Always fix. Causes confusion and divergent behavior.
- **PUSH** (logic should move to daemon): Fix when both clients duplicate the same computation.
- **INSTANCE** (multiple instances of singleton resource): Fix when they can interfere (same notification ID, same Live Activity).
- **THIN** (push handler too thick): Fix when handler duplicates ViewModel logic.
