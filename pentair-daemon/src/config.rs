use crate::scenes::SceneConfig;
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

    #[serde(default)]
    pub notifications: NotificationsConfig,

    /// Scene definitions for multi-command orchestration.
    ///
    /// Example:
    /// ```toml
    /// [[scenes]]
    /// name = "pool-party"
    /// label = "Pool Party"
    /// commands = [
    ///   { target = "spa", action = "on" },
    ///   { target = "jets", action = "on" },
    ///   { target = "lights", action = "mode", value = "caribbean" },
    /// ]
    /// ```
    #[serde(default)]
    pub scenes: Vec<SceneConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FcmConfig {
    #[serde(default)]
    pub service_account: String,
    #[serde(default)]
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default)]
    pub spa_heat: SpaHeatNotificationsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpaHeatNotificationsConfig {
    #[serde(default = "default_notification_enabled")]
    pub enabled: bool,
    #[serde(default = "default_notification_enabled")]
    pub heating_started: bool,
    #[serde(default = "default_notification_enabled")]
    pub estimate_ready: bool,
    #[serde(default = "default_notification_enabled")]
    pub halfway: bool,
    #[serde(default = "default_notification_enabled")]
    pub almost_ready: bool,
    #[serde(default = "default_notification_enabled")]
    pub at_temp: bool,
    #[serde(default = "default_notification_minimum_delta_f")]
    pub minimum_delta_f: f64,
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
    #[serde(default = "default_shared_equipment_temp_warmup_seconds")]
    pub shared_equipment_temp_warmup_seconds: u64,
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
    #[serde(default)]
    pub dimensions: Option<BodyDimensionsConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BodyDimensionsConfig {
    #[serde(default)]
    pub length_ft: Option<f64>,
    #[serde(default)]
    pub width_ft: Option<f64>,
    #[serde(default, alias = "depth_ft")]
    pub average_depth_ft: Option<f64>,
    #[serde(default = "default_shape_factor")]
    pub shape_factor: f64,
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
fn default_shared_equipment_temp_warmup_seconds() -> u64 {
    120
}
fn default_notification_enabled() -> bool {
    true
}
fn default_notification_minimum_delta_f() -> f64 {
    4.0
}
fn default_shape_factor() -> f64 {
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
            notifications: Default::default(),
            scenes: Default::default(),
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
            shared_equipment_temp_warmup_seconds: default_shared_equipment_temp_warmup_seconds(),
            heater: Default::default(),
            pool: Default::default(),
            spa: Default::default(),
        }
    }
}

impl Default for BodyDimensionsConfig {
    fn default() -> Self {
        Self {
            length_ft: None,
            width_ft: None,
            average_depth_ft: None,
            shape_factor: default_shape_factor(),
        }
    }
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            spa_heat: Default::default(),
        }
    }
}

impl Default for SpaHeatNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_notification_enabled(),
            heating_started: default_notification_enabled(),
            estimate_ready: default_notification_enabled(),
            halfway: default_notification_enabled(),
            almost_ready: default_notification_enabled(),
            at_temp: default_notification_enabled(),
            minimum_delta_f: default_notification_minimum_delta_f(),
        }
    }
}

impl BodyHeatingConfig {
    pub fn effective_volume_gallons(&self) -> Option<f64> {
        self.volume_gallons.or_else(|| {
            self.dimensions
                .as_ref()
                .and_then(BodyDimensionsConfig::volume_gallons)
        })
    }
}

impl BodyDimensionsConfig {
    pub fn volume_gallons(&self) -> Option<f64> {
        let length_ft = self.length_ft?;
        let width_ft = self.width_ft?;
        let average_depth_ft = self.average_depth_ft?;

        if length_ft <= 0.0 || width_ft <= 0.0 || average_depth_ft <= 0.0 {
            return None;
        }

        let shape_factor = self.shape_factor.max(0.0);
        let cubic_feet = length_ft * width_ft * average_depth_ft * shape_factor;
        let gallons = cubic_feet * 7.48;

        (gallons.is_finite() && gallons > 0.0).then_some(gallons)
    }
}

#[cfg(test)]
mod config_tests {
    use super::Config;

    #[test]
    fn notification_defaults_are_enabled() {
        let config: Config = toml::from_str("").expect("default config should deserialize");

        assert!(config.notifications.spa_heat.enabled);
        assert!(config.notifications.spa_heat.heating_started);
        assert!(config.notifications.spa_heat.estimate_ready);
        assert!(config.notifications.spa_heat.halfway);
        assert!(config.notifications.spa_heat.almost_ready);
        assert!(config.notifications.spa_heat.at_temp);
        assert_eq!(config.notifications.spa_heat.minimum_delta_f, 4.0);
    }

    #[test]
    fn explicit_notification_settings_override_defaults() {
        let config: Config = toml::from_str(
            r#"
            [notifications.spa_heat]
            enabled = false
            heating_started = false
            estimate_ready = false
            halfway = false
            almost_ready = false
            at_temp = true
            minimum_delta_f = 6.5
            "#,
        )
        .expect("explicit config should deserialize");

        assert!(!config.notifications.spa_heat.enabled);
        assert!(!config.notifications.spa_heat.heating_started);
        assert!(!config.notifications.spa_heat.estimate_ready);
        assert!(!config.notifications.spa_heat.halfway);
        assert!(!config.notifications.spa_heat.almost_ready);
        assert!(config.notifications.spa_heat.at_temp);
        assert_eq!(config.notifications.spa_heat.minimum_delta_f, 6.5);
    }

    #[test]
    fn config_without_scenes_has_empty_scenes() {
        let config: Config = toml::from_str("").expect("default config should deserialize");
        assert!(config.scenes.is_empty());
    }

    #[test]
    fn config_with_scenes_parses_correctly() {
        let config: Config = toml::from_str(
            r#"
            [[scenes]]
            name = "pool-party"
            label = "Pool Party"
            commands = [
                { target = "spa", action = "on" },
                { target = "jets", action = "on" },
                { target = "lights", action = "mode", value = "caribbean" },
            ]

            [[scenes]]
            name = "all-off"
            label = "All Off"
            commands = [
                { target = "spa", action = "off" },
                { target = "lights", action = "off" },
            ]
            "#,
        )
        .expect("scenes config should deserialize");

        assert_eq!(config.scenes.len(), 2);
        assert_eq!(config.scenes[0].name, "pool-party");
        assert_eq!(config.scenes[0].label, "Pool Party");
        assert_eq!(config.scenes[0].commands.len(), 3);
        assert_eq!(config.scenes[0].commands[2].value.as_deref(), Some("caribbean"));
        assert_eq!(config.scenes[1].name, "all-off");
        assert_eq!(config.scenes[1].commands.len(), 2);
    }
}
