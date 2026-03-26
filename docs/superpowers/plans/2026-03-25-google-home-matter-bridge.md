# Google Home Matter Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `pentair-matter`, a sidecar binary that exposes the Pentair pool system to Google Home via the Matter protocol.

**Architecture:** Standalone Rust binary consuming the daemon's REST API + WebSocket. Runs rs-matter on a dedicated embassy thread, tokio for HTTP/WS, connected via `std::sync::mpsc` channels. Three Matter endpoints: Spa (Thermostat, endpoint 1), Jets (OnOff, endpoint 2), Lights (ModeSelect, endpoint 3). Pool excluded — schedule-managed, not voice-controlled.

**Tech Stack:** Rust, rs-matter, embassy, tokio, reqwest, tokio-tungstenite, clap, serde, tracing

**Spec:** `docs/superpowers/specs/2026-03-25-google-home-matter-bridge-design.md`

---

## Strategy

Build all pure-logic modules first (config, daemon client, state cache, conversions). These are fully testable without rs-matter. Then integrate with rs-matter last, when the translation layer is solid and tested.

```
  Task 1: Scaffold crate
  Task 2: Config (CLI + TOML)           ← no rs-matter dependency
  Task 3: PoolSystem types + parsing     ← no rs-matter dependency
  Task 4: Temperature conversion         ← no rs-matter dependency
  Task 5: Light mode mapping             ← no rs-matter dependency
  Task 6: Daemon HTTP client             ← no rs-matter dependency
  Task 7: Daemon WebSocket subscriber    ← no rs-matter dependency
  Task 8: State cache + change detection ← no rs-matter dependency
  ──────────────────────────────────────
  Task 9: rs-matter spike               ← FIRST rs-matter code
  Task 10: Spa Thermostat endpoint       ← rs-matter integration
  Task 11: Jets OnOff endpoint
  Task 12: Lights ModeSelect endpoint
  Task 13: Bridge assembly + main loop
  Task 14: Integration tests with mock daemon
```

---

### Task 1: Scaffold pentair-matter crate

**Files:**
- Create: `pentair-matter/Cargo.toml`
- Create: `pentair-matter/src/main.rs`
- Modify: `Cargo.toml` (workspace root — add member)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "pentair-matter"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
reqwest = { version = "0.12", features = ["json"] }
tokio-tungstenite = "0.24"
futures-util = "0.3"
clap = { version = "4", features = ["derive"] }
toml = "0.8"
dirs = "5"
```

Note: `rs-matter` is NOT added yet. We build all pure-logic modules first.

- [ ] **Step 2: Create minimal main.rs**

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "pentair-matter", about = "Matter bridge for Pentair pool control")]
struct Cli {
    /// Daemon URL (e.g., http://localhost:8080)
    #[arg(long, env = "PENTAIR_DAEMON_URL")]
    daemon_url: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    tracing::info!("pentair-matter starting");
}
```

- [ ] **Step 3: Add to workspace**

Add `"pentair-matter"` to the `members` array in the root `Cargo.toml`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p pentair-matter`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```
feat(matter): scaffold pentair-matter sidecar crate
```

---

### Task 2: Config module (CLI args + TOML)

**Files:**
- Create: `pentair-matter/src/config.rs`
- Modify: `pentair-matter/src/main.rs`

- [ ] **Step 1: Write config tests**

Create `pentair-matter/src/config.rs`:

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub daemon_url: String,
    pub discriminator: u16,
    pub passcode: u32,
    pub fabric_path: PathBuf,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    daemon_url: Option<String>,
    discriminator: Option<u16>,
    passcode: Option<u32>,
    fabric_path: Option<String>,
}

impl Config {
    /// Load config: CLI args override TOML file override defaults.
    pub fn load(
        daemon_url: Option<String>,
        discriminator: Option<u16>,
        passcode: Option<u32>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let file_config = config_path
            .or_else(|| {
                dirs::home_dir().map(|h| h.join(".pentair").join("matter.toml"))
            })
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .and_then(|s| toml::from_str::<FileConfig>(&s).ok())
            .unwrap_or_default();

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        Config {
            daemon_url: daemon_url
                .or(file_config.daemon_url)
                .unwrap_or_else(|| "http://localhost:8080".to_string()),
            discriminator: discriminator
                .or(file_config.discriminator)
                .unwrap_or(3840),
            passcode: passcode
                .or(file_config.passcode)
                .unwrap_or(20202021),
            fabric_path: file_config
                .fabric_path
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".pentair").join("matter-fabrics")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_args_no_file() {
        let config = Config::load(None, None, None, Some(PathBuf::from("/nonexistent")));
        assert_eq!(config.daemon_url, "http://localhost:8080");
        assert_eq!(config.discriminator, 3840);
        assert_eq!(config.passcode, 20202021);
    }

    #[test]
    fn cli_args_override_defaults() {
        let config = Config::load(
            Some("http://10.0.0.5:9090".to_string()),
            Some(1234),
            Some(99999),
            Some(PathBuf::from("/nonexistent")),
        );
        assert_eq!(config.daemon_url, "http://10.0.0.5:9090");
        assert_eq!(config.discriminator, 1234);
        assert_eq!(config.passcode, 99999);
    }

    #[test]
    fn toml_file_parsed() {
        let dir = std::env::temp_dir().join("pentair-matter-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("matter.toml");
        std::fs::write(&path, r#"
            daemon_url = "http://192.168.1.50:8080"
            discriminator = 5555
            passcode = 12345678
        "#).unwrap();

        let config = Config::load(None, None, None, Some(path));
        assert_eq!(config.daemon_url, "http://192.168.1.50:8080");
        assert_eq!(config.discriminator, 5555);
        assert_eq!(config.passcode, 12345678);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter`
Expected: 3 tests pass.

- [ ] **Step 3: Wire into main.rs**

Update `main.rs` to use the config module and log loaded config:

```rust
mod config;

use clap::Parser;
use config::Config;

#[derive(Parser)]
#[command(name = "pentair-matter", about = "Matter bridge for Pentair pool control")]
struct Cli {
    #[arg(long, env = "PENTAIR_DAEMON_URL")]
    daemon_url: Option<String>,
    #[arg(long)]
    discriminator: Option<u16>,
    #[arg(long)]
    passcode: Option<u32>,
    #[arg(long)]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config = Config::load(cli.daemon_url, cli.discriminator, cli.passcode, cli.config);
    tracing::info!(daemon_url = %config.daemon_url, "pentair-matter starting");
}
```

- [ ] **Step 4: Verify build + tests**

Run: `cargo build -p pentair-matter && cargo test -p pentair-matter`
Expected: Compiles, 3 tests pass.

- [ ] **Step 5: Commit**

```
feat(matter): add config module with CLI + TOML + defaults
```

---

### Task 3: PoolSystem types + JSON parsing

**Files:**
- Create: `pentair-matter/src/pool_types.rs`

These are serde types matching the daemon's `GET /api/pool` response. Only the fields we need for Matter endpoints.

- [ ] **Step 1: Write types + parsing tests**

Create `pentair-matter/src/pool_types.rs`:

```rust
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
        // Controller with no spa or lights configured
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter`
Expected: 5 tests pass (3 config + 2 pool_types).

- [ ] **Step 3: Add module to main.rs**

Add `mod pool_types;` to `main.rs`.

- [ ] **Step 4: Commit**

```
feat(matter): add PoolSystem types for daemon API response parsing
```

---

### Task 4: Temperature conversion utilities

**Files:**
- Create: `pentair-matter/src/convert.rs`

- [ ] **Step 1: Write conversion functions + tests**

Create `pentair-matter/src/convert.rs`:

```rust
/// Convert Fahrenheit (integer) to Matter temperature (0.01°C units, i16).
pub fn fahrenheit_to_matter(f: i32) -> i16 {
    let celsius = (f as f64 - 32.0) * 5.0 / 9.0;
    (celsius * 100.0).round() as i16
}

/// Convert Matter temperature (0.01°C units, i16) to Fahrenheit (rounded integer).
pub fn matter_to_fahrenheit(matter: i16) -> i32 {
    let celsius = matter as f64 / 100.0;
    (celsius * 9.0 / 5.0 + 32.0).round() as i32
}

/// Map Pentair heat_mode string to Matter SystemMode.
/// Matter SystemMode: 0=Off, 1=Auto, 3=Cool, 4=Heat
pub fn pentair_heat_mode_to_matter(mode: &str) -> u8 {
    match mode {
        "off" => 0,    // Off
        _ => 4,        // Heat (solar, solar-preferred, heat-pump all map to "Heat")
    }
}

/// Map Matter SystemMode to Pentair heat_mode action.
/// Returns None for unsupported modes (Cool, Auto).
pub fn matter_to_pentair_heat_mode(system_mode: u8) -> Option<&'static str> {
    match system_mode {
        0 => Some("off"),
        4 => Some("heat"),  // Caller uses the device's configured heat mode
        _ => None,          // Cool (3), Auto (1) not supported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezing_point() {
        assert_eq!(fahrenheit_to_matter(32), 0);
        assert_eq!(matter_to_fahrenheit(0), 32);
    }

    #[test]
    fn spa_temperature_104f() {
        let matter = fahrenheit_to_matter(104);
        assert_eq!(matter, 4000); // 40.00°C
        assert_eq!(matter_to_fahrenheit(4000), 104);
    }

    #[test]
    fn zero_fahrenheit() {
        let matter = fahrenheit_to_matter(0);
        assert_eq!(matter, -1778); // -17.78°C
        assert_eq!(matter_to_fahrenheit(-1778), 0);
    }

    #[test]
    fn boiling_point() {
        assert_eq!(fahrenheit_to_matter(212), 10000); // 100.00°C
        assert_eq!(matter_to_fahrenheit(10000), 212);
    }

    #[test]
    fn round_trip_common_spa_temps() {
        for f in [98, 100, 102, 104, 106] {
            let matter = fahrenheit_to_matter(f);
            let back = matter_to_fahrenheit(matter);
            assert_eq!(back, f, "Round trip failed for {}°F", f);
        }
    }

    #[test]
    fn heat_mode_mapping() {
        assert_eq!(pentair_heat_mode_to_matter("off"), 0);
        assert_eq!(pentair_heat_mode_to_matter("heat-pump"), 4);
        assert_eq!(pentair_heat_mode_to_matter("solar"), 4);
        assert_eq!(pentair_heat_mode_to_matter("solar-preferred"), 4);
    }

    #[test]
    fn matter_mode_mapping() {
        assert_eq!(matter_to_pentair_heat_mode(0), Some("off"));
        assert_eq!(matter_to_pentair_heat_mode(4), Some("heat"));
        assert_eq!(matter_to_pentair_heat_mode(1), None); // Auto unsupported
        assert_eq!(matter_to_pentair_heat_mode(3), None); // Cool unsupported
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter -- convert`
Expected: 7 tests pass.

- [ ] **Step 3: Add module to main.rs**

Add `mod convert;` to `main.rs`.

- [ ] **Step 4: Commit**

```
feat(matter): add temperature + heat mode conversion utilities
```

---

### Task 5: Light mode mapping

**Files:**
- Create: `pentair-matter/src/light_modes.rs`

- [ ] **Step 1: Write light mode mapper + tests**

Create `pentair-matter/src/light_modes.rs`:

```rust
/// Non-selectable modes that should be filtered from the ModeSelect picker.
const FILTERED_MODES: &[&str] = &["off", "on", "set", "sync"];

/// Maps IntelliBrite mode names to stable numeric indices for Matter ModeSelect.
#[derive(Debug, Clone)]
pub struct LightModeMap {
    /// Ordered list of selectable mode names (filtered, stable order).
    modes: Vec<String>,
}

impl LightModeMap {
    /// Build the mapping from the daemon's `lights.available_modes` list.
    pub fn from_available_modes(available: &[String]) -> Self {
        let modes: Vec<String> = available
            .iter()
            .filter(|m| !FILTERED_MODES.contains(&m.as_str()))
            .cloned()
            .collect();
        Self { modes }
    }

    /// Get mode name by index. Returns None for invalid index.
    pub fn name_by_index(&self, index: u8) -> Option<&str> {
        self.modes.get(index as usize).map(|s| s.as_str())
    }

    /// Get index by mode name. Returns None if not in the selectable list.
    pub fn index_by_name(&self, name: &str) -> Option<u8> {
        self.modes.iter().position(|m| m == name).map(|i| i as u8)
    }

    /// Get current mode index from daemon's mode value. Returns None for null/unknown.
    pub fn current_mode_index(&self, mode: Option<&str>) -> Option<u8> {
        mode.and_then(|m| self.index_by_name(m))
    }

    /// Number of selectable modes.
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }

    /// Iterator over (index, name) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u8, &str)> {
        self.modes.iter().enumerate().map(|(i, m)| (i as u8, m.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon_modes() -> Vec<String> {
        vec![
            "off", "on", "set", "sync", "swim", "party", "romantic",
            "caribbean", "american", "sunset", "royal", "blue", "green",
            "red", "white", "purple",
        ].into_iter().map(String::from).collect()
    }

    #[test]
    fn filters_non_selectable_modes() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.len(), 12); // 16 total - 4 filtered
        assert!(map.index_by_name("off").is_none());
        assert!(map.index_by_name("on").is_none());
        assert!(map.index_by_name("set").is_none());
        assert!(map.index_by_name("sync").is_none());
    }

    #[test]
    fn stable_indices() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.name_by_index(0), Some("swim"));
        assert_eq!(map.name_by_index(1), Some("party"));
        assert_eq!(map.name_by_index(3), Some("caribbean"));
        assert_eq!(map.index_by_name("caribbean"), Some(3));
    }

    #[test]
    fn current_mode_null() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.current_mode_index(None), None);
    }

    #[test]
    fn current_mode_known() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.current_mode_index(Some("caribbean")), Some(3));
    }

    #[test]
    fn round_trip() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        for (idx, name) in map.iter() {
            assert_eq!(map.name_by_index(idx), Some(name));
            assert_eq!(map.index_by_name(name), Some(idx));
        }
    }

    #[test]
    fn invalid_index() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.name_by_index(255), None);
    }

    #[test]
    fn empty_modes() {
        let map = LightModeMap::from_available_modes(&[]);
        assert_eq!(map.len(), 0);
        assert_eq!(map.current_mode_index(Some("party")), None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter -- light_modes`
Expected: 7 tests pass.

- [ ] **Step 3: Add module to main.rs**

Add `mod light_modes;` to `main.rs`.

- [ ] **Step 4: Commit**

```
feat(matter): add light mode index mapping with filtering
```

---

### Task 6: Daemon HTTP client

**Files:**
- Create: `pentair-matter/src/daemon_client.rs`

- [ ] **Step 1: Write the daemon HTTP client**

Create `pentair-matter/src/daemon_client.rs`:

```rust
use crate::pool_types::PoolSystem;

#[derive(Clone)]
pub struct DaemonClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("daemon unreachable: {0}")]
    Unreachable(#[from] reqwest::Error),
    #[error("daemon returned error: {status} {body}")]
    ApiError { status: u16, body: String },
}

impl DaemonClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Fetch the full pool system state.
    pub async fn get_pool(&self) -> Result<PoolSystem, DaemonError> {
        let resp = self.http.get(format!("{}/api/pool", self.base_url)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DaemonError::ApiError { status, body });
        }
        Ok(resp.json().await?)
    }

    /// Send a POST command to the daemon. Returns Ok(()) on success.
    pub async fn post(&self, path: &str, body: Option<serde_json::Value>) -> Result<(), DaemonError> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.http.post(&url);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DaemonError::ApiError { status, body });
        }
        Ok(())
    }

    /// WebSocket URL for state subscriptions.
    pub fn ws_url(&self) -> String {
        let ws_base = self.base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        format!("{}/api/ws", ws_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_from_http() {
        let client = DaemonClient::new("http://localhost:8080");
        assert_eq!(client.ws_url(), "ws://localhost:8080/api/ws");
    }

    #[test]
    fn ws_url_strips_trailing_slash() {
        let client = DaemonClient::new("http://localhost:8080/");
        assert_eq!(client.ws_url(), "ws://localhost:8080/api/ws");
    }
}
```

- [ ] **Step 2: Add thiserror dependency**

Add `thiserror = { workspace = true }` to `pentair-matter/Cargo.toml`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p pentair-matter -- daemon_client`
Expected: 2 tests pass.

- [ ] **Step 4: Add module to main.rs**

Add `mod daemon_client;` to `main.rs`.

- [ ] **Step 5: Commit**

```
feat(matter): add daemon HTTP client with get_pool + post commands
```

---

### Task 7: Daemon WebSocket subscriber

**Files:**
- Create: `pentair-matter/src/ws_subscriber.rs`

- [ ] **Step 1: Write the WebSocket subscriber**

Create `pentair-matter/src/ws_subscriber.rs`:

```rust
use crate::pool_types::PoolSystem;
use futures_util::StreamExt;
use tokio::sync::watch;
use tokio_tungstenite::connect_async;

/// Subscribes to the daemon's WebSocket and publishes state updates
/// via a tokio watch channel.
pub async fn run_ws_subscriber(
    ws_url: String,
    state_tx: watch::Sender<Option<PoolSystem>>,
) {
    let mut backoff: u64 = 1;
    loop {
        tracing::info!(url = %ws_url, "connecting to daemon WebSocket");
        match connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("WebSocket connected");
                backoff = 1; // Reset backoff on successful connection
                let (_, mut read) = ws_stream.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                            match serde_json::from_str::<PoolSystem>(&text) {
                                Ok(pool) => {
                                    let _ = state_tx.send(Some(pool));
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to parse WS message");
                                }
                            }
                        }
                        Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                            tracing::info!("WebSocket closed by daemon");
                            break;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "WebSocket error");
                            break;
                        }
                        _ => {} // Ignore ping/pong/binary
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "WebSocket connection failed");
            }
        }

        // Reconnect with exponential backoff
        tracing::info!(backoff_secs = backoff, "reconnecting...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30); // 1, 2, 4, 8, 16, 30 cap
    }
}
```

- [ ] **Step 2: Run build**

Run: `cargo build -p pentair-matter`
Expected: Compiles. (No unit tests for this — it's async IO. Integration-tested with mock daemon later.)

- [ ] **Step 3: Add module to main.rs**

Add `mod ws_subscriber;` to `main.rs`.

- [ ] **Step 4: Commit**

```
feat(matter): add WebSocket subscriber for daemon state updates
```

---

### Task 8: State cache + change detection

**Files:**
- Create: `pentair-matter/src/state.rs`

- [ ] **Step 1: Write state cache with change detection**

Create `pentair-matter/src/state.rs`:

```rust
use crate::convert;
use crate::light_modes::LightModeMap;
use crate::pool_types::PoolSystem;

/// Cached Matter-relevant state derived from PoolSystem.
/// Used to detect changes and push only updated attributes to Matter.
#[derive(Debug, Clone, PartialEq)]
pub struct MatterState {
    /// Whether each device is present (Some in daemon response). Maps to Reachable attribute.
    pub spa_reachable: bool,
    pub lights_reachable: bool,
    pub spa_on: bool,
    pub spa_temp_matter: i16,       // 0.01°C
    pub spa_setpoint_matter: i16,   // 0.01°C
    pub spa_system_mode: u8,        // Matter SystemMode
    pub jets_on: bool,
    pub lights_on: bool,
    pub light_mode_index: Option<u8>,
}

impl MatterState {
    pub fn from_pool_system(ps: &PoolSystem, mode_map: &LightModeMap) -> Self {
        Self {
            spa_reachable: ps.spa.is_some(),
            lights_reachable: ps.lights.is_some(),
            spa_on: ps.spa.as_ref().map(|s| s.active).unwrap_or(false),
            spa_temp_matter: ps.spa.as_ref().map(|s| convert::fahrenheit_to_matter(s.temperature)).unwrap_or(0),
            spa_setpoint_matter: ps.spa.as_ref().map(|s| convert::fahrenheit_to_matter(s.setpoint)).unwrap_or(0),
            spa_system_mode: ps.spa.as_ref().map(|s| convert::pentair_heat_mode_to_matter(&s.heat_mode)).unwrap_or(0),
            jets_on: ps.spa.as_ref().and_then(|s| s.accessories.get("jets").copied()).unwrap_or(false),
            lights_on: ps.lights.as_ref().map(|l| l.on).unwrap_or(false),
            light_mode_index: ps.lights.as_ref().and_then(|l| mode_map.current_mode_index(l.mode.as_deref())),
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
            pool: Body {
                on: false, active: false, temperature: 82, setpoint: 59,
                heat_mode: "off".to_string(), heating: "off".to_string(),
            },
            spa: SpaBody {
                on: spa_active, active: spa_active, temperature: spa_temp, setpoint: spa_setpoint,
                heat_mode: "heat-pump".to_string(), heating: if spa_active { "heater" } else { "off" }.to_string(),
                accessories: if jets { HashMap::from([("jets".to_string(), true)]) } else { HashMap::new() },
            },
            lights: Lights {
                on: true, mode: Some("caribbean".to_string()),
                available_modes: vec!["off","on","set","sync","swim","party","romantic","caribbean"].into_iter().map(String::from).collect(),
            },
            system: System { pool_spa_shared_pump: true },
        }
    }

    #[test]
    fn converts_state_correctly() {
        let ps = make_pool_system(104, 104, true, true);
        let mode_map = LightModeMap::from_available_modes(&ps.lights.available_modes);
        let state = MatterState::from_pool_system(&ps, &mode_map);

        assert!(state.spa_on);
        assert_eq!(state.spa_temp_matter, 4000); // 104°F = 40.00°C
        assert_eq!(state.spa_setpoint_matter, 4000);
        assert_eq!(state.spa_system_mode, 4); // Heat
        assert!(state.jets_on);
        assert!(state.lights_on);
        assert_eq!(state.light_mode_index, Some(3)); // caribbean = index 3 after filtering
    }

    #[test]
    fn detects_change() {
        let ps1 = make_pool_system(103, 104, true, false);
        let ps2 = make_pool_system(104, 104, true, true);
        let mode_map = LightModeMap::from_available_modes(&ps1.lights.available_modes);
        let state1 = MatterState::from_pool_system(&ps1, &mode_map);
        let state2 = MatterState::from_pool_system(&ps2, &mode_map);

        assert_ne!(state1, state2); // Temperature and jets changed
        assert_ne!(state1.spa_temp_matter, state2.spa_temp_matter);
        assert_ne!(state1.jets_on, state2.jets_on);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter -- state`
Expected: 2 tests pass.

- [ ] **Step 3: Add module to main.rs**

Add `mod state;` to `main.rs`.

- [ ] **Step 4: Commit**

```
feat(matter): add state cache with change detection for Matter attributes
```

---

### Task 9: rs-matter spike — validate the crate works on Linux

**This is the GO/NO-GO gate.** If rs-matter doesn't work for our use case, the fallback is documented in the spec.

**Files:**
- Modify: `pentair-matter/Cargo.toml` (add rs-matter)
- Create: `pentair-matter/src/matter_bridge.rs`

- [ ] **Step 1: Add rs-matter dependency**

Add to `pentair-matter/Cargo.toml`:
```toml
rs-matter = { git = "https://github.com/project-chip/rs-matter", features = ["std"] }
rs-matter-stack = { git = "https://github.com/project-chip/rs-matter", features = ["std"] }
```

Note: Use git dependency since crates.io releases may be outdated. Pin to a specific commit after the spike succeeds.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p pentair-matter`

If this fails due to dependency conflicts or missing features, document the error and evaluate fallbacks (separate process, different Matter library). This is the spike's first gate.

- [ ] **Step 3: Write minimal Matter bridge initialization**

Create `pentair-matter/src/matter_bridge.rs` with a minimal bridge that:
1. Creates a Matter stack
2. Registers one OnOff endpoint (simplest possible)
3. Starts mDNS advertisement
4. Prints the pairing code

The exact API depends on rs-matter's current interface. This step is exploratory — the code will evolve. Write what compiles and runs.

- [ ] **Step 4: Test manually**

Run: `cargo run -p pentair-matter -- --daemon-url http://localhost:8080`
Expected:
- Prints a pairing code / QR to terminal
- `_matter._tcp` appears on the network (verify with `avahi-browse -a` or `dns-sd -B _matter._tcp`)

- [ ] **Step 5: Test commissioning from Google Home**

Open Google Home app → "Set up device" → "Matter device" → enter the pairing code.
Expected: Device appears in Google Home (even if it does nothing yet).

If this fails: document the error. This determines whether to proceed with rs-matter or pursue the fallback.

- [ ] **Step 6: Commit**

```
feat(matter): rs-matter spike — bridge init + mDNS + commissioning
```

**GO/NO-GO:** If steps 2-5 all succeed, proceed to Task 10. If any fail and can't be resolved within ~2 hours, evaluate the fallback (separate process using the C++ Matter SDK, or matterbridge in TypeScript calling the REST API).

---

### Task 10: Spa Thermostat endpoint

**Files:**
- Modify: `pentair-matter/src/matter_bridge.rs`

This task depends heavily on rs-matter's API shape discovered during the spike. The logic is:

- [ ] **Step 1: Register Thermostat endpoint at ID 1**

Add a Thermostat device type endpoint to the bridge with:
- OnOff cluster (read: `spa_on`, write: POST spa on/off)
- Thermostat cluster (LocalTemperature read, OccupiedHeatingSetpoint r/w, SystemMode r/w)

Use the `MatterState` struct for read values and `DaemonClient` for write commands.

- [ ] **Step 2: Wire state updates**

When the WebSocket subscriber receives a new `PoolSystem`:
1. Build `MatterState` from the new snapshot
2. Compare with previous `MatterState`
3. If spa attributes changed, push updated attribute reports to Matter subscribers

- [ ] **Step 3: Wire command handlers**

When Matter sends an OnOff or Thermostat command:
1. Map to the appropriate `DaemonClient::post()` call
2. On success, let the WS state update flow naturally
3. On failure, report error to Matter

- [ ] **Step 4: Manual test**

With the daemon running:
- "Hey Google, turn on the spa" → verify spa circuit activates
- "Hey Google, set spa to 104" → verify setpoint changes
- Change spa temp from Android app → verify Google Home shows updated temp

- [ ] **Step 5: Commit**

```
feat(matter): spa thermostat endpoint with bidirectional state sync
```

---

### Task 11: Jets OnOff endpoint

**Files:**
- Modify: `pentair-matter/src/matter_bridge.rs`

- [ ] **Step 1: Register Jets OnOff endpoint at ID 2**

OnOff cluster: read `jets_on`, write POST jets on/off. Jets auto-enables spa via the daemon's existing smart behavior.

- [ ] **Step 2: Wire state updates**

Same pattern as spa — compare MatterState, push changed attributes.

- [ ] **Step 3: Manual test**

- "Hey Google, turn on the jets" → jets on (spa auto-enables)
- "Hey Google, turn off the jets" → jets off

- [ ] **Step 4: Commit**

```
feat(matter): jets OnOff endpoint
```

---

### Task 12: Lights ModeSelect endpoint

**Files:**
- Modify: `pentair-matter/src/matter_bridge.rs`

- [ ] **Step 1: Register Lights endpoint at ID 3**

OnOff cluster: read `lights_on`, write POST lights on/off.
ModeSelect cluster: SupportedModes from `LightModeMap`, CurrentMode from state, ChangeToMode → POST lights mode.

Note: If rs-matter doesn't support ModeSelect cluster, fall back to OnOff-only and log a warning. Track ModeSelect as a TODO for when rs-matter adds support.

- [ ] **Step 2: Wire state updates**

Push light on/off and mode index changes.

- [ ] **Step 3: Manual test**

- "Hey Google, turn on pool lights" → lights on
- "Hey Google, set lights to caribbean" → mode changes (if ModeSelect supported)
- Verify mode picker appears in Google Home app

- [ ] **Step 4: Commit**

```
feat(matter): lights endpoint with ModeSelect for IntelliBrite modes
```

---

### Task 13: Bridge assembly + main loop

**Files:**
- Modify: `pentair-matter/src/main.rs`

- [ ] **Step 1: Assemble the full main loop**

Wire everything together in `main.rs`:

```rust
// 1. Load config
// 2. Create DaemonClient
// 3. Fetch initial state via GET /api/pool
// 4. Build LightModeMap from available_modes
// 5. Start WebSocket subscriber (tokio::spawn)
// 6. Start Matter bridge on dedicated thread (std::thread::spawn)
// 7. Bridge loop: read state from watch channel, push to Matter; read commands from Matter, dispatch via DaemonClient
// 8. Graceful shutdown on SIGTERM/SIGINT
```

- [ ] **Step 2: Add graceful shutdown**

Handle `tokio::signal::ctrl_c()` to cleanly stop the Matter thread and WebSocket subscriber.

- [ ] **Step 3: Test full flow**

Run with daemon:
```
cargo run -p pentair-matter -- --daemon-url http://localhost:8080
```

Verify:
- Pairing code printed
- Commission from Google Home
- All 3 device endpoints visible (Spa, Jets, Lights)
- Voice commands work
- State sync works
- Daemon restart → sidecar reconnects
- Sidecar restart → Google Home recognizes without re-pairing

- [ ] **Step 4: Commit**

```
feat(matter): assemble full bridge with state sync + command dispatch
```

---

### Task 14: Integration tests with mock daemon

**Files:**
- Create: `pentair-matter/tests/integration.rs`

- [ ] **Step 1: Write integration tests using a mock HTTP server**

Use `axum` (already in workspace) to spin up a mock daemon that returns canned `PoolSystem` JSON and records POST commands.

Test:
1. `DaemonClient::get_pool()` parses mock response correctly
2. `DaemonClient::post("/api/spa/on", None)` sends correct request
3. Full state → MatterState pipeline: mock daemon → WS message → state cache → correct MatterState

- [ ] **Step 2: Run tests**

Run: `cargo test -p pentair-matter`
Expected: All unit + integration tests pass.

- [ ] **Step 3: Verify workspace still passes**

Run: `cargo test --workspace`
Expected: All tests pass (existing + new).

- [ ] **Step 4: Commit**

```
test(matter): add integration tests with mock daemon
```

---

## Post-Implementation

After all tasks complete:
1. Update `README.md` with Matter bridge setup instructions
2. Update `CLAUDE.md` with pentair-matter crate description and build commands
3. Run `cargo test --workspace` to verify nothing broke
