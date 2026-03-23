use crate::config::HeatingConfig;
use crate::heat::HeatEstimator;
use pentair_protocol::responses::*;
use pentair_protocol::semantic::{self, CircuitMap, PoolSystem, PoolSystemInput};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    circuit_map: Option<CircuitMap>,
}

impl CachedState {
    pub fn pool_system(&self) -> Option<PoolSystem> {
        let (mut system, _) = self.build_semantic()?;
        self.heat.apply_to_system(&mut system);
        Some(system)
    }

    pub fn resolve_circuit(&self, id: &str) -> Option<i32> {
        self.circuit_map.as_ref()?.resolve(id)
    }

    pub fn refresh_semantic_state(&mut self) {
        if let Some((system, map)) = self.build_semantic() {
            self.circuit_map = Some(map);
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

pub fn new_shared_state(
    spa_associations: Vec<String>,
    heating: HeatingConfig,
    heating_history_path: PathBuf,
) -> SharedState {
    Arc::new(RwLock::new(CachedState {
        pumps: vec![None; 8],
        spa_associations,
        heat: HeatEstimator::load(heating, heating_history_path),
        status: None,
        config: None,
        chem: None,
        scg: None,
        version: None,
        light_mode: None,
        circuit_map: None,
    }))
}
