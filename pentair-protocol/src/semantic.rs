//! Semantic pool system model.
//!
//! Transforms raw protocol responses (status, config, pump data) into a
//! human-friendly view of the pool system. Auto-discovers topology from
//! pump speed tables and circuit function codes.
//!
//! The semantic model hides all protocol internals (circuit IDs, wire offsets,
//! function codes, body types). Clients interact using semantic identifiers
//! like "spa", "pool", "jets", "lights".

use crate::responses::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ─── Public model (what clients see) ────────────────────────────────────

/// The top-level semantic view of the entire pool system.
#[derive(Debug, Clone, Serialize)]
pub struct PoolSystem {
    pub pool: Option<BodyState>,
    pub spa: Option<SpaState>,
    pub lights: Option<LightState>,
    pub auxiliaries: Vec<AuxState>,
    pub pump: Option<PumpInfo>,
    pub system: SystemInfo,
    /// True when there are user-initiated things to turn off (spa on or lights on).
    /// Used by UIs to show/hide a "goodnight" button.
    #[serde(default)]
    pub goodnight_available: bool,
}

/// Server-side estimate of time remaining to reach setpoint.
#[derive(Debug, Clone, Serialize)]
pub struct HeatEstimate {
    pub available: bool,
    pub minutes_remaining: Option<u32>,
    pub current_temperature: i32,
    pub target_temperature: i32,
    pub confidence: String,
    pub source: String,
    pub reason: String,
    pub observed_rate_per_hour: Option<f64>,
    pub learned_rate_per_hour: Option<f64>,
    pub configured_rate_per_hour: Option<f64>,
    pub baseline_rate_per_hour: Option<f64>,
    pub updated_at_unix_ms: i64,
}

/// UI-oriented server display contract for body temperature presentation.
#[derive(Debug, Clone, Serialize)]
pub struct TemperatureDisplay {
    pub value: Option<i32>,
    pub is_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reliable_at_unix_ms: Option<i64>,
}

/// UI-oriented server display contract for heat estimate presentation.
#[derive(Debug, Clone, Serialize)]
pub struct HeatEstimateDisplay {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_in_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes_remaining: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_temperature: Option<i32>,
}

/// UI-oriented server display contract for spa heat progress.
///
/// Consolidates all spa heating progress state so clients can drive
/// notifications, Live Activities, and progress UI without local tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaHeatProgress {
    /// Whether the spa is actively heating (on, heat_mode != "off", heating != "off").
    pub active: bool,
    /// "started" = heating just began (no ETA yet), "tracking" = ETA available,
    /// "reached" = current_temp >= target, "off" = not heating.
    pub phase: String,
    /// Temperature when the current heating session began (from the HeatEstimator's
    /// trusted session). None when no active session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_temp_f: Option<i32>,
    /// Current water temperature.
    pub current_temp_f: i32,
    /// Heat setpoint.
    pub target_temp_f: i32,
    /// 0-100 progress from start to target.
    pub progress_pct: u8,
    /// Minutes remaining from the heat estimate, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes_remaining: Option<u32>,
    /// Identifier for the current heating session (unix ms of session start).
    /// Clients can use this to detect session changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Milestone label for the current heating state, if any.
    /// Values: "heating_started", "halfway", "almost_ready", "at_temp".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
}

impl Default for SpaHeatProgress {
    /// Default is the "not heating" state.
    /// `current_temp_f` and `target_temp_f` default to 0, meaning "not available".
    /// Clients should display "--" when these are 0 (the widget already does this).
    fn default() -> Self {
        Self {
            active: false,
            phase: "off".to_string(),
            start_temp_f: None,
            current_temp_f: 0,
            target_temp_f: 0,
            progress_pct: 0,
            minutes_remaining: None,
            session_id: None,
            milestone: None,
        }
    }
}

/// Pool body — on/off, temperature, heating.
#[derive(Debug, Clone, Serialize)]
pub struct BodyState {
    /// Circuit is commanded on by the controller.
    pub on: bool,
    /// Water is actually flowing (circuit on AND pump running with RPM > 0).
    /// When `on` is true but `active` is false, the circuit was just turned on
    /// and the pump hasn't ramped up yet, or something is wrong.
    pub active: bool,
    pub temperature: i32,
    pub temperature_reliable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reliable_temperature: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reliable_temperature_at_unix_ms: Option<i64>,
    pub setpoint: i32,
    pub heat_mode: String,
    pub heating: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_estimate: Option<HeatEstimate>,
    pub temperature_display: TemperatureDisplay,
    pub heat_estimate_display: HeatEstimateDisplay,
}

/// Spa body — everything pool has, plus accessories like jets.
#[derive(Debug, Clone, Serialize)]
pub struct SpaState {
    /// Circuit is commanded on by the controller.
    pub on: bool,
    /// Water is actually flowing (circuit on AND pump running with RPM > 0).
    pub active: bool,
    pub temperature: i32,
    pub temperature_reliable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reliable_temperature: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reliable_temperature_at_unix_ms: Option<i64>,
    pub setpoint: i32,
    pub heat_mode: String,
    pub heating: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_estimate: Option<HeatEstimate>,
    pub temperature_display: TemperatureDisplay,
    pub heat_estimate_display: HeatEstimateDisplay,
    /// Server-computed spa heat progress display contract.
    pub spa_heat_progress: SpaHeatProgress,
    /// Spa accessories (jets, blower, etc.) keyed by slug ID.
    pub accessories: HashMap<String, bool>,
}

/// Lights — on/off plus color mode (daemon-tracked).
#[derive(Debug, Clone, Serialize)]
pub struct LightState {
    pub on: bool,
    /// Last known color mode. None = unknown (not yet set via this daemon session).
    pub mode: Option<String>,
    /// Available color modes for reference.
    pub available_modes: Vec<&'static str>,
}

/// An auxiliary device (water feature, yard light, floor cleaner, etc.)
#[derive(Debug, Clone, Serialize)]
pub struct AuxState {
    /// Slug identifier for API use (e.g., "water_feature", "yard_light").
    pub id: String,
    /// Human-readable name from the controller.
    pub name: String,
    pub on: bool,
}

/// Pump status summary.
#[derive(Debug, Clone, Serialize)]
pub struct PumpInfo {
    pub pump_type: String,
    pub running: bool,
    pub watts: u32,
    pub rpm: u32,
    pub gpm: u32,
}

/// System-level info.
#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub controller: String,
    pub firmware: Option<String>,
    pub temp_unit: &'static str,
    pub air_temperature: i32,
    pub freeze_protection: bool,
    /// Pool and Spa share a pump — mutually exclusive operation.
    pub pool_spa_shared_pump: bool,
}

// ─── Resolution map (daemon uses this to map semantic actions → circuit IDs) ──

/// Maps semantic identifiers to logical circuit IDs.
/// Built alongside the PoolSystem so the daemon knows how to dispatch actions.
#[derive(Debug, Clone, Default)]
pub struct CircuitMap {
    map: HashMap<String, i32>,
}

impl CircuitMap {
    /// Look up the logical circuit ID for a semantic identifier.
    /// E.g., "pool" → 6, "spa" → 1, "jets" → 4, "lights" → 2, "water_feature" → 3
    pub fn resolve(&self, id: &str) -> Option<i32> {
        self.map.get(id).copied()
    }

    /// Body type for heat commands: "pool" → 0, "spa" → 1.
    pub fn body_type(id: &str) -> Option<i32> {
        match id {
            "pool" => Some(0),
            "spa" => Some(1),
            _ => None,
        }
    }
}

// ─── Builder ────────────────────────────────────────────────────────────

/// Input data for building the semantic model.
pub struct PoolSystemInput<'a> {
    pub status: &'a PoolStatus,
    pub config: &'a ControllerConfig,
    pub pumps: &'a [Option<PumpStatus>],
    pub version: Option<&'a str>,
    /// Daemon-tracked light mode (fire-and-forget state).
    pub light_mode: Option<&'a str>,
    /// Config overrides: circuit names to force-associate with spa.
    /// If empty, name convention is used.
    pub spa_associations: &'a [String],
}

const AVAILABLE_LIGHT_MODES: &[&str] = &[
    "off",
    "on",
    "set",
    "sync",
    "swim",
    "party",
    "romantic",
    "caribbean",
    "american",
    "sunset",
    "royal",
    "blue",
    "green",
    "red",
    "white",
    "purple",
];

/// Build the semantic pool system model and circuit map from raw protocol data.
pub fn build_pool_system(input: &PoolSystemInput) -> (PoolSystem, CircuitMap) {
    let status = input.status;
    let config = input.config;

    let mut circuit_map = CircuitMap::default();

    // ── Step 1: Discover pump topology ──────────────────────────────
    let mut pump_circuit_ids: HashSet<i32> = HashSet::new();
    let mut pool_spa_shared = false;
    let mut primary_pump: Option<PumpInfo> = None;

    for (_i, maybe_pump) in input.pumps.iter().enumerate() {
        if let Some(pump) = maybe_pump {
            if pump.pump_type == 0 {
                continue;
            }

            if primary_pump.is_none() {
                primary_pump = Some(PumpInfo {
                    pump_type: match pump.pump_type {
                        1 => "VF",
                        2 => "VS",
                        3 => "VSF",
                        _ => "Unknown",
                    }
                    .to_string(),
                    running: pump.is_running,
                    watts: pump.watts,
                    rpm: pump.rpm,
                    gpm: pump.gpm,
                });
            }

            let mut has_pool = false;
            let mut has_spa = false;
            for pc in &pump.circuits {
                if pc.circuit_id == 0 {
                    continue;
                }
                let cid = pc.circuit_id as i32;
                pump_circuit_ids.insert(cid);
                if let Some(circ) = config.circuits.iter().find(|c| (c.circuit_id - 499) == cid) {
                    if circ.function == 2 {
                        has_pool = true;
                    }
                    if circ.function == 1 {
                        has_spa = true;
                    }
                }
            }
            if has_pool && has_spa {
                pool_spa_shared = true;
            }
        }
    }

    // ── Step 2: Classify circuits ───────────────────────────────────
    let circuit_state = |wire_id: i32| -> bool {
        status
            .circuits
            .iter()
            .find(|c| c.circuit_id == wire_id)
            .map(|c| c.state)
            .unwrap_or(false)
    };

    let mut pool_circuit: Option<&CircuitConfig> = None;
    let mut spa_circuit: Option<&CircuitConfig> = None;
    let mut light_circuit: Option<&CircuitConfig> = None;
    let mut spa_accessory_circuits: Vec<&CircuitConfig> = Vec::new();
    let mut aux_circuits: Vec<&CircuitConfig> = Vec::new();

    for circ in &config.circuits {
        let logical_id = circ.circuit_id - 499;
        match circ.function {
            2 => {
                pool_circuit = Some(circ);
                circuit_map.map.insert("pool".to_string(), logical_id);
            }
            1 => {
                spa_circuit = Some(circ);
                circuit_map.map.insert("spa".to_string(), logical_id);
            }
            16 | 7 | 9 | 10 | 12 | 19 => {
                light_circuit = Some(circ);
                circuit_map.map.insert("lights".to_string(), logical_id);
            }
            _ => {
                // Check if this circuit is a spa accessory:
                // 1. Config override (explicit list in TOML)
                // 2. Name convention ("jets", "blower", etc.)
                let is_spa_accessory = input
                    .spa_associations
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&circ.name))
                    || associate_by_name(&circ.name) == Some("spa");

                if is_spa_accessory {
                    spa_accessory_circuits.push(circ);
                    let slug = slugify(&circ.name);
                    circuit_map.map.insert(slug, logical_id);
                } else {
                    let slug = slugify(&circ.name);
                    circuit_map.map.insert(slug.clone(), logical_id);
                    aux_circuits.push(circ);
                }
            }
        }
    }

    // ── Step 3: Build bodies ────────────────────────────────────────
    let pool_body = status.bodies.iter().find(|b| b.body_type == 0);
    let spa_body = status.bodies.iter().find(|b| b.body_type == 1);

    // Pump is actively flowing water when running with measurable RPM.
    let pump_flowing = primary_pump
        .as_ref()
        .map(|p| p.running && p.rpm > 0)
        .unwrap_or(false);

    let pool_state = pool_body.map(|body| {
        let on = pool_circuit
            .map(|c| circuit_state(c.circuit_id))
            .unwrap_or(false);
        BodyState {
            on,
            active: on && pump_flowing,
            temperature: body.current_temp,
            temperature_reliable: true,
            temperature_reason: None,
            last_reliable_temperature: None,
            last_reliable_temperature_at_unix_ms: None,
            setpoint: body.set_point,
            heat_mode: fmt_heat_mode(body.heat_mode),
            heating: fmt_heat_status(body.heat_status),
            heat_estimate: None,
            temperature_display: TemperatureDisplay {
                value: Some(body.current_temp),
                is_stale: false,
                stale_reason: None,
                last_reliable_at_unix_ms: None,
            },
            heat_estimate_display: HeatEstimateDisplay {
                state: "unavailable".to_string(),
                reason: Some("not-heating".to_string()),
                available_in_seconds: None,
                minutes_remaining: None,
                target_temperature: None,
            },
        }
    });

    let spa_state = spa_body.map(|body| {
        let on = spa_circuit
            .map(|c| circuit_state(c.circuit_id))
            .unwrap_or(false);
        let mut accessories = HashMap::new();
        for circ in &spa_accessory_circuits {
            let slug = slugify(&circ.name);
            accessories.insert(slug, circuit_state(circ.circuit_id));
        }
        SpaState {
            on,
            active: on && pump_flowing,
            temperature: body.current_temp,
            temperature_reliable: true,
            temperature_reason: None,
            last_reliable_temperature: None,
            last_reliable_temperature_at_unix_ms: None,
            setpoint: body.set_point,
            heat_mode: fmt_heat_mode(body.heat_mode),
            heating: fmt_heat_status(body.heat_status),
            heat_estimate: None,
            temperature_display: TemperatureDisplay {
                value: Some(body.current_temp),
                is_stale: false,
                stale_reason: None,
                last_reliable_at_unix_ms: None,
            },
            heat_estimate_display: HeatEstimateDisplay {
                state: "unavailable".to_string(),
                reason: Some("not-heating".to_string()),
                available_in_seconds: None,
                minutes_remaining: None,
                target_temperature: None,
            },
            spa_heat_progress: SpaHeatProgress {
                current_temp_f: body.current_temp,
                target_temp_f: body.set_point,
                ..SpaHeatProgress::default()
            },
            accessories,
        }
    });

    // ── Step 4: Lights ──────────────────────────────────────────────
    let light_state = light_circuit.map(|circ| LightState {
        on: circuit_state(circ.circuit_id),
        mode: input.light_mode.map(|m| m.to_string()),
        available_modes: AVAILABLE_LIGHT_MODES.to_vec(),
    });

    // ── Step 5: Auxiliaries ─────────────────────────────────────────
    let auxiliaries: Vec<AuxState> = aux_circuits
        .iter()
        .map(|circ| AuxState {
            id: slugify(&circ.name),
            name: circ.name.clone(),
            on: circuit_state(circ.circuit_id),
        })
        .collect();

    // ── Step 6: System ──────────────────────────────────────────────
    let system = SystemInfo {
        controller: match config.controller_type {
            1 => "IntelliTouch",
            2 => "EasyTouch",
            _ => "Unknown",
        }
        .to_string(),
        firmware: input.version.map(|v| v.to_string()),
        temp_unit: if config.is_celsius { "°C" } else { "°F" },
        air_temperature: status.air_temp,
        freeze_protection: status.freeze_mode,
        pool_spa_shared_pump: pool_spa_shared,
    };

    let goodnight_available = spa_state.as_ref().is_some_and(|s| s.on)
        || light_state.as_ref().is_some_and(|l| l.on);

    let pool_system = PoolSystem {
        pool: pool_state,
        spa: spa_state,
        lights: light_state,
        auxiliaries,
        pump: primary_pump,
        system,
        goodnight_available,
    };

    (pool_system, circuit_map)
}

// ─── Helpers ────────────────────────────────────────────────────────────

/// Convention-based association for Generic circuits.
fn associate_by_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if lower.contains("jet") || lower.contains("blower") || lower.contains("bubbl") {
        return Some("spa");
    }
    None
}

/// Turn a circuit name into a URL-safe slug.
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn fmt_heat_mode(m: i32) -> String {
    match m {
        0 => "off",
        1 => "solar",
        2 => "solar-preferred",
        3 => "heat-pump",
        _ => "unknown",
    }
    .to_string()
}

fn fmt_heat_status(s: i32) -> String {
    match s {
        0 => "off",
        1 => "solar",
        2 => "heater",
        3 => "both",
        _ => "unknown",
    }
    .to_string()
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::HEADER_SIZE;

    #[test]
    fn semantic_model_from_fixtures() {
        let status_data = include_bytes!("../../test-fixtures/status_response.bin");
        let config_data = include_bytes!("../../test-fixtures/controller_config_response.bin");
        let pump0_data = include_bytes!("../../test-fixtures/pump_status_0_response.bin");

        let status = parse_pool_status(&status_data[HEADER_SIZE..]).unwrap();
        let config = parse_controller_config(&config_data[HEADER_SIZE..]).unwrap();
        let pump0 = parse_pump_status(&pump0_data[HEADER_SIZE..]).unwrap();

        let mut pumps: Vec<Option<PumpStatus>> = vec![None; 8];
        pumps[0] = Some(pump0);

        let no_overrides: Vec<String> = vec![];
        let input = PoolSystemInput {
            status: &status,
            config: &config,
            pumps: &pumps,
            version: Some("POOL: 5.2 Build 738.0 Rel"),
            light_mode: Some("party"),
            spa_associations: &no_overrides,
        };

        let (system, map) = build_pool_system(&input);

        // Pool
        let pool = system.pool.as_ref().expect("should have pool");
        assert!(!pool.on);
        assert!(!pool.active); // off circuit can't be active
        assert_eq!(pool.temperature, 105);
        assert_eq!(pool.setpoint, 59);
        assert_eq!(pool.heat_mode, "heat-pump");

        // Spa with jets as accessory
        let spa = system.spa.as_ref().expect("should have spa");
        assert!(!spa.on);
        assert!(!spa.active);
        assert_eq!(spa.temperature, 103);
        assert_eq!(spa.setpoint, 104);
        assert_eq!(spa.accessories.get("jets"), Some(&false));

        // Lights with tracked mode
        let lights = system.lights.as_ref().expect("should have lights");
        assert!(!lights.on);
        assert_eq!(lights.mode, Some("party".to_string()));
        assert!(lights.available_modes.contains(&"caribbean"));

        // Auxiliaries — no circuit_id exposed
        assert!(system.auxiliaries.iter().any(|a| a.id == "floor_cleaner"));
        assert!(system.auxiliaries.iter().any(|a| a.id == "water_feature"));
        assert!(system.auxiliaries.iter().any(|a| a.id == "yard_light"));
        // Jets should NOT be an auxiliary
        assert!(!system.auxiliaries.iter().any(|a| a.id == "jets"));

        // System
        assert!(system.system.pool_spa_shared_pump);
        assert_eq!(system.system.controller, "IntelliTouch");

        // Circuit map — daemon uses this to resolve semantic IDs
        assert_eq!(map.resolve("pool"), Some(6));
        assert_eq!(map.resolve("spa"), Some(1));
        assert_eq!(map.resolve("jets"), Some(4));
        assert_eq!(map.resolve("lights"), Some(2));
        assert_eq!(map.resolve("water_feature"), Some(3));
        assert_eq!(map.resolve("yard_light"), Some(7));
        assert_eq!(map.resolve("floor_cleaner"), Some(5));

        // Body type resolution
        assert_eq!(CircuitMap::body_type("pool"), Some(0));
        assert_eq!(CircuitMap::body_type("spa"), Some(1));
        assert_eq!(CircuitMap::body_type("jets"), None);
    }
}
