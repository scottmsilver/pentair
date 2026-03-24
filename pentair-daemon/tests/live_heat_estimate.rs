//! Live daemon ETA tests against a real ScreenLogic controller.
//!
//! These tests start a loopback-only daemon instance with heating estimation
//! enabled, drive the spa through the daemon HTTP API, and restore the
//! original state before exiting.
//!
//! Run with:
//!   cargo test --test live_heat_estimate -p pentair-daemon -- --ignored --test-threads=1 --nocapture
//!
//! Optional env overrides:
//!   PENTAIR_HOST=192.168.1.89
//!   PENTAIR_TEST_HEATER_KIND=heat-pump
//!   PENTAIR_TEST_HEATER_BTU_PER_HOUR=140000
//!   PENTAIR_TEST_POOL_GALLONS=16000
//!   PENTAIR_TEST_SPA_LENGTH_FT=8
//!   PENTAIR_TEST_SPA_WIDTH_FT=8
//!   PENTAIR_TEST_SPA_DEPTH_FT=4
//!   PENTAIR_TEST_SPA_SHAPE_FACTOR=1.0
//!   PENTAIR_TEST_SENSOR_WARMUP_SECONDS=120
//!   PENTAIR_TEST_ETA_OBSERVE_SECONDS=180

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::{Child, Command};

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    spa_max_setpoint: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiPoolSystem {
    pool: Option<ApiBodyState>,
    spa: Option<ApiSpaState>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiHeatEstimate {
    available: bool,
    minutes_remaining: Option<u32>,
    source: String,
    reason: String,
    observed_rate_per_hour: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiBodyState {
    on: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSpaState {
    on: bool,
    active: bool,
    temperature: i32,
    setpoint: i32,
    heat_mode: String,
    heating: String,
    heat_estimate: Option<ApiHeatEstimate>,
    accessories: HashMap<String, bool>,
}

#[derive(Debug, Clone)]
struct OriginalState {
    pool_on: bool,
    spa_on: bool,
    jets_on: bool,
    spa_setpoint: i32,
    spa_heat_mode: String,
}

#[tokio::test]
#[ignore]
async fn live_spa_heat_estimate_tracks_real_session() {
    let run_id = unique_run_id();
    let port = reserve_loopback_port();
    let config_path = std::env::temp_dir().join(format!("pentair-live-heat-{run_id}.toml"));
    let log_path = std::env::temp_dir().join(format!("pentair-live-heat-{run_id}.log"));
    let store_path = std::env::temp_dir().join(format!("pentair-live-heat-{run_id}.json"));
    let base_url = format!("http://127.0.0.1:{port}");

    write_test_config(&config_path, &store_path, port).expect("write test config");

    let mut daemon = spawn_daemon(&config_path, &log_path)
        .await
        .expect("spawn daemon");
    let http = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("http client");

    let test_result = async {
        wait_for_system(&http, &base_url, Duration::from_secs(30))
            .await
            .map_err(|e| format!("daemon never became ready: {e}"))?;

        let initial_system = get_system(&http, &base_url)
            .await
            .ok_or_else(|| "missing semantic pool system".to_string())?;
        let raw_config = get_raw_config(&http, &base_url)
            .await
            .ok_or_else(|| "missing raw config".to_string())?;

        let original = capture_original_state(&initial_system)?;
        let outcome = run_eta_observation(&http, &base_url, &initial_system, &raw_config).await;
        let restore = restore_original_state(&http, &base_url, &original).await;

        if let Err(error) = restore {
            return Err(format!("state restoration failed: {error}"));
        }

        outcome
    }
    .await;

    shutdown_daemon(&mut daemon).await;
    cleanup_temp_file(&config_path);
    cleanup_temp_file(&log_path);
    cleanup_temp_file(&store_path);

    if let Err(error) = test_result {
        panic!("{error}");
    }
}

async fn run_eta_observation(
    http: &Client,
    base_url: &str,
    initial_system: &ApiPoolSystem,
    raw_config: &RawConfig,
) -> Result<(), String> {
    let initial_spa = initial_system
        .spa
        .as_ref()
        .ok_or_else(|| "semantic system has no spa".to_string())?;

    let desired_mode = writable_heat_mode(&initial_spa.heat_mode)
        .unwrap_or("heat-pump")
        .to_string();
    let target_setpoint = (initial_spa.temperature + 1)
        .max(initial_spa.setpoint)
        .min(raw_config.spa_max_setpoint);

    if target_setpoint <= initial_spa.temperature {
        eprintln!(
            "[live_heat_estimate] skipping observation: spa is already at/above max setpoint (temp={}, setpoint={}, max={})",
            initial_spa.temperature, initial_spa.setpoint, raw_config.spa_max_setpoint
        );
        return Ok(());
    }

    post_json(
        http,
        &format!("{base_url}/api/spa/heat"),
        &json!({ "setpoint": target_setpoint, "mode": desired_mode }),
    )
    .await?;
    post_empty(http, &format!("{base_url}/api/spa/on")).await?;

    let sensor_warmup_seconds = env_u64("PENTAIR_TEST_SENSOR_WARMUP_SECONDS", 120);
    let configured_system = poll_system(
        http,
        base_url,
        Duration::from_secs(sensor_warmup_seconds + 60),
        "spa configured ETA",
        |system| {
            let Some(spa) = system.spa.as_ref() else {
                return false;
            };
            let Some(estimate) = spa.heat_estimate.as_ref() else {
                return false;
            };
            spa.on
                && spa.active
                && spa.temperature < target_setpoint
                && estimate.available
                && estimate.source == "configured"
                && estimate.minutes_remaining.unwrap_or_default() > 0
        },
    )
    .await?;

    let configured_spa = configured_system.spa.as_ref().expect("spa present");
    let initial_eta = configured_spa
        .heat_estimate
        .as_ref()
        .ok_or_else(|| "configured observation had no heat estimate".to_string())?;
    let initial_minutes = initial_eta
        .minutes_remaining
        .ok_or_else(|| "configured observation had no ETA minutes".to_string())?;
    let initial_temp = configured_spa.temperature;

    eprintln!(
        "[live_heat_estimate] initial configured ETA: {} min at {}° -> {}° ({})",
        initial_minutes,
        configured_spa.temperature,
        configured_spa.setpoint,
        configured_spa.heating
    );

    let observe_seconds = env_u64("PENTAIR_TEST_ETA_OBSERVE_SECONDS", 180);
    let observed_system = poll_system(
        http,
        base_url,
        Duration::from_secs(observe_seconds),
        "ETA progression",
        |system| {
            let Some(spa) = system.spa.as_ref() else {
                return false;
            };
            let Some(estimate) = spa.heat_estimate.as_ref() else {
                return false;
            };
            spa.temperature > initial_temp
                || estimate.observed_rate_per_hour.is_some()
                || matches!(estimate.source.as_str(), "observed" | "blended" | "learned")
        },
    )
    .await;

    match observed_system {
        Ok(system) => {
            let spa = system.spa.as_ref().expect("spa present");
            let estimate = spa
                .heat_estimate
                .as_ref()
                .ok_or_else(|| "observed state had no heat estimate".to_string())?;
            let later_minutes = estimate
                .minutes_remaining
                .ok_or_else(|| "observed state had no ETA minutes".to_string())?;

            assert!(
                later_minutes <= initial_minutes + 1,
                "ETA should not grow materially once we observe the session: initial={} later={} source={} temp={} observed_rate={:?}",
                initial_minutes,
                later_minutes,
                estimate.source,
                spa.temperature,
                estimate.observed_rate_per_hour,
            );
            eprintln!(
                "[live_heat_estimate] observed ETA progression: {} -> {} min, source={}, temp={}°",
                initial_minutes, later_minutes, estimate.source, spa.temperature
            );
            Ok(())
        }
        Err(timeout_error) => {
            let latest = get_system(http, base_url)
                .await
                .ok_or_else(|| format!("{timeout_error}; also failed to read final state"))?;
            let spa = latest
                .spa
                .as_ref()
                .ok_or_else(|| "final state missing spa".to_string())?;
            let estimate = spa
                .heat_estimate
                .as_ref()
                .ok_or_else(|| "final state missing heat estimate".to_string())?;

            assert!(
                estimate.available,
                "ETA disappeared during observation window: reason={} source={}",
                estimate.reason, estimate.source
            );
            eprintln!(
                "[live_heat_estimate] no observed temp rise within {}s; ETA stayed {} min from source={} ({})",
                observe_seconds,
                estimate.minutes_remaining.unwrap_or_default(),
                estimate.source,
                timeout_error
            );
            Ok(())
        }
    }
}

async fn restore_original_state(
    http: &Client,
    base_url: &str,
    original: &OriginalState,
) -> Result<(), String> {
    let mut heat_payload = serde_json::Map::new();
    heat_payload.insert("setpoint".to_string(), json!(original.spa_setpoint));
    if let Some(mode) = writable_heat_mode(&original.spa_heat_mode) {
        heat_payload.insert("mode".to_string(), json!(mode));
    }
    post_json(
        http,
        &format!("{base_url}/api/spa/heat"),
        &serde_json::Value::Object(heat_payload),
    )
    .await?;

    if original.spa_on {
        post_empty(http, &format!("{base_url}/api/spa/on")).await?;
        if original.jets_on {
            post_empty(http, &format!("{base_url}/api/spa/jets/on")).await?;
        } else {
            post_empty(http, &format!("{base_url}/api/spa/jets/off")).await?;
        }
    } else {
        post_empty(http, &format!("{base_url}/api/spa/off")).await?;
    }

    if original.pool_on {
        post_empty(http, &format!("{base_url}/api/pool/on")).await?;
    } else if !original.spa_on {
        post_empty(http, &format!("{base_url}/api/pool/off")).await?;
    }

    let restored = poll_system(
        http,
        base_url,
        Duration::from_secs(30),
        "state restoration",
        |system| matches_original_state(system, original),
    )
    .await?;

    assert!(
        matches_original_state(&restored, original),
        "restored state still mismatched: expected {:?}, got {:?}",
        original,
        restored.spa.as_ref().map(|spa| (
            restored.pool.as_ref().map(|pool| pool.on),
            spa.on,
            spa.accessories.get("jets").copied().unwrap_or(false)
        ))
    );
    Ok(())
}

fn matches_original_state(system: &ApiPoolSystem, original: &OriginalState) -> bool {
    let pool_matches = system.pool.as_ref().map(|pool| pool.on) == Some(original.pool_on);
    let spa = match system.spa.as_ref() {
        Some(spa) => spa,
        None => return false,
    };
    let jets_on = spa.accessories.get("jets").copied().unwrap_or(false);
    pool_matches
        && spa.on == original.spa_on
        && jets_on == original.jets_on
        && spa.setpoint == original.spa_setpoint
        && spa.heat_mode == original.spa_heat_mode
}

fn capture_original_state(system: &ApiPoolSystem) -> Result<OriginalState, String> {
    let spa = system
        .spa
        .as_ref()
        .ok_or_else(|| "semantic system has no spa".to_string())?;
    let pool_on = system.pool.as_ref().map(|pool| pool.on).unwrap_or(false);
    let jets_on = spa.accessories.get("jets").copied().unwrap_or(false);

    Ok(OriginalState {
        pool_on,
        spa_on: spa.on,
        jets_on,
        spa_setpoint: spa.setpoint,
        spa_heat_mode: spa.heat_mode.clone(),
    })
}

async fn wait_for_system(http: &Client, base_url: &str, timeout: Duration) -> Result<(), String> {
    poll_system(http, base_url, timeout, "daemon readiness", |_| true)
        .await
        .map(|_| ())
}

async fn poll_system(
    http: &Client,
    base_url: &str,
    timeout: Duration,
    description: &str,
    predicate: impl Fn(&ApiPoolSystem) -> bool,
) -> Result<ApiPoolSystem, String> {
    let start = tokio::time::Instant::now();
    let interval = Duration::from_secs(2);
    let mut last_system = None;

    loop {
        if let Some(system) = get_system(http, base_url).await {
            if predicate(&system) {
                return Ok(system);
            }
            last_system = Some(system);
        }

        if start.elapsed() >= timeout {
            return Err(format!(
                "timed out waiting for {description}; last state: {:?}",
                last_system
                    .as_ref()
                    .and_then(|system| system.spa.as_ref())
                    .map(|spa| (
                        &spa.on,
                        &spa.active,
                        &spa.temperature,
                        &spa.setpoint,
                        &spa.heating,
                        &spa.heat_estimate
                    ))
            ));
        }

        tokio::time::sleep(interval).await;
    }
}

async fn get_system(http: &Client, base_url: &str) -> Option<ApiPoolSystem> {
    let value: serde_json::Value = http
        .get(format!("{base_url}/api/pool"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    if value.get("error").is_some() {
        return None;
    }

    serde_json::from_value(value).ok()
}

async fn get_raw_config(http: &Client, base_url: &str) -> Option<RawConfig> {
    http.get(format!("{base_url}/api/raw/config"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()
}

async fn post_empty(http: &Client, url: &str) -> Result<(), String> {
    let response = http
        .post(url)
        .send()
        .await
        .map_err(|error| format!("POST {url} failed: {error}"))?;
    let api = response
        .error_for_status()
        .map_err(|error| format!("POST {url} returned error status: {error}"))?
        .json::<ApiResponse>()
        .await
        .map_err(|error| format!("POST {url} returned invalid JSON: {error}"))?;

    if api.ok {
        Ok(())
    } else {
        Err(api.error.unwrap_or_else(|| format!("POST {url} failed")))
    }
}

async fn post_json(http: &Client, url: &str, body: &serde_json::Value) -> Result<(), String> {
    let response = http
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|error| format!("POST {url} failed: {error}"))?;
    let api = response
        .error_for_status()
        .map_err(|error| format!("POST {url} returned error status: {error}"))?
        .json::<ApiResponse>()
        .await
        .map_err(|error| format!("POST {url} returned invalid JSON: {error}"))?;

    if api.ok {
        Ok(())
    } else {
        Err(api.error.unwrap_or_else(|| format!("POST {url} failed")))
    }
}

async fn spawn_daemon(config_path: &Path, log_path: &Path) -> Result<Child, String> {
    let log_file = File::create(log_path)
        .map_err(|error| format!("create log file {}: {error}", log_path.display()))?;
    let stderr_file = log_file
        .try_clone()
        .map_err(|error| format!("clone log file {}: {error}", log_path.display()))?;

    let mut command = Command::new(env!("CARGO_BIN_EXE_pentair-daemon"));
    command
        .env("PENTAIR_CONFIG", config_path)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file));

    command
        .spawn()
        .map_err(|error| format!("spawn daemon {}: {error}", config_path.display()))
}

async fn shutdown_daemon(child: &mut Child) {
    if child.id().is_some() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    }
}

fn write_test_config(config_path: &Path, store_path: &Path, port: u16) -> Result<(), String> {
    let mut contents = String::new();

    if let Ok(host) = std::env::var("PENTAIR_HOST") {
        contents.push_str(&format!("adapter_host = \"{}\"\n", host));
    } else {
        contents.push_str("adapter_host = \"\"\n");
    }
    contents.push_str(&format!("bind = \"127.0.0.1:{port}\"\n\n"));
    contents.push_str("[heating]\n");
    contents.push_str("enabled = true\n");
    contents.push_str(&format!("history_path = \"{}\"\n", store_path.display()));
    contents.push_str("sample_window_minutes = 180\n");
    contents.push_str("minimum_runtime_minutes = 0\n");
    contents.push_str("minimum_temp_rise_f = 0.0\n\n");
    contents.push_str(&format!(
        "shared_equipment_temp_warmup_seconds = {}\n\n",
        env_u64("PENTAIR_TEST_SENSOR_WARMUP_SECONDS", 120)
    ));
    contents.push_str("[heating.heater]\n");
    contents.push_str(&format!(
        "kind = \"{}\"\n",
        std::env::var("PENTAIR_TEST_HEATER_KIND").unwrap_or_else(|_| "heat-pump".to_string())
    ));
    contents.push_str(&format!(
        "output_btu_per_hr = {}\n",
        env_u64("PENTAIR_TEST_HEATER_BTU_PER_HOUR", 140_000)
    ));
    contents.push_str("efficiency = 1.0\n\n");
    contents.push_str("[heating.pool]\n");
    contents.push_str(&format!(
        "volume_gallons = {}\n\n",
        env_u64("PENTAIR_TEST_POOL_GALLONS", 16_000)
    ));
    contents.push_str("[heating.spa.dimensions]\n");
    contents.push_str(&format!(
        "length_ft = {}\n",
        env_f64("PENTAIR_TEST_SPA_LENGTH_FT", 8.0)
    ));
    contents.push_str(&format!(
        "width_ft = {}\n",
        env_f64("PENTAIR_TEST_SPA_WIDTH_FT", 8.0)
    ));
    contents.push_str(&format!(
        "depth_ft = {}\n",
        env_f64("PENTAIR_TEST_SPA_DEPTH_FT", 4.0)
    ));
    contents.push_str(&format!(
        "shape_factor = {}\n",
        env_f64("PENTAIR_TEST_SPA_SHAPE_FACTOR", 1.0)
    ));

    let mut file = File::create(config_path)
        .map_err(|error| format!("create config {}: {error}", config_path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("write config {}: {error}", config_path.display()))
}

fn reserve_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind loopback port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn writable_heat_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "off" => Some("off"),
        "solar" => Some("solar"),
        "solar-preferred" => Some("solar-preferred"),
        "heat-pump" | "heater" => Some("heat-pump"),
        _ => None,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

fn unique_run_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}-{now}", std::process::id())
}

fn cleanup_temp_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}
