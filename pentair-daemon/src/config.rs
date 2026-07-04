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
    pub apns: ApnsConfig,

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

    #[serde(default)]
    pub web: WebConfig,

    /// OpenWeather configuration for the pool-temperature predictor.
    #[serde(default)]
    pub weather: WeatherConfig,

    /// Predictive comfort scheduler (advisory / evaluation only). The feature is
    /// OFF unless `[comfort].windows` is non-empty. See
    /// `docs/2026-06-29-pool-comfort-scheduler-v1.md`.
    #[serde(default)]
    pub comfort: ComfortConfig,

    /// Gas-heater model parameters for the comfort scheduler.
    #[serde(default)]
    pub gasheater: GasHeaterConfig,

    /// Flat natural-gas price for the comfort scheduler's fuel-cost model.
    #[serde(default)]
    pub gas: GasConfig,

    /// Time-of-use electricity rates — used only to cost the circulation pump.
    #[serde(default)]
    pub rates: RatesConfig,
}

// ─── Comfort scheduler config (advisory / evaluation only) ──────────────────
//
// These sections feed the PURE `scheduler.rs` model. Nothing here actuates: the
// scheduler only computes and reports a recommended plan + projected cost. The
// `[comfort].actuate` flag below is RESERVED and INERT — no code in v1 reads it
// to take an action (see `comfort_enabled` / the read-only `/api/pool/heat-plan`
// handler). Leaving it `true` changes nothing.

/// `[comfort]` — comfort windows + the reserved (inert) actuate flag.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComfortConfig {
    /// RESERVED, NOT IMPLEMENTED in v1. Defaults to `false`. This flag is inert:
    /// no code path reads it to actuate. It exists only so a future version can
    /// opt in to actuation behind an explicit toggle. Setting it `true` today
    /// has no effect whatsoever.
    ///
    /// INTENTIONALLY UNREAD: the `dead_code` allow is the compiler-enforced
    /// proof of inertness — if any future code reads this field to actuate, the
    /// allow can be removed and the field becomes "used", making the change
    /// visible in review. Do not wire this to any command path in v1.
    #[serde(default)]
    #[allow(dead_code)]
    pub actuate: bool,
    /// Comfort windows. An empty list (the default) means the feature is OFF.
    #[serde(default)]
    pub windows: Vec<ComfortWindowConfig>,
    /// Local UTC offset in seconds (e.g. `-28800` for PST) used to resolve the
    /// local-wall-clock windows and rate periods. Defaults to `0` (UTC).
    #[serde(default)]
    pub utc_offset_seconds: i64,
    /// Slot length in hours for the discretized horizon (default 0.5h = 30min).
    #[serde(default = "default_comfort_slot_hours")]
    pub slot_hours: f64,
    /// Planning horizon in hours (default 48h).
    #[serde(default = "default_comfort_horizon_hours")]
    pub horizon_hours: f64,
    /// Constant setpoint (°F) the "dumb" baseline holds for the savings
    /// comparison. Defaults to the max window target when unset.
    #[serde(default)]
    pub baseline_setpoint_f: Option<f64>,
}

/// One comfort window: `{ days, start, end, target_f }` in local wall-clock.
#[derive(Debug, Clone, Deserialize)]
pub struct ComfortWindowConfig {
    /// 3-letter weekday abbreviations, e.g. `["Sat", "Sun"]`.
    #[serde(default)]
    pub days: Vec<String>,
    /// Window start, `"HH:MM"` local.
    pub start: String,
    /// Window end, `"HH:MM"` local (exclusive).
    pub end: String,
    /// Target water temperature (°F) to hold across the window.
    pub target_f: f64,
}

/// `[gasheater]` — single gas heater: delivered output + combustion efficiency +
/// circulation-pump draw. A gas heater's output is constant (no COP curve).
#[derive(Debug, Clone, Deserialize)]
pub struct GasHeaterConfig {
    #[serde(default = "default_rated_btu_per_hr")]
    pub rated_btu_per_hr: f64,
    /// Combustion efficiency (delivered BTU ÷ gas BTU consumed), e.g. 0.82.
    #[serde(default = "default_thermal_efficiency")]
    pub thermal_efficiency: f64,
    /// Circulation-pump electrical draw while heating (kW), e.g. 0.75.
    #[serde(default = "default_pump_kw")]
    pub pump_kw: f64,
}

impl Default for GasHeaterConfig {
    fn default() -> Self {
        Self {
            rated_btu_per_hr: default_rated_btu_per_hr(),
            thermal_efficiency: default_thermal_efficiency(),
            pump_kw: default_pump_kw(),
        }
    }
}

/// `[gas]` — flat natural-gas price (no intraday time-of-use).
#[derive(Debug, Clone, Deserialize)]
pub struct GasConfig {
    #[serde(default = "default_usd_per_therm")]
    pub usd_per_therm: f64,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            usd_per_therm: default_usd_per_therm(),
        }
    }
}

/// `[rates]` — time-of-use electricity. Flat `default_usd_per_kwh` when no
/// periods are configured.
#[derive(Debug, Clone, Deserialize)]
pub struct RatesConfig {
    #[serde(default)]
    pub periods: Vec<RatePeriodConfig>,
    #[serde(default = "default_usd_per_kwh")]
    pub default_usd_per_kwh: f64,
}

impl Default for RatesConfig {
    fn default() -> Self {
        Self {
            periods: Vec::new(),
            default_usd_per_kwh: default_usd_per_kwh(),
        }
    }
}

/// One TOU rate period: `{ days, start, end, usd_per_kwh }` in local wall-clock.
#[derive(Debug, Clone, Deserialize)]
pub struct RatePeriodConfig {
    #[serde(default)]
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
    pub usd_per_kwh: f64,
}

fn default_comfort_slot_hours() -> f64 {
    0.5
}
fn default_comfort_horizon_hours() -> f64 {
    48.0
}
fn default_rated_btu_per_hr() -> f64 {
    250_000.0
}
fn default_thermal_efficiency() -> f64 {
    0.82
}
fn default_pump_kw() -> f64 {
    0.75
}
fn default_usd_per_therm() -> f64 {
    1.80
}
fn default_usd_per_kwh() -> f64 {
    0.30
}

/// Everything the read-only `/api/pool/heat-plan` handler needs from config,
/// bundled so it can be cloned into the API `AppState` without dragging the
/// whole `Config`. Advisory only — none of these fields drive actuation.
#[derive(Debug, Clone, Default)]
pub struct ComfortPlanConfig {
    pub comfort: ComfortConfig,
    pub gasheater: GasHeaterConfig,
    pub gas: GasConfig,
    pub rates: RatesConfig,
    /// Pool volume (gallons) for the thermal-mass term, resolved from
    /// `[heating.pool]` at startup. `None` disables the plan (no mass known).
    pub pool_volume_gallons: Option<f64>,
}

impl ComfortConfig {
    /// The feature is enabled iff at least one comfort window is configured.
    /// (The reserved `actuate` flag deliberately plays NO role here — it is
    /// inert in v1.)
    pub fn enabled(&self) -> bool {
        !self.windows.is_empty()
    }
}

/// OpenWeather configuration for the pool-temperature predictor.
///
/// `latitude`/`longitude` live in the gitignored `pentair.toml`; the API key is
/// read ONLY from the `OPENWEATHER_API_KEY` env var and never appears here. A
/// missing key or `enabled = false` must never break startup.
#[derive(Debug, Clone, Deserialize)]
pub struct WeatherConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default = "default_weather_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            latitude: None,
            longitude: None,
            poll_interval_seconds: default_weather_poll_interval_seconds(),
        }
    }
}

fn default_weather_poll_interval_seconds() -> u64 {
    900
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FcmConfig {
    #[serde(default)]
    pub service_account: String,
    #[serde(default)]
    pub project_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApnsConfig {
    #[serde(default)]
    pub key_id: String,
    #[serde(default)]
    pub team_id: String,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub bundle_id: String,
    #[serde(default = "default_apns_environment")]
    pub environment: String,
}

fn default_apns_environment() -> String {
    "development".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebConfig {
    #[serde(default)]
    pub remote_domain: String,
    #[serde(default)]
    pub firebase: WebFirebaseConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebFirebaseConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub auth_domain: String,
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
    #[serde(default = "default_heating_enabled")]
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
    /// Covered-when-idle cooling model for the temperature predictor.
    #[serde(default)]
    pub cooling: CoolingConfig,
    /// Continuous thermal calibrator (advisory; spec §12).
    #[serde(default)]
    pub calibration: CalibrationConfig,
}

/// Cooling-model configuration for the pool-temperature predictor.
///
/// Closed-loop calibration fills and persists the fitted constants; the optional
/// seeds here only prime the first projection before any calibration has run.
#[derive(Debug, Clone, Deserialize)]
pub struct CoolingConfig {
    /// Hard cap on how far forward we project before reverting to "measured N
    /// ago". This cap alone is the cutoff (no tau-based clamp).
    #[serde(default = "default_max_projection_hours")]
    pub max_projection_hours: f64,
    /// Optional seed for the covered-idle relaxation time constant (hours).
    #[serde(default)]
    pub tau_covered_hours: Option<f64>,
    /// Optional Dalton evaporation base coefficient seed (°F/hour per kPa).
    #[serde(default)]
    pub evap_a: Option<f64>,
    /// Optional Dalton evaporation wind coefficient seed (°F/hour per kPa per mph).
    #[serde(default)]
    pub evap_b: Option<f64>,
    /// Optional solar heating-rate coefficient seed `g` (°F·hr⁻¹ per kW/m²).
    #[serde(default)]
    pub solar_gain_f: Option<f64>,
    /// Fraction of incident shortwave the cover passes into the water, in
    /// `[0, 1]`. A solar/heat-retention cover transmits most of it; the seed is
    /// high (~0.75) because live validation showed the pool warming under cover.
    #[serde(default = "default_cover_solar_transmission")]
    pub cover_solar_transmission: f64,
}

impl Default for CoolingConfig {
    fn default() -> Self {
        Self {
            max_projection_hours: default_max_projection_hours(),
            tau_covered_hours: None,
            evap_a: None,
            evap_b: None,
            solar_gain_f: None,
            cover_solar_transmission: default_cover_solar_transmission(),
        }
    }
}

fn default_max_projection_hours() -> f64 {
    12.0
}

fn default_cover_solar_transmission() -> f64 {
    0.75
}

/// `[heating.calibration]` — continuous thermal calibrator (spec
/// docs/2026-07-01-thermal-calibrator-v1.md §12). Advisory only; nothing here
/// actuates.
#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Rolling fit window (days of stored cooling intervals).
    #[serde(default = "default_calibration_window_days")]
    pub window_days: f64,
    /// Re-fit trigger: at least this many fresh intervals since the last fit.
    #[serde(default = "default_min_new_intervals")]
    pub min_new_intervals: usize,
    /// Re-fit rate limit: at most one fit per body per this many hours.
    #[serde(default = "default_refit_min_hours")]
    pub refit_min_hours: f64,
    /// Damping alpha for accepted fits: new = (1-a)*old + a*fit.
    #[serde(default = "default_damping_alpha")]
    pub damping_alpha: f64,
    /// Accept tolerance (°F) on the same-holdout MAE comparison.
    #[serde(default = "default_accept_tolerance_f")]
    pub accept_tolerance_f: f64,
    /// Re-fit trigger: rolling prediction MAE above this (°F) forces a fit.
    #[serde(default = "default_mae_drift_f")]
    pub mae_drift_f: f64,
    /// Exclusion-deadlock escape hatch (spec §9): trip when the exclusion rate
    /// over `exclusion_window_days` exceeds this fraction.
    #[serde(default = "default_exclusion_rate_threshold")]
    pub exclusion_rate_threshold: f64,
    #[serde(default = "default_exclusion_window_days")]
    pub exclusion_window_days: f64,
    /// Outlet-offset learning: a settled read must land within this many hours
    /// of a completed heating session to pair with it.
    #[serde(default = "default_offset_settle_window_hours")]
    pub offset_settle_window_hours: f64,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_days: default_calibration_window_days(),
            min_new_intervals: default_min_new_intervals(),
            refit_min_hours: default_refit_min_hours(),
            damping_alpha: default_damping_alpha(),
            accept_tolerance_f: default_accept_tolerance_f(),
            mae_drift_f: default_mae_drift_f(),
            exclusion_rate_threshold: default_exclusion_rate_threshold(),
            exclusion_window_days: default_exclusion_window_days(),
            offset_settle_window_hours: default_offset_settle_window_hours(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_calibration_window_days() -> f64 {
    14.0
}
fn default_min_new_intervals() -> usize {
    4
}
fn default_refit_min_hours() -> f64 {
    24.0
}
fn default_damping_alpha() -> f64 {
    0.3
}
fn default_accept_tolerance_f() -> f64 {
    0.15
}
fn default_mae_drift_f() -> f64 {
    1.5
}
fn default_exclusion_rate_threshold() -> f64 {
    0.5
}
fn default_exclusion_window_days() -> f64 {
    5.0
}
fn default_offset_settle_window_hours() -> f64 {
    6.0
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
fn default_heating_enabled() -> bool {
    true
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
            apns: Default::default(),
            heating: Default::default(),
            notifications: Default::default(),
            scenes: Default::default(),
            web: Default::default(),
            weather: Default::default(),
            comfort: Default::default(),
            gasheater: Default::default(),
            gas: Default::default(),
            rates: Default::default(),
        }
    }
}

impl Default for HeatingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_path: default_heating_history_path(),
            sample_window_minutes: default_sample_window_minutes(),
            minimum_runtime_minutes: default_minimum_runtime_minutes(),
            minimum_temp_rise_f: default_minimum_temp_rise_f(),
            shared_equipment_temp_warmup_seconds: default_shared_equipment_temp_warmup_seconds(),
            heater: Default::default(),
            pool: Default::default(),
            spa: Default::default(),
            cooling: Default::default(),
            calibration: Default::default(),
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

    #[test]
    fn web_config_defaults_to_empty() {
        let config: Config = toml::from_str("").expect("default config should deserialize");
        assert_eq!(config.web.remote_domain, "");
        assert_eq!(config.web.firebase.api_key, "");
        assert_eq!(config.web.firebase.auth_domain, "");
        assert_eq!(config.web.firebase.project_id, "");
    }

    #[test]
    fn web_config_full() {
        let config: Config = toml::from_str(
            r#"
            [web]
            remote_domain = "example.com"

            [web.firebase]
            api_key = "AIzaTest123"
            auth_domain = "myapp.firebaseapp.com"
            project_id = "myapp-12345"
            "#,
        )
        .expect("full web config should deserialize");
        assert_eq!(config.web.remote_domain, "example.com");
        assert_eq!(config.web.firebase.api_key, "AIzaTest123");
        assert_eq!(config.web.firebase.auth_domain, "myapp.firebaseapp.com");
        assert_eq!(config.web.firebase.project_id, "myapp-12345");
    }

    #[test]
    fn web_config_partial_domain_only() {
        let config: Config = toml::from_str(
            r#"
            [web]
            remote_domain = "example.com"
            "#,
        )
        .expect("partial web config should deserialize");
        assert_eq!(config.web.remote_domain, "example.com");
        assert_eq!(config.web.firebase.api_key, "");
        assert_eq!(config.web.firebase.auth_domain, "");
        assert_eq!(config.web.firebase.project_id, "");
    }

    #[test]
    fn comfort_disabled_by_default_and_actuate_false() {
        let config: Config = toml::from_str("").expect("default config should deserialize");
        assert!(!config.comfort.enabled(), "no windows => feature off");
        assert!(!config.comfort.actuate, "actuate defaults to false (reserved/inert)");
        assert!(config.comfort.windows.is_empty());
        // Gas-heater + gas + rate defaults match the spec.
        assert_eq!(config.gasheater.rated_btu_per_hr, 250_000.0);
        assert_eq!(config.gasheater.thermal_efficiency, 0.82);
        assert_eq!(config.gasheater.pump_kw, 0.75);
        assert_eq!(config.gas.usd_per_therm, 1.80);
        assert!(config.rates.periods.is_empty());
        assert_eq!(config.rates.default_usd_per_kwh, 0.30);
    }

    #[test]
    fn comfort_windows_enable_feature() {
        let config: Config = toml::from_str(
            r#"
            [comfort]
            actuate = true
            utc_offset_seconds = -28800

            [[comfort.windows]]
            days = ["Sat", "Sun"]
            start = "15:00"
            end = "20:00"
            target_f = 88.0

            [gasheater]
            rated_btu_per_hr = 300000
            thermal_efficiency = 0.85

            [gas]
            usd_per_therm = 1.95

            [rates]
            default_usd_per_kwh = 0.25
            [[rates.periods]]
            days = ["Mon", "Tue", "Wed", "Thu", "Fri"]
            start = "16:00"
            end = "21:00"
            usd_per_kwh = 0.55
            "#,
        )
        .expect("comfort config should deserialize");
        assert!(config.comfort.enabled());
        // actuate parses but is never acted upon (inert).
        assert!(config.comfort.actuate);
        assert_eq!(config.comfort.utc_offset_seconds, -28800);
        assert_eq!(config.comfort.windows.len(), 1);
        assert_eq!(config.comfort.windows[0].target_f, 88.0);
        assert_eq!(config.gasheater.rated_btu_per_hr, 300000.0);
        assert_eq!(config.gasheater.thermal_efficiency, 0.85);
        // pump_kw falls back to its default.
        assert_eq!(config.gasheater.pump_kw, 0.75);
        assert_eq!(config.gas.usd_per_therm, 1.95);
        assert_eq!(config.rates.periods.len(), 1);
        assert_eq!(config.rates.periods[0].usd_per_kwh, 0.55);
    }

    #[test]
    fn web_firebase_config_parses() {
        let config: Config = toml::from_str(
            r#"
            [web.firebase]
            api_key = "test-key"
            auth_domain = "test.firebaseapp.com"
            project_id = "test-project"
            "#,
        )
        .expect("firebase-only web config should deserialize");
        assert_eq!(config.web.remote_domain, "");
        assert_eq!(config.web.firebase.api_key, "test-key");
        assert_eq!(config.web.firebase.auth_domain, "test.firebaseapp.com");
        assert_eq!(config.web.firebase.project_id, "test-project");
    }

    #[test]
    fn calibration_config_defaults_and_parse() {
        // Absent section -> defaults.
        let config: Config = toml::from_str("adapter_host = \"h\"").expect("parse");
        let cal = &config.heating.calibration;
        assert!(cal.enabled);
        assert_eq!(cal.window_days, 14.0);
        assert_eq!(cal.min_new_intervals, 4);
        assert_eq!(cal.refit_min_hours, 24.0);
        assert_eq!(cal.damping_alpha, 0.3);
        assert_eq!(cal.accept_tolerance_f, 0.15);
        assert_eq!(cal.mae_drift_f, 1.5);
        assert_eq!(cal.exclusion_rate_threshold, 0.5);
        assert_eq!(cal.exclusion_window_days, 5.0);
        assert_eq!(cal.offset_settle_window_hours, 6.0);

        // Explicit section overrides.
        let config: Config = toml::from_str(
            "adapter_host = \"h\"\n[heating.calibration]\nenabled = false\nwindow_days = 7.0\n",
        )
        .expect("parse");
        assert!(!config.heating.calibration.enabled);
        assert_eq!(config.heating.calibration.window_days, 7.0);
    }
}
