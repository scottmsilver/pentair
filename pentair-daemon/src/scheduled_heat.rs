//! Timed heating scheduler.
//!
//! Supports "have spa ready at 7pm" by computing the optimal start time from
//! the existing ETA engine, starting heating automatically when the timer fires,
//! and persisting schedules across daemon restarts.

use chrono::{Local, NaiveTime};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ── Persistence model ───────────────────────────────────────────────────

/// A scheduled heat-at entry, persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledHeatEntry {
    /// Target time in HH:MM format.
    pub ready_by: String,
    /// Heat setpoint in Fahrenheit.
    pub setpoint: i32,
    /// Estimated minutes to heat (at time of scheduling).
    pub estimated_minutes: Option<u32>,
    /// Computed start time as unix timestamp (ms).
    pub start_at_unix_ms: i64,
    /// The ready-by time as unix timestamp (ms) — the deadline.
    pub ready_by_unix_ms: i64,
    /// Whether heating was started immediately because we were already late.
    pub started_immediately: bool,
    /// Whether the scheduler has already fired and started heating.
    pub fired: bool,
}

/// On-disk format for scheduled heat persistence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ScheduledHeatStore {
    spa: Option<ScheduledHeatEntry>,
}

// ── Default estimate when ETA engine has no data ────────────────────────

/// Conservative default assuming a typical spa heats ~3°F/hour without prior
/// observations. Real rates are 4-8 F/hr depending on heater type and ambient
/// conditions.
const DEFAULT_SPA_RATE_F_PER_HOUR: f64 = 3.0;

// ── Scheduler state ─────────────────────────────────────────────────────

/// Shared scheduler state, protected by RwLock.
pub struct ScheduledHeatState {
    path: PathBuf,
    pub entry: Option<ScheduledHeatEntry>,
    /// Handle to cancel the pending timer task.
    cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

pub type SharedScheduledHeat = Arc<RwLock<ScheduledHeatState>>;

impl ScheduledHeatState {
    fn new(path: PathBuf) -> Self {
        let entry = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => {
                    let store: ScheduledHeatStore =
                        serde_json::from_str(&contents).unwrap_or_default();
                    store.spa
                }
                Err(e) => {
                    warn!("failed to read scheduled heat file {:?}: {}", path, e);
                    None
                }
            }
        } else {
            None
        };

        if let Some(ref entry) = entry {
            info!(
                "loaded persisted heat schedule: ready_by={}, setpoint={}, fired={}",
                entry.ready_by, entry.setpoint, entry.fired
            );
        }

        Self {
            path,
            entry,
            cancel_tx: None,
        }
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let store = ScheduledHeatStore {
            spa: self.entry.clone(),
        };

        match serde_json::to_string_pretty(&store) {
            Ok(json) => {
                // Write to a temp file then atomically rename so a crash mid-write
                // cannot leave a truncated/corrupt JSON file on disk.
                let tmp = self.path.with_extension("json.tmp");
                if let Err(e) = std::fs::write(&tmp, &json) {
                    warn!("failed to write scheduled heat temp file: {}", e);
                    return;
                }
                if let Err(e) = std::fs::rename(&tmp, &self.path) {
                    warn!("failed to rename scheduled heat temp file: {}", e);
                }
            }
            Err(e) => warn!("failed to serialize scheduled heat: {}", e),
        }
    }

    fn clear(&mut self) {
        self.entry = None;
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
        self.persist();
    }
}

/// Create a new shared scheduler state, loading any persisted schedule.
pub fn new_shared_scheduled_heat(path: PathBuf) -> SharedScheduledHeat {
    Arc::new(RwLock::new(ScheduledHeatState::new(path)))
}

// ── Schedule computation ────────────────────────────────────────────────

/// Request body for `POST /api/spa/heat-at`.
#[derive(Debug, Deserialize)]
pub struct HeatAtRequest {
    /// Target time in HH:MM format.
    pub ready_by: String,
    /// Heat setpoint in Fahrenheit.
    pub setpoint: i32,
}

/// Response body for schedule creation.
#[derive(Debug, Serialize)]
pub struct HeatAtResponse {
    pub scheduled: bool,
    pub start_at: String,
    pub ready_by: String,
    pub estimated_minutes: Option<u32>,
    pub started_immediately: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Response body for `GET /api/spa/heat-at`.
#[derive(Debug, Serialize)]
pub struct HeatAtStatusResponse {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setpoint: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fired: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_immediately: Option<bool>,
}

/// Parse a "HH:MM" time string into a NaiveTime.
pub fn parse_time(s: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| format!("invalid time '{}': {}", s, e))
}

/// Compute the target unix timestamp for a ready_by time.
/// If the time has already passed today, schedule for tomorrow.
pub fn compute_ready_by_unix_ms(ready_by_time: NaiveTime) -> i64 {
    let now = Local::now();
    let today = now.date_naive();

    let target_today = today.and_time(ready_by_time);
    let target_dt = if target_today <= now.naive_local() {
        // Time has passed today — schedule for tomorrow
        let tomorrow = today
            .succ_opt()
            .unwrap_or(today);
        tomorrow.and_time(ready_by_time)
    } else {
        target_today
    };

    // Convert to unix ms using local timezone.
    // During DST "fall back", a local time can be ambiguous (two possible instants).
    // Use `.latest()` to pick the later one — the user wants the spa ready by that
    // wall-clock time, so the later interpretation gives us more heating time.
    let target_local = target_dt
        .and_local_timezone(now.timezone())
        .latest()
        .unwrap_or_else(|| {
            warn!("could not resolve target time to local timezone, using current time");
            now
        });

    target_local.timestamp_millis()
}

/// Estimate how many minutes it will take to heat the spa to the setpoint,
/// given the current state. Returns None if we can't estimate (no rate data).
pub fn estimate_heating_minutes(
    current_temp: Option<i32>,
    setpoint: i32,
    learned_rate: Option<f64>,
    configured_rate: Option<f64>,
) -> Option<u32> {
    let current = current_temp? as f64;
    let target = setpoint as f64;

    if target <= current {
        return Some(0);
    }

    // Favor learned rate (observed from actual hardware) over configured default,
    // but keep some configured weight as a floor for early sessions with limited data.
    let rate = match (learned_rate, configured_rate) {
        (Some(l), Some(c)) => l * 0.7 + c * 0.3,
        (Some(l), None) => l,
        (None, Some(c)) => c,
        (None, None) => return None,
    };

    if rate <= 0.0 || !rate.is_finite() {
        return None;
    }

    let delta = target - current;
    let minutes = ((delta / rate) * 60.0).ceil() as u32;
    Some(minutes.max(1))
}

/// Compute the start time given the ready-by deadline and estimated minutes.
/// Returns (start_at_unix_ms, started_immediately).
pub fn compute_start_time(
    ready_by_unix_ms: i64,
    estimated_minutes: u32,
    now_unix_ms: i64,
) -> (i64, bool) {
    let needed_ms = estimated_minutes as i64 * 60_000;
    let ideal_start = ready_by_unix_ms - needed_ms;

    if ideal_start <= now_unix_ms {
        // Too late — start immediately
        (now_unix_ms, true)
    } else {
        (ideal_start, false)
    }
}

/// Format a unix timestamp (ms) as an ISO 8601 string in local time.
pub fn format_unix_ms_local(unix_ms: i64) -> String {
    let dt = chrono::DateTime::from_timestamp_millis(unix_ms)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    let local = dt.with_timezone(&Local);
    local.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

// ── Schedule management ─────────────────────────────────────────────────

/// Create or replace a heat-at schedule.
///
/// This computes the start time, persists the schedule, and spawns the timer
/// task (or fires immediately) — all while holding the write lock so that
/// concurrent requests cannot overwrite each other's cancel handles.
pub async fn create_schedule(
    sched: &SharedScheduledHeat,
    request: &HeatAtRequest,
    current_temp: Option<i32>,
    learned_rate: Option<f64>,
    configured_rate: Option<f64>,
    shared_state: crate::state::SharedState,
    cmd_tx: tokio::sync::mpsc::Sender<crate::adapter::AdapterCommand>,
) -> Result<HeatAtResponse, String> {
    // Pentair controllers accept setpoints in the range 40-104 F.
    if request.setpoint < 40 || request.setpoint > 104 {
        return Err(format!(
            "setpoint {} out of range (40-104 F)",
            request.setpoint
        ));
    }
    let ready_by_time = parse_time(&request.ready_by)?;
    let ready_by_unix_ms = compute_ready_by_unix_ms(ready_by_time);
    let now_unix_ms = chrono::Utc::now().timestamp_millis();

    // Estimate heating time
    let estimated_minutes = estimate_heating_minutes(
        current_temp,
        request.setpoint,
        learned_rate,
        configured_rate,
    );

    // Use estimate or fallback
    let effective_minutes = estimated_minutes.unwrap_or_else(|| {
        // Fallback: assume DEFAULT_SPA_RATE_F_PER_HOUR
        let delta = (request.setpoint as f64 - current_temp.unwrap_or(70) as f64).max(0.0);
        ((delta / DEFAULT_SPA_RATE_F_PER_HOUR) * 60.0).ceil().max(1.0) as u32
    });

    let (start_at_unix_ms, started_immediately) =
        compute_start_time(ready_by_unix_ms, effective_minutes, now_unix_ms);

    let warning = if estimated_minutes.is_none() {
        Some("No heating rate data available; using default estimate. Actual time may vary.".to_string())
    } else if started_immediately {
        Some("Not enough time to reach target temperature. Starting immediately.".to_string())
    } else {
        None
    };

    let entry = ScheduledHeatEntry {
        ready_by: request.ready_by.clone(),
        setpoint: request.setpoint,
        estimated_minutes,
        start_at_unix_ms,
        ready_by_unix_ms,
        started_immediately,
        fired: started_immediately, // If starting immediately, mark as fired
    };

    // Hold the write lock for the entire store-and-spawn sequence so that
    // concurrent requests cannot overwrite each other's cancel handles.
    {
        let mut state = sched.write().await;
        // Cancel any existing schedule
        if let Some(tx) = state.cancel_tx.take() {
            let _ = tx.send(());
        }
        state.entry = Some(entry);
        state.persist();

        // Spawn the timer (or fire immediately) while still holding the lock
        let sched_clone = sched.clone();
        if started_immediately {
            tokio::spawn(async move {
                fire_heat_with_spa_on(&sched_clone, &shared_state, &cmd_tx).await;
            });
        } else {
            let delay_ms = (start_at_unix_ms - now_unix_ms).max(0) as u64;
            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
            state.cancel_tx = Some(cancel_tx);

            info!(
                "scheduling heat timer: fires in {}m {}s",
                delay_ms / 60_000,
                (delay_ms % 60_000) / 1000
            );

            tokio::spawn(async move {
                let delay = tokio::time::Duration::from_millis(delay_ms);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        info!("heat timer fired — starting spa heat");
                        fire_heat_with_spa_on(&sched_clone, &shared_state, &cmd_tx).await;
                    }
                    _ = cancel_rx => {
                        info!("heat timer cancelled");
                    }
                }
            });
        }
    }

    Ok(HeatAtResponse {
        scheduled: true,
        start_at: format_unix_ms_local(start_at_unix_ms),
        ready_by: format_unix_ms_local(ready_by_unix_ms),
        estimated_minutes,
        started_immediately,
        warning,
    })
}

/// Get the current schedule status.
pub async fn get_schedule(sched: &SharedScheduledHeat) -> HeatAtStatusResponse {
    let state = sched.read().await;
    match &state.entry {
        Some(entry) => HeatAtStatusResponse {
            active: true,
            ready_by: Some(entry.ready_by.clone()),
            setpoint: Some(entry.setpoint),
            start_at: Some(format_unix_ms_local(entry.start_at_unix_ms)),
            estimated_minutes: entry.estimated_minutes,
            fired: Some(entry.fired),
            started_immediately: Some(entry.started_immediately),
        },
        None => HeatAtStatusResponse {
            active: false,
            ready_by: None,
            setpoint: None,
            start_at: None,
            estimated_minutes: None,
            fired: None,
            started_immediately: None,
        },
    }
}

/// Cancel any active schedule.
pub async fn cancel_schedule(sched: &SharedScheduledHeat) -> bool {
    let mut state = sched.write().await;
    let had_schedule = state.entry.is_some();
    state.clear();
    had_schedule
}

/// Fire the scheduled heat: turn on spa circuit, set heat setpoint, and set heat mode.
/// Needs shared state for circuit resolution.
pub async fn fire_heat_with_spa_on(
    sched: &SharedScheduledHeat,
    shared_state: &crate::state::SharedState,
    cmd_tx: &tokio::sync::mpsc::Sender<crate::adapter::AdapterCommand>,
) {
    let setpoint = {
        let state = sched.read().await;
        match &state.entry {
            Some(entry) => entry.setpoint,
            None => {
                warn!("heat timer fired but no schedule found");
                return;
            }
        }
    };

    // 1) Set heat setpoint for spa (body_type=1)
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = cmd_tx
        .send(crate::adapter::AdapterCommand::SetHeatSetpoint {
            body_type: 1,
            temp: setpoint,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => info!("spa heat setpoint set to {}", setpoint),
        Ok(Err(e)) => warn!("failed to set spa heat setpoint: {}", e),
        Err(_) => warn!("adapter disconnected while setting heat setpoint"),
    }

    // 2) Set heat mode to heater (mode=3) for spa
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = cmd_tx
        .send(crate::adapter::AdapterCommand::SetHeatMode {
            body_type: 1,
            mode: 3,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => info!("spa heat mode set to heater"),
        Ok(Err(e)) => warn!("failed to set spa heat mode: {}", e),
        Err(_) => warn!("adapter disconnected while setting heat mode"),
    }

    // 3) Turn on spa circuit (resolve circuit ID from semantic state)
    let circuit_id = {
        let s = shared_state.read().await;
        s.resolve_circuit("spa")
    };

    if let Some(circuit_id) = circuit_id {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = cmd_tx
            .send(crate::adapter::AdapterCommand::SetCircuit {
                circuit_id,
                state: true,
                reply: tx,
            })
            .await;
        match rx.await {
            Ok(Ok(())) => info!("spa circuit turned on"),
            Ok(Err(e)) => warn!("failed to turn on spa: {}", e),
            Err(_) => warn!("adapter disconnected while turning on spa"),
        }
    } else {
        warn!("could not resolve spa circuit ID — spa may not turn on");
    }

    // Mark schedule as fired
    {
        let mut state = sched.write().await;
        if let Some(entry) = &mut state.entry {
            entry.fired = true;
        }
        state.persist();
    }

    info!("scheduled heat fired: spa on, setpoint={}, heater mode enabled", setpoint);
}

/// Spawn a timer that uses `fire_heat_with_spa_on` when it fires.
pub async fn spawn_heat_timer_full(
    sched: SharedScheduledHeat,
    shared_state: crate::state::SharedState,
    cmd_tx: tokio::sync::mpsc::Sender<crate::adapter::AdapterCommand>,
) {
    let (entry_start_at, already_fired) = {
        let state = sched.read().await;
        match &state.entry {
            Some(entry) => (entry.start_at_unix_ms, entry.fired),
            None => return,
        }
    };

    if already_fired {
        info!("persisted heat schedule already fired, skipping timer");
        return;
    }

    let now_unix_ms = chrono::Utc::now().timestamp_millis();

    // If the schedule's ready_by time has passed, discard it
    let ready_by_unix_ms = {
        let state = sched.read().await;
        state.entry.as_ref().map(|e| e.ready_by_unix_ms).unwrap_or(0)
    };

    if ready_by_unix_ms < now_unix_ms {
        info!("persisted heat schedule has expired (ready_by in the past), clearing");
        let mut state = sched.write().await;
        state.clear();
        return;
    }

    let delay_ms = (entry_start_at - now_unix_ms).max(0) as u64;

    info!(
        "scheduling heat timer: fires in {}m {}s",
        delay_ms / 60_000,
        (delay_ms % 60_000) / 1000
    );

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut state = sched.write().await;
        if let Some(old_tx) = state.cancel_tx.take() {
            let _ = old_tx.send(());
        }
        state.cancel_tx = Some(cancel_tx);
    }

    let sched_clone = sched.clone();
    tokio::spawn(async move {
        let delay = tokio::time::Duration::from_millis(delay_ms);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {
                info!("heat timer fired — starting spa heat");
                fire_heat_with_spa_on(&sched_clone, &shared_state, &cmd_tx).await;
            }
            _ = cancel_rx => {
                info!("heat timer cancelled");
            }
        }
    });
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    /// Build dummy shared state and command channel for tests.
    /// The receiver is dropped so any spawned timer fire will fail gracefully.
    fn test_deps() -> (crate::state::SharedState, tokio::sync::mpsc::Sender<crate::adapter::AdapterCommand>) {
        let shared = crate::state::new_shared_state(
            Vec::new(),
            Default::default(),
            Default::default(),
            std::path::PathBuf::from("/tmp/test-heat-estimator.json"),
        );
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(1);
        (shared, cmd_tx)
    }

    #[test]
    fn test_parse_time_valid() {
        let t = parse_time("19:00").unwrap();
        assert_eq!(t.hour(), 19);
        assert_eq!(t.minute(), 0);
    }

    #[test]
    fn test_parse_time_leading_zero() {
        let t = parse_time("07:30").unwrap();
        assert_eq!(t.hour(), 7);
        assert_eq!(t.minute(), 30);
    }

    #[test]
    fn test_parse_time_invalid() {
        assert!(parse_time("25:00").is_err());
        assert!(parse_time("abc").is_err());
        assert!(parse_time("").is_err());
    }

    #[test]
    fn test_estimate_heating_minutes_at_temp() {
        // Already at or above setpoint
        let result = estimate_heating_minutes(Some(104), 104, Some(5.0), None);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_estimate_heating_minutes_with_learned_rate() {
        // 10F delta at 5 F/hour = 120 minutes
        let result = estimate_heating_minutes(Some(94), 104, Some(5.0), None);
        assert_eq!(result, Some(120));
    }

    #[test]
    fn test_estimate_heating_minutes_with_both_rates() {
        // 10F delta, learned=5 F/hr, configured=10 F/hr
        // blended = 5*0.7 + 10*0.3 = 6.5 F/hr
        // 10/6.5 * 60 = 92.3 -> ceil = 93
        let result = estimate_heating_minutes(Some(94), 104, Some(5.0), Some(10.0));
        assert_eq!(result, Some(93));
    }

    #[test]
    fn test_estimate_heating_minutes_no_rate() {
        let result = estimate_heating_minutes(Some(94), 104, None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_estimate_heating_minutes_no_current_temp() {
        let result = estimate_heating_minutes(None, 104, Some(5.0), None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_compute_start_time_plenty_of_time() {
        let now = 1000 * 60_000; // arbitrary "now" in ms
        let ready_by = now + 120 * 60_000; // 2 hours from now
        let estimated = 45; // 45 minutes

        let (start, immediate) = compute_start_time(ready_by, estimated, now);
        assert!(!immediate);
        assert_eq!(start, ready_by - 45 * 60_000);
        assert!(start > now);
    }

    #[test]
    fn test_compute_start_time_too_late() {
        let now = 1000 * 60_000;
        let ready_by = now + 30 * 60_000; // 30 minutes from now
        let estimated = 60; // 60 minutes needed

        let (start, immediate) = compute_start_time(ready_by, estimated, now);
        assert!(immediate);
        assert_eq!(start, now);
    }

    #[test]
    fn test_compute_start_time_exactly_on_time() {
        let now = 1000 * 60_000;
        let ready_by = now + 60 * 60_000; // 60 minutes from now
        let estimated = 60; // 60 minutes needed

        let (_start, immediate) = compute_start_time(ready_by, estimated, now);
        // start = ready_by - 60min = now, so it starts now but isn't "too late"
        assert!(immediate); // start <= now, so started_immediately is true
    }

    #[test]
    fn test_compute_ready_by_unix_ms_future_today() {
        // This is a time-sensitive test; it should work unless run exactly at 23:59
        let far_future_time = NaiveTime::from_hms_opt(23, 59, 0).unwrap();
        let now = Local::now();
        let today_2359 = now
            .date_naive()
            .and_time(far_future_time)
            .and_local_timezone(now.timezone())
            .single()
            .unwrap();

        if now < today_2359 {
            let result = compute_ready_by_unix_ms(far_future_time);
            assert_eq!(result, today_2359.timestamp_millis());
        }
    }

    #[test]
    fn test_format_unix_ms_local() {
        // Just verify it doesn't panic and produces a non-empty string
        let s = format_unix_ms_local(1711396800000); // Some timestamp
        assert!(!s.is_empty());
        assert!(s.contains("T")); // ISO format
    }

    #[tokio::test]
    async fn test_schedule_create_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");
        let sched = new_shared_scheduled_heat(path.clone());
        let (shared, cmd_tx) = test_deps();

        let request = HeatAtRequest {
            ready_by: "23:59".to_string(),
            setpoint: 104,
        };

        let result = create_schedule(&sched, &request, Some(80), Some(5.0), None, shared, cmd_tx).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.scheduled);

        // Check status
        let status = get_schedule(&sched).await;
        assert!(status.active);
        assert_eq!(status.ready_by, Some("23:59".to_string()));
        assert_eq!(status.setpoint, Some(104));
    }

    #[tokio::test]
    async fn test_schedule_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");
        let sched = new_shared_scheduled_heat(path.clone());
        let (shared, cmd_tx) = test_deps();

        let request = HeatAtRequest {
            ready_by: "23:59".to_string(),
            setpoint: 104,
        };

        let _ = create_schedule(&sched, &request, Some(80), Some(5.0), None, shared, cmd_tx).await;

        let had = cancel_schedule(&sched).await;
        assert!(had);

        let status = get_schedule(&sched).await;
        assert!(!status.active);

        // Cancel again — no schedule
        let had = cancel_schedule(&sched).await;
        assert!(!had);
    }

    #[tokio::test]
    async fn test_schedule_persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");

        // Create and persist
        {
            let sched = new_shared_scheduled_heat(path.clone());
            let (shared, cmd_tx) = test_deps();
            let request = HeatAtRequest {
                ready_by: "23:59".to_string(),
                setpoint: 104,
            };
            let _ = create_schedule(&sched, &request, Some(80), Some(5.0), None, shared, cmd_tx).await;
        }

        // Reload from disk
        {
            let sched = new_shared_scheduled_heat(path.clone());
            let status = get_schedule(&sched).await;
            assert!(status.active);
            assert_eq!(status.ready_by, Some("23:59".to_string()));
            assert_eq!(status.setpoint, Some(104));
        }
    }

    #[tokio::test]
    async fn test_schedule_too_late_starts_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");
        let sched = new_shared_scheduled_heat(path);

        // Schedule for 1 minute from now but with a huge heating time needed
        let now = Local::now();
        let soon = (now + chrono::Duration::minutes(1)).time();
        let ready_by = format!("{:02}:{:02}", soon.hour(), soon.minute());

        let request = HeatAtRequest {
            ready_by,
            setpoint: 104, // Max valid setpoint
        };

        // With current temp of 40 and rate of 0.5 F/hr, heating 64F would take ~128 hours
        // which is way more than 1 minute, so it starts immediately.
        let (shared, cmd_tx) = test_deps();
        let result = create_schedule(&sched, &request, Some(40), Some(0.5), None, shared, cmd_tx).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.started_immediately);
    }

    #[tokio::test]
    async fn test_schedule_no_rate_uses_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");
        let sched = new_shared_scheduled_heat(path);

        let request = HeatAtRequest {
            ready_by: "23:59".to_string(),
            setpoint: 104,
        };

        // No learned or configured rate
        let (shared, cmd_tx) = test_deps();
        let result = create_schedule(&sched, &request, Some(80), None, None, shared, cmd_tx).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.scheduled);
        assert!(resp.estimated_minutes.is_none()); // No official estimate
        assert!(resp.warning.is_some()); // Should warn about default
    }

    #[tokio::test]
    async fn test_schedule_rejects_invalid_setpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scheduled-heat.json");
        let sched = new_shared_scheduled_heat(path);

        // Too low
        let request = HeatAtRequest {
            ready_by: "23:59".to_string(),
            setpoint: 39,
        };
        let (shared, cmd_tx) = test_deps();
        let result = create_schedule(&sched, &request, Some(80), Some(5.0), None, shared, cmd_tx).await;
        assert!(result.is_err());

        // Too high
        let request = HeatAtRequest {
            ready_by: "23:59".to_string(),
            setpoint: 105,
        };
        let (shared, cmd_tx) = test_deps();
        let result = create_schedule(&sched, &request, Some(80), Some(5.0), None, shared, cmd_tx).await;
        assert!(result.is_err());
    }
}
