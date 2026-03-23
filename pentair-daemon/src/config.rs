use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_adapter_host")]
    pub adapter_host: String,
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Override which circuits are spa accessories.
    /// By default, circuits named "jets", "blower", etc. are auto-detected.
    /// Use this to override if your circuit names don't match conventions.
    ///
    /// Example:
    /// ```toml
    /// [associations]
    /// spa = ["Bubbler", "Air Blower"]
    /// ```
    #[serde(default)]
    pub associations: Associations,

    #[serde(default)]
    pub fcm: FcmConfig,

    #[serde(default)]
    pub heating: HeatingConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FcmConfig {
    #[serde(default)]
    pub service_account: String,
    #[serde(default)]
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeatingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_heating_history_path")]
    pub history_path: String,
    #[serde(default = "default_sample_window_minutes")]
    pub sample_window_minutes: u64,
    #[serde(default = "default_minimum_runtime_minutes")]
    pub minimum_runtime_minutes: u64,
    #[serde(default = "default_minimum_temp_rise_f")]
    pub minimum_temp_rise_f: f64,
    #[serde(default)]
    pub heater: HeaterConfig,
    #[serde(default)]
    pub pool: BodyHeatingConfig,
    #[serde(default)]
    pub spa: BodyHeatingConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HeaterConfig {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub output_btu_per_hr: f64,
    #[serde(default)]
    pub efficiency: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BodyHeatingConfig {
    #[serde(default)]
    pub volume_gallons: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Associations {
    /// Circuit names to associate with the spa (as accessories like jets).
    #[serde(default)]
    pub spa: Vec<String>,
}

fn default_adapter_host() -> String {
    String::new()
}
fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_heating_history_path() -> String {
    "~/.pentair/heat-estimator.json".to_string()
}
fn default_sample_window_minutes() -> u64 {
    180
}
fn default_minimum_runtime_minutes() -> u64 {
    10
}
fn default_minimum_temp_rise_f() -> f64 {
    1.0
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&contents)?)
        } else {
            Ok(Self::default())
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            adapter_host: default_adapter_host(),
            bind: default_bind(),
            associations: Default::default(),
            fcm: Default::default(),
            heating: Default::default(),
        }
    }
}

impl Default for HeatingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            history_path: default_heating_history_path(),
            sample_window_minutes: default_sample_window_minutes(),
            minimum_runtime_minutes: default_minimum_runtime_minutes(),
            minimum_temp_rise_f: default_minimum_temp_rise_f(),
            heater: Default::default(),
            pool: Default::default(),
            spa: Default::default(),
        }
    }
}
