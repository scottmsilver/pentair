use crate::convert;
use crate::light_modes::LightModeMap;
use crate::pool_types::PoolSystem;

/// Cached Matter-relevant state derived from PoolSystem.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatterState {
    pub pool_reachable: bool,
    pub pool_on: bool,
    pub pool_temp_matter: i16,
    pub pool_setpoint_matter: i16,
    pub pool_system_mode: u8,
    pub spa_reachable: bool,
    pub lights_reachable: bool,
    pub spa_on: bool,
    pub spa_temp_matter: i16,
    pub spa_setpoint_matter: i16,
    pub spa_system_mode: u8,
    pub jets_on: bool,
    pub lights_on: bool,
    pub light_mode_index: Option<u8>,
    pub light_mode_name: Option<String>,
}

impl MatterState {
    pub fn from_pool_system(ps: &PoolSystem, mode_map: &LightModeMap) -> Self {
        Self {
            pool_reachable: ps.pool.is_some(),
            pool_on: ps.pool.as_ref().map(|p| p.active).unwrap_or(false),
            pool_temp_matter: ps.pool.as_ref().map(|p| convert::fahrenheit_to_matter(p.temperature)).unwrap_or(0),
            pool_setpoint_matter: ps.pool.as_ref().map(|p| convert::fahrenheit_to_matter(p.setpoint)).unwrap_or(0),
            pool_system_mode: ps.pool.as_ref().map(|p| convert::pentair_heat_mode_to_matter(&p.heat_mode)).unwrap_or(0),
            spa_reachable: ps.spa.is_some(),
            lights_reachable: ps.lights.is_some(),
            spa_on: ps.spa.as_ref().map(|s| s.active).unwrap_or(false),
            spa_temp_matter: ps.spa.as_ref().map(|s| convert::fahrenheit_to_matter(s.temperature)).unwrap_or(0),
            spa_setpoint_matter: ps.spa.as_ref().map(|s| convert::fahrenheit_to_matter(s.setpoint)).unwrap_or(0),
            spa_system_mode: ps.spa.as_ref().map(|s| convert::pentair_heat_mode_to_matter(&s.heat_mode)).unwrap_or(0),
            jets_on: ps.spa.as_ref().and_then(|s| s.accessories.get("jets").copied()).unwrap_or(false),
            lights_on: ps.lights.as_ref().map(|l| l.on).unwrap_or(false),
            light_mode_index: ps.lights.as_ref().and_then(|l| mode_map.current_mode_index(l.mode.as_deref())),
            light_mode_name: ps.lights.as_ref().and_then(|l| l.mode.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool_types::*;
    use std::collections::HashMap;

    fn make_pool_system(spa_temp: i32, spa_setpoint: i32, spa_active: bool, jets: bool) -> PoolSystem {
        PoolSystem {
            pool: Some(Body {
                on: false, active: false, temperature: 82, setpoint: 59,
                heat_mode: "off".to_string(), heating: "off".to_string(),
            }),
            spa: Some(SpaBody {
                on: spa_active, active: spa_active, temperature: spa_temp, setpoint: spa_setpoint,
                heat_mode: "heat-pump".to_string(), heating: if spa_active { "heater" } else { "off" }.to_string(),
                accessories: if jets { HashMap::from([("jets".to_string(), true)]) } else { HashMap::new() },
            }),
            lights: Some(Lights {
                on: true, mode: Some("caribbean".to_string()),
                available_modes: vec!["off","on","set","sync","swim","party","romantic","caribbean"].into_iter().map(String::from).collect(),
            }),
            system: System { pool_spa_shared_pump: true },
        }
    }

    #[test]
    fn converts_state_correctly() {
        let ps = make_pool_system(104, 104, true, true);
        let mode_map = LightModeMap::from_available_modes(&ps.lights.as_ref().unwrap().available_modes);
        let state = MatterState::from_pool_system(&ps, &mode_map);

        assert!(state.spa_reachable);
        assert!(state.spa_on);
        assert_eq!(state.spa_temp_matter, 4000);
        assert_eq!(state.spa_setpoint_matter, 4000);
        assert_eq!(state.spa_system_mode, 4);
        assert!(state.jets_on);
        assert!(state.lights_on);
        assert!(state.lights_reachable);
        assert_eq!(state.light_mode_index, Some(3));
        assert_eq!(state.light_mode_name, Some("caribbean".to_string()));
        // Pool fields (from make_pool_system fixture: temp=82, setpoint=59, heat_mode="off")
        assert!(state.pool_reachable);
        assert!(!state.pool_on);
        assert_eq!(state.pool_temp_matter, convert::fahrenheit_to_matter(82));
        assert_eq!(state.pool_setpoint_matter, convert::fahrenheit_to_matter(59));
        assert_eq!(state.pool_system_mode, 0); // "off" → 0
    }

    #[test]
    fn detects_change() {
        let ps1 = make_pool_system(103, 104, true, false);
        let ps2 = make_pool_system(104, 104, true, true);
        let mode_map = LightModeMap::from_available_modes(&ps1.lights.as_ref().unwrap().available_modes);
        let state1 = MatterState::from_pool_system(&ps1, &mode_map);
        let state2 = MatterState::from_pool_system(&ps2, &mode_map);

        assert_ne!(state1, state2);
        assert_ne!(state1.spa_temp_matter, state2.spa_temp_matter);
        assert_ne!(state1.jets_on, state2.jets_on);
    }

    #[test]
    fn handles_missing_spa() {
        let ps = PoolSystem {
            pool: Some(Body { on: false, active: false, temperature: 82, setpoint: 59, heat_mode: "off".to_string(), heating: "off".to_string() }),
            spa: None,
            lights: None,
            system: System { pool_spa_shared_pump: false },
        };
        let mode_map = LightModeMap::from_available_modes(&[]);
        let state = MatterState::from_pool_system(&ps, &mode_map);

        assert!(!state.spa_reachable);
        assert!(!state.lights_reachable);
        assert!(!state.spa_on);
        assert_eq!(state.spa_temp_matter, 0);
        assert!(!state.jets_on);
        assert!(!state.lights_on);
        // Pool is present in this test
        assert!(state.pool_reachable);
    }

    #[test]
    fn handles_missing_pool() {
        let ps = PoolSystem {
            pool: None,
            spa: None,
            lights: None,
            system: System { pool_spa_shared_pump: false },
        };
        let mode_map = LightModeMap::from_available_modes(&[]);
        let state = MatterState::from_pool_system(&ps, &mode_map);

        assert!(!state.pool_reachable);
        assert!(!state.pool_on);
        assert_eq!(state.pool_temp_matter, 0);
        assert_eq!(state.pool_setpoint_matter, 0);
        assert_eq!(state.pool_system_mode, 0);
        assert!(state.light_mode_name.is_none());
    }
}
