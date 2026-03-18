use pentair_protocol::responses::*;
use pentair_protocol::semantic::{self, CircuitMap, PoolSystem, PoolSystemInput};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
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
    circuit_map: Option<CircuitMap>,
}

impl CachedState {
    pub fn pool_system(&self) -> Option<PoolSystem> {
        let (system, _) = self.build_semantic()?;
        Some(system)
    }

    pub fn resolve_circuit(&self, id: &str) -> Option<i32> {
        self.circuit_map.as_ref()?.resolve(id)
    }

    pub fn rebuild_semantic(&mut self) {
        if let Some((_, map)) = self.build_semantic() {
            self.circuit_map = Some(map);
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

pub fn new_shared_state(spa_associations: Vec<String>) -> SharedState {
    Arc::new(RwLock::new(CachedState {
        pumps: vec![None; 8],
        spa_associations,
        ..Default::default()
    }))
}
