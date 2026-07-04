use crate::config::{HeatingConfig, SpaHeatNotificationsConfig};
use crate::heat::HeatEstimator;
use crate::spa_notifications::SpaHeatNotificationEvent;
use crate::weather::WeatherCache;
use pentair_protocol::responses::*;
use pentair_protocol::semantic::{self, CircuitMap, PoolSystem, PoolSystemInput};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A cached controller snapshot older than this is STALE: its "live"
/// temperatures can no longer be trusted (the adapter link is down), so the
/// bodies flip to not-reliable and the weather-informed estimate takes over.
/// The adapter normally refreshes every few seconds; 5 minutes is generous.
const STALE_SNAPSHOT_MS: i64 = 5 * 60_000;

#[derive(Debug)]
pub struct CachedState {
    pub status: Option<PoolStatus>,
    pub config: Option<ControllerConfig>,
    pub chem: Option<ChemData>,
    pub scg: Option<ScgConfig>,
    pub pumps: Vec<Option<PumpStatus>>,
    pub version: Option<VersionResponse>,
    pub light_mode: Option<String>,
    /// Config-driven spa associations (circuit names to treat as spa accessories).
    pub spa_associations: Vec<String>,
    pub heat: HeatEstimator,
    /// Recent observed + forecast weather samples for the temperature predictor.
    pub weather: WeatherCache,
    /// When `status` was last refreshed from the controller (unix ms). Used to
    /// detect a frozen snapshot after the adapter link drops.
    pub status_updated_unix_ms: Option<i64>,
    circuit_map: Option<CircuitMap>,
}

impl CachedState {
    /// True when the cached controller snapshot is too old to present as live
    /// (see [`STALE_SNAPSHOT_MS`]).
    fn snapshot_stale(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.status_updated_unix_ms
            .is_some_and(|at| now_ms.saturating_sub(at) > STALE_SNAPSHOT_MS)
    }

    pub fn pool_system(&self) -> Option<PoolSystem> {
        let (mut system, _) = self.build_semantic()?;
        self.heat
            .apply_to_system_with_staleness(&mut system, &self.weather, self.snapshot_stale());
        Some(system)
    }

    pub fn resolve_circuit(&self, id: &str) -> Option<i32> {
        self.circuit_map.as_ref()?.resolve(id)
    }

    pub fn pool_system_and_spa_notification_events(
        &mut self,
    ) -> (Option<PoolSystem>, Vec<SpaHeatNotificationEvent>) {
        let Some((mut system, _)) = self.build_semantic() else {
            return (None, Vec::new());
        };

        let stale = self.snapshot_stale();
        self.heat
            .apply_to_system_with_staleness(&mut system, &self.weather, stale);
        let events = self.heat.spa_heat_notification_events_for_system(&system);
        (Some(system), events)
    }

    pub fn refresh_semantic_state(&mut self) {
        if let Some((system, map)) = self.build_semantic() {
            self.circuit_map = Some(map);
            // Closed-loop calibration must run BEFORE `update` overwrites the
            // stored last-reliable anchor with the fresh reading.
            self.heat.calibrate_predictions(&system, &self.weather);
            self.heat.update(&system);
        }
    }

    fn build_semantic(&self) -> Option<(PoolSystem, CircuitMap)> {
        let status = self.status.as_ref()?;
        let config = self.config.as_ref()?;
        let input = PoolSystemInput {
            status,
            config,
            pumps: &self.pumps,
            version: self.version.as_ref().map(|v| v.version.as_str()),
            light_mode: self.light_mode.as_deref(),
            spa_associations: &self.spa_associations,
        };
        Some(semantic::build_pool_system(&input))
    }
}

pub type SharedState = Arc<RwLock<CachedState>>;

#[allow(clippy::too_many_arguments)]
pub fn new_shared_state(
    spa_associations: Vec<String>,
    heating: HeatingConfig,
    spa_notifications: SpaHeatNotificationsConfig,
    heating_history_path: PathBuf,
    weather_cache_path: PathBuf,
    solar_location: Option<(f64, f64)>,
) -> SharedState {
    let weather = WeatherCache::load(&weather_cache_path);
    let mut heat = HeatEstimator::load_with_notifications(
        heating,
        spa_notifications,
        heating_history_path,
    );
    heat.set_solar_location(solar_location);
    Arc::new(RwLock::new(CachedState {
        pumps: vec![None; 8],
        spa_associations,
        heat,
        weather,
        status: None,
        config: None,
        chem: None,
        scg: None,
        version: None,
        light_mode: None,
        status_updated_unix_ms: None,
        circuit_map: None,
    }))
}
