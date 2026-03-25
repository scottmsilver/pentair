use serde::Deserialize;
use std::collections::HashMap;

/// Subset of the daemon's GET /api/pool response — only fields needed for Matter endpoints.
/// Pool, spa, and lights are Option because they may not be configured on all controllers.
#[derive(Debug, Clone, Deserialize)]
pub struct PoolSystem {
    pub pool: Option<Body>,
    pub spa: Option<SpaBody>,
    pub lights: Option<Lights>,
    pub system: System,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Body {
    pub on: bool,
    #[serde(default)]
    pub active: bool,
    pub temperature: i32,
    pub setpoint: i32,
    pub heat_mode: String,
    pub heating: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpaBody {
    pub on: bool,
    #[serde(default)]
    pub active: bool,
    pub temperature: i32,
    pub setpoint: i32,
    pub heat_mode: String,
    pub heating: String,
    #[serde(default)]
    pub accessories: HashMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lights {
    pub on: bool,
    pub mode: Option<String>,
    pub available_modes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct System {
    pub pool_spa_shared_pump: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{
            "pool": {"on": false, "active": false, "temperature": 82, "setpoint": 59, "heat_mode": "off", "heating": "off",
                     "temperature_reliable": true, "last_reliable_temperature": 82, "last_reliable_temperature_at_unix_ms": 0,
                     "temperature_display": {"value": 82, "is_stale": false}, "heat_estimate_display": {"state": "unavailable", "reason": "not-heating", "target_temperature": 59},
                     "heat_estimate": {"available": false, "minutes_remaining": null, "current_temperature": 82, "target_temperature": 59, "confidence": "none", "source": "none", "reason": "not-heating", "updated_at_unix_ms": 0}},
            "spa": {"on": true, "active": true, "temperature": 103, "setpoint": 104, "heat_mode": "heat-pump", "heating": "heater",
                    "temperature_reliable": true, "last_reliable_temperature": 103, "last_reliable_temperature_at_unix_ms": 0,
                    "temperature_display": {"value": 103, "is_stale": false}, "heat_estimate_display": {"state": "ready", "minutes_remaining": 5, "target_temperature": 104},
                    "heat_estimate": {"available": true, "minutes_remaining": 5, "current_temperature": 103, "target_temperature": 104, "confidence": "high", "source": "observed", "reason": null, "updated_at_unix_ms": 0},
                    "accessories": {"jets": true}},
            "lights": {"on": true, "mode": "caribbean", "available_modes": ["off","on","set","sync","swim","party","romantic","caribbean","american","sunset","royal","blue","green","red","white","purple"]},
            "auxiliaries": [],
            "pump": {"pump_type": "VS", "running": true, "watts": 1200, "rpm": 2700, "gpm": 45},
            "system": {"controller": "IntelliTouch", "firmware": "5.2", "temp_unit": "°F", "air_temperature": 72, "freeze_protection": false, "pool_spa_shared_pump": true}
        }"#
    }

    #[test]
    fn parse_full_response() {
        let ps: PoolSystem = serde_json::from_str(sample_json()).unwrap();
        let spa = ps.spa.as_ref().unwrap();
        assert!(spa.active);
        assert_eq!(spa.temperature, 103);
        assert_eq!(spa.setpoint, 104);
        assert_eq!(spa.heat_mode, "heat-pump");
        assert!(spa.accessories.get("jets").copied().unwrap_or(false));
        let lights = ps.lights.as_ref().unwrap();
        assert!(lights.on);
        assert_eq!(lights.mode.as_deref(), Some("caribbean"));
        assert!(ps.system.pool_spa_shared_pump);
    }

    #[test]
    fn parse_with_null_optional_bodies() {
        let json = r#"{
            "pool": {"on": false, "temperature": 80, "setpoint": 59, "heat_mode": "off", "heating": "off"},
            "spa": null,
            "lights": null,
            "auxiliaries": [],
            "pump": {"pump_type": "VS", "running": false, "watts": 0, "rpm": 0, "gpm": 0},
            "system": {"controller": "IntelliTouch", "firmware": "5.2", "temp_unit": "°F", "air_temperature": 70, "freeze_protection": false, "pool_spa_shared_pump": false}
        }"#;
        let ps: PoolSystem = serde_json::from_str(json).unwrap();
        assert!(ps.spa.is_none());
        assert!(ps.lights.is_none());
        assert!(ps.pool.is_some());
    }
}
