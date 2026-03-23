use crate::config::HeatingConfig;
use pentair_protocol::semantic::{BodyState, HeatEstimate, PoolSystem, SpaState};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

const WATER_LB_PER_GALLON: f64 = 8.34;
const MAX_RECENT_SESSIONS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum HeatingBodyKind {
    Pool,
    Spa,
}

#[derive(Debug, Clone)]
struct BodyTelemetry {
    on: bool,
    active: bool,
    temperature: i32,
    setpoint: i32,
    temperature_f: f64,
    setpoint_f: f64,
    heat_mode: String,
    heating: String,
    air_temp_f: Option<f64>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HeatEstimatorStore {
    pool_learned_rate_f_per_hour: Option<f64>,
    spa_learned_rate_f_per_hour: Option<f64>,
    recent_sessions: Vec<CompletedHeatingSession>,
}

#[derive(Debug)]
pub struct HeatEstimator {
    config: HeatingConfig,
    path: PathBuf,
    store: HeatEstimatorStore,
    pool_session: Option<ActiveHeatingSession>,
    spa_session: Option<ActiveHeatingSession>,
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
        }
    }

    pub fn update(&mut self, system: &PoolSystem) {
        if !self.config.enabled {
            return;
        }

        let use_celsius = uses_celsius(system.system.temp_unit);
        let air_temp_f = Some(to_fahrenheit(
            system.system.air_temperature as f64,
            use_celsius,
        ));
        let now_unix_ms = unix_time_ms();

        let pool = system
            .pool
            .as_ref()
            .map(|body| BodyTelemetry::from_pool(body, air_temp_f, use_celsius));
        let spa = system
            .spa
            .as_ref()
            .map(|body| BodyTelemetry::from_spa(body, air_temp_f, use_celsius));

        self.update_session_for_body(HeatingBodyKind::Pool, pool, now_unix_ms);
        self.update_session_for_body(HeatingBodyKind::Spa, spa, now_unix_ms);
    }

    pub fn apply_to_system(&self, system: &mut PoolSystem) {
        if !self.config.enabled {
            return;
        }

        let use_celsius = uses_celsius(system.system.temp_unit);
        let air_temp_f = Some(to_fahrenheit(
            system.system.air_temperature as f64,
            use_celsius,
        ));

        if let Some(pool) = system.pool.as_mut() {
            pool.heat_estimate = Some(self.estimate_for_body(
                HeatingBodyKind::Pool,
                BodyTelemetry::from_pool(pool, air_temp_f, use_celsius),
                use_celsius,
            ));
        }

        if let Some(spa) = system.spa.as_mut() {
            spa.heat_estimate = Some(self.estimate_for_body(
                HeatingBodyKind::Spa,
                BodyTelemetry::from_spa(spa, air_temp_f, use_celsius),
                use_celsius,
            ));
        }
    }

    fn update_session_for_body(
        &mut self,
        body: HeatingBodyKind,
        telemetry: Option<BodyTelemetry>,
        now_unix_ms: i64,
    ) {
        let sample_window_minutes = self.config.sample_window_minutes;
        let should_track = telemetry
            .as_ref()
            .map(Self::should_track_heating_session)
            .unwrap_or(false);

        let slot = self.session_slot_mut(body);

        match (slot.as_mut(), telemetry.as_ref(), should_track) {
            (Some(session), Some(telemetry), true) => {
                session.record(telemetry, now_unix_ms, sample_window_minutes);
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
        telemetry: BodyTelemetry,
        use_celsius: bool,
    ) -> HeatEstimate {
        let now_unix_ms = unix_time_ms();

        if !telemetry.on {
            return unavailable_estimate(&telemetry, now_unix_ms, "not-heating");
        }
        if telemetry.heat_mode == "off" {
            return unavailable_estimate(&telemetry, now_unix_ms, "heat-off");
        }
        if !telemetry.active {
            return unavailable_estimate(&telemetry, now_unix_ms, "waiting-for-flow");
        }
        if telemetry.setpoint <= telemetry.temperature {
            return unavailable_estimate(&telemetry, now_unix_ms, "at-temp");
        }

        let configured_rate_f_per_hour = self.configured_rate_f_per_hour(body);
        let learned_rate_f_per_hour = self.learned_rate_f_per_hour(body);
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
            configured_rate_per_hour: configured_rate_f_per_hour
                .map(|rate| from_fahrenheit_rate(rate, use_celsius)),
            updated_at_unix_ms: now_unix_ms,
        }
    }

    fn configured_rate_f_per_hour(&self, body: HeatingBodyKind) -> Option<f64> {
        let volume_gallons = match body {
            HeatingBodyKind::Pool => self.config.pool.volume_gallons,
            HeatingBodyKind::Spa => self.config.spa.volume_gallons,
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

    fn learned_rate_f_per_hour(&self, body: HeatingBodyKind) -> Option<f64> {
        match body {
            HeatingBodyKind::Pool => self.store.pool_learned_rate_f_per_hour,
            HeatingBodyKind::Spa => self.store.spa_learned_rate_f_per_hour,
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
    fn from_pool(body: &BodyState, air_temp_f: Option<f64>, use_celsius: bool) -> Self {
        Self {
            on: body.on,
            active: body.active,
            temperature: body.temperature,
            setpoint: body.setpoint,
            temperature_f: to_fahrenheit(body.temperature as f64, use_celsius),
            setpoint_f: to_fahrenheit(body.setpoint as f64, use_celsius),
            heat_mode: body.heat_mode.to_lowercase(),
            heating: body.heating.to_lowercase(),
            air_temp_f,
        }
    }

    fn from_spa(body: &SpaState, air_temp_f: Option<f64>, use_celsius: bool) -> Self {
        Self {
            on: body.on,
            active: body.active,
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
        configured_rate_per_hour: None,
        updated_at_unix_ms,
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
                setpoint: 82,
                heat_mode: "heat-pump".to_string(),
                heating: "off".to_string(),
                heat_estimate: None,
            }),
            spa: Some(SpaState {
                on: spa_on,
                active: spa_active,
                temperature: spa_temp,
                setpoint: spa_setpoint,
                heat_mode: "heat-pump".to_string(),
                heating: spa_heating.to_string(),
                heat_estimate: None,
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

    #[test]
    fn configured_spa_eta_is_available_without_history() {
        let path = test_store_path("configured-spa");
        let estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, true, true, "heater");

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
        let estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, false, false, "off");

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
}
