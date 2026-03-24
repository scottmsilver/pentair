use crate::config::HeatingConfig;
use chrono::{NaiveDate, NaiveDateTime};
use pentair_protocol::responses::{HistoryData, TimeRangePoint};
use pentair_protocol::semantic::{
    BodyState, HeatEstimate, HeatEstimateDisplay, PoolSystem, SpaState, TemperatureDisplay,
};
use pentair_protocol::types::SLDateTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

const WATER_LB_PER_GALLON: f64 = 8.34;
const MAX_RECENT_SESSIONS: usize = 24;
const AMBIENT_RATE_WEIGHT_SPAN_F: f64 = 10.0;
const LAST_RELIABLE_PERSIST_INTERVAL_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum HeatingBodyKind {
    Pool,
    Spa,
}

#[derive(Debug, Clone)]
struct BodyTelemetry {
    on: bool,
    active: bool,
    pool_spa_shared_pump: bool,
    temperature: i32,
    setpoint: i32,
    temperature_f: f64,
    setpoint_f: f64,
    heat_mode: String,
    heating: String,
    air_temp_f: Option<f64>,
}

#[derive(Debug, Clone)]
struct TemperatureTrust {
    reliable: bool,
    reason: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ReliableTemperatureObservation {
    temperature: i32,
    observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeatingSample {
    at_unix_ms: i64,
    temperature_f: f64,
    target_temp_f: f64,
    air_temp_f: Option<f64>,
    heating: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompletedHeatingSession {
    body: HeatingBodyKind,
    started_at_unix_ms: i64,
    ended_at_unix_ms: i64,
    start_temp_f: f64,
    end_temp_f: f64,
    target_temp_f: f64,
    average_air_temp_f: Option<f64>,
    duration_minutes: f64,
    average_rate_f_per_hour: f64,
}

#[derive(Debug, Clone)]
struct ActiveHeatingSession {
    body: HeatingBodyKind,
    started_at_unix_ms: i64,
    latest_at_unix_ms: i64,
    start_temp_f: f64,
    latest_temp_f: f64,
    target_temp_f: f64,
    samples: Vec<HeatingSample>,
}

impl ActiveHeatingSession {
    fn start(body: HeatingBodyKind, telemetry: &BodyTelemetry, now_unix_ms: i64) -> Self {
        let sample = HeatingSample {
            at_unix_ms: now_unix_ms,
            temperature_f: telemetry.temperature_f,
            target_temp_f: telemetry.setpoint_f,
            air_temp_f: telemetry.air_temp_f,
            heating: telemetry.heating.clone(),
        };

        Self {
            body,
            started_at_unix_ms: now_unix_ms,
            latest_at_unix_ms: now_unix_ms,
            start_temp_f: telemetry.temperature_f,
            latest_temp_f: telemetry.temperature_f,
            target_temp_f: telemetry.setpoint_f,
            samples: vec![sample],
        }
    }

    fn record(&mut self, telemetry: &BodyTelemetry, now_unix_ms: i64, sample_window_minutes: u64) {
        self.latest_at_unix_ms = now_unix_ms;
        self.latest_temp_f = telemetry.temperature_f;
        self.target_temp_f = telemetry.setpoint_f;
        self.samples.push(HeatingSample {
            at_unix_ms: now_unix_ms,
            temperature_f: telemetry.temperature_f,
            target_temp_f: telemetry.setpoint_f,
            air_temp_f: telemetry.air_temp_f,
            heating: telemetry.heating.clone(),
        });

        let cutoff = now_unix_ms - (sample_window_minutes as i64 * 60_000);
        self.samples.retain(|sample| sample.at_unix_ms >= cutoff);
    }

    fn elapsed_minutes(&self) -> f64 {
        (self.latest_at_unix_ms - self.started_at_unix_ms) as f64 / 60_000.0
    }

    fn observed_rate_f_per_hour(
        &self,
        minimum_runtime_minutes: u64,
        minimum_temp_rise_f: f64,
    ) -> Option<f64> {
        let elapsed_minutes = self.elapsed_minutes();
        if elapsed_minutes < minimum_runtime_minutes as f64 {
            return None;
        }

        let delta_f = self.latest_temp_f - self.start_temp_f;
        if delta_f < minimum_temp_rise_f {
            return None;
        }

        let elapsed_hours = elapsed_minutes / 60.0;
        if elapsed_hours <= 0.0 {
            return None;
        }

        let rate = delta_f / elapsed_hours;
        (rate.is_finite() && rate > 0.0).then_some(rate)
    }

    fn average_air_temp_f(&self) -> Option<f64> {
        let mut total = 0.0;
        let mut count = 0usize;
        for sample in &self.samples {
            if let Some(air_temp_f) = sample.air_temp_f {
                total += air_temp_f;
                count += 1;
            }
        }
        (count > 0).then_some(total / count as f64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HeatEstimatorStore {
    pool_learned_rate_f_per_hour: Option<f64>,
    spa_learned_rate_f_per_hour: Option<f64>,
    pool_last_reliable_temperature: Option<ReliableTemperatureObservation>,
    spa_last_reliable_temperature: Option<ReliableTemperatureObservation>,
    recent_sessions: Vec<CompletedHeatingSession>,
}

impl HeatEstimatorStore {
    fn seed_last_reliable_from_recent_sessions(&mut self) {
        if self.pool_last_reliable_temperature.is_none() {
            self.pool_last_reliable_temperature =
                self.latest_reliable_from_recent_sessions(HeatingBodyKind::Pool);
        }
        if self.spa_last_reliable_temperature.is_none() {
            self.spa_last_reliable_temperature =
                self.latest_reliable_from_recent_sessions(HeatingBodyKind::Spa);
        }
    }

    fn latest_reliable_from_recent_sessions(
        &self,
        body: HeatingBodyKind,
    ) -> Option<ReliableTemperatureObservation> {
        self.recent_sessions
            .iter()
            .filter(|session| session.body == body)
            .max_by_key(|session| session.ended_at_unix_ms)
            .map(|session| ReliableTemperatureObservation {
                temperature: session.end_temp_f.round() as i32,
                observed_at_unix_ms: session.ended_at_unix_ms,
            })
    }
}

#[derive(Debug)]
pub struct HeatEstimator {
    config: HeatingConfig,
    path: PathBuf,
    store: HeatEstimatorStore,
    pool_session: Option<ActiveHeatingSession>,
    spa_session: Option<ActiveHeatingSession>,
    pool_active_since_unix_ms: Option<i64>,
    spa_active_since_unix_ms: Option<i64>,
    pool_last_active_observed: Option<bool>,
    spa_last_active_observed: Option<bool>,
}

impl HeatEstimator {
    pub fn load(config: HeatingConfig, path: PathBuf) -> Self {
        let store = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => HeatEstimatorStore::default(),
            }
        } else {
            HeatEstimatorStore::default()
        };
        let mut store = store;
        store.seed_last_reliable_from_recent_sessions();

        info!(
            "heat estimator: loaded {} completed session(s) from {:?}",
            store.recent_sessions.len(),
            path
        );

        Self {
            config,
            path,
            store,
            pool_session: None,
            spa_session: None,
            pool_active_since_unix_ms: None,
            spa_active_since_unix_ms: None,
            pool_last_active_observed: None,
            spa_last_active_observed: None,
        }
    }

    pub fn update(&mut self, system: &PoolSystem) {
        let use_celsius = uses_celsius(system.system.temp_unit);
        let air_temp_f = Some(to_fahrenheit(
            system.system.air_temperature as f64,
            use_celsius,
        ));
        let now_unix_ms = unix_time_ms();
        let shared_pump = system.system.pool_spa_shared_pump;

        let pool = system
            .pool
            .as_ref()
            .map(|body| BodyTelemetry::from_pool(body, air_temp_f, use_celsius, shared_pump));
        let spa = system
            .spa
            .as_ref()
            .map(|body| BodyTelemetry::from_spa(body, air_temp_f, use_celsius, shared_pump));

        self.update_active_since_for_body(HeatingBodyKind::Pool, pool.as_ref(), now_unix_ms);
        self.update_active_since_for_body(HeatingBodyKind::Spa, spa.as_ref(), now_unix_ms);
        self.update_last_reliable_temperature_for_body(
            HeatingBodyKind::Pool,
            pool.as_ref(),
            now_unix_ms,
        );
        self.update_last_reliable_temperature_for_body(
            HeatingBodyKind::Spa,
            spa.as_ref(),
            now_unix_ms,
        );

        if !self.config.enabled {
            return;
        }

        self.update_session_for_body(HeatingBodyKind::Pool, pool, now_unix_ms);
        self.update_session_for_body(HeatingBodyKind::Spa, spa, now_unix_ms);
    }

    pub fn seed_last_reliable_from_controller_history(
        &mut self,
        history: &HistoryData,
        controller_now: &SLDateTime,
        now_unix_ms: i64,
        pool_spa_shared_pump: bool,
    ) {
        let Ok(controller_now_naive) = sl_to_naive(controller_now) else {
            warn!("failed to parse controller time for history backfill");
            return;
        };

        let pool_candidate = latest_history_observation_for_body(
            HeatingBodyKind::Pool,
            history,
            &controller_now_naive,
            now_unix_ms,
            pool_spa_shared_pump,
            self.config.shared_equipment_temp_warmup_seconds,
        );
        let spa_candidate = latest_history_observation_for_body(
            HeatingBodyKind::Spa,
            history,
            &controller_now_naive,
            now_unix_ms,
            pool_spa_shared_pump,
            self.config.shared_equipment_temp_warmup_seconds,
        );

        let mut changed = false;
        changed |= self.apply_history_candidate(HeatingBodyKind::Pool, pool_candidate);
        changed |= self.apply_history_candidate(HeatingBodyKind::Spa, spa_candidate);

        if changed {
            self.persist();
        }
    }

    pub fn apply_to_system(&self, system: &mut PoolSystem) {
        let use_celsius = uses_celsius(system.system.temp_unit);
        let air_temp_f = Some(to_fahrenheit(
            system.system.air_temperature as f64,
            use_celsius,
        ));
        let shared_pump = system.system.pool_spa_shared_pump;
        let now_unix_ms = unix_time_ms();

        if let Some(pool) = system.pool.as_mut() {
            let telemetry = BodyTelemetry::from_pool(pool, air_temp_f, use_celsius, shared_pump);
            let trust =
                self.temperature_trust_for_body(HeatingBodyKind::Pool, &telemetry, now_unix_ms);
            let last_reliable = self.last_reliable_temperature(HeatingBodyKind::Pool);
            pool.temperature_reliable = trust.reliable;
            pool.temperature_reason = trust.reason.map(str::to_string);
            pool.last_reliable_temperature =
                last_reliable.map(|observation| observation.temperature);
            pool.last_reliable_temperature_at_unix_ms =
                last_reliable.map(|observation| observation.observed_at_unix_ms);
            if self.config.enabled {
                pool.heat_estimate =
                    Some(self.estimate_for_body(HeatingBodyKind::Pool, &telemetry, use_celsius));
            }
            pool.temperature_display = temperature_display(pool);
            pool.heat_estimate_display = self.heat_estimate_display(
                HeatingBodyKind::Pool,
                &telemetry,
                now_unix_ms,
                pool.heat_estimate.as_ref(),
            );
        }

        if let Some(spa) = system.spa.as_mut() {
            let telemetry = BodyTelemetry::from_spa(spa, air_temp_f, use_celsius, shared_pump);
            let trust =
                self.temperature_trust_for_body(HeatingBodyKind::Spa, &telemetry, now_unix_ms);
            let last_reliable = self.last_reliable_temperature(HeatingBodyKind::Spa);
            spa.temperature_reliable = trust.reliable;
            spa.temperature_reason = trust.reason.map(str::to_string);
            spa.last_reliable_temperature =
                last_reliable.map(|observation| observation.temperature);
            spa.last_reliable_temperature_at_unix_ms =
                last_reliable.map(|observation| observation.observed_at_unix_ms);
            if self.config.enabled {
                spa.heat_estimate =
                    Some(self.estimate_for_body(HeatingBodyKind::Spa, &telemetry, use_celsius));
            }
            spa.temperature_display = temperature_display(spa);
            spa.heat_estimate_display = self.heat_estimate_display(
                HeatingBodyKind::Spa,
                &telemetry,
                now_unix_ms,
                spa.heat_estimate.as_ref(),
            );
        }
    }

    fn update_active_since_for_body(
        &mut self,
        body: HeatingBodyKind,
        telemetry: Option<&BodyTelemetry>,
        now_unix_ms: i64,
    ) {
        let warmup_ms = self.config.shared_equipment_temp_warmup_seconds as i64 * 1000;
        let (slot, last_active_observed) = match body {
            HeatingBodyKind::Pool => (
                &mut self.pool_active_since_unix_ms,
                &mut self.pool_last_active_observed,
            ),
            HeatingBodyKind::Spa => (
                &mut self.spa_active_since_unix_ms,
                &mut self.spa_last_active_observed,
            ),
        };

        match telemetry {
            Some(telemetry) if telemetry.active => {
                if *last_active_observed == Some(false) {
                    *slot = Some(now_unix_ms);
                } else if slot.is_none() {
                    // On daemon restart we may first observe a body that has already been
                    // circulating for a while. Seed it as already warmed instead of forcing
                    // a new warmup window for an in-progress run.
                    *slot = Some(now_unix_ms.saturating_sub(warmup_ms.max(0)));
                }
                *last_active_observed = Some(true);
            }
            Some(_) => {
                *slot = None;
                *last_active_observed = Some(false);
            }
            None => {
                *slot = None;
                *last_active_observed = None;
            }
        }
    }

    fn update_session_for_body(
        &mut self,
        body: HeatingBodyKind,
        telemetry: Option<BodyTelemetry>,
        now_unix_ms: i64,
    ) {
        let sample_window_minutes = self.config.sample_window_minutes;
        let trust_reliable = telemetry
            .as_ref()
            .map(|telemetry| {
                self.temperature_trust_for_body(body, telemetry, now_unix_ms)
                    .reliable
            })
            .unwrap_or(false);
        let should_track = telemetry
            .as_ref()
            .map(|telemetry| trust_reliable && Self::should_track_heating_session(telemetry))
            .unwrap_or(false);

        let slot = self.session_slot_mut(body);

        match (slot.as_mut(), telemetry.as_ref(), should_track) {
            (Some(session), Some(telemetry), true) => {
                session.record(telemetry, now_unix_ms, sample_window_minutes);
            }
            (Some(session), Some(telemetry), false) => {
                if trust_reliable && telemetry.on && telemetry.active {
                    session.record(telemetry, now_unix_ms, sample_window_minutes);
                }
                let finished = slot.take();
                self.finalize_session(finished, now_unix_ms);
            }
            (Some(_), _, false) => {
                let finished = slot.take();
                self.finalize_session(finished, now_unix_ms);
            }
            (None, Some(telemetry), true) => {
                *slot = Some(ActiveHeatingSession::start(body, telemetry, now_unix_ms));
            }
            (None, _, false) => {}
            _ => {}
        }
    }

    fn update_last_reliable_temperature_for_body(
        &mut self,
        body: HeatingBodyKind,
        telemetry: Option<&BodyTelemetry>,
        now_unix_ms: i64,
    ) {
        let Some(telemetry) = telemetry else {
            return;
        };

        let trust = self.temperature_trust_for_body(body, telemetry, now_unix_ms);
        if !trust.reliable {
            return;
        }

        let next = ReliableTemperatureObservation {
            temperature: telemetry.temperature,
            observed_at_unix_ms: now_unix_ms,
        };
        let slot = self.last_reliable_temperature_slot_mut(body);
        let should_persist = match *slot {
            Some(previous)
                if previous.temperature == next.temperature
                    && now_unix_ms - previous.observed_at_unix_ms
                        < LAST_RELIABLE_PERSIST_INTERVAL_MS =>
            {
                false
            }
            _ => true,
        };

        *slot = Some(next);
        if should_persist {
            self.persist();
        }
    }

    fn apply_history_candidate(
        &mut self,
        body: HeatingBodyKind,
        candidate: Option<ReliableTemperatureObservation>,
    ) -> bool {
        let Some(candidate) = candidate else {
            return false;
        };
        let slot = self.last_reliable_temperature_slot_mut(body);
        let should_update = match *slot {
            Some(existing) => candidate.observed_at_unix_ms > existing.observed_at_unix_ms,
            None => true,
        };
        if should_update {
            *slot = Some(candidate);
            true
        } else {
            false
        }
    }

    fn finalize_session(&mut self, session: Option<ActiveHeatingSession>, now_unix_ms: i64) {
        let Some(session) = session else {
            return;
        };

        let Some(rate_f_per_hour) = session.observed_rate_f_per_hour(
            self.config.minimum_runtime_minutes,
            self.config.minimum_temp_rise_f,
        ) else {
            return;
        };

        let completed = CompletedHeatingSession {
            body: session.body,
            started_at_unix_ms: session.started_at_unix_ms,
            ended_at_unix_ms: now_unix_ms,
            start_temp_f: session.start_temp_f,
            end_temp_f: session.latest_temp_f,
            target_temp_f: session.target_temp_f,
            average_air_temp_f: session.average_air_temp_f(),
            duration_minutes: session.elapsed_minutes(),
            average_rate_f_per_hour: rate_f_per_hour,
        };

        self.update_learned_rate(session.body, rate_f_per_hour);
        self.store.recent_sessions.push(completed);
        if self.store.recent_sessions.len() > MAX_RECENT_SESSIONS {
            let excess = self.store.recent_sessions.len() - MAX_RECENT_SESSIONS;
            self.store.recent_sessions.drain(0..excess);
        }
        self.persist();
    }

    fn estimate_for_body(
        &self,
        body: HeatingBodyKind,
        telemetry: &BodyTelemetry,
        use_celsius: bool,
    ) -> HeatEstimate {
        let now_unix_ms = unix_time_ms();
        if !telemetry.on {
            return unavailable_estimate(&telemetry, now_unix_ms, "not-heating");
        }
        if telemetry.heat_mode == "off" {
            return unavailable_estimate(&telemetry, now_unix_ms, "heat-off");
        }

        let trust = self.temperature_trust_for_body(body, &telemetry, now_unix_ms);

        if !trust.reliable {
            return unavailable_estimate(
                &telemetry,
                now_unix_ms,
                trust.reason.unwrap_or("temperature-unreliable"),
            );
        }

        if !telemetry.active {
            return unavailable_estimate(&telemetry, now_unix_ms, "waiting-for-flow");
        }
        if telemetry.setpoint <= telemetry.temperature {
            return unavailable_estimate(&telemetry, now_unix_ms, "at-temp");
        }

        let configured_rate_f_per_hour = self.configured_rate_f_per_hour(body);
        let learned_rate_f_per_hour = self.learned_rate_f_per_hour(body, telemetry.air_temp_f);
        let observed_rate_f_per_hour = self.session_slot(body).and_then(|session| {
            session.observed_rate_f_per_hour(
                self.config.minimum_runtime_minutes,
                self.config.minimum_temp_rise_f,
            )
        });

        let baseline_rate_f_per_hour = match (learned_rate_f_per_hour, configured_rate_f_per_hour) {
            (Some(learned), Some(configured)) => Some(learned * 0.7 + configured * 0.3),
            (Some(learned), None) => Some(learned),
            (None, Some(configured)) => Some(configured),
            (None, None) => None,
        };

        let (effective_rate_f_per_hour, source, confidence) =
            match (observed_rate_f_per_hour, baseline_rate_f_per_hour) {
                (Some(observed), Some(baseline)) => {
                    let evidence = self
                        .session_slot(body)
                        .map(|session| (session.elapsed_minutes() / 30.0).clamp(0.0, 1.0))
                        .unwrap_or(0.0);
                    let blended = baseline * (1.0 - evidence) + observed * evidence;
                    let confidence = if evidence >= 0.75 { "high" } else { "medium" };
                    (Some(blended), "blended".to_string(), confidence.to_string())
                }
                (Some(observed), None) => {
                    (Some(observed), "observed".to_string(), "medium".to_string())
                }
                (None, Some(_)) if learned_rate_f_per_hour.is_some() => (
                    baseline_rate_f_per_hour,
                    "learned".to_string(),
                    "medium".to_string(),
                ),
                (None, Some(_)) => (
                    baseline_rate_f_per_hour,
                    "configured".to_string(),
                    "low".to_string(),
                ),
                (None, None) => (None, "none".to_string(), "none".to_string()),
            };

        let Some(rate_f_per_hour) =
            effective_rate_f_per_hour.filter(|rate| rate.is_finite() && *rate > 0.0)
        else {
            let reason = if telemetry.heating == "off" || telemetry.heating == "unknown" {
                "insufficient-data"
            } else {
                "missing-config"
            };
            return unavailable_estimate(&telemetry, now_unix_ms, reason);
        };

        let remaining_delta_f = (telemetry.setpoint_f - telemetry.temperature_f).max(0.0);
        let minutes_remaining = ((remaining_delta_f / rate_f_per_hour) * 60.0).ceil();

        HeatEstimate {
            available: true,
            minutes_remaining: Some(minutes_remaining.max(1.0) as u32),
            current_temperature: telemetry.temperature,
            target_temperature: telemetry.setpoint,
            confidence,
            source,
            reason: "estimating".to_string(),
            observed_rate_per_hour: observed_rate_f_per_hour
                .map(|rate| from_fahrenheit_rate(rate, use_celsius)),
            learned_rate_per_hour: learned_rate_f_per_hour
                .map(|rate| from_fahrenheit_rate(rate, use_celsius)),
            configured_rate_per_hour: configured_rate_f_per_hour
                .map(|rate| from_fahrenheit_rate(rate, use_celsius)),
            baseline_rate_per_hour: baseline_rate_f_per_hour
                .map(|rate| from_fahrenheit_rate(rate, use_celsius)),
            updated_at_unix_ms: now_unix_ms,
        }
    }

    fn configured_rate_f_per_hour(&self, body: HeatingBodyKind) -> Option<f64> {
        let volume_gallons = match body {
            HeatingBodyKind::Pool => self.config.pool.effective_volume_gallons(),
            HeatingBodyKind::Spa => self.config.spa.effective_volume_gallons(),
        }?;

        if volume_gallons <= 0.0 {
            return None;
        }

        let output_btu_per_hr = self.config.heater.output_btu_per_hr;
        if output_btu_per_hr <= 0.0 {
            return None;
        }

        let efficiency = self.config.heater.efficiency.unwrap_or_else(|| {
            match self.config.heater.kind.as_str() {
                "gas" => 0.84,
                "hybrid" => 0.9,
                _ => 1.0,
            }
        });

        let effective_btu_per_hr = output_btu_per_hr * efficiency.max(0.0);
        let rate_f_per_hour = effective_btu_per_hr / (volume_gallons * WATER_LB_PER_GALLON);

        (rate_f_per_hour.is_finite() && rate_f_per_hour > 0.0).then_some(rate_f_per_hour)
    }

    fn learned_rate_slot(&self, body: HeatingBodyKind) -> Option<f64> {
        match body {
            HeatingBodyKind::Pool => self.store.pool_learned_rate_f_per_hour,
            HeatingBodyKind::Spa => self.store.spa_learned_rate_f_per_hour,
        }
    }

    fn learned_rate_from_recent_sessions(
        &self,
        body: HeatingBodyKind,
        current_air_temp_f: Option<f64>,
    ) -> Option<f64> {
        let mut weighted_total = 0.0;
        let mut total_weight = 0.0;

        for session in self
            .store
            .recent_sessions
            .iter()
            .filter(|session| session.body == body)
        {
            let weight = match (current_air_temp_f, session.average_air_temp_f) {
                (Some(current), Some(session_air)) => {
                    1.0 / (1.0 + ((current - session_air).abs() / AMBIENT_RATE_WEIGHT_SPAN_F))
                }
                _ => 1.0,
            };
            weighted_total += session.average_rate_f_per_hour * weight;
            total_weight += weight;
        }

        (total_weight > 0.0).then_some(weighted_total / total_weight)
    }

    fn learned_rate_f_per_hour(
        &self,
        body: HeatingBodyKind,
        current_air_temp_f: Option<f64>,
    ) -> Option<f64> {
        match (
            self.learned_rate_slot(body),
            self.learned_rate_from_recent_sessions(body, current_air_temp_f),
        ) {
            (Some(slot), Some(ambient_adjusted)) => Some(slot * 0.4 + ambient_adjusted * 0.6),
            (Some(slot), None) => Some(slot),
            (None, Some(ambient_adjusted)) => Some(ambient_adjusted),
            (None, None) => None,
        }
    }

    fn active_since_slot(&self, body: HeatingBodyKind) -> Option<i64> {
        match body {
            HeatingBodyKind::Pool => self.pool_active_since_unix_ms,
            HeatingBodyKind::Spa => self.spa_active_since_unix_ms,
        }
    }

    fn last_reliable_temperature(
        &self,
        body: HeatingBodyKind,
    ) -> Option<ReliableTemperatureObservation> {
        match body {
            HeatingBodyKind::Pool => self.store.pool_last_reliable_temperature,
            HeatingBodyKind::Spa => self.store.spa_last_reliable_temperature,
        }
    }

    fn last_reliable_temperature_slot_mut(
        &mut self,
        body: HeatingBodyKind,
    ) -> &mut Option<ReliableTemperatureObservation> {
        match body {
            HeatingBodyKind::Pool => &mut self.store.pool_last_reliable_temperature,
            HeatingBodyKind::Spa => &mut self.store.spa_last_reliable_temperature,
        }
    }

    fn temperature_trust_for_body(
        &self,
        body: HeatingBodyKind,
        telemetry: &BodyTelemetry,
        now_unix_ms: i64,
    ) -> TemperatureTrust {
        if !telemetry.pool_spa_shared_pump {
            return TemperatureTrust {
                reliable: true,
                reason: None,
            };
        }

        if !telemetry.on {
            return TemperatureTrust {
                reliable: false,
                reason: Some("inactive-shared-body"),
            };
        }

        if !telemetry.active {
            return TemperatureTrust {
                reliable: false,
                reason: Some("waiting-for-flow"),
            };
        }

        let warmup_ms = self.config.shared_equipment_temp_warmup_seconds as i64 * 1000;
        if warmup_ms > 0 {
            if let Some(active_since_unix_ms) = self.active_since_slot(body) {
                if now_unix_ms - active_since_unix_ms < warmup_ms {
                    return TemperatureTrust {
                        reliable: false,
                        reason: Some("sensor-warmup"),
                    };
                }
            }
        }

        TemperatureTrust {
            reliable: true,
            reason: None,
        }
    }

    fn warmup_remaining_seconds(
        &self,
        body: HeatingBodyKind,
        telemetry: &BodyTelemetry,
        now_unix_ms: i64,
    ) -> Option<u32> {
        if !telemetry.pool_spa_shared_pump || !telemetry.on || !telemetry.active {
            return None;
        }

        let warmup_ms = self.config.shared_equipment_temp_warmup_seconds as i64 * 1000;
        if warmup_ms <= 0 {
            return None;
        }

        let active_since_unix_ms = self.active_since_slot(body)?;
        let elapsed_ms = now_unix_ms - active_since_unix_ms;
        if elapsed_ms >= warmup_ms {
            return None;
        }

        let remaining_ms = (warmup_ms - elapsed_ms).max(0);
        Some(((remaining_ms + 999) / 1000) as u32)
    }

    fn heat_estimate_display(
        &self,
        body: HeatingBodyKind,
        telemetry: &BodyTelemetry,
        now_unix_ms: i64,
        estimate: Option<&HeatEstimate>,
    ) -> HeatEstimateDisplay {
        match estimate {
            Some(estimate) if estimate.available => HeatEstimateDisplay {
                state: "ready".to_string(),
                reason: None,
                available_in_seconds: None,
                minutes_remaining: estimate.minutes_remaining,
                target_temperature: Some(estimate.target_temperature),
            },
            Some(estimate)
                if estimate.reason == "sensor-warmup" || estimate.reason == "insufficient-data" =>
            {
                HeatEstimateDisplay {
                    state: "pending".to_string(),
                    reason: Some(estimate.reason.clone()),
                    available_in_seconds: if estimate.reason == "sensor-warmup" {
                        self.warmup_remaining_seconds(body, telemetry, now_unix_ms)
                    } else {
                        None
                    },
                    minutes_remaining: None,
                    target_temperature: Some(estimate.target_temperature),
                }
            }
            Some(estimate) => HeatEstimateDisplay {
                state: "unavailable".to_string(),
                reason: Some(estimate.reason.clone()),
                available_in_seconds: None,
                minutes_remaining: None,
                target_temperature: Some(estimate.target_temperature),
            },
            None => HeatEstimateDisplay {
                state: "unavailable".to_string(),
                reason: Some("not-configured".to_string()),
                available_in_seconds: None,
                minutes_remaining: None,
                target_temperature: None,
            },
        }
    }

    fn update_learned_rate(&mut self, body: HeatingBodyKind, observed_rate_f_per_hour: f64) {
        let slot = match body {
            HeatingBodyKind::Pool => &mut self.store.pool_learned_rate_f_per_hour,
            HeatingBodyKind::Spa => &mut self.store.spa_learned_rate_f_per_hour,
        };

        *slot = Some(match *slot {
            Some(existing) => existing * 0.7 + observed_rate_f_per_hour * 0.3,
            None => observed_rate_f_per_hour,
        });
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string_pretty(&self.store) {
            Ok(json) => {
                if let Err(error) = std::fs::write(&self.path, json) {
                    warn!("failed to persist heat estimator store: {}", error);
                }
            }
            Err(error) => warn!("failed to serialize heat estimator store: {}", error),
        }
    }

    fn session_slot(&self, body: HeatingBodyKind) -> Option<&ActiveHeatingSession> {
        match body {
            HeatingBodyKind::Pool => self.pool_session.as_ref(),
            HeatingBodyKind::Spa => self.spa_session.as_ref(),
        }
    }

    fn session_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Option<ActiveHeatingSession> {
        match body {
            HeatingBodyKind::Pool => &mut self.pool_session,
            HeatingBodyKind::Spa => &mut self.spa_session,
        }
    }

    fn should_track_heating_session(telemetry: &BodyTelemetry) -> bool {
        telemetry.on
            && telemetry.active
            && telemetry.heat_mode != "off"
            && telemetry.setpoint_f > telemetry.temperature_f
    }
}

impl BodyTelemetry {
    fn from_pool(
        body: &BodyState,
        air_temp_f: Option<f64>,
        use_celsius: bool,
        pool_spa_shared_pump: bool,
    ) -> Self {
        Self {
            on: body.on,
            active: body.active,
            pool_spa_shared_pump,
            temperature: body.temperature,
            setpoint: body.setpoint,
            temperature_f: to_fahrenheit(body.temperature as f64, use_celsius),
            setpoint_f: to_fahrenheit(body.setpoint as f64, use_celsius),
            heat_mode: body.heat_mode.to_lowercase(),
            heating: body.heating.to_lowercase(),
            air_temp_f,
        }
    }

    fn from_spa(
        body: &SpaState,
        air_temp_f: Option<f64>,
        use_celsius: bool,
        pool_spa_shared_pump: bool,
    ) -> Self {
        Self {
            on: body.on,
            active: body.active,
            pool_spa_shared_pump,
            temperature: body.temperature,
            setpoint: body.setpoint,
            temperature_f: to_fahrenheit(body.temperature as f64, use_celsius),
            setpoint_f: to_fahrenheit(body.setpoint as f64, use_celsius),
            heat_mode: body.heat_mode.to_lowercase(),
            heating: body.heating.to_lowercase(),
            air_temp_f,
        }
    }
}

fn latest_history_observation_for_body(
    body: HeatingBodyKind,
    history: &HistoryData,
    controller_now: &NaiveDateTime,
    now_unix_ms: i64,
    pool_spa_shared_pump: bool,
    shared_warmup_seconds: u64,
) -> Option<ReliableTemperatureObservation> {
    let (temps, runs) = match body {
        HeatingBodyKind::Pool => (&history.pool_temps, &history.pool_runs),
        HeatingBodyKind::Spa => (&history.spa_temps, &history.spa_runs),
    };
    let warmup_ms = if pool_spa_shared_pump {
        shared_warmup_seconds as i64 * 1000
    } else {
        0
    };

    temps
        .iter()
        .filter_map(|point| {
            let sample_unix_ms =
                controller_history_time_to_unix_ms(&point.time, controller_now, now_unix_ms)?;
            let inside_run = runs.iter().any(|run| {
                history_run_contains_sample(
                    run,
                    controller_now,
                    now_unix_ms,
                    sample_unix_ms,
                    warmup_ms,
                )
            });
            inside_run.then_some(ReliableTemperatureObservation {
                temperature: point.temp,
                observed_at_unix_ms: sample_unix_ms,
            })
        })
        .max_by_key(|observation| observation.observed_at_unix_ms)
}

fn history_run_contains_sample(
    run: &TimeRangePoint,
    controller_now: &NaiveDateTime,
    now_unix_ms: i64,
    sample_unix_ms: i64,
    warmup_ms: i64,
) -> bool {
    let Some(on_unix_ms) = controller_history_time_to_unix_ms(&run.on, controller_now, now_unix_ms)
    else {
        return false;
    };
    let Some(off_unix_ms) =
        controller_history_time_to_unix_ms(&run.off, controller_now, now_unix_ms)
    else {
        return false;
    };

    if off_unix_ms < on_unix_ms {
        return false;
    }

    sample_unix_ms >= on_unix_ms.saturating_add(warmup_ms) && sample_unix_ms <= off_unix_ms
}

fn controller_history_time_to_unix_ms(
    time: &SLDateTime,
    controller_now: &NaiveDateTime,
    now_unix_ms: i64,
) -> Option<i64> {
    let sample = sl_to_naive(time).ok()?;
    let delta_ms = sample
        .signed_duration_since(*controller_now)
        .num_milliseconds();
    Some(now_unix_ms.saturating_add(delta_ms))
}

fn sl_to_naive(time: &SLDateTime) -> Result<NaiveDateTime, ()> {
    let Some(date) = NaiveDate::from_ymd_opt(time.year as i32, time.month as u32, time.day as u32)
    else {
        return Err(());
    };
    date.and_hms_milli_opt(
        time.hour as u32,
        time.minute as u32,
        time.second as u32,
        time.millisecond as u32,
    )
    .ok_or(())
}

fn unavailable_estimate(
    telemetry: &BodyTelemetry,
    updated_at_unix_ms: i64,
    reason: &str,
) -> HeatEstimate {
    HeatEstimate {
        available: false,
        minutes_remaining: None,
        current_temperature: telemetry.temperature,
        target_temperature: telemetry.setpoint,
        confidence: "none".to_string(),
        source: "none".to_string(),
        reason: reason.to_string(),
        observed_rate_per_hour: None,
        learned_rate_per_hour: None,
        configured_rate_per_hour: None,
        baseline_rate_per_hour: None,
        updated_at_unix_ms,
    }
}

fn temperature_display(body: &impl TemperatureDisplaySource) -> TemperatureDisplay {
    TemperatureDisplay {
        value: if body.temperature_reliable() {
            Some(body.temperature())
        } else {
            body.last_reliable_temperature()
        },
        is_stale: !body.temperature_reliable(),
        stale_reason: body.temperature_reason().map(str::to_string),
        last_reliable_at_unix_ms: body.last_reliable_temperature_at_unix_ms(),
    }
}

trait TemperatureDisplaySource {
    fn temperature(&self) -> i32;
    fn temperature_reliable(&self) -> bool;
    fn temperature_reason(&self) -> Option<&str>;
    fn last_reliable_temperature(&self) -> Option<i32>;
    fn last_reliable_temperature_at_unix_ms(&self) -> Option<i64>;
}

impl TemperatureDisplaySource for BodyState {
    fn temperature(&self) -> i32 {
        self.temperature
    }

    fn temperature_reliable(&self) -> bool {
        self.temperature_reliable
    }

    fn temperature_reason(&self) -> Option<&str> {
        self.temperature_reason.as_deref()
    }

    fn last_reliable_temperature(&self) -> Option<i32> {
        self.last_reliable_temperature
    }

    fn last_reliable_temperature_at_unix_ms(&self) -> Option<i64> {
        self.last_reliable_temperature_at_unix_ms
    }
}

impl TemperatureDisplaySource for SpaState {
    fn temperature(&self) -> i32 {
        self.temperature
    }

    fn temperature_reliable(&self) -> bool {
        self.temperature_reliable
    }

    fn temperature_reason(&self) -> Option<&str> {
        self.temperature_reason.as_deref()
    }

    fn last_reliable_temperature(&self) -> Option<i32> {
        self.last_reliable_temperature
    }

    fn last_reliable_temperature_at_unix_ms(&self) -> Option<i64> {
        self.last_reliable_temperature_at_unix_ms
    }
}

fn uses_celsius(temp_unit: &str) -> bool {
    temp_unit.contains('C')
}

fn to_fahrenheit(value: f64, use_celsius: bool) -> f64 {
    if use_celsius {
        value * 9.0 / 5.0 + 32.0
    } else {
        value
    }
}

fn from_fahrenheit_rate(rate_f_per_hour: f64, use_celsius: bool) -> f64 {
    if use_celsius {
        rate_f_per_hour / 1.8
    } else {
        rate_f_per_hour
    }
}

fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use pentair_protocol::responses::{HistoryData, TimeRangePoint, TimeTempPoint};
    use pentair_protocol::semantic::{
        AuxState, BodyState, LightState, PumpInfo, SpaState, SystemInfo,
    };
    use std::collections::HashMap;

    fn test_config() -> HeatingConfig {
        let mut config = HeatingConfig::default();
        config.enabled = true;
        config.heater.kind = "gas".to_string();
        config.heater.output_btu_per_hr = 400_000.0;
        config.heater.efficiency = Some(0.84);
        config.pool.volume_gallons = Some(16_000.0);
        config.spa.volume_gallons = Some(500.0);
        config.minimum_runtime_minutes = 0;
        config.minimum_temp_rise_f = 0.0;
        config
    }

    fn test_config_with_spa_dimensions() -> HeatingConfig {
        let mut config = test_config();
        config.spa.volume_gallons = None;
        config.spa.dimensions = Some(crate::config::BodyDimensionsConfig {
            length_ft: Some(8.0),
            width_ft: Some(8.0),
            average_depth_ft: Some(4.0),
            shape_factor: 1.0,
        });
        config
    }

    fn test_system(
        spa_temp: i32,
        spa_setpoint: i32,
        spa_on: bool,
        spa_active: bool,
        spa_heating: &str,
    ) -> PoolSystem {
        PoolSystem {
            pool: Some(BodyState {
                on: false,
                active: false,
                temperature: 80,
                temperature_reliable: true,
                temperature_reason: None,
                last_reliable_temperature: None,
                last_reliable_temperature_at_unix_ms: None,
                setpoint: 82,
                heat_mode: "heat-pump".to_string(),
                heating: "off".to_string(),
                heat_estimate: None,
                temperature_display: TemperatureDisplay {
                    value: Some(80),
                    is_stale: false,
                    stale_reason: None,
                    last_reliable_at_unix_ms: None,
                },
                heat_estimate_display: HeatEstimateDisplay {
                    state: "unavailable".to_string(),
                    reason: None,
                    available_in_seconds: None,
                    minutes_remaining: None,
                    target_temperature: None,
                },
            }),
            spa: Some(SpaState {
                on: spa_on,
                active: spa_active,
                temperature: spa_temp,
                temperature_reliable: true,
                temperature_reason: None,
                last_reliable_temperature: None,
                last_reliable_temperature_at_unix_ms: None,
                setpoint: spa_setpoint,
                heat_mode: "heat-pump".to_string(),
                heating: spa_heating.to_string(),
                heat_estimate: None,
                temperature_display: TemperatureDisplay {
                    value: Some(spa_temp),
                    is_stale: false,
                    stale_reason: None,
                    last_reliable_at_unix_ms: None,
                },
                heat_estimate_display: HeatEstimateDisplay {
                    state: "unavailable".to_string(),
                    reason: None,
                    available_in_seconds: None,
                    minutes_remaining: None,
                    target_temperature: None,
                },
                accessories: HashMap::new(),
            }),
            lights: Some(LightState {
                on: false,
                mode: None,
                available_modes: vec!["off", "party"],
            }),
            auxiliaries: vec![AuxState {
                id: "aux1".to_string(),
                name: "Aux 1".to_string(),
                on: false,
            }],
            pump: Some(PumpInfo {
                pump_type: "VSF".to_string(),
                running: true,
                watts: 2000,
                rpm: 2600,
                gpm: 45,
            }),
            system: SystemInfo {
                controller: "IntelliTouch".to_string(),
                firmware: None,
                temp_unit: "°F",
                air_temperature: 70,
                freeze_protection: false,
                pool_spa_shared_pump: true,
            },
        }
    }

    fn test_store_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pentair-heat-estimator-{}-{}.json",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn history_time(day: u16, hour: u16, minute: u16, second: u16) -> SLDateTime {
        SLDateTime {
            year: 2026,
            month: 3,
            day_of_week: 0,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        }
    }

    #[test]
    fn configured_spa_eta_is_available_without_history() {
        let path = test_store_path("configured-spa");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, true, true, "heater");

        estimator.update(&system);
        estimator.spa_active_since_unix_ms = Some(unix_time_ms() - 121_000);
        estimator.apply_to_system(&mut system);

        let estimate = system
            .spa
            .and_then(|spa| spa.heat_estimate)
            .expect("spa estimate");
        assert!(estimate.available);
        assert_eq!(estimate.source, "configured");
        assert_eq!(estimate.confidence, "low");
        assert!(estimate.minutes_remaining.unwrap() > 0);
    }

    #[test]
    fn estimate_is_unavailable_when_body_is_off() {
        let path = test_store_path("body-off");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, false, false, "off");
        system.system.pool_spa_shared_pump = false;

        estimator.update(&system);
        estimator.apply_to_system(&mut system);

        let estimate = system
            .spa
            .and_then(|spa| spa.heat_estimate)
            .expect("spa estimate");
        assert!(!estimate.available);
        assert_eq!(estimate.reason, "not-heating");
    }

    #[test]
    fn observed_session_updates_learning() {
        let path = test_store_path("observed-learning");
        let mut estimator = HeatEstimator::load(test_config(), path);

        let system = test_system(99, 104, true, true, "heater");
        estimator.update(&system);

        if let Some(session) = estimator.spa_session.as_mut() {
            session.started_at_unix_ms -= 20 * 60_000;
            session.start_temp_f = 96.0;
            session.latest_at_unix_ms = unix_time_ms();
            session.latest_temp_f = 100.0;
        }

        let stopping_system = test_system(100, 104, false, false, "off");
        estimator.update(&stopping_system);

        assert!(estimator.store.spa_learned_rate_f_per_hour.is_some());
    }

    #[test]
    fn ambient_adjusted_learning_prefers_similar_air_temperatures() {
        let path = test_store_path("ambient-weighted-learning");
        let mut estimator = HeatEstimator::load(test_config(), path);
        estimator.store.spa_learned_rate_f_per_hour = Some(16.0);
        estimator.store.recent_sessions = vec![
            CompletedHeatingSession {
                body: HeatingBodyKind::Spa,
                started_at_unix_ms: 0,
                ended_at_unix_ms: 0,
                start_temp_f: 90.0,
                end_temp_f: 95.0,
                target_temp_f: 97.0,
                average_air_temp_f: Some(40.0),
                duration_minutes: 20.0,
                average_rate_f_per_hour: 10.0,
            },
            CompletedHeatingSession {
                body: HeatingBodyKind::Spa,
                started_at_unix_ms: 0,
                ended_at_unix_ms: 0,
                start_temp_f: 90.0,
                end_temp_f: 95.0,
                target_temp_f: 97.0,
                average_air_temp_f: Some(70.0),
                duration_minutes: 10.0,
                average_rate_f_per_hour: 20.0,
            },
        ];

        let learned = estimator
            .learned_rate_f_per_hour(HeatingBodyKind::Spa, Some(68.0))
            .expect("learned rate");
        assert!(learned > 16.0);
    }

    #[test]
    fn session_waits_for_reliable_temperature_after_shared_warmup() {
        let path = test_store_path("session-waits-for-warmup");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let off_system = test_system(92, 97, false, false, "off");
        let active_system = test_system(92, 97, true, true, "heater");

        estimator.update(&off_system);
        estimator.update(&active_system);
        assert!(estimator.spa_session.is_none());

        estimator.spa_active_since_unix_ms = Some(unix_time_ms() - 121_000);
        estimator.update(&active_system);
        assert!(estimator.spa_session.is_some());
    }

    #[test]
    fn session_records_final_at_temp_sample_before_learning() {
        let path = test_store_path("session-final-at-temp");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let off_system = test_system(92, 97, false, false, "off");
        let heating_system = test_system(92, 97, true, true, "heater");
        let at_temp_system = test_system(97, 97, true, true, "heater");

        estimator.update(&off_system);
        estimator.update(&heating_system);
        estimator.spa_active_since_unix_ms = Some(unix_time_ms() - 121_000);
        estimator.update(&heating_system);
        if let Some(session) = estimator.spa_session.as_mut() {
            session.started_at_unix_ms -= 5 * 60_000;
        }
        estimator.update(&at_temp_system);

        let completed = estimator
            .store
            .recent_sessions
            .last()
            .expect("completed session");
        assert_eq!(completed.start_temp_f, 92.0);
        assert_eq!(completed.end_temp_f, 97.0);
    }

    #[test]
    fn configured_spa_eta_can_derive_volume_from_dimensions() {
        let path = test_store_path("configured-spa-dimensions");
        let mut estimator = HeatEstimator::load(test_config_with_spa_dimensions(), path);
        let mut system = test_system(99, 104, true, true, "heater");

        estimator.update(&system);
        estimator.spa_active_since_unix_ms = Some(unix_time_ms() - 121_000);
        estimator.apply_to_system(&mut system);

        let estimate = system
            .spa
            .and_then(|spa| spa.heat_estimate)
            .expect("spa estimate");
        assert!(estimate.available);
        assert_eq!(estimate.source, "configured");
        assert!(estimate.configured_rate_per_hour.unwrap_or_default() > 0.0);
    }

    #[test]
    fn shared_body_temperature_is_unreliable_while_off() {
        let path = test_store_path("shared-body-off");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, false, false, "off");

        estimator.update(&system);
        estimator.apply_to_system(&mut system);

        let spa = system.spa.expect("spa");
        assert!(!spa.temperature_reliable);
        assert_eq!(
            spa.temperature_reason.as_deref(),
            Some("inactive-shared-body")
        );
        let estimate = spa.heat_estimate.expect("spa estimate");
        assert!(!estimate.available);
        assert_eq!(estimate.reason, "not-heating");
    }

    #[test]
    fn shared_body_uses_last_reliable_temperature_snapshot() {
        let path = test_store_path("shared-body-last-reliable");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let trusted_system = test_system(98, 104, true, true, "heater");
        let mut stale_system = test_system(104, 104, false, false, "off");

        estimator.update(&trusted_system);
        estimator.spa_active_since_unix_ms = Some(unix_time_ms() - 121_000);
        estimator.update(&trusted_system);
        estimator.update(&stale_system);
        estimator.apply_to_system(&mut stale_system);

        let spa = stale_system.spa.expect("spa");
        assert!(!spa.temperature_reliable);
        assert_eq!(spa.last_reliable_temperature, Some(98));
        assert!(spa.last_reliable_temperature_at_unix_ms.is_some());
    }

    #[test]
    fn shared_body_temperature_warms_up_before_eta() {
        let path = test_store_path("sensor-warmup");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let off_system = test_system(104, 105, false, false, "off");
        let mut system = test_system(104, 105, true, true, "heater");

        estimator.update(&off_system);
        estimator.update(&system);
        estimator.apply_to_system(&mut system);

        let spa = system.spa.expect("spa");
        assert!(!spa.temperature_reliable);
        assert_eq!(spa.temperature_reason.as_deref(), Some("sensor-warmup"));
        let estimate = spa.heat_estimate.expect("spa estimate");
        assert!(!estimate.available);
        assert_eq!(estimate.reason, "sensor-warmup");
        assert_eq!(spa.heat_estimate_display.state, "pending");
        assert_eq!(
            spa.heat_estimate_display.available_in_seconds,
            Some(120)
        );
    }

    #[test]
    fn non_heating_body_does_not_enter_sensor_warmup_pending_state() {
        let path = test_store_path("non-heating-body-no-warmup-pending");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut inactive_system = test_system(93, 102, true, true, "off");
        let mut active_system = test_system(93, 102, true, true, "off");

        {
            let pool = inactive_system.pool.as_mut().expect("pool");
            pool.on = true;
            pool.active = false;
            pool.temperature = 86;
            pool.setpoint = 80;
            pool.heat_mode = "off".to_string();
            pool.heating = "off".to_string();
        }

        {
            let pool = active_system.pool.as_mut().expect("pool");
            pool.on = true;
            pool.active = true;
            pool.temperature = 86;
            pool.setpoint = 80;
            pool.heat_mode = "off".to_string();
            pool.heating = "off".to_string();
        }

        estimator.update(&inactive_system);
        estimator.update(&active_system);
        estimator.apply_to_system(&mut active_system);

        let pool = active_system.pool.expect("pool");
        let estimate = pool.heat_estimate.expect("pool estimate");
        assert!(!estimate.available);
        assert_eq!(estimate.reason, "heat-off");
        assert_eq!(pool.heat_estimate_display.state, "unavailable");
        assert_eq!(pool.heat_estimate_display.reason.as_deref(), Some("heat-off"));
    }

    #[test]
    fn first_active_sample_after_restart_is_treated_as_already_warm() {
        let path = test_store_path("active-after-restart");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(100, 104, true, true, "heater");

        estimator.update(&system);
        estimator.apply_to_system(&mut system);

        let spa = system.spa.expect("spa");
        assert!(spa.temperature_reliable);
        assert_eq!(spa.temperature_reason, None);
        let estimate = spa.heat_estimate.expect("spa estimate");
        assert!(estimate.available);
    }

    #[test]
    fn history_backfill_only_uses_samples_inside_run_window() {
        let path = test_store_path("history-backfill-run-window");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let controller_now = history_time(24, 6, 0, 0);
        let now_unix_ms = 1_700_000_000_000i64;
        let history = HistoryData {
            air_temps: vec![],
            pool_temps: vec![],
            pool_set_point_temps: vec![],
            spa_temps: vec![
                TimeTempPoint {
                    time: history_time(23, 21, 10, 0),
                    temp: 90,
                },
                TimeTempPoint {
                    time: history_time(23, 21, 20, 0),
                    temp: 98,
                },
                TimeTempPoint {
                    time: history_time(24, 5, 45, 0),
                    temp: 100,
                },
            ],
            spa_set_point_temps: vec![],
            pool_runs: vec![],
            spa_runs: vec![TimeRangePoint {
                on: history_time(23, 21, 13, 57),
                off: history_time(23, 21, 25, 32),
            }],
            solar_runs: vec![],
            heater_runs: vec![],
            light_runs: vec![],
        };

        estimator.seed_last_reliable_from_controller_history(
            &history,
            &controller_now,
            now_unix_ms,
            false,
        );

        let observation = estimator
            .last_reliable_temperature(HeatingBodyKind::Spa)
            .expect("spa observation");
        assert_eq!(observation.temperature, 98);
    }

    #[test]
    fn history_backfill_respects_shared_equipment_warmup() {
        let path = test_store_path("history-backfill-warmup");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let controller_now = history_time(24, 6, 0, 0);
        let now_unix_ms = 1_700_000_000_000i64;
        let history = HistoryData {
            air_temps: vec![],
            pool_temps: vec![],
            pool_set_point_temps: vec![],
            spa_temps: vec![
                TimeTempPoint {
                    time: history_time(23, 21, 14, 30),
                    temp: 96,
                },
                TimeTempPoint {
                    time: history_time(23, 21, 16, 30),
                    temp: 99,
                },
            ],
            spa_set_point_temps: vec![],
            pool_runs: vec![],
            spa_runs: vec![TimeRangePoint {
                on: history_time(23, 21, 13, 57),
                off: history_time(23, 21, 25, 32),
            }],
            solar_runs: vec![],
            heater_runs: vec![],
            light_runs: vec![],
        };

        estimator.seed_last_reliable_from_controller_history(
            &history,
            &controller_now,
            now_unix_ms,
            true,
        );

        let observation = estimator
            .last_reliable_temperature(HeatingBodyKind::Spa)
            .expect("spa observation");
        assert_eq!(observation.temperature, 99);
    }
}
