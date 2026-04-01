//! Live control tests against a running pentair-daemon.
//!
//! These tests verify that each web UI control correctly drives the pool
//! hardware via the daemon API, and that state is restored afterwards.
//! Requires a running daemon (the tests talk to it, not spawn their own,
//! because the ScreenLogic adapter only allows one TCP client).
//!
//! Run with:
//!   cargo test --test live_controls -p pentair-daemon -- --ignored --test-threads=1 --nocapture
//!
//! Optional:
//!   PENTAIR_DAEMON_URL=http://127.0.0.1:8080  (default)

use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

fn daemon_url() -> String {
    std::env::var("PENTAIR_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

// ── Deserialization ────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct PoolState {
    pool: Option<BodyState>,
    spa: Option<SpaState>,
    lights: Option<LightsState>,
    #[allow(dead_code)]
    auxiliaries: Vec<AuxState>,
    goodnight_available: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct BodyState {
    on: bool,
    temperature: i32,
    setpoint: i32,
    heating: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SpaState {
    on: bool,
    #[allow(dead_code)]
    temperature: i32,
    setpoint: i32,
    #[allow(dead_code)]
    heating: String,
    accessories: HashMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct LightsState {
    on: bool,
    mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct AuxState {
    id: String,
    name: String,
    on: bool,
}

// ── Helpers ────────────────────────────────────────

async fn get_state(client: &Client) -> PoolState {
    let base = daemon_url();
    let resp = client
        .get(&format!("{base}/api/pool"))
        .send()
        .await
        .expect("GET /api/pool");
    let status = resp.status();
    let text = resp.text().await.expect("read body");
    serde_json::from_str::<PoolState>(&text)
        .unwrap_or_else(|e| panic!("parse pool state (status {status}): {e}\nbody: {text}"))
}

async fn post_ok(path: &str, body: Option<Value>, client: &Client) {
    let base = daemon_url();
    let mut req = client.post(&format!("{base}{path}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap_or_else(|e| panic!("POST {path}: {e}"));
    assert!(resp.status().is_success(), "POST {path} returned {}", resp.status());
}

/// Wait for hardware to settle after a command.
async fn settle() {
    sleep(Duration::from_secs(2)).await;
}

// ── Snapshot / Restore ─────────────────────────────

#[derive(Debug)]
struct Snapshot {
    spa_on: bool,
    spa_jets: bool,
    spa_setpoint: i32,
    lights_on: bool,
    lights_mode: Option<String>,
}

async fn snapshot(client: &Client) -> Snapshot {
    let s = get_state(client).await;
    let spa = s.spa.as_ref().expect("spa");
    Snapshot {
        spa_on: spa.on,
        spa_jets: *spa.accessories.get("jets").unwrap_or(&false),
        spa_setpoint: spa.setpoint,
        lights_on: s.lights.as_ref().map_or(false, |l| l.on),
        lights_mode: s.lights.as_ref().and_then(|l| l.mode.clone()),
    }
}

async fn restore(snap: &Snapshot, client: &Client) {
    println!("  restoring: {snap:?}");
    if snap.spa_jets {
        post_ok("/api/spa/jets/on", None, client).await;
    } else if snap.spa_on {
        post_ok("/api/spa/on", None, client).await;
        post_ok("/api/spa/jets/off", None, client).await;
    } else {
        post_ok("/api/spa/off", None, client).await;
    }
    post_ok("/api/spa/heat", Some(json!({"setpoint": snap.spa_setpoint})), client).await;
    if snap.lights_on {
        if let Some(mode) = &snap.lights_mode {
            post_ok("/api/lights/mode", Some(json!({"mode": mode})), client).await;
        }
    } else {
        post_ok("/api/lights/off", None, client).await;
    }
    settle().await;
    println!("  restore complete");
}

// ── Tests ──────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn spa_on_off_cycle() {
    let client = Client::new();
    let snap = snapshot(&client).await;
    println!("snapshot: {snap:?}");

    // Start from off
    post_ok("/api/spa/off", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert!(!s.spa.unwrap().on, "spa should be off");

    // Turn on
    post_ok("/api/spa/on", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert!(s.spa.unwrap().on, "spa should be on");

    // Turn off
    post_ok("/api/spa/off", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert!(!s.spa.unwrap().on, "spa should be off");

    restore(&snap, &client).await;
}

#[tokio::test]
#[ignore]
async fn jets_on_off_cycle() {
    let client = Client::new();
    let snap = snapshot(&client).await;

    // Spa on first
    post_ok("/api/spa/on", None, &client).await;
    settle().await;

    // Jets on
    post_ok("/api/spa/jets/on", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    let spa = s.spa.unwrap();
    assert!(spa.on, "spa on");
    assert_eq!(spa.accessories.get("jets"), Some(&true), "jets on");

    // Jets off
    post_ok("/api/spa/jets/off", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    let spa = s.spa.unwrap();
    assert!(spa.on, "spa still on");
    assert_eq!(spa.accessories.get("jets"), Some(&false), "jets off");

    restore(&snap, &client).await;
}

#[tokio::test]
#[ignore]
async fn lights_mode_cycle() {
    let client = Client::new();
    let snap = snapshot(&client).await;

    // Set caribbean (mode + on are separate calls)
    post_ok("/api/lights/mode", Some(json!({"mode": "caribbean"})), &client).await;
    post_ok("/api/lights/on", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    let l = s.lights.unwrap();
    assert!(l.on, "lights on");
    assert_eq!(l.mode.as_deref(), Some("caribbean"));

    // Switch to blue
    post_ok("/api/lights/mode", Some(json!({"mode": "blue"})), &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert_eq!(s.lights.unwrap().mode.as_deref(), Some("blue"));

    // Off
    post_ok("/api/lights/off", None, &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert!(!s.lights.unwrap().on, "lights off");

    restore(&snap, &client).await;
}

#[tokio::test]
#[ignore]
async fn setpoint_change_and_restore() {
    let client = Client::new();
    let snap = snapshot(&client).await;
    let original = snap.spa_setpoint;
    let test_val = if original >= 100 { original - 2 } else { original + 2 };

    post_ok("/api/spa/heat", Some(json!({"setpoint": test_val})), &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert_eq!(s.spa.unwrap().setpoint, test_val, "setpoint changed to {test_val}");

    // Restore
    post_ok("/api/spa/heat", Some(json!({"setpoint": original})), &client).await;
    settle().await;
    let s = get_state(&client).await;
    assert_eq!(s.spa.unwrap().setpoint, original, "setpoint restored to {original}");
}

#[tokio::test]
#[ignore]
async fn goodnight_turns_everything_off() {
    let client = Client::new();
    let snap = snapshot(&client).await;

    // Turn things on (mode + on are separate for lights)
    post_ok("/api/spa/on", None, &client).await;
    post_ok("/api/lights/mode", Some(json!({"mode": "party"})), &client).await;
    post_ok("/api/lights/on", None, &client).await;
    settle().await;

    let s = get_state(&client).await;
    assert!(s.spa.as_ref().unwrap().on, "spa on before goodnight");
    assert!(s.lights.as_ref().unwrap().on, "lights on before goodnight");

    // Goodnight
    post_ok("/api/goodnight", None, &client).await;
    sleep(Duration::from_secs(3)).await;

    let s = get_state(&client).await;
    assert!(!s.spa.as_ref().unwrap().on, "spa off after goodnight");
    assert!(!s.lights.as_ref().unwrap().on, "lights off after goodnight");

    restore(&snap, &client).await;
}

#[tokio::test]
#[ignore]
async fn web_ui_serves_html_with_security() {
    let client = Client::new();
    let base = daemon_url();

    let resp = client.get(&format!("{base}/")).send().await.expect("GET /");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();

    assert!(body.contains("Pool Controller"), "should contain title");
    assert!(body.contains("textContent"), "should use textContent for temps");
    assert!(body.contains("firebaseConfig") || body.contains("firebase.initializeApp"),
        "should have Firebase auth code");
    assert!(body.contains("needsAuth"), "should have tunnel auth gate");
}
