# Continuous Thermal Calibrator — v1 design

**Status:** proposed · **Scope:** pentair-daemon · **Advisory:** updates the thermal
*model* only. Actuation invariant: **the continuous loops and gap-fill probes never
fire the heater** (probes are pump-only); the heater runs only inside an explicit,
user-initiated characterization campaign (§5).

## 1. Goal

Turn the one-off manual characterization we did by hand (heat/cool cycles → fit
`k`, `g`, evaporation; catch the evap double-count) into a standing service the
daemon runs on its own, forever. It keeps each body's covered-idle cooling model
and heater rate matched to reality from normal operation, so the temperature
**estimate** (and the comfort scheduler that rides on it) stays accurate without
anyone re-running experiments.

### Non-goals
- No autonomous control actuation. The calibrator writes model params + quality
  metrics. The only things it may actuate: sparse pump-only *sampling probes* (§5,
  heater never fires), and — solely when a user explicitly requests one — a
  *characterization campaign* (§5), whose heat leg does fire the heater. The
  continuous loops themselves never actuate anything.
- Not a user-facing feature. It is infrastructure. Per decision, auto-applied fits
  produce **no event/history-log entries** (see §7).

## 2. What it calibrates (per body: pool, spa)

- **`CoolingParams`** — the covered-idle relaxation model already in `thermal.rs`:
  `k0_per_hour`, `k_wind_per_hour_per_mph`, `solar_gain_f` (g), `evap_a`, `evap_b`.
- **Heater rate** (°F/h, *bulk*) — for the heating ETA + scheduler.
- **Outlet-vs-bulk offset** (°F) — the sensor sits on the heater return and reads
  hot while firing; track the offset to de-bias live display + heater-rate fits.

Each is stored per body (the store already has `pool_cooling_params` /
`spa_cooling_params`; add heater-rate + offset slots).

## 3. The core problem: observability, then classification

Water temp is only trustworthy when the pump circulates (`temperature_reliable`,
past warmup). Idle + covered → no data. So the pipeline is **sample → classify →
(only clean samples) fit**. Every candidate `(temp, t, weather)` is tagged:

| Regime | Condition | Use |
|---|---|---|
| **idle-covered cooling** | heater off, settled reliable read, cover assumed on | fit k, g, evap |
| **heating** | heater on, circulating, rising | fit heater rate (bulk, §8) |
| **in-use / uncovered** | observed loss ≫ model (cover off / swim / spa use) | **exclude** from covered fit |
| **contaminated** | shared-sensor cross-body, or not-yet-settled warmup | **drop** |

**Shared-sensor rule (concrete).** The controller has one water-temp sensor shared
across bodies; while the spa circulates, the pool body can report `reliable=true`
with *spa* water temperatures (observed directly during the spa experiments — the
daemon already emits `temperature_reason: inactive-shared-body`). Rule: a reading
is attributed **only to the actively circulating body**; on shared-pump systems the
inactive body's readings are dropped regardless of their reliable flag.

**Cooling interval = idle across the whole gap, not just at the endpoints.** Two
consecutive *idle-covered* settled reads bound a candidate interval, but it is
valid only if **no pump, heater, or body-on activity occurred anywhere in
`(t0, t1)`** — the daemon knows its own state history, so any such event splits or
discards the interval. (Otherwise a 20-minute spa heat in the middle of a 10-hour
gap would poison a "cooling" interval whose endpoints both look clean.) The logged
weather spanning the gap completes the interval — the fit's unit of data.

Uncovered detection is heuristic (no cover sensor): if a fresh interval's implied
loss is many σ above the current model, tag `uncovered`, exclude it, and surface it
as an anomaly rather than letting it corrupt the covered fit. This heuristic has a
deadlock failure mode when reality changes persistently — see the escape hatch in
§9.

## 4. Architecture

A `calibrator.rs` module running as a periodic tokio task beside the weather poll
and heat estimator. Pure/injected core (clock, weather, samples, params all passed
in) so accept/reject + fit logic unit-tests deterministically, matching
`thermal.rs`/`scheduler.rs`.

**Reuses:** `thermal::fit_cooling_params`, `thermal::project_temperature`, the
`WeatherCache`, the reliable-observation stream, the `HeatEstimatorStore`.

Three nested loops on different cadences:

1. **Per-reading (continuous, cheap).** On every reliable read: compare to the
   prediction → update rolling MAE (drives confidence + uncertainty band, already
   exposed); capture + classify the sample into the interval buffer. *(Extends the
   existing `calibrate_predictions`.)*
2. **Re-fit (slow).** Triggered by whichever comes first: nightly quiet-hours tick,
   ≥ N fresh clean cooling intervals accumulated, or MAE drifting past a threshold.
   Runs the fit + accept gate (§6) and, if accepted, **auto-applies** (§7).
3. **Probe (as needed).** If passive samples are too sparse, schedule a gentle
   pump-only sampling run (§5).

## 5. Sampling model — C (hybrid)

Passive by default; opportunistic; sparse active probes to fill gaps; plus a
bootstrap campaign.

- **Passive / opportunistic:** ride any circulation that already happens
  (filtration schedule, spa use, cleaner cycles). The per-reading loop (§4) samples
  continuously whenever a body is circulating, so every settled read during any
  pump run — whatever its reason — is a free sample. Zero added actuation.
- **Active probe (gap-fill):** if no clean idle-covered read in the last
  `probe_gap_hours` (default ~6–8h) during an allowed window, run a short pump
  cycle: **heater locked off** (drop setpoint below temp — the proven trick),
  circulate until `temperature_reliable` + a settle margin, record, restore. Bounded
  by: quiet-hours allow-list, max probes/day, min spacing, never during
  freeze-protect or an active user session. Config-capped; can be disabled.
  **Crash-safe restore (required):** the setpoint drop mutates real user-visible
  state, so before dropping it the daemon persists a
  `probe_in_progress { body, original_setpoint }` record; on boot it restores any
  dangling probe's setpoint (the daemon equivalent of our shell experiments'
  `trap`). The probe captures the setpoint *as of probe start*, and aborts —
  restoring immediately — if any user command targeting that body arrives
  mid-probe. Without this, a crash mid-probe leaves the spa setpoint at 70
  permanently.
- **Bootstrap characterization campaign (self-run, in scope):** a one-shot,
  daemon-run version of our manual experiment to seed params fast on a cold system
  or on demand. Cool leg (periodic no-heat reads) + optional heat leg (fire to
  setpoint, dense reads) → initial `CoolingParams` + heater rate + outlet offset.
  Safe by construction (persisted restore-on-exit as above, heater never fires
  during the cool leg, bounded duration). Exposed as
  `POST /api/pool/calibrate/characterize` (idempotent, cancellable). **Heat-leg
  scoping:** per-body opt-in in the request body, default **spa-only**. The spa
  heat leg costs ~26 min of gas; the pool (24k gal at ~250 kBTU/h ≈ 1.2 °F/h)
  would need *hours* of gas burn for a measurable bulk rise — the pool heater rate
  is instead learned passively from user/scheduled heats (§8). After the campaign
  seeds, the continuous loops refine.

## 6. The fit + accept gate

- **Window:** rolling 7–14 days of clean cooling intervals + the weather segments
  covering them.
- **Fit:** `fit_cooling_params(intervals, segments, current_or_seed)`.
- **Priors / regularization:** pull toward the physics seed so a sparse or
  ill-conditioned window can't yield garbage; specifically nudge **covered-idle
  `evap_a`/`evap_b` toward 0** (the cover blocks evaporation; the fitted `k`
  already absorbs the true covered loss — this is the double-count we hit).
  Same class of guard for solar: a window with night-only intervals cannot
  identify `solar_gain_f`, so **hold `g` at its current value/prior unless the
  window contains daytime intervals**.
- **Holdout validation — same data for both sides.** Fit on a subset of intervals;
  then score **both the candidate and the current params on the same held-out
  intervals**. (Comparing candidate-holdout-MAE against the *live rolling* MAE
  would be apples-to-oranges — different data.)
- **Accept criteria (all must hold):** candidate holdout MAE finite and ≤ current
  params' holdout MAE + tolerance; every param finite and within physical clamps
  (`k>0`, `g≥0`, `evap≥0`, sane `τ` band); change per param below a max step.
- **Damping:** on accept, blend `new = (1−α)·old + α·fit` (α≈0.3) so params glide,
  never jump. **The blend is what actually gets applied, so the blend is what gets
  validated** — score the blended params on the holdout too and require they beat
  the current params before writing. Reject → keep current, log (debug only),
  retry next cycle.

## 7. Auto-apply policy (per decision: apply, but not in the event/history log)

Accepted fits are written **silently**:
- Update `*_cooling_params` / heater-rate / offset + rolling MAE in the store.
- **No entries in any user-facing event log or change-history feed.** The only
  persistent record is the *current* calibration snapshot (overwritten in place)
  plus ordinary `tracing` debug lines (not the event log). No per-change audit
  trail is kept.
- Safety clamps + the accept gate (§6) are the guardrail that makes silent
  auto-apply tolerable: a clearly-bad fit is rejected before it can apply. They
  bound but don't eliminate subtle in-range degradation — the **rolling MAE trend
  (§10) is the real detector** for that, so keeping it visible on the calibration
  endpoint is part of this policy, not an optional nicety.

## 8. Heater rate + outlet-vs-bulk offset

The return-line sensor reads ~6–10°F above true bulk while firing (measured). So:
- **Heater rate = (settled pre-heat bulk → settled post-heat bulk) ÷ heating
  hours**, *not* the during-firing outlet slope. Requires a settled read bracketing
  the heat event (the bootstrap campaign produces these; passive heating events
  do too when a settled read exists before + after).
- **Offset** = mean(during-firing reading − settled post-mix bulk). Track it to (a)
  de-bias the live temperature display while heating and (b) correct any heater-leg
  samples that leak into the cooling fit.

## 9. Safety, guardrails & failure modes

- The heater is never fired by the continuous loops or probes (setpoint-drop
  trick; sampling happens only with the heater off). The **only** heater actuation
  is a user-initiated campaign's opt-in heat leg (§5).
- Probes: bounded frequency + spacing + quiet-hours; skip during freeze-protect,
  scheduled heat, or an active user session; global on/off + daily cap in config;
  persisted crash-safe setpoint restore (§5).
- Param writes: physical clamps + accept gate + damping; never NaN/inf.
- Fully decoupled from control: the calibrator can be disabled and the daemon's
  control behavior is unchanged.

### Known failure modes and their mitigations

- **Exclusion deadlock.** The uncovered heuristic (§3) excludes intervals that
  disagree with the current model. If reality changes *persistently* (cover
  degrades, cover left off for the season, new cover), every fresh interval looks
  anomalous → all excluded → the fit only ever sees old-regime data → the model
  can never learn the new reality, silently and forever. **Escape hatch:** if the
  exclusion rate exceeds a threshold over a trailing window (e.g., >50% of
  intervals for >5 days) — or MAE stays drifted while exclusions run high —
  surface the condition on `/api/pool/calibration` and run a re-fit with widened
  acceptance over the recent (excluded-included) data instead of continuing to
  reject.
- **Crash mid-probe.** Without the persisted `probe_in_progress` record (§5), a
  daemon crash between setpoint-drop and restore leaves the body's setpoint at 70
  indefinitely. Restore-on-boot from the persisted record is mandatory, not
  best-effort.

## 10. Observability

`GET /api/pool/calibration` (read-only) → per body: current params, confidence,
last-fit time, rolling MAE + short MAE trend, sample counts by regime, next
scheduled probe, campaign status. This is *current state* (overwritten), consistent
with §7's "no change-history log."

## 11. Persistence

No database. The daemon's convention is small, human-inspectable **JSON files in
`~/.pentair/`** (`devices.json`, `weather-cache.json`, `scheduled-heat.json`,
`heat-estimator.json`) — one serde struct each, loaded on boot, rewritten in place.
That's also what let us warm-start this deploy by hand-seeding the files. The
calibrator adds **no new storage tech**: it extends the existing
**`HeatEstimatorStore` (`heat-estimator.json`)** with a few bounded fields.

| What | Field | Notes |
|---|---|---|
| fitted model | `pool/spa_cooling_params` | already present |
| heater | `pool/spa_heater_rate_f_per_h`, `pool/spa_outlet_offset_f` | new, per body |
| quality | `pool/spa_prediction_mae_f` + bounded **MAE ring** (~50 pts) | MAE present; ring feeds the §10 trend only |
| fit input | `pool/spa_cooling_intervals` — bounded buffer (≤ ~14 days / ~200) | the fit's raw data |

**Self-contained intervals (the one real decision).** The fit window is 7–14 days,
but the `WeatherCache` only retains ~48h — so the fit must **not** depend on the
weather cache. Each stored cooling interval carries the weather it experienced:
`{ t0, T0, t1, T1, regime, weather }`. Consequences: the fit is a pure function of
the stored buffer, a restart loses nothing, and no long weather retention (or
time-series store) is needed. This denormalization is what keeps the whole thing a
plain bounded JSON blob.

**Per-interval weather must itself be bounded.** Raw 15-minute segments would blow
the size claim (a 16-h gap ≈ 64 segments ≈ ~6 KB; × 200 intervals ≈ >1 MB).
Summarize each interval's weather to **hourly buckets, capped at 24 per interval**
(mean air/wind/humidity/cloud per bucket) — ample resolution for a relaxation with
τ ≈ 100 h, and it keeps the file honestly in the tens-of-KB range.

**Properties that fall out of the §7 decisions:**
- **Current-state only, no change history.** Params are overwritten in place; the
  MAE ring is a trend sparkline, not an audit trail. Nothing grows unbounded.
- **Bounded everything** — caps on `cooling_intervals`, per-interval weather
  buckets, and MAE points keep the file in the tens-of-KB range.

**Single writer.** The heat estimator and the calibrator both mutate
`heat-estimator.json`. All writes funnel through one owner — the calibrator calls
into `HeatEstimator` rather than writing the file itself — so two tasks can't
read-modify-write race each other.

**Atomicity.** `heat.rs` currently persists with a direct `fs::write`; the
calibrator's writes must use the **tmp-file + atomic rename** pattern (as
`scheduled_heat.rs` already does) so a crash mid-write can't corrupt the store.

## 12. Config

`[heating.calibration]`: `enabled` (default true), `refit_*` triggers (nightly hour,
min-new-intervals, mae-drift threshold), window days, damping α, param clamps.
`[heating.calibration.probe]`: `enabled`, gap hours, quiet-hours window, max/day,
min spacing.

## 13. Testing

- Pure-core unit tests: classification, accept/reject gate (candidate vs current
  on the same holdout; blend validated), damping, clamp, holdout — deterministic
  on injected inputs. Plus: mid-gap pump/heater activity splits or discards an
  interval; shared-sensor readings attributed only to the circulating body; the
  exclusion-deadlock escape hatch fires when the exclusion rate stays high.
- Probe lifecycle: crash between setpoint-drop and restore → on next boot the
  persisted `probe_in_progress` record restores the original setpoint; a user
  command mid-probe aborts and restores immediately.
- Synthetic end-to-end: generate cooling/heating intervals from a known model +
  noise → assert the calibrator recovers the params within tolerance and that a
  planted uncovered/contaminated interval is excluded.
- Regression: replay the recorded soak + spa cooldown logs → assert the fit lands
  near our hand-fit (pool τ≈120h, spa τ≈96h) and evap→~0.

## 14. Phased rollout

1. **Passive + per-reading MAE + re-fit + accept gate + silent auto-apply**
   (no probes, no campaign) — smallest safe increment; actuates nothing; validates
   the loop on natural cycles.
2. **Observability API** — see it working.
3. **Bootstrap characterization campaign** — fast cold-start / on-demand reseed.
   User-initiated actuation only (heat leg opt-in, spa-only by default).
4. **Active gap-fill probes** — last: the only *autonomous* actuation (pump-only),
   and it ships with the crash-safe setpoint restore from day one.
