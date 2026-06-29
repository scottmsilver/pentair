//! Live daemon pool-temperature-prediction test against a real ScreenLogic
//! controller, with a **mocked** OpenWeather endpoint.
//!
//! This mirrors `tests/live_heat_estimate.rs`: it starts a loopback-only daemon
//! instance pointed at a real controller, then asserts that an idle (covered)
//! body with a stale reading exposes a forward-projected `predicted_temperature`
//! with an honest `prediction_basis`.
//!
//! Weather is ALWAYS served by an in-process mock (a tiny local HTTP server
//! injected via `OPENWEATHER_BASE_URL`) so the test NEVER hits the live
//! OpenWeather API and needs no real API key. The only live dependency is the
//! pool controller itself.
//!
//! Gated behind `#[ignore]` AND a real controller: it skips (does not fail) when
//! `PENTAIR_HOST` is unset, because without a controller there is no
//! last-reliable anchor to project from.
//!
//! Run with:
//!   PENTAIR_HOST=192.168.1.89 \
//!     cargo test --test live_thermal_prediction -p pentair-daemon \
//!     -- --ignored --test-threads=1 --nocapture
//!
//! Optional env overrides:
//!   PENTAIR_TEST_WEATHER_LAT=37.3688
//!   PENTAIR_TEST_WEATHER_LON=-122.0363
//!   PENTAIR_TEST_WEATHER_AIR_F=58        (mock outdoor air temperature, °F)
//!   PENTAIR_TEST_PREDICTION_WAIT_SECONDS=60

use reqwest::Client;
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Deserialize)]
struct ApiPoolSystem {
    pool: Option<ApiBodyState>,
    spa: Option<ApiBodyState>,
    system: ApiSystemInfo,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSystemInfo {
    pool_spa_shared_pump: bool,
}

/// The prediction-relevant subset of a semantic body. Both pool and spa carry
/// the same additive prediction fields, so one shape covers both.
#[derive(Debug, Clone, Deserialize)]
struct ApiBodyState {
    on: bool,
    temperature: i32,
    temperature_reliable: bool,
    last_reliable_temperature: Option<i32>,
    predicted_temperature: Option<i32>,
    predicted_temperature_f_precise: Option<f64>,
    prediction_confidence: Option<String>,
    prediction_uncertainty_f: Option<f64>,
    prediction_as_of_unix_ms: Option<i64>,
    prediction_basis: Option<String>,
    temperature_display: ApiTemperatureDisplay,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiTemperatureDisplay {
    #[serde(default)]
    is_predicted: bool,
}

#[tokio::test]
#[ignore]
async fn live_idle_body_exposes_weather_prediction() {
    let Ok(_host) = std::env::var("PENTAIR_HOST") else {
        eprintln!(
            "[live_thermal_prediction] skipping: PENTAIR_HOST is unset, so there is no real \
             controller to provide a last-reliable anchor. Set PENTAIR_HOST to run."
        );
        return;
    };

    let run_id = unique_run_id();
    let port = reserve_loopback_port();
    let config_path = std::env::temp_dir().join(format!("pentair-live-predict-{run_id}.toml"));
    let log_path = std::env::temp_dir().join(format!("pentair-live-predict-{run_id}.log"));
    let store_path = std::env::temp_dir().join(format!("pentair-live-predict-{run_id}.json"));
    let base_url = format!("http://127.0.0.1:{port}");

    // Stand up the mock OpenWeather server BEFORE the daemon so its very first
    // poll succeeds. The daemon reaches it via OPENWEATHER_BASE_URL.
    let air_f = env_f64("PENTAIR_TEST_WEATHER_AIR_F", 58.0);
    let mock = spawn_mock_openweather(air_f).await;

    write_test_config(&config_path, &store_path, port).expect("write test config");

    let mut daemon = spawn_daemon(&config_path, &log_path, &mock.base_url)
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

        // Pick the body that is idle on a shared pump: that is the one whose
        // live reading is unreliable and which therefore gets a projection.
        let wait_seconds = env_u64("PENTAIR_TEST_PREDICTION_WAIT_SECONDS", 60);
        let system = poll_system(
            &http,
            &base_url,
            Duration::from_secs(wait_seconds),
            "weather projection on the idle body",
            |system| idle_body(system).is_some_and(|b| b.predicted_temperature.is_some()),
        )
        .await?;

        let body = idle_body(&system)
            .ok_or_else(|| "no idle/unreliable body present to predict".to_string())?;

        assert!(
            !body.temperature_reliable,
            "the projected body should have an unreliable live reading"
        );
        let predicted = body
            .predicted_temperature
            .ok_or_else(|| "idle body produced no predicted_temperature".to_string())?;
        let anchor = body
            .last_reliable_temperature
            .ok_or_else(|| "idle body produced no last-reliable anchor".to_string())?;
        let basis = body
            .prediction_basis
            .as_deref()
            .ok_or_else(|| "idle body produced no prediction_basis".to_string())?;

        assert!(
            matches!(basis, "projected-weather" | "projected-cooling-only"),
            "with fresh mock weather the basis should be a projection, got {basis:?}"
        );
        assert!(
            body.temperature_display.is_predicted,
            "temperature_display.is_predicted should drive the UI swap"
        );
        assert!(
            body.predicted_temperature_f_precise.is_some(),
            "a precise projection value should accompany the rounded one"
        );
        assert!(
            body.prediction_uncertainty_f.unwrap_or_default() > 0.0,
            "a non-zero ± band should be reported for a real gap"
        );
        assert!(
            body.prediction_as_of_unix_ms.is_some(),
            "the projection instant should be reported"
        );
        assert!(
            matches!(
                body.prediction_confidence.as_deref(),
                Some("high" | "medium" | "low")
            ),
            "a projecting body should report a non-none confidence: {:?}",
            body.prediction_confidence
        );

        // Physical sanity: the mock air is cooler than a heated pool, so the
        // projection should sit between the cool air and the anchor, and never
        // drift implausibly far from the last reliable reading.
        assert!(
            (predicted - anchor).abs() <= 15,
            "projection {predicted}° drifted too far from anchor {anchor}° for a short gap"
        );

        eprintln!(
            "[live_thermal_prediction] basis={basis} confidence={:?} predicted={predicted}° \
             anchor={anchor}° live={}° ±{:?}",
            body.prediction_confidence, body.temperature, body.prediction_uncertainty_f,
        );
        Ok::<(), String>(())
    }
    .await;

    shutdown_daemon(&mut daemon).await;
    mock.shutdown();
    cleanup_temp_file(&config_path);
    cleanup_temp_file(&log_path);
    cleanup_temp_file(&store_path);

    if let Err(error) = test_result {
        panic!("{error}");
    }
}

/// The idle body on a shared pump (or any body whose reading is unreliable):
/// the projection target.
fn idle_body(system: &ApiPoolSystem) -> Option<&ApiBodyState> {
    if system.system.pool_spa_shared_pump {
        // Whichever shared body is off has the stale reading.
        match (system.pool.as_ref(), system.spa.as_ref()) {
            (Some(pool), _) if !pool.on && !pool.temperature_reliable => Some(pool),
            (_, Some(spa)) if !spa.on && !spa.temperature_reliable => Some(spa),
            _ => None,
        }
    } else {
        system
            .pool
            .as_ref()
            .filter(|b| !b.temperature_reliable)
            .or_else(|| system.spa.as_ref().filter(|b| !b.temperature_reliable))
    }
}

// ─── Mock OpenWeather server ───────────────────────────────────────────────

struct MockWeather {
    base_url: String,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl MockWeather {
    fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Spin up a tiny local HTTP server that answers OpenWeather Current/Forecast
/// 2.5 requests with deterministic JSON. This is what keeps the test off the
/// live API entirely.
async fn spawn_mock_openweather(air_f: f64) -> MockWeather {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock weather port");
    let addr = listener.local_addr().expect("mock weather addr");
    let base_url = format!("http://{addr}");
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let Ok((mut stream, _)) = accepted else { continue };
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        let request = String::from_utf8_lossy(&buf[..n]);
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let body = if path.contains("/forecast") {
                            forecast_json(air_f)
                        } else {
                            current_json(air_f)
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.flush().await;
                    });
                }
            }
        }
    });

    MockWeather {
        base_url,
        shutdown: shutdown_tx,
    }
}

fn current_json(air_f: f64) -> String {
    // `dt` is "now" so the sample is treated as a current observation, not stale.
    let now_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "{{\"main\":{{\"temp\":{air_f},\"humidity\":55}},\
         \"wind\":{{\"speed\":6.0}},\"clouds\":{{\"all\":40}},\"dt\":{now_s}}}"
    )
}

fn forecast_json(air_f: f64) -> String {
    let now_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entries: Vec<String> = (1..=4)
        .map(|i| {
            let dt = now_s + i * 3 * 3600;
            format!(
                "{{\"main\":{{\"temp\":{air_f},\"humidity\":55}},\
                 \"wind\":{{\"speed\":6.0}},\"clouds\":{{\"all\":40}},\"dt\":{dt}}}"
            )
        })
        .collect();
    format!("{{\"list\":[{}]}}", entries.join(","))
}

// ─── Daemon harness (mirrors live_heat_estimate.rs) ─────────────────────────

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
    let mut last_seen = false;

    loop {
        if let Some(system) = get_system(http, base_url).await {
            last_seen = true;
            if predicate(&system) {
                return Ok(system);
            }
        }

        if start.elapsed() >= timeout {
            return Err(format!(
                "timed out waiting for {description} (reached daemon: {last_seen})"
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

async fn spawn_daemon(
    config_path: &Path,
    log_path: &Path,
    mock_base_url: &str,
) -> Result<Child, String> {
    let log_file = File::create(log_path)
        .map_err(|error| format!("create log file {}: {error}", log_path.display()))?;
    let stderr_file = log_file
        .try_clone()
        .map_err(|error| format!("clone log file {}: {error}", log_path.display()))?;

    let mut command = Command::new(env!("CARGO_BIN_EXE_pentair-daemon"));
    command
        .env("PENTAIR_CONFIG", config_path)
        // Mock weather: a dummy key (never used against the live API) plus the
        // local mock host. This is what keeps the test off the real OpenWeather.
        .env("OPENWEATHER_API_KEY", "test-mock-key")
        .env("OPENWEATHER_BASE_URL", mock_base_url)
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

    contents.push_str("[heating.heater]\n");
    contents.push_str("kind = \"heat-pump\"\n");
    contents.push_str("output_btu_per_hr = 140000\n");
    contents.push_str("efficiency = 1.0\n\n");

    contents.push_str("[heating.pool]\n");
    contents.push_str("volume_gallons = 16000\n\n");

    contents.push_str("[heating.cooling]\n");
    contents.push_str("max_projection_hours = 12\n\n");

    // Weather enabled, pointed at the mock host (set via OPENWEATHER_BASE_URL).
    // Poll quickly so the projection appears within the test window.
    contents.push_str("[weather]\n");
    contents.push_str("enabled = true\n");
    contents.push_str(&format!(
        "latitude = {}\n",
        env_f64("PENTAIR_TEST_WEATHER_LAT", 37.3688)
    ));
    contents.push_str(&format!(
        "longitude = {}\n",
        env_f64("PENTAIR_TEST_WEATHER_LON", -122.0363)
    ));
    contents.push_str("poll_interval_seconds = 2\n");

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
