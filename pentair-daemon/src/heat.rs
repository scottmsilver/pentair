use crate::config::{HeatingConfig, SpaHeatNotificationsConfig};
use crate::spa_notifications::{
    evaluate_spa_heat_notifications, SpaHeatNotificationEvent, SpaHeatNotificationInput,
    SpaHeatNotificationState,
};
use crate::thermal::{self, CoolingParams, PredictionBasis, ReliableSample, WeatherSegment};
use crate::weather::WeatherCache;
use chrono::{NaiveDate, NaiveDateTime};
use pentair_protocol::responses::{HistoryData, TimeRangePoint};
use pentair_protocol::semantic::{
    BodyState, HeatEstimate, HeatEstimateDisplay, PoolSystem, SpaHeatProgress, SpaState,
    TemperatureDisplay,
};
use pentair_protocol::types::SLDateTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info, warn};

const WATER_LB_PER_GALLON: f64 = 8.34;
const MAX_RECENT_SESSIONS: usize = 24;
const AMBIENT_RATE_WEIGHT_SPAN_F: f64 = 10.0;
const LAST_RELIABLE_PERSIST_INTERVAL_MS: i64 = 60_000;

/// Smallest sensing gap worth using for a closed-loop calibration step (ms).
/// Below this the prediction and the fresh reading are effectively identical.
const MIN_CALIBRATION_GAP_MS: i64 = 30 * 60_000;
/// EWMA weight for the rolling prediction MAE and the k0 calibration nudge.
const PREDICTION_MAE_ALPHA: f64 = 0.3;
const CALIBRATION_LEARNING_RATE: f64 = 0.3;
/// Sanity bounds on the relaxation time constant (hours), mirroring thermal.rs.
const TAU_MIN_HOURS: f64 = 2.0;
const TAU_MAX_HOURS: f64 = 200.0;
/// Multiplier turning the persisted rolling MAE (°F) into an uncertainty floor:
/// the surfaced ± band is `max(gap-heuristic, MAE_UNCERTAINTY_K * MAE)`.
const MAE_UNCERTAINTY_K: f64 = 1.5;
/// Above this rolling MAE (°F) the prediction confidence is dropped one tier.
const MAE_CONFIDENCE_DOWNGRADE_F: f64 = 2.0;
/// A projection's weather is "fresh" only if the newest observed sample is no
/// older than this. Beyond it we fall back to the controller-air cooling-only
/// tier (or to none/measured when no air temperature is available either).
const WEATHER_FRESHNESS_MS: i64 = 2 * 3_600_000;
/// Below this |∂pred/∂k0| the closed-loop secant step is ill-conditioned (the
/// reading barely constrains k0); skip the step rather than divide by ~0.
const K0_SENSITIVITY_FLOOR: f64 = 1.0e-3;
/// Cap a single secant step to this fraction of the current k0, so one noisy
/// reading can never swing the cooling constant hard.
const K0_MAX_STEP_FRACTION: f64 = 0.25;
/// Floor on the per-step k0 cap, so the step never collapses to zero when k0 is
/// already tiny.
const MIN_K0_STEP: f64 = 1.0e-6;
/// Hard cap on stored calibrator cooling intervals per body (spec-driven; see
/// `crate::calibrator`), so the store stays bounded on long-running installs.
const MAX_INTERVALS_PER_BODY: usize = 200;
/// Residual beyond ANOMALY_SIGMA * max(rolling MAE, floor) tags the interval
/// ExcludedAnomalous (spec §3 uncovered/in-use heuristic).
const ANOMALY_SIGMA: f64 = 3.0;
const ANOMALY_MAE_FLOOR_F: f64 = 0.75;
/// Hard cap on stored body-activity windows (pump/body on->off transitions).
const MAX_ACTIVITY_WINDOWS: usize = 200;
/// Hard cap on stored rolling-MAE history points.
const MAX_MAE_POINTS: usize = 100;

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

/// A recorded pump/body activity span (on -> off), used by the calibrator to
/// reject cooling intervals that weren't idle across their whole span.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ActivityWindow {
    body: HeatingBodyKind,
    start_unix_ms: i64,
    end_unix_ms: i64,
}

/// One rolling-MAE sample, kept for trend/debugging visibility alongside the
/// scalar `*_prediction_mae_f` EWMA.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct MaePoint {
    body: HeatingBodyKind,
    at_unix_ms: i64,
    mae_f: f64,
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
    /// Fitted covered-idle cooling constants (history seed, refined closed-loop).
    #[serde(default)]
    pool_cooling_params: Option<CoolingParams>,
    #[serde(default)]
    spa_cooling_params: Option<CoolingParams>,
    /// Rolling mean-absolute prediction error (°F) from closed-loop calibration.
    #[serde(default)]
    pool_prediction_mae_f: Option<f64>,
    #[serde(default)]
    spa_prediction_mae_f: Option<f64>,
    /// Captured cooling intervals (spec: continuous thermal calibrator) feeding
    /// the closed-loop cooling-params fit. Bounded to `MAX_INTERVALS_PER_BODY`.
    #[serde(default)]
    pool_cooling_intervals: Vec<crate::calibrator::CoolingInterval>,
    #[serde(default)]
    spa_cooling_intervals: Vec<crate::calibrator::CoolingInterval>,
    /// Pump/body on->off activity spans, used to reject cooling intervals that
    /// weren't idle across their whole span. Bounded to `MAX_ACTIVITY_WINDOWS`.
    #[serde(default)]
    activity_windows: Vec<ActivityWindow>,
    /// Rolling-MAE history points, bounded to `MAX_MAE_POINTS`.
    #[serde(default)]
    mae_history: Vec<MaePoint>,
    #[serde(default)]
    pool_outlet_offset_f: Option<f64>,
    #[serde(default)]
    spa_outlet_offset_f: Option<f64>,
    #[serde(default)]
    pool_bulk_rate_f_per_hour: Option<f64>,
    #[serde(default)]
    spa_bulk_rate_f_per_hour: Option<f64>,
    #[serde(default)]
    pool_last_refit_unix_ms: Option<i64>,
    #[serde(default)]
    spa_last_refit_unix_ms: Option<i64>,
    #[serde(default)]
    pool_offset_done_session_end_ms: Option<i64>,
    #[serde(default)]
    spa_offset_done_session_end_ms: Option<i64>,
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
    spa_notification_config: SpaHeatNotificationsConfig,
    path: PathBuf,
    store: HeatEstimatorStore,
    pool_session: Option<ActiveHeatingSession>,
    spa_session: Option<ActiveHeatingSession>,
    spa_notification_state: SpaHeatNotificationState,
    pool_active_since_unix_ms: Option<i64>,
    spa_active_since_unix_ms: Option<i64>,
    pool_last_active_observed: Option<bool>,
    spa_last_active_observed: Option<bool>,
    /// Site location for the solar-gain term. `None` until configured, in which
    /// case the solar term is disabled (segments relax toward plain air temp).
    solar_location: Option<(f64, f64)>,
}

impl HeatEstimator {
    #[cfg(test)]
    pub fn load(config: HeatingConfig, path: PathBuf) -> Self {
        Self::load_with_notifications(config, SpaHeatNotificationsConfig::default(), path)
    }

    /// Test-only helper: drives `update_active_since_for_body` with a minimal
    /// synthetic telemetry sample at a caller-supplied clock, so activity-window
    /// recording can be exercised without a full `PoolSystem`.
    #[cfg(test)]
    fn test_note_activity(&mut self, body: HeatingBodyKind, active: bool, now_unix_ms: i64) {
        let telemetry = BodyTelemetry {
            on: active,
            active,
            pool_spa_shared_pump: true,
            temperature: 80,
            setpoint: 80,
            temperature_f: 80.0,
            setpoint_f: 80.0,
            heat_mode: "off".to_string(),
            heating: "off".to_string(),
            air_temp_f: None,
        };
        self.update_active_since_for_body(body, Some(&telemetry), now_unix_ms);
    }

    pub fn load_with_notifications(
        config: HeatingConfig,
        spa_notification_config: SpaHeatNotificationsConfig,
        path: PathBuf,
    ) -> Self {
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
            spa_notification_config,
            path,
            store,
            pool_session: None,
            spa_session: None,
            spa_notification_state: SpaHeatNotificationState::default(),
            pool_active_since_unix_ms: None,
            spa_active_since_unix_ms: None,
            pool_last_active_observed: None,
            spa_last_active_observed: None,
            solar_location: None,
        }
    }

    /// Configure the site location used by the solar-gain term. Passing `None`
    /// (or never calling this) leaves the solar term disabled.
    pub fn set_solar_location(&mut self, location: Option<(f64, f64)>) {
        self.solar_location = location;
    }

    /// The solar-site parameters for this estimator: the configured location
    /// plus the cover transmission seed, or [`SolarSite::disabled`] when no
    /// location is configured.
    fn solar_site(&self) -> thermal::SolarSite {
        match self.solar_location {
            Some((lat, lon)) => thermal::SolarSite {
                latitude_deg: lat,
                longitude_deg: lon,
                cover_solar_transmission: self
                    .config
                    .cooling
                    .cover_solar_transmission
                    .clamp(0.0, 1.0),
            },
            None => thermal::SolarSite::disabled(),
        }
    }

    /// Effective (fitted-or-seeded) covered-idle cooling params for the **pool**.
    /// Read-only accessor used by the advisory comfort scheduler — exposes the
    /// same `(k, g, evap)` the temperature predictor uses, so the plan projects
    /// passive physics consistently. Does not actuate or mutate anything.
    pub fn pool_cooling_params(&self) -> CoolingParams {
        self.cooling_params(HeatingBodyKind::Pool)
    }

    /// The solar-site parameters (location + cover transmission) used to build
    /// weather segments for the pool. Read-only accessor for the comfort
    /// scheduler. Returns [`thermal::SolarSite::disabled`] when no location set.
    pub fn pool_solar_site(&self) -> thermal::SolarSite {
        self.solar_site()
    }

    /// The last reliable pool water temperature (°F), if one is known. Read-only
    /// accessor used as the comfort scheduler's starting water temperature.
    pub fn pool_last_reliable_temp_f(&self) -> Option<f64> {
        self.last_reliable_temperature(HeatingBodyKind::Pool)
            .map(|obs| obs.temperature as f64)
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

    /// Weak prior: seed the covered-idle cooling constant from the controller's
    /// 48h history (heater-OFF cooling intervals only), using no extra controller
    /// round-trips. Only fills an empty slot — closed-loop calibration is primary
    /// and must not be overwritten by the history fit. Cooling-only Newtonian fit
    /// is scale-free in `k`, so controller display units need no conversion.
    pub fn seed_cooling_params_from_history(
        &mut self,
        history: &HistoryData,
        controller_now: &SLDateTime,
        now_unix_ms: i64,
        pool_spa_shared_pump: bool,
    ) {
        let Ok(controller_now_naive) = sl_to_naive(controller_now) else {
            warn!("failed to parse controller time for cooling-param seed");
            return;
        };

        let mut changed = false;
        for body in [HeatingBodyKind::Pool, HeatingBodyKind::Spa] {
            if self.cooling_params_slot_mut(body).is_some() {
                continue; // never override a learned/closed-loop fit
            }
            if let Some(params) = self.fit_history_cooling(
                body,
                history,
                &controller_now_naive,
                now_unix_ms,
                pool_spa_shared_pump,
            ) {
                *self.cooling_params_slot_mut(body) = Some(params);
                changed = true;
            }
        }

        if changed {
            self.persist();
        }
    }

    fn fit_history_cooling(
        &self,
        body: HeatingBodyKind,
        history: &HistoryData,
        controller_now: &NaiveDateTime,
        now_unix_ms: i64,
        pool_spa_shared_pump: bool,
    ) -> Option<CoolingParams> {
        let observations = reliable_history_observations_for_body(
            body,
            history,
            controller_now,
            now_unix_ms,
            pool_spa_shared_pump,
            self.config.shared_equipment_temp_warmup_seconds,
        );

        // Keep only heater-OFF samples: cooling intervals. Heater-ON subsegments
        // are governed by the rate model, never the cooling model.
        let cooling_samples: Vec<ReliableSample> = observations
            .into_iter()
            .filter(|observation| {
                !history.heater_runs.iter().any(|run| {
                    history_run_contains_sample(
                        run,
                        controller_now,
                        now_unix_ms,
                        observation.observed_at_unix_ms,
                        0,
                    )
                })
            })
            .map(|observation| ReliableSample {
                temperature_f: observation.temperature as f64,
                observed_at_unix_ms: observation.observed_at_unix_ms,
            })
            .collect();

        if cooling_samples.len() < 2 {
            return None;
        }

        // Air-temperature segments from the same history (cooling-only: no wind /
        // humidity). Real timestamps + the configured site let the fit see any
        // daytime solar warming intervals; with no location the site is disabled
        // and the fit reduces to a plain Newtonian relaxation.
        let site = self.solar_site();
        let mut air_segments: Vec<thermal::WeatherSegment> = history
            .air_temps
            .iter()
            .filter_map(|point| {
                let at =
                    controller_history_time_to_unix_ms(&point.time, controller_now, now_unix_ms)?;
                Some((at, point.temp as f64))
            })
            .collect::<Vec<_>>()
            .windows(2)
            .filter_map(|window| {
                let (start, air) = window[0];
                let (end, _) = window[1];
                (end > start).then_some(thermal::WeatherSegment {
                    start_unix_ms: start,
                    end_unix_ms: end,
                    air_temp_f: air,
                    wind_mph: None,
                    humidity_fraction: None,
                    cloud_fraction: None,
                    latitude_deg: site.latitude_deg,
                    longitude_deg: site.longitude_deg,
                    cover_solar_transmission: site.cover_solar_transmission,
                })
            })
            .collect();
        air_segments.sort_by_key(|segment| segment.start_unix_ms);
        if air_segments.is_empty() {
            return None;
        }

        let seed = self.cooling_params(body);
        let fit = thermal::fit_cooling_params(&cooling_samples, &air_segments, &seed);
        // A degenerate fit returns the seed with NaN residual; don't persist that.
        fit.residual_mae_f.is_finite().then_some(fit.params)
    }

    pub fn apply_to_system(&self, system: &mut PoolSystem, weather: &WeatherCache) {
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
            self.apply_prediction(
                HeatingBodyKind::Pool,
                &trust,
                &telemetry,
                weather,
                use_celsius,
                now_unix_ms,
                &mut PredictionFields::pool(pool),
            );
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
            self.apply_prediction(
                HeatingBodyKind::Spa,
                &trust,
                &telemetry,
                weather,
                use_celsius,
                now_unix_ms,
                &mut PredictionFields::spa(spa),
            );
            spa.temperature_display = temperature_display(spa);
            spa.heat_estimate_display = self.heat_estimate_display(
                HeatingBodyKind::Spa,
                &telemetry,
                now_unix_ms,
                spa.heat_estimate.as_ref(),
            );
            spa.spa_heat_progress = self.build_spa_heat_progress(spa);
        }
    }

    /// Project the last reliable temperature forward and write the predicted
    /// fields onto a body when the live reading is unreliable. Covered-when-idle:
    /// an unreliable *idle* shared body is covered, so the gap is pure cooling —
    /// heater-on subsegments never arise on this path.
    ///
    /// Degradation order (most → least trustworthy):
    /// full weather → controller-air cooling-only → none/measured.
    #[allow(clippy::too_many_arguments)]
    fn apply_prediction(
        &self,
        body: HeatingBodyKind,
        trust: &TemperatureTrust,
        telemetry: &BodyTelemetry,
        weather: &WeatherCache,
        use_celsius: bool,
        now_unix_ms: i64,
        fields: &mut PredictionFields<'_>,
    ) {
        if trust.reliable {
            return;
        }
        // Idle guard: only project cooling for a body that is genuinely idle
        // (covered). A body that is on + circulating but momentarily unreliable
        // (e.g. inside its shared-pump sensor warmup) is NOT cooling — projecting
        // a cooling number there would be wrong, so fall back to measured/none.
        let is_idle = !(telemetry.on && telemetry.active);
        if !is_idle {
            return;
        }
        let Some(anchor_obs) = self.last_reliable_temperature(body) else {
            return;
        };

        let anchor = ReliableSample {
            temperature_f: to_fahrenheit(anchor_obs.temperature as f64, use_celsius),
            observed_at_unix_ms: anchor_obs.observed_at_unix_ms,
        };
        let params = self.cooling_params(body);

        // Prefer fresh live weather; otherwise drop to a single controller-air
        // cooling-only segment (no wind/humidity → ProjectedCoolingOnly), which
        // is less trustworthy and so gets its confidence dropped a tier below.
        let weather_fresh = weather
            .latest_observation()
            .is_some_and(|sample| now_unix_ms - sample.observed_at_unix_ms <= WEATHER_FRESHNESS_MS);
        let site = self.solar_site();
        let (projected, controller_air_fallback) = if weather_fresh {
            let segments = weather.to_segments(site);
            (
                thermal::project_temperature(anchor, &segments, &params, now_unix_ms),
                false,
            )
        } else if let Some(air_temp_f) = telemetry.air_temp_f {
            let segment = WeatherSegment {
                start_unix_ms: anchor.observed_at_unix_ms,
                end_unix_ms: now_unix_ms,
                air_temp_f,
                wind_mph: None,
                humidity_fraction: None,
                cloud_fraction: None,
                latitude_deg: site.latitude_deg,
                longitude_deg: site.longitude_deg,
                cover_solar_transmission: site.cover_solar_transmission,
            };
            (
                thermal::project_temperature(anchor, &[segment], &params, now_unix_ms),
                true,
            )
        } else {
            // No fresh weather and no controller air temperature → cannot honestly
            // project; project_temperature with no segments yields basis `none`.
            (
                thermal::project_temperature(anchor, &[], &params, now_unix_ms),
                false,
            )
        };

        // Fold the persisted rolling MAE into the surfaced confidence/uncertainty,
        // and drop a tier when the projection rests on the controller air sensor.
        let mae = self.prediction_mae(body).filter(|m| m.is_finite() && *m >= 0.0);
        let mut confidence = projected.confidence;
        if controller_air_fallback {
            confidence = confidence.downgraded();
        }
        if mae.is_some_and(|m| m > MAE_CONFIDENCE_DOWNGRADE_F) {
            confidence = confidence.downgraded();
        }

        // Always surface the basis/confidence so the client knows whether a
        // number is honest; only populate the value for a real projection.
        *fields.prediction_basis = Some(projected.basis.as_str().to_string());
        *fields.prediction_confidence = Some(confidence.as_str().to_string());
        *fields.prediction_as_of_unix_ms = Some(projected.as_of_unix_ms);

        let projected_value = matches!(
            projected.basis,
            PredictionBasis::ProjectedWeather | PredictionBasis::ProjectedCoolingOnly
        ) && projected.predicted_f.is_finite();
        if projected_value {
            // Uncertainty floor: never claim more precision than the measured
            // rolling error supports.
            let uncertainty = match mae {
                Some(m) => projected.uncertainty_f.max(MAE_UNCERTAINTY_K * m),
                None => projected.uncertainty_f,
            };
            *fields.predicted_temperature_f_precise = Some(projected.predicted_f);
            *fields.predicted_temperature =
                Some(from_fahrenheit(projected.predicted_f, use_celsius).round() as i32);
            *fields.prediction_uncertainty_f = Some(uncertainty);
        }
    }

    fn build_spa_heat_progress(&self, spa: &SpaState) -> SpaHeatProgress {
        let heating_active = spa.on
            && spa.heat_mode != "off"
            && spa.heating != "off"
            && spa.heating != "unknown";

        if !heating_active {
            return SpaHeatProgress {
                current_temp_f: spa.temperature,
                target_temp_f: spa.setpoint,
                ..SpaHeatProgress::default()
            };
        }

        // Use the trusted session data when available for accurate start temp
        let session_start_temp = self
            .spa_session
            .as_ref()
            .map(|s| s.start_temp_f as i32);
        let session_id = self.spa_session.as_ref().map(|s| {
            let secs = s.started_at_unix_ms / 1000;
            chrono::DateTime::from_timestamp(secs, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| s.started_at_unix_ms.to_string())
        });

        let start = session_start_temp.unwrap_or(spa.temperature);
        let current = spa.temperature;
        let target = spa.setpoint;

        // Determine phase
        let phase = if current >= target {
            "reached"
        } else if spa
            .heat_estimate
            .as_ref()
            .map(|e| e.available && e.minutes_remaining.is_some())
            .unwrap_or(false)
        {
            "tracking"
        } else {
            "started"
        };

        // Compute progress
        let progress_pct = if phase == "reached" {
            100u8
        } else {
            let delta = target - start;
            if delta > 0 {
                (((current - start).max(0) as f64 / delta as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            } else {
                0u8
            }
        };

        let minutes_remaining = spa
            .heat_estimate
            .as_ref()
            .filter(|e| e.available)
            .and_then(|e| e.minutes_remaining);

        // Derive milestone from progress/phase
        let milestone = if phase == "reached" {
            Some("at_temp".to_string())
        } else if progress_pct >= 90 {
            Some("almost_ready".to_string())
        } else if progress_pct >= 50 {
            Some("halfway".to_string())
        } else if progress_pct < 5 && minutes_remaining.is_none() {
            Some("heating_started".to_string())
        } else {
            None
        };

        SpaHeatProgress {
            active: true,
            phase: phase.to_string(),
            start_temp_f: session_start_temp,
            current_temp_f: current,
            target_temp_f: target,
            progress_pct,
            minutes_remaining,
            session_id,
            milestone,
        }
    }

    /// Get the best available heating rate for the spa in F/hour.
    /// Used by the timed heating scheduler to estimate how long heating will take.
    /// Pass current air temperature to weight recent sessions by ambient similarity.
    pub fn spa_learned_rate_f_per_hour(&self, current_air_temp_f: Option<f64>) -> Option<f64> {
        self.learned_rate_f_per_hour(HeatingBodyKind::Spa, current_air_temp_f)
    }

    /// Get the configured (physics-based) heating rate for the spa in F/hour.
    pub fn spa_configured_rate_f_per_hour(&self) -> Option<f64> {
        self.configured_rate_f_per_hour(HeatingBodyKind::Spa)
    }

    /// Get the last reliable spa temperature observation.
    pub fn spa_last_reliable_temp(&self) -> Option<i32> {
        self.last_reliable_temperature(HeatingBodyKind::Spa)
            .map(|obs| obs.temperature)
    }

    pub fn spa_heat_notification_events_for_system(
        &mut self,
        system: &PoolSystem,
    ) -> Vec<SpaHeatNotificationEvent> {
        if !self.config.enabled {
            self.spa_notification_state = SpaHeatNotificationState::default();
            return Vec::new();
        }

        let Some(spa) = system.spa.as_ref() else {
            self.spa_notification_state = SpaHeatNotificationState::default();
            return Vec::new();
        };

        let input = SpaHeatNotificationInput {
            spa_on: spa.on,
            heat_mode_off: spa.heat_mode == "off",
            heating_active: spa.on && spa.heating != "off" && spa.heating != "unknown",
            current_temp: spa.temperature,
            target_temp: spa.setpoint,
            minutes_remaining: spa
                .heat_estimate
                .as_ref()
                .and_then(|estimate| estimate.available.then_some(estimate.minutes_remaining))
                .flatten(),
            trusted_session_start_temp_f: self.spa_session.as_ref().map(|session| session.start_temp_f),
            trusted_session_current_temp_f: self.spa_session.as_ref().map(|session| session.latest_temp_f),
            trusted_session_target_temp_f: self.spa_session.as_ref().map(|session| session.target_temp_f),
            trusted_session_id: self.spa_session.as_ref().map(|session| session.started_at_unix_ms),
        };

        evaluate_spa_heat_notifications(
            &self.spa_notification_config,
            &input,
            &mut self.spa_notification_state,
        )
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

        let mut activity_window_to_push: Option<ActivityWindow> = None;

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
                if *last_active_observed == Some(true) {
                    // Body just turned off: record the activity window so the
                    // calibrator can reject cooling intervals it overlaps
                    // (spec §3: idle across the WHOLE gap, not just endpoints).
                    let start = slot.unwrap_or(now_unix_ms);
                    activity_window_to_push = Some(ActivityWindow {
                        body,
                        start_unix_ms: start,
                        end_unix_ms: now_unix_ms,
                    });
                }
                *slot = None;
                *last_active_observed = Some(false);
            }
            None => {
                *slot = None;
                *last_active_observed = None;
            }
        }

        if let Some(window) = activity_window_to_push {
            self.store.activity_windows.push(window);
            let len = self.store.activity_windows.len();
            if len > MAX_ACTIVITY_WINDOWS {
                self.store.activity_windows.drain(..len - MAX_ACTIVITY_WINDOWS);
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

    /// Effective covered-idle cooling parameters for a body: the persisted
    /// fit when present, otherwise the physics seed primed by `[heating.cooling]`
    /// config seeds. `max_projection_hours` always tracks current config.
    fn cooling_params(&self, body: HeatingBodyKind) -> CoolingParams {
        let stored = match body {
            HeatingBodyKind::Pool => self.store.pool_cooling_params,
            HeatingBodyKind::Spa => self.store.spa_cooling_params,
        };
        let cooling = &self.config.cooling;
        let mut params = stored.unwrap_or_else(CoolingParams::seed);
        if stored.is_none() {
            if let Some(tau) = cooling.tau_covered_hours.filter(|tau| *tau > 0.0) {
                params.k0_per_hour = 1.0 / tau;
            }
            if let Some(evap_a) = cooling.evap_a {
                params.evap_a = evap_a;
            }
            if let Some(evap_b) = cooling.evap_b {
                params.evap_b = evap_b;
            }
            if let Some(solar_gain_f) = cooling.solar_gain_f.filter(|g| *g >= 0.0) {
                params.solar_gain_f = solar_gain_f;
            }
        }
        params.max_projection_hours = cooling.max_projection_hours.max(0.0);
        params
    }

    fn cooling_params_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Option<CoolingParams> {
        match body {
            HeatingBodyKind::Pool => &mut self.store.pool_cooling_params,
            HeatingBodyKind::Spa => &mut self.store.spa_cooling_params,
        }
    }

    fn prediction_mae_slot_mut(&mut self, body: HeatingBodyKind) -> &mut Option<f64> {
        match body {
            HeatingBodyKind::Pool => &mut self.store.pool_prediction_mae_f,
            HeatingBodyKind::Spa => &mut self.store.spa_prediction_mae_f,
        }
    }

    /// Persisted rolling prediction MAE (°F) for a body, if any.
    fn prediction_mae(&self, body: HeatingBodyKind) -> Option<f64> {
        match body {
            HeatingBodyKind::Pool => self.store.pool_prediction_mae_f,
            HeatingBodyKind::Spa => self.store.spa_prediction_mae_f,
        }
    }

    fn intervals(&self, body: HeatingBodyKind) -> &[crate::calibrator::CoolingInterval] {
        match body {
            HeatingBodyKind::Pool => &self.store.pool_cooling_intervals,
            HeatingBodyKind::Spa => &self.store.spa_cooling_intervals,
        }
    }

    fn intervals_slot_mut(
        &mut self,
        body: HeatingBodyKind,
    ) -> &mut Vec<crate::calibrator::CoolingInterval> {
        match body {
            HeatingBodyKind::Pool => &mut self.store.pool_cooling_intervals,
            HeatingBodyKind::Spa => &mut self.store.spa_cooling_intervals,
        }
    }

    /// The slow re-fit loop (spec §4.2, §6, §7): gated, holdout-validated,
    /// damped, and SILENT (store-only; tracing debug; no event-log anywhere).
    fn maybe_refit(&mut self, body: HeatingBodyKind, now_unix_ms: i64) {
        let cal = self.config.calibration.clone();
        if !cal.enabled {
            return;
        }
        let last_refit = match body {
            HeatingBodyKind::Pool => self.store.pool_last_refit_unix_ms,
            HeatingBodyKind::Spa => self.store.spa_last_refit_unix_ms,
        };
        let min_gap_ms = (cal.refit_min_hours * 3_600_000.0) as i64;
        if let Some(last) = last_refit {
            if now_unix_ms - last < min_gap_ms {
                return;
            }
        }
        let window_start = now_unix_ms - (cal.window_days * 24.0 * 3_600_000.0) as i64;
        let since_fit = last_refit.unwrap_or(i64::MIN);
        let intervals = self.intervals(body);
        let fresh_clean = intervals
            .iter()
            .filter(|i| {
                i.t1_unix_ms > since_fit
                    && i.regime == crate::calibrator::IntervalRegime::IdleCovered
            })
            .count();
        let mae_drifted = self.prediction_mae(body).is_some_and(|m| m > cal.mae_drift_f);
        if fresh_clean < cal.min_new_intervals && !mae_drifted {
            return;
        }

        // Escape hatch (spec §9): a persistently-high exclusion rate means the
        // model, not the water, is wrong — fit over EVERYTHING in the window.
        let exclusion_window_start =
            now_unix_ms - (cal.exclusion_window_days * 24.0 * 3_600_000.0) as i64;
        let escape = crate::calibrator::exclusion_rate(intervals, exclusion_window_start)
            > cal.exclusion_rate_threshold;
        let window: Vec<crate::calibrator::CoolingInterval> = intervals
            .iter()
            .filter(|i| {
                i.t1_unix_ms >= window_start
                    && (escape || i.regime == crate::calibrator::IntervalRegime::IdleCovered)
            })
            .cloned()
            .collect();
        if window.len() < 3 {
            return; // holdout_split needs both halves populated
        }

        let site = self.solar_site();
        let current = self.cooling_params(body);
        let (fit_set, holdout) = crate::calibrator::holdout_split(&window);
        let candidate = crate::calibrator::fit_intervals(&fit_set, &current, site);
        let accepted = crate::calibrator::evaluate_candidate(
            &current, &candidate, &holdout, site, cal.damping_alpha, cal.accept_tolerance_f,
        );
        match accepted {
            Some(blend) => {
                debug!(?body, tau_hours = 1.0 / blend.k0_per_hour, escape,
                    "calibrator: refit accepted (silent auto-apply)");
                *self.cooling_params_slot_mut(body) = Some(blend);
            }
            None => {
                debug!(?body, escape, "calibrator: refit rejected, keeping current");
            }
        }

        // Bookkeeping either way: rate-limit stamp + MAE trend point.
        match body {
            HeatingBodyKind::Pool => self.store.pool_last_refit_unix_ms = Some(now_unix_ms),
            HeatingBodyKind::Spa => self.store.spa_last_refit_unix_ms = Some(now_unix_ms),
        }
        if let Some(mae) = self.prediction_mae(body) {
            self.store.mae_history.push(MaePoint { body, at_unix_ms: now_unix_ms, mae_f: mae });
            let len = self.store.mae_history.len();
            if len > MAX_MAE_POINTS {
                self.store.mae_history.drain(..len - MAX_MAE_POINTS);
            }
        }
        self.persist();
    }

    /// True when any recorded pump/body activity window for `body` overlaps the
    /// open gap. Complements `heating_overlapped_gap` (heater sessions) so a
    /// cooling interval is idle across its WHOLE span (spec §3).
    fn activity_overlapped_gap(
        &self,
        body: HeatingBodyKind,
        gap_start_unix_ms: i64,
        gap_end_unix_ms: i64,
    ) -> bool {
        self.store.activity_windows.iter().any(|w| {
            w.body == body
                && w.start_unix_ms < gap_end_unix_ms
                && w.end_unix_ms > gap_start_unix_ms
        })
    }

    /// True when a heating session for `body` overlapped the sensing gap
    /// `(gap_start, gap_end]`. The closed-loop calibration assumes the gap was
    /// pure cooling (covered-when-idle); if the heater actually fired mid-gap the
    /// cooling-only projection would double-count losses, so calibration is
    /// skipped. A continuously-heating body cannot produce a gap (it stays
    /// reliable), so only a *completed* session inside the window matters here —
    /// this is a defensive guard against a future change that breaks that
    /// invariant.
    fn heating_overlapped_gap(
        &self,
        body: HeatingBodyKind,
        gap_start_unix_ms: i64,
        gap_end_unix_ms: i64,
    ) -> bool {
        self.store.recent_sessions.iter().any(|session| {
            session.body == body
                && session.started_at_unix_ms < gap_end_unix_ms
                && session.ended_at_unix_ms > gap_start_unix_ms
        })
    }

    /// Capture one classified cooling interval (spec §3, §11). The caller has
    /// already established the gap is heater-free and the endpoint reading is
    /// trusted; this adds the whole-gap pump/body-activity check, classifies
    /// the residual, and stores a bounded, self-contained interval.
    fn capture_cooling_interval(
        &mut self,
        body: HeatingBodyKind,
        anchor: thermal::ReliableSample,
        actual_f: f64,
        error_f: f64,
        segments: &[thermal::WeatherSegment],
        now_unix_ms: i64,
    ) {
        if !self.config.calibration.enabled {
            return;
        }
        if self.activity_overlapped_gap(body, anchor.observed_at_unix_ms, now_unix_ms) {
            return; // the gap wasn't idle end-to-end — not a cooling interval
        }
        let weather =
            crate::calibrator::bucket_weather(segments, anchor.observed_at_unix_ms, now_unix_ms);
        if weather.is_empty() {
            return; // no weather spanned the gap — interval is unusable
        }
        let scale = self.prediction_mae(body).unwrap_or(0.0).max(ANOMALY_MAE_FLOOR_F);
        let regime = if error_f.abs() > ANOMALY_SIGMA * scale {
            crate::calibrator::IntervalRegime::ExcludedAnomalous
        } else {
            crate::calibrator::IntervalRegime::IdleCovered
        };
        let interval = crate::calibrator::CoolingInterval {
            t0_unix_ms: anchor.observed_at_unix_ms,
            t1_unix_ms: now_unix_ms,
            temp0_f: anchor.temperature_f,
            temp1_f: actual_f,
            regime,
            weather,
        };
        let buffer = self.intervals_slot_mut(body);
        buffer.push(interval);
        let len = buffer.len();
        if len > MAX_INTERVALS_PER_BODY {
            buffer.drain(..len - MAX_INTERVALS_PER_BODY);
        }
    }

    /// Closed-loop calibration (primary). When a body yields a fresh post-warmup
    /// reliable reading after a sensing gap, compare it to what the predictor
    /// would have said for this instant, fold the residual into a rolling MAE,
    /// and nudge `k0` with a damped secant step. Must run BEFORE `update`, while
    /// the stored last-reliable still points at the pre-gap anchor.
    pub fn calibrate_predictions(&mut self, system: &PoolSystem, weather: &WeatherCache) {
        let use_celsius = uses_celsius(system.system.temp_unit);
        let air_temp_f = Some(to_fahrenheit(
            system.system.air_temperature as f64,
            use_celsius,
        ));
        let shared_pump = system.system.pool_spa_shared_pump;
        let now_unix_ms = unix_time_ms();

        if let Some(pool) = system.pool.as_ref() {
            let telemetry = BodyTelemetry::from_pool(pool, air_temp_f, use_celsius, shared_pump);
            self.calibrate_body(
                HeatingBodyKind::Pool,
                &telemetry,
                weather,
                use_celsius,
                now_unix_ms,
            );
        }
        if let Some(spa) = system.spa.as_ref() {
            let telemetry = BodyTelemetry::from_spa(spa, air_temp_f, use_celsius, shared_pump);
            self.calibrate_body(
                HeatingBodyKind::Spa,
                &telemetry,
                weather,
                use_celsius,
                now_unix_ms,
            );
        }

        self.maybe_refit(HeatingBodyKind::Pool, now_unix_ms);
        self.maybe_refit(HeatingBodyKind::Spa, now_unix_ms);
    }

    fn calibrate_body(
        &mut self,
        body: HeatingBodyKind,
        telemetry: &BodyTelemetry,
        weather: &WeatherCache,
        use_celsius: bool,
        now_unix_ms: i64,
    ) {
        // Only learn from a fresh, trustworthy reading.
        if !self
            .temperature_trust_for_body(body, telemetry, now_unix_ms)
            .reliable
        {
            return;
        }
        let Some(anchor_obs) = self.last_reliable_temperature(body) else {
            return;
        };
        let gap_ms = now_unix_ms - anchor_obs.observed_at_unix_ms;
        if gap_ms < MIN_CALIBRATION_GAP_MS {
            return;
        }
        // Defensive: never calibrate across a gap a heater run overlapped, or the
        // cooling-only projection would be compared against a partly-heated body.
        if self.heating_overlapped_gap(body, anchor_obs.observed_at_unix_ms, now_unix_ms) {
            return;
        }

        let anchor = ReliableSample {
            temperature_f: to_fahrenheit(anchor_obs.temperature as f64, use_celsius),
            observed_at_unix_ms: anchor_obs.observed_at_unix_ms,
        };
        let params = self.cooling_params(body);
        let segments = weather.to_segments(self.solar_site());
        let projected = thermal::project_temperature(anchor, &segments, &params, now_unix_ms);
        // Cannot validate a non-projection (gap beyond cutoff / no weather).
        if !matches!(
            projected.basis,
            PredictionBasis::ProjectedWeather | PredictionBasis::ProjectedCoolingOnly
        ) {
            return;
        }

        let actual_f = telemetry.temperature_f;
        let error_f = actual_f - projected.predicted_f;

        // Rolling MAE (EWMA).
        let mae_slot = self.prediction_mae_slot_mut(body);
        *mae_slot = Some(match *mae_slot {
            Some(previous) => {
                previous * (1.0 - PREDICTION_MAE_ALPHA) + error_f.abs() * PREDICTION_MAE_ALPHA
            }
            None => error_f.abs(),
        });

        // Persist the classified interval for the slow re-fit loop (spec §4.2).
        self.capture_cooling_interval(body, anchor, actual_f, error_f, &segments, now_unix_ms);

        // Damped secant step on k0 to drive the residual toward zero.
        let bumped = CoolingParams {
            k0_per_hour: (params.k0_per_hour * 1.05)
                .clamp(1.0 / TAU_MAX_HOURS, 1.0 / TAU_MIN_HOURS),
            ..params
        };
        let pred_bumped =
            thermal::project_temperature(anchor, &segments, &bumped, now_unix_ms).predicted_f;
        let dk = bumped.k0_per_hour - params.k0_per_hour;
        let dpred = pred_bumped - projected.predicted_f;
        // Sensitivity ∂pred/∂k0. When it is tiny the reading barely constrains
        // k0, so the secant step is ill-conditioned (near divide-by-zero); reject
        // it rather than take a wild leap.
        let sensitivity = dpred / dk;
        if dk.abs() > f64::EPSILON && sensitivity.abs() > K0_SENSITIVITY_FLOOR {
            let raw_step = CALIBRATION_LEARNING_RATE * error_f / sensitivity;
            // Clamp the per-step delta to a fraction of the current k0 (and EWMA-
            // damped by CALIBRATION_LEARNING_RATE above) so one noisy reading
            // cannot swing the cooling constant hard.
            let max_step = (params.k0_per_hour * K0_MAX_STEP_FRACTION).max(MIN_K0_STEP);
            let step = raw_step.clamp(-max_step, max_step);
            let k0 = (params.k0_per_hour + step).clamp(1.0 / TAU_MAX_HOURS, 1.0 / TAU_MIN_HOURS);
            if k0.is_finite() {
                let mut tuned = params;
                tuned.k0_per_hour = k0;
                *self.cooling_params_slot_mut(body) = Some(tuned);
            }
        }

        self.persist();
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
        // Serialize while we hold `&self` (cheap, in-memory), then hand the bytes
        // off so the synchronous filesystem write does not block the async worker
        // that holds the shared-state lock. See `write_json_off_lock`.
        let json = match serde_json::to_string_pretty(&self.store) {
            Ok(json) => json,
            Err(error) => {
                warn!("failed to serialize heat estimator store: {}", error);
                return;
            }
        };
        write_json_off_lock(self.path.clone(), json, "heat estimator store");
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

/// All reliable (temperature, time) samples for a body from controller history:
/// temperature points that fall inside a circulation run (past the shared-pump
/// warmup). Sorted ascending by time. Shared by the last-reliable backfill and
/// the cooling-parameter history seed.
fn reliable_history_observations_for_body(
    body: HeatingBodyKind,
    history: &HistoryData,
    controller_now: &NaiveDateTime,
    now_unix_ms: i64,
    pool_spa_shared_pump: bool,
    shared_warmup_seconds: u64,
) -> Vec<ReliableTemperatureObservation> {
    let (temps, runs) = match body {
        HeatingBodyKind::Pool => (&history.pool_temps, &history.pool_runs),
        HeatingBodyKind::Spa => (&history.spa_temps, &history.spa_runs),
    };
    let warmup_ms = if pool_spa_shared_pump {
        shared_warmup_seconds as i64 * 1000
    } else {
        0
    };

    let mut observations: Vec<ReliableTemperatureObservation> = temps
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
        .collect();
    observations.sort_by_key(|observation| observation.observed_at_unix_ms);
    observations
}

/// Thin wrapper: the most recent reliable observation for a body.
fn latest_history_observation_for_body(
    body: HeatingBodyKind,
    history: &HistoryData,
    controller_now: &NaiveDateTime,
    now_unix_ms: i64,
    pool_spa_shared_pump: bool,
    shared_warmup_seconds: u64,
) -> Option<ReliableTemperatureObservation> {
    reliable_history_observations_for_body(
        body,
        history,
        controller_now,
        now_unix_ms,
        pool_spa_shared_pump,
        shared_warmup_seconds,
    )
    .into_iter()
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
        is_predicted: body.predicted_temperature().is_some(),
    }
}

trait TemperatureDisplaySource {
    fn temperature(&self) -> i32;
    fn temperature_reliable(&self) -> bool;
    fn temperature_reason(&self) -> Option<&str>;
    fn last_reliable_temperature(&self) -> Option<i32>;
    fn last_reliable_temperature_at_unix_ms(&self) -> Option<i64>;
    fn predicted_temperature(&self) -> Option<i32>;
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

    fn predicted_temperature(&self) -> Option<i32> {
        self.predicted_temperature
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

    fn predicted_temperature(&self) -> Option<i32> {
        self.predicted_temperature
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

fn from_fahrenheit(value_f: f64, use_celsius: bool) -> f64 {
    if use_celsius {
        (value_f - 32.0) * 5.0 / 9.0
    } else {
        value_f
    }
}

/// Mutable borrows of a body's additive prediction fields, so the projection
/// logic can be written once for both [`BodyState`] and [`SpaState`].
struct PredictionFields<'a> {
    predicted_temperature: &'a mut Option<i32>,
    predicted_temperature_f_precise: &'a mut Option<f64>,
    prediction_confidence: &'a mut Option<String>,
    prediction_uncertainty_f: &'a mut Option<f64>,
    prediction_as_of_unix_ms: &'a mut Option<i64>,
    prediction_basis: &'a mut Option<String>,
}

impl<'a> PredictionFields<'a> {
    fn pool(body: &'a mut BodyState) -> Self {
        Self {
            predicted_temperature: &mut body.predicted_temperature,
            predicted_temperature_f_precise: &mut body.predicted_temperature_f_precise,
            prediction_confidence: &mut body.prediction_confidence,
            prediction_uncertainty_f: &mut body.prediction_uncertainty_f,
            prediction_as_of_unix_ms: &mut body.prediction_as_of_unix_ms,
            prediction_basis: &mut body.prediction_basis,
        }
    }

    fn spa(body: &'a mut SpaState) -> Self {
        Self {
            predicted_temperature: &mut body.predicted_temperature,
            predicted_temperature_f_precise: &mut body.predicted_temperature_f_precise,
            prediction_confidence: &mut body.prediction_confidence,
            prediction_uncertainty_f: &mut body.prediction_uncertainty_f,
            prediction_as_of_unix_ms: &mut body.prediction_as_of_unix_ms,
            prediction_basis: &mut body.prediction_basis,
        }
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

/// Write `json` to `path` without blocking the async worker while the shared
/// state lock is held. When a Tokio runtime is available the synchronous
/// filesystem write is offloaded to a blocking thread (atomic temp-file +
/// rename, so a reader never sees a torn file); otherwise — e.g. unit tests with
/// no runtime — it is written inline so behaviour stays deterministic.
fn write_json_off_lock(path: PathBuf, json: String, label: &'static str) {
    let write = move || {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("tmp");
        if let Err(error) = std::fs::write(&tmp, json.as_bytes()) {
            warn!("failed to persist {}: {}", label, error);
            return;
        }
        if let Err(error) = std::fs::rename(&tmp, &path) {
            warn!("failed to persist {}: {}", label, error);
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(write);
        }
        Err(_) => write(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pentair_protocol::responses::{HistoryData, TimeRangePoint, TimeTempPoint};
    use pentair_protocol::semantic::{
        AuxState, BodyState, LightState, PumpInfo, SpaHeatProgress, SpaState, SystemInfo,
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
                predicted_temperature: None,
                predicted_temperature_f_precise: None,
                prediction_confidence: None,
                prediction_uncertainty_f: None,
                prediction_as_of_unix_ms: None,
                prediction_basis: None,
                temperature_display: TemperatureDisplay {
                    value: Some(80),
                    is_stale: false,
                    stale_reason: None,
                    last_reliable_at_unix_ms: None,
                    is_predicted: false,
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
                predicted_temperature: None,
                predicted_temperature_f_precise: None,
                prediction_confidence: None,
                prediction_uncertainty_f: None,
                prediction_as_of_unix_ms: None,
                prediction_basis: None,
                temperature_display: TemperatureDisplay {
                    value: Some(spa_temp),
                    is_stale: false,
                    stale_reason: None,
                    last_reliable_at_unix_ms: None,
                    is_predicted: false,
                },
                heat_estimate_display: HeatEstimateDisplay {
                    state: "unavailable".to_string(),
                    reason: None,
                    available_in_seconds: None,
                    minutes_remaining: None,
                    target_temperature: None,
                },
                spa_heat_progress: SpaHeatProgress {
                    current_temp_f: spa_temp,
                    target_temp_f: spa_setpoint,
                    ..SpaHeatProgress::default()
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
            goodnight_available: spa_on,
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

    /// Fixture: a fresh estimator backed by a unique temp-file store path (no
    /// existing tempdir-tuple fixture exists in this module, so this follows
    /// `test_store_path`'s pid-plus-name-based uniqueness instead).
    fn test_estimator() -> HeatEstimator {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = test_store_path(&format!("activity-windows-{id}"));
        HeatEstimator::load(test_config(), path)
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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut stale_system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut active_system, &WeatherCache::default());

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
        estimator.apply_to_system(&mut system, &WeatherCache::default());

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

    fn weather_sample(at_ms: i64, air_f: f64) -> crate::weather::WeatherSample {
        crate::weather::WeatherSample {
            observed_at_unix_ms: at_ms,
            air_temp_f: air_f,
            wind_mph: Some(5.0),
            humidity_fraction: Some(0.5),
            cloud_fraction: Some(0.3),
            is_forecast: false,
        }
    }

    /// Hourly observed weather covering `hours` back from `now`.
    fn gap_weather(now: i64, hours: i64, air_f: f64) -> WeatherCache {
        let mut weather = WeatherCache::default();
        for h in 0..hours {
            weather.record_observation(weather_sample(now - (hours - h) * 3_600_000, air_f), now);
        }
        weather
    }

    /// A single full-weather `WeatherSegment` spanning `[start_ms, end_ms)`,
    /// with a disabled solar site so `capture_cooling_interval` tests don't
    /// need real geometry.
    fn test_weather_segment(start_ms: i64, end_ms: i64, air_f: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start_ms,
            end_unix_ms: end_ms,
            air_temp_f: air_f,
            wind_mph: Some(0.0),
            humidity_fraction: Some(0.6),
            cloud_fraction: Some(0.0),
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }

    #[test]
    fn unreliable_shared_body_gets_weather_prediction() {
        let path = test_store_path("prediction-wiring");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        // 2h-old reliable spa reading as the anchor.
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 98,
            observed_at_unix_ms: now - 2 * 3_600_000,
        });
        // Spa off on a shared pump => unreliable => prediction engages.
        let mut system = test_system(98, 104, false, false, "off");
        let weather = gap_weather(now, 3, 60.0); // cool air

        estimator.apply_to_system(&mut system, &weather);

        let spa = system.spa.expect("spa");
        assert!(!spa.temperature_reliable);
        assert_eq!(spa.prediction_basis.as_deref(), Some("projected-weather"));
        let predicted = spa.predicted_temperature.expect("predicted temperature");
        assert!(
            predicted < 98,
            "water should cool toward cool air: {predicted}"
        );
        assert!(spa.predicted_temperature_f_precise.unwrap() < 98.0);
        assert!(spa.prediction_uncertainty_f.unwrap() > 0.0);
        assert_eq!(spa.prediction_as_of_unix_ms, Some(now));
        assert!(spa.temperature_display.is_predicted);
    }

    #[test]
    fn reliable_body_has_no_prediction_fields() {
        let path = test_store_path("prediction-skip-reliable");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let mut system = test_system(99, 104, true, true, "heater");
        system.system.pool_spa_shared_pump = false; // always reliable

        estimator.update(&system);
        let now = unix_time_ms();
        let weather = gap_weather(now, 3, 60.0);
        estimator.apply_to_system(&mut system, &weather);

        let spa = system.spa.expect("spa");
        assert!(spa.temperature_reliable);
        assert!(spa.predicted_temperature.is_none());
        assert!(spa.prediction_basis.is_none());
        assert!(!spa.temperature_display.is_predicted);
    }

    #[test]
    fn calibration_records_mae_after_a_gap() {
        let path = test_store_path("calibration-mae");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        // Anchor 3h ago; fresh reliable reading now.
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 100,
            observed_at_unix_ms: now - 3 * 3_600_000,
        });
        let system = test_system(90, 104, true, true, "heater");
        estimator.spa_active_since_unix_ms = Some(now - 121_000); // past warmup
        let weather = gap_weather(now, 4, 70.0);

        estimator.calibrate_predictions(&system, &weather);

        assert!(
            estimator.store.spa_prediction_mae_f.is_some(),
            "a fresh reading after a gap should update the rolling MAE"
        );
    }

    #[test]
    fn calibration_skipped_when_heater_ran_during_gap() {
        let path = test_store_path("calibration-heater-overlap");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        // Anchor 3h ago; a completed heating session sits inside the gap.
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 100,
            observed_at_unix_ms: now - 3 * 3_600_000,
        });
        estimator.store.recent_sessions.push(CompletedHeatingSession {
            body: HeatingBodyKind::Spa,
            started_at_unix_ms: now - 2 * 3_600_000,
            ended_at_unix_ms: now - 3_600_000,
            start_temp_f: 95.0,
            end_temp_f: 102.0,
            target_temp_f: 104.0,
            average_air_temp_f: Some(70.0),
            duration_minutes: 60.0,
            average_rate_f_per_hour: 7.0,
        });
        let system = test_system(90, 104, true, true, "heater");
        estimator.spa_active_since_unix_ms = Some(now - 121_000); // past warmup
        let weather = gap_weather(now, 4, 70.0);

        estimator.calibrate_predictions(&system, &weather);

        assert!(
            estimator.store.spa_prediction_mae_f.is_none(),
            "a heater run overlapping the gap must skip calibration (no MAE update)"
        );
    }

    #[test]
    fn capture_stores_idle_interval_and_tags_anomalies() {
        let mut estimator = test_estimator();
        let segs = vec![test_weather_segment(0, 20 * 3_600_000, 60.0)];
        let anchor = thermal::ReliableSample {
            temperature_f: 90.0,
            observed_at_unix_ms: 0,
        };
        // Normal residual -> IdleCovered.
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa,
            anchor,
            89.0,
            0.3,
            &segs,
            10 * 3_600_000,
        );
        assert_eq!(estimator.intervals(HeatingBodyKind::Spa).len(), 1);
        assert_eq!(
            estimator.intervals(HeatingBodyKind::Spa)[0].regime,
            crate::calibrator::IntervalRegime::IdleCovered
        );
        // Huge residual (way past 3 * max(mae, 0.75)) -> ExcludedAnomalous.
        let anchor2 = thermal::ReliableSample {
            temperature_f: 89.0,
            observed_at_unix_ms: 10 * 3_600_000,
        };
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa,
            anchor2,
            80.0,
            -9.0,
            &segs,
            20 * 3_600_000,
        );
        assert_eq!(
            estimator.intervals(HeatingBodyKind::Spa)[1].regime,
            crate::calibrator::IntervalRegime::ExcludedAnomalous
        );
    }

    #[test]
    fn capture_discards_gap_overlapping_body_activity() {
        let mut estimator = test_estimator();
        // Body ran 4h..5h inside the 0..10h gap.
        estimator.test_note_activity(HeatingBodyKind::Spa, true, 4 * 3_600_000);
        estimator.test_note_activity(HeatingBodyKind::Spa, false, 5 * 3_600_000);
        let segs = vec![test_weather_segment(0, 20 * 3_600_000, 60.0)];
        let anchor = thermal::ReliableSample {
            temperature_f: 90.0,
            observed_at_unix_ms: 0,
        };
        estimator.capture_cooling_interval(
            HeatingBodyKind::Spa,
            anchor,
            89.0,
            0.3,
            &segs,
            10 * 3_600_000,
        );
        assert!(estimator.intervals(HeatingBodyKind::Spa).is_empty());
    }

    #[test]
    fn interval_buffer_is_bounded() {
        let mut estimator = test_estimator();
        // A single segment spanning the whole test range. `bucket_weather`'s
        // bucket count is driven only by each call's own `[anchor, now]` gap
        // (8h here), not by the segment's own span, so an oversized segment
        // end (matching the spec's stress-test shape) stays cheap.
        let segs = vec![test_weather_segment(0, i64::MAX / 2, 60.0)];
        for i in 0..(MAX_INTERVALS_PER_BODY + 10) {
            let t0 = i as i64 * 10 * 3_600_000;
            let anchor = thermal::ReliableSample {
                temperature_f: 90.0,
                observed_at_unix_ms: t0,
            };
            estimator.capture_cooling_interval(
                HeatingBodyKind::Pool,
                anchor,
                89.5,
                0.1,
                &segs,
                t0 + 8 * 3_600_000,
            );
        }
        assert_eq!(
            estimator.intervals(HeatingBodyKind::Pool).len(),
            MAX_INTERVALS_PER_BODY
        );
    }

    #[test]
    fn active_warmup_body_gets_no_cooling_projection() {
        // Idle guard: a body that is on + active but unreliable (sensor warmup)
        // is circulating, not cooling — it must NOT receive a cooling projection.
        let path = test_store_path("prediction-warmup-guard");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 98,
            observed_at_unix_ms: now - 2 * 3_600_000,
        });
        // on + active, but it just became active → still inside warmup → unreliable.
        estimator.spa_active_since_unix_ms = Some(now);
        let mut system = test_system(98, 104, true, true, "heater");
        let weather = gap_weather(now, 3, 60.0);

        estimator.apply_to_system(&mut system, &weather);

        let spa = system.spa.expect("spa");
        assert!(!spa.temperature_reliable, "warmup body is unreliable");
        assert!(
            spa.predicted_temperature.is_none(),
            "an actively-circulating (warmup) body must not get a cooling projection"
        );
        assert!(spa.prediction_basis.is_none());
        assert!(!spa.temperature_display.is_predicted);
    }

    #[test]
    fn stale_weather_falls_back_to_controller_air_cooling_only() {
        // No fresh weather sample → degrade to a controller-air cooling-only
        // projection (basis projected-cooling-only), not none.
        let path = test_store_path("prediction-controller-air");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 98,
            observed_at_unix_ms: now - 2 * 3_600_000,
        });
        // Idle shared body → unreliable → prediction engages.
        let mut system = test_system(98, 104, false, false, "off");
        // Controller air temp is 70 °F (test_system default); much cooler than 98.
        let weather = WeatherCache::default(); // empty → no fresh sample

        estimator.apply_to_system(&mut system, &weather);

        let spa = system.spa.expect("spa");
        assert_eq!(
            spa.prediction_basis.as_deref(),
            Some("projected-cooling-only"),
            "empty cache should degrade to the controller-air cooling-only tier"
        );
        let predicted = spa.predicted_temperature.expect("predicted temperature");
        assert!(predicted < 98, "water should cool toward 70 °F air: {predicted}");
        assert!(spa.temperature_display.is_predicted);
    }

    #[test]
    fn large_mae_widens_uncertainty_band() {
        // The persisted rolling MAE sets a floor under the surfaced ± band.
        let path = test_store_path("prediction-mae-uncertainty");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let now = unix_time_ms();
        estimator.store.spa_last_reliable_temperature = Some(ReliableTemperatureObservation {
            temperature: 98,
            observed_at_unix_ms: now - 3_600_000, // 1h gap → small heuristic band
        });
        estimator.store.spa_prediction_mae_f = Some(5.0); // large measured error
        let mut system = test_system(98, 104, false, false, "off");
        let weather = gap_weather(now, 2, 60.0);

        estimator.apply_to_system(&mut system, &weather);

        let spa = system.spa.expect("spa");
        let band = spa.prediction_uncertainty_f.expect("uncertainty");
        assert!(
            band >= MAE_UNCERTAINTY_K * 5.0,
            "uncertainty must honor the MAE floor: {band}"
        );
    }

    #[test]
    fn cooling_params_seed_from_history_cooling_interval() {
        let path = test_store_path("cooling-seed");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let controller_now = history_time(24, 6, 0, 0);
        let now_unix_ms = 1_700_000_000_000i64;
        // A long heater-off circulation run with the pool slowly cooling toward
        // a steady air temperature => a fittable cooling interval.
        let history = HistoryData {
            air_temps: vec![
                TimeTempPoint {
                    time: history_time(24, 0, 0, 0),
                    temp: 60,
                },
                TimeTempPoint {
                    time: history_time(24, 6, 0, 0),
                    temp: 60,
                },
            ],
            pool_temps: vec![
                TimeTempPoint {
                    time: history_time(24, 0, 30, 0),
                    temp: 90,
                },
                TimeTempPoint {
                    time: history_time(24, 1, 30, 0),
                    temp: 87,
                },
                TimeTempPoint {
                    time: history_time(24, 2, 30, 0),
                    temp: 84,
                },
                TimeTempPoint {
                    time: history_time(24, 3, 30, 0),
                    temp: 82,
                },
                TimeTempPoint {
                    time: history_time(24, 4, 30, 0),
                    temp: 80,
                },
            ],
            pool_set_point_temps: vec![],
            spa_temps: vec![],
            spa_set_point_temps: vec![],
            pool_runs: vec![TimeRangePoint {
                on: history_time(24, 0, 0, 0),
                off: history_time(24, 5, 0, 0),
            }],
            spa_runs: vec![],
            solar_runs: vec![],
            heater_runs: vec![], // heater off the whole time => cooling
            light_runs: vec![],
        };

        estimator.seed_cooling_params_from_history(&history, &controller_now, now_unix_ms, false);

        let params = estimator
            .store
            .pool_cooling_params
            .expect("cooling params seeded");
        let tau = 1.0 / params.k0_per_hour;
        assert!(
            (2.0..=200.0).contains(&tau),
            "fitted tau should be physically plausible: {tau}"
        );
    }

    /// Fixture-replay: a synthetic but realistic 48h controller `HistoryData`
    /// carrying a heater-OFF cooling interval (pump circulating, heater idle) is
    /// fed through the *real* calibration path
    /// (`seed_cooling_params_from_history` -> `fit_history_cooling` ->
    /// `thermal::fit_cooling_params`). We assert (a) the fitted time constant is
    /// physically plausible, and (b) the fit generalizes: projecting with the
    /// fitted params over an independent *held-out* cooling interval keeps the
    /// mean absolute error under threshold. Fully offline — no controller, no
    /// live API.
    #[test]
    fn fixture_replay_history_fit_is_plausible_and_holds_out() {
        // Ground-truth covered-idle physics: Newtonian relaxation toward steady
        // outdoor air at a known time constant. Integer controller temps add
        // realistic quantization noise to the fixture.
        let air_f = 55.0_f64;
        let k_true = 1.0 / 20.0; // tau = 20h, comfortably inside [2h, 200h]
        let cooled = |start_f: f64, hours: f64| air_f + (start_f - air_f) * (-k_true * hours).exp();

        let path = test_store_path("fixture-replay-history");
        let mut estimator = HeatEstimator::load(test_config(), path);
        let controller_now = history_time(3, 0, 0, 0); // day 3, midnight
        let now_unix_ms = 1_700_000_000_000i64;

        // A single continuous 23h heater-off circulation run on day 1: the pool
        // slowly cools, sampled hourly and rounded to whole degrees like the
        // real controller. Monotonic, so no cross-run pairing artifacts.
        let train_start_f = 95.0;
        let train_hours: u16 = 23;
        let pool_temps: Vec<TimeTempPoint> = (0..=train_hours)
            .map(|h| TimeTempPoint {
                time: history_time(1, h, 0, 0),
                temp: cooled(train_start_f, h as f64).round() as i32,
            })
            .collect();

        // Air temperature reported across the full 48h window (day 1 -> day 2).
        let air_temps = vec![
            TimeTempPoint {
                time: history_time(1, 0, 0, 0),
                temp: air_f as i32,
            },
            TimeTempPoint {
                time: history_time(2, 23, 0, 0),
                temp: air_f as i32,
            },
        ];

        let history = HistoryData {
            air_temps,
            pool_temps,
            pool_set_point_temps: vec![],
            spa_temps: vec![],
            spa_set_point_temps: vec![],
            pool_runs: vec![TimeRangePoint {
                on: history_time(1, 0, 0, 0),
                off: history_time(1, 23, 0, 0),
            }],
            spa_runs: vec![],
            solar_runs: vec![],
            heater_runs: vec![], // heater off the whole time => cooling interval
            light_runs: vec![],
        };

        // Run the real calibration path (weak history-interval seed).
        estimator.seed_cooling_params_from_history(&history, &controller_now, now_unix_ms, false);

        let params = estimator
            .store
            .pool_cooling_params
            .expect("cooling params should be seeded from the 48h history");
        let tau = 1.0 / params.k0_per_hour;
        assert!(
            (2.0..=200.0).contains(&tau),
            "fitted tau must be physically plausible: {tau}"
        );
        // The fixture is a clean cooling curve; the fit should land near truth.
        assert!(
            (tau - 20.0).abs() < 6.0,
            "fitted tau {tau} should be close to the ground-truth 20h"
        );

        // Hold-out generalization: an independent cooling interval (a different
        // starting temperature) the fit never saw. Project it forward with the
        // fitted params and compare to the ground-truth integer temperatures.
        let holdout_start_f = 88.0;
        let base = now_unix_ms;
        let hour_ms = 3_600_000i64;
        let anchor = ReliableSample {
            temperature_f: cooled(holdout_start_f, 0.0).round(),
            observed_at_unix_ms: base,
        };
        let segment = thermal::WeatherSegment {
            start_unix_ms: base - hour_ms,
            end_unix_ms: base + 12 * hour_ms,
            air_temp_f: air_f,
            wind_mph: None,
            humidity_fraction: None,
            cloud_fraction: None,
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        };

        let mut total_abs_error = 0.0;
        let holdout_steps: i64 = 10; // gaps 1..=10h all sit under the 12h cutoff
        for h in 1..=holdout_steps {
            let projected =
                thermal::project_temperature(anchor, &[segment], &params, base + h * hour_ms);
            assert_eq!(
                projected.basis,
                PredictionBasis::ProjectedCoolingOnly,
                "cooling-only fixture should project on the cooling-only basis"
            );
            let actual = cooled(holdout_start_f, h as f64).round();
            total_abs_error += (projected.predicted_f - actual).abs();
        }
        let holdout_mae = total_abs_error / holdout_steps as f64;
        assert!(
            holdout_mae < 1.5,
            "held-out projection MAE {holdout_mae} °F should be under threshold"
        );
    }

    #[test]
    fn store_new_fields_default_and_round_trip_old_file() {
        // An old-format store (missing every new field) must deserialize.
        let old = r#"{
            "pool_learned_rate_f_per_hour": 2.0,
            "spa_learned_rate_f_per_hour": null,
            "pool_last_reliable_temperature": null,
            "spa_last_reliable_temperature": null,
            "recent_sessions": []
        }"#;
        let store: HeatEstimatorStore = serde_json::from_str(old).expect("back-compat");
        assert!(store.pool_cooling_intervals.is_empty());
        assert!(store.activity_windows.is_empty());
        assert!(store.mae_history.is_empty());
        assert!(store.pool_outlet_offset_f.is_none());
        assert!(store.pool_last_refit_unix_ms.is_none());
        // And the extended store round-trips.
        let json = serde_json::to_string(&store).expect("serialize");
        let _: HeatEstimatorStore = serde_json::from_str(&json).expect("round-trip");
    }

    #[test]
    fn activity_windows_recorded_on_body_off_transition_and_bounded() {
        let mut estimator = test_estimator();
        for i in 0..(MAX_ACTIVITY_WINDOWS + 10) {
            let t_on = (i as i64) * 100_000;
            estimator.test_note_activity(HeatingBodyKind::Spa, true, t_on);
            estimator.test_note_activity(HeatingBodyKind::Spa, false, t_on + 50_000);
        }
        assert_eq!(estimator.store.activity_windows.len(), MAX_ACTIVITY_WINDOWS);
        // Overlap detection: last window overlaps a gap spanning it.
        let last = *estimator.store.activity_windows.last().unwrap();
        assert!(estimator.activity_overlapped_gap(
            HeatingBodyKind::Spa, last.start_unix_ms - 10, last.end_unix_ms + 10));
        assert!(!estimator.activity_overlapped_gap(
            HeatingBodyKind::Spa, last.end_unix_ms + 10, last.end_unix_ms + 20));
        assert!(!estimator.activity_overlapped_gap(
            HeatingBodyKind::Pool, last.start_unix_ms - 10, last.end_unix_ms + 10));
    }

    /// Fill the estimator with synthetic idle intervals generated from `truth`.
    fn seed_synthetic_intervals(
        estimator: &mut HeatEstimator,
        body: HeatingBodyKind,
        truth: &CoolingParams,
        count: usize,
    ) {
        for i in 0..count {
            let t0 = i as i64 * 12 * 3_600_000;
            let t1 = t0 + 10 * 3_600_000;
            let segs = vec![test_weather_segment(t0, t1, 60.0)];
            let weather = crate::calibrator::bucket_weather(&segs, t0, t1);
            let mut interval = crate::calibrator::CoolingInterval {
                t0_unix_ms: t0, t1_unix_ms: t1, temp0_f: 95.0, temp1_f: 0.0,
                regime: crate::calibrator::IntervalRegime::IdleCovered, weather,
            };
            interval.temp1_f = crate::calibrator::predict_interval_end(
                &interval, truth, thermal::SolarSite::disabled());
            estimator.intervals_slot_mut(body).push(interval);
        }
    }

    #[test]
    fn refit_applies_validated_blend_silently() {
        let mut estimator = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        // Current params badly wrong.
        *estimator.cooling_params_slot_mut(HeatingBodyKind::Spa) =
            Some(CoolingParams { k0_per_hour: 1.0 / 30.0, ..CoolingParams::seed() });
        // Adaptation: mae_history is only appended when a rolling prediction
        // MAE is already known (spec §10: it mirrors the closed-loop EWMA
        // trend), which a brand-new estimator doesn't have yet. Seed one so
        // the "mae_history recorded on accept" assertion below is meaningful.
        *estimator.prediction_mae_slot_mut(HeatingBodyKind::Spa) = Some(2.5);
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 8);
        // Adaptation: shortly after the last seeded interval (t1 ~= 94h), not
        // the brief's `100 * 12h` (~50 days) — with the real default
        // `window_days` (14), that would place every synthetic interval
        // outside the rolling fit window and the refit would no-op.
        let now = 9 * 12 * 3_600_000;
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        let after = estimator.cooling_params(HeatingBodyKind::Spa);
        // Moved toward truth (damped: between old 1/30 and truth 1/96).
        assert!(after.k0_per_hour < 1.0 / 30.0);
        assert!(after.k0_per_hour > 1.0 / 96.0);
        assert_eq!(estimator.store.spa_last_refit_unix_ms, Some(now));
        assert!(!estimator.store.mae_history.is_empty());
    }

    #[test]
    fn refit_rate_limited_by_refit_min_hours() {
        let mut estimator = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 8);
        let now = 100 * 12 * 3_600_000;
        estimator.store.spa_last_refit_unix_ms = Some(now - 3_600_000); // 1h ago < 24h
        let before = estimator.cooling_params(HeatingBodyKind::Spa);
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert_eq!(estimator.cooling_params(HeatingBodyKind::Spa), before);
        assert_eq!(estimator.store.spa_last_refit_unix_ms, Some(now - 3_600_000));
    }

    #[test]
    fn refit_needs_min_new_intervals_unless_mae_drifts() {
        let mut estimator = test_estimator();
        let truth = CoolingParams { k0_per_hour: 1.0 / 96.0, ..CoolingParams::seed() };
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 2); // < 4
        // Adaptation: same reasoning as `refit_applies_validated_blend_silently`
        // — keep `now` within the rolling `window_days` window of the seeded
        // synthetic intervals (max t1 ~= 46h after the second seeding below).
        let now = 5 * 12 * 3_600_000;
        let before = estimator.cooling_params(HeatingBodyKind::Spa);
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert_eq!(estimator.cooling_params(HeatingBodyKind::Spa), before, "too few intervals, no drift");
        // Drifted MAE forces the fit even with few intervals... but the accept
        // gate needs a holdout, so seed enough for a split first.
        seed_synthetic_intervals(&mut estimator, HeatingBodyKind::Spa, &truth, 4);
        *estimator.prediction_mae_slot_mut(HeatingBodyKind::Spa) = Some(5.0); // > mae_drift_f
        estimator.maybe_refit(HeatingBodyKind::Spa, now);
        assert!(estimator.store.spa_last_refit_unix_ms.is_some(), "drift trigger fired");
    }
}
