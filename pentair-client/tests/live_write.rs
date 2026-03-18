//! Live write tests for the Pentair ScreenLogic adapter.
//!
//! These tests connect to real hardware and mutate state. Every test:
//! 1. Connects to the adapter
//! 2. Captures the original state before any mutation
//! 3. Performs the write operation
//! 4. Polls status until the change is reflected (up to 10s)
//! 5. Restores the original state
//! 6. Polls again to verify restoration
//! 7. Disconnects
//!
//! If restoration fails, the test PANICs with a loud message showing
//! what was changed and what the original value was, so you can fix it manually.
//!
//! Run with:
//!   PENTAIR_HOST=192.168.1.89 cargo test --test live_write -p pentair-client -- --ignored --test-threads=1 --nocapture
//!
//! IMPORTANT: --test-threads=1 is required. The adapter only handles one
//! TCP connection at a time reliably.

use pentair_client::client::Client;
use pentair_protocol::responses::PoolStatus;
use std::time::Duration;

async fn connect() -> Client {
    let addr = resolve_addr().await;
    eprintln!("[live_write] connecting to {}", addr);
    Client::connect(&addr)
        .await
        .expect("failed to connect to adapter — is it reachable?")
}

async fn resolve_addr() -> String {
    if let Ok(host) = std::env::var("PENTAIR_HOST") {
        if host.contains(':') {
            host
        } else {
            format!("{}:80", host)
        }
    } else {
        let resp = pentair_client::discovery::discover()
            .await
            .expect("adapter discovery failed — set PENTAIR_HOST");
        format!(
            "{}.{}.{}.{}:{}",
            resp.ip[0], resp.ip[1], resp.ip[2], resp.ip[3], resp.port
        )
    }
}

/// Poll get_status() until `check` returns true, or timeout after `max_wait`.
/// Returns the last status read.
async fn poll_status(
    client: &mut Client,
    max_wait: Duration,
    desc: &str,
    check: impl Fn(&PoolStatus) -> bool,
) -> PoolStatus {
    let start = tokio::time::Instant::now();
    let interval = Duration::from_secs(1);

    loop {
        // Wait before reading so the adapter has time to process the command
        tokio::time::sleep(interval).await;

        let status = client.get_status().await.expect("get_status failed during poll");
        if check(&status) {
            return status;
        }
        if start.elapsed() > max_wait {
            eprintln!("[poll_status] timed out waiting for: {}", desc);
            return status;
        }
    }
}

const POLL_TIMEOUT: Duration = Duration::from_secs(10);

/// If restoration failed, PANIC LOUDLY so the user knows to fix it.
macro_rules! assert_restored {
    ($original:expr, $current:expr, $desc:expr) => {
        if $current != $original {
            panic!(
                "\n\n\
                ╔══════════════════════════════════════════════════════════════╗\n\
                ║  ⚠  STATE RESTORATION FAILED — MANUAL FIX REQUIRED  ⚠     ║\n\
                ╠══════════════════════════════════════════════════════════════╣\n\
                ║  What: {:<51} ║\n\
                ║  Original value: {:<42} ║\n\
                ║  Current value:  {:<42} ║\n\
                ║                                                            ║\n\
                ║  The adapter was NOT restored to its original state.       ║\n\
                ║  Please fix this manually via the ScreenLogic app or CLI.  ║\n\
                ╚══════════════════════════════════════════════════════════════╝\n\n",
                $desc,
                format!("{}", $original),
                format!("{}", $current),
            );
        }
    };
}

// ─── Circuit toggle ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_circuit_toggle() {
    let mut client = connect().await;

    let config = client.get_controller_config().await.unwrap();
    let status_before = client.get_status().await.unwrap();

    // Pick "Yard Light" or fall back to last circuit
    let target = config
        .circuits
        .iter()
        .find(|c| c.name.to_lowercase().contains("yard"))
        .or_else(|| config.circuits.last())
        .expect("no circuits found");

    let wire_id = target.circuit_id;
    let logical_id = wire_id - 499;
    let circuit_name = target.name.clone();

    let original_state = status_before
        .circuits
        .iter()
        .find(|c| c.circuit_id == wire_id)
        .map(|c| c.state)
        .expect("circuit not found in status");

    let new_state = !original_state;

    eprintln!(
        "[write_circuit_toggle] '{}' (id={}): {} → {}",
        circuit_name, logical_id,
        if original_state { "ON" } else { "OFF" },
        if new_state { "ON" } else { "OFF" },
    );

    // Write
    client.set_circuit(logical_id, new_state).await.unwrap();

    // Poll until verified
    let status = poll_status(&mut client, POLL_TIMEOUT, "circuit toggle", |s| {
        s.circuits.iter().find(|c| c.circuit_id == wire_id).map(|c| c.state) == Some(new_state)
    }).await;
    let read_back = status.circuits.iter().find(|c| c.circuit_id == wire_id).unwrap().state;
    assert_eq!(read_back, new_state, "circuit write didn't take effect");
    eprintln!("[write_circuit_toggle] verified: now {}", if new_state { "ON" } else { "OFF" });

    // Restore
    client.set_circuit(logical_id, original_state).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "circuit restore", |s| {
        s.circuits.iter().find(|c| c.circuit_id == wire_id).map(|c| c.state) == Some(original_state)
    }).await;
    let restored = status.circuits.iter().find(|c| c.circuit_id == wire_id).unwrap().state;
    assert_restored!(original_state, restored, format!("Circuit '{}' (id={})", circuit_name, logical_id));
    eprintln!("[write_circuit_toggle] restored: back to {}", if original_state { "ON" } else { "OFF" });

    client.disconnect().await.unwrap();
}

// ─── Heat set point: pool ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_heat_setpoint_pool() {
    let mut client = connect().await;

    let status = client.get_status().await.unwrap();
    let original = status.bodies.iter().find(|b| b.body_type == 0).expect("no pool body").set_point;
    let test_val = if original >= 104 { original - 1 } else { original + 1 };

    eprintln!("[write_heat_setpoint_pool] set point: {} → {}", original, test_val);

    // Write
    client.set_heat_setpoint(0, test_val).await.unwrap();

    // Poll
    let status = poll_status(&mut client, POLL_TIMEOUT, "pool setpoint write", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.set_point) == Some(test_val)
    }).await;
    let new_sp = status.bodies.iter().find(|b| b.body_type == 0).unwrap().set_point;
    assert_eq!(new_sp, test_val, "pool set point write didn't take: expected {}, got {}", test_val, new_sp);
    eprintln!("[write_heat_setpoint_pool] verified: now {}", new_sp);

    // Restore
    client.set_heat_setpoint(0, original).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "pool setpoint restore", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.set_point) == Some(original)
    }).await;
    let restored = status.bodies.iter().find(|b| b.body_type == 0).unwrap().set_point;
    assert_restored!(original, restored, "Pool heat set point");
    eprintln!("[write_heat_setpoint_pool] restored: back to {}", restored);

    client.disconnect().await.unwrap();
}

// ─── Heat set point: spa ────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_heat_setpoint_spa() {
    let mut client = connect().await;

    let status = client.get_status().await.unwrap();
    let original = status.bodies.iter().find(|b| b.body_type == 1).expect("no spa body").set_point;
    let test_val = if original >= 104 { original - 1 } else { original + 1 };

    eprintln!("[write_heat_setpoint_spa] set point: {} → {}", original, test_val);

    // Write
    client.set_heat_setpoint(1, test_val).await.unwrap();

    // Poll
    let status = poll_status(&mut client, POLL_TIMEOUT, "spa setpoint write", |s| {
        s.bodies.iter().find(|b| b.body_type == 1).map(|b| b.set_point) == Some(test_val)
    }).await;
    let new_sp = status.bodies.iter().find(|b| b.body_type == 1).unwrap().set_point;
    assert_eq!(new_sp, test_val, "spa set point write didn't take");
    eprintln!("[write_heat_setpoint_spa] verified: now {}", new_sp);

    // Restore
    client.set_heat_setpoint(1, original).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "spa setpoint restore", |s| {
        s.bodies.iter().find(|b| b.body_type == 1).map(|b| b.set_point) == Some(original)
    }).await;
    let restored = status.bodies.iter().find(|b| b.body_type == 1).unwrap().set_point;
    assert_restored!(original, restored, "Spa heat set point");
    eprintln!("[write_heat_setpoint_spa] restored: back to {}", restored);

    client.disconnect().await.unwrap();
}

// ─── Heat mode: pool ────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_heat_mode_pool() {
    let mut client = connect().await;

    let status = client.get_status().await.unwrap();
    let original = status.bodies.iter().find(|b| b.body_type == 0).expect("no pool body").heat_mode;
    let test_val = if original == 0 { 3 } else { 0 };

    eprintln!("[write_heat_mode_pool] heat mode: {} → {}", fmt_mode(original), fmt_mode(test_val));

    // Write
    client.set_heat_mode(0, test_val).await.unwrap();

    // Poll
    let status = poll_status(&mut client, POLL_TIMEOUT, "pool heat mode write", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.heat_mode) == Some(test_val)
    }).await;
    let new_mode = status.bodies.iter().find(|b| b.body_type == 0).unwrap().heat_mode;
    assert_eq!(new_mode, test_val, "pool heat mode write didn't take");
    eprintln!("[write_heat_mode_pool] verified: now {}", fmt_mode(new_mode));

    // Restore
    client.set_heat_mode(0, original).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "pool heat mode restore", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.heat_mode) == Some(original)
    }).await;
    let restored = status.bodies.iter().find(|b| b.body_type == 0).unwrap().heat_mode;
    assert_restored!(original, restored, "Pool heat mode");
    eprintln!("[write_heat_mode_pool] restored: back to {}", fmt_mode(restored));

    client.disconnect().await.unwrap();
}

// ─── Heat mode: spa ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_heat_mode_spa() {
    let mut client = connect().await;

    let status = client.get_status().await.unwrap();
    let original = status.bodies.iter().find(|b| b.body_type == 1).expect("no spa body").heat_mode;
    let test_val = if original == 0 { 3 } else { 0 };

    eprintln!("[write_heat_mode_spa] heat mode: {} → {}", fmt_mode(original), fmt_mode(test_val));

    client.set_heat_mode(1, test_val).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "spa heat mode write", |s| {
        s.bodies.iter().find(|b| b.body_type == 1).map(|b| b.heat_mode) == Some(test_val)
    }).await;
    let new_mode = status.bodies.iter().find(|b| b.body_type == 1).unwrap().heat_mode;
    assert_eq!(new_mode, test_val, "spa heat mode write didn't take");
    eprintln!("[write_heat_mode_spa] verified: now {}", fmt_mode(new_mode));

    client.set_heat_mode(1, original).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "spa heat mode restore", |s| {
        s.bodies.iter().find(|b| b.body_type == 1).map(|b| b.heat_mode) == Some(original)
    }).await;
    let restored = status.bodies.iter().find(|b| b.body_type == 1).unwrap().heat_mode;
    assert_restored!(original, restored, "Spa heat mode");
    eprintln!("[write_heat_mode_spa] restored: back to {}", fmt_mode(restored));

    client.disconnect().await.unwrap();
}

// ─── Cool set point: pool ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_cool_setpoint_pool() {
    let mut client = connect().await;

    let status = client.get_status().await.unwrap();
    let original = status.bodies.iter().find(|b| b.body_type == 0).expect("no pool body").cool_set_point;
    let test_val = if original >= 104 { original - 1 } else { original + 1 };

    eprintln!("[write_cool_setpoint_pool] cool set point: {} → {}", original, test_val);

    client.set_cool_setpoint(0, test_val).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "pool cool setpoint write", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.cool_set_point) == Some(test_val)
    }).await;
    let new_sp = status.bodies.iter().find(|b| b.body_type == 0).unwrap().cool_set_point;
    assert_eq!(new_sp, test_val, "pool cool setpoint write didn't take: expected {}, got {}", test_val, new_sp);
    eprintln!("[write_cool_setpoint_pool] verified: now {}", new_sp);

    client.set_cool_setpoint(0, original).await.unwrap();

    let status = poll_status(&mut client, POLL_TIMEOUT, "pool cool setpoint restore", |s| {
        s.bodies.iter().find(|b| b.body_type == 0).map(|b| b.cool_set_point) == Some(original)
    }).await;
    let restored = status.bodies.iter().find(|b| b.body_type == 0).unwrap().cool_set_point;
    assert_restored!(original, restored, "Pool cool set point");
    eprintln!("[write_cool_setpoint_pool] restored: back to {}", restored);

    client.disconnect().await.unwrap();
}

// ─── Cool set point: spa (rejected by IntelliTouch — expected error) ────

#[tokio::test]
#[ignore]
async fn write_cool_setpoint_spa_rejected() {
    let mut client = connect().await;

    // IntelliTouch silently ignores cool setpoint writes on the spa body.
    // Our client detects this and returns WriteRejected.
    eprintln!("[write_cool_setpoint_spa_rejected] sending cool setpoint (expect WriteRejected)");
    let result = client.set_cool_setpoint(1, 60).await;
    assert!(result.is_err(), "spa cool setpoint should fail on IntelliTouch");

    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("not supported"), "error should mention 'not supported': {}", msg);
    eprintln!("[write_cool_setpoint_spa_rejected] correctly rejected: {}", msg);

    // Adapter should still be responsive
    let status = client.get_status().await.unwrap();
    assert!(!status.bodies.is_empty());
    eprintln!("[write_cool_setpoint_spa_rejected] adapter still responsive after rejection");

    client.disconnect().await.unwrap();
}

// ─── Ping ───────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_ping() {
    let mut client = connect().await;

    eprintln!("[write_ping] sending ping");
    client.ping().await.unwrap();
    eprintln!("[write_ping] pong received");

    // Verify adapter still responsive
    let status = client.get_status().await.unwrap();
    assert!(!status.bodies.is_empty());
    eprintln!("[write_ping] adapter still responsive after ping");

    client.disconnect().await.unwrap();
}

// ─── System time ────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn read_system_time() {
    let mut client = connect().await;

    eprintln!("[read_system_time] reading system time");
    let time = client.get_system_time().await.unwrap();
    eprintln!(
        "[read_system_time] {}-{:02}-{:02} {:02}:{:02}:{:02} (DST={})",
        time.time.year, time.time.month, time.time.day,
        time.time.hour, time.time.minute, time.time.second,
        time.adjust_for_dst
    );

    // Sanity check: year should be reasonable
    assert!(time.time.year >= 2024 && time.time.year <= 2030, "year {} seems wrong", time.time.year);
    assert!(time.time.month >= 1 && time.time.month <= 12);
    assert!(time.time.day >= 1 && time.time.day <= 31);

    client.disconnect().await.unwrap();
}

// ─── Circuit by name resolution ─────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_circuit_by_name() {
    let mut client = connect().await;

    // Get config and find Yard Light's logical ID by name
    let config = client.get_controller_config().await.unwrap();
    let target = config.circuits.iter()
        .find(|c| c.name == "Yard Light")
        .expect("no Yard Light circuit");
    let logical_id = target.circuit_id - 499;
    let wire_id = target.circuit_id;

    let status = client.get_status().await.unwrap();
    let original = status.circuits.iter().find(|c| c.circuit_id == wire_id).unwrap().state;

    eprintln!("[write_circuit_by_name] 'Yard Light' resolved to id={}, toggling", logical_id);

    // Toggle
    client.set_circuit(logical_id, !original).await.unwrap();
    let status = poll_status(&mut client, POLL_TIMEOUT, "circuit by name toggle", |s| {
        s.circuits.iter().find(|c| c.circuit_id == wire_id).map(|c| c.state) == Some(!original)
    }).await;
    let toggled = status.circuits.iter().find(|c| c.circuit_id == wire_id).unwrap().state;
    assert_eq!(toggled, !original);
    eprintln!("[write_circuit_by_name] verified toggle");

    // Restore
    client.set_circuit(logical_id, original).await.unwrap();
    let status = poll_status(&mut client, POLL_TIMEOUT, "circuit by name restore", |s| {
        s.circuits.iter().find(|c| c.circuit_id == wire_id).map(|c| c.state) == Some(original)
    }).await;
    let restored = status.circuits.iter().find(|c| c.circuit_id == wire_id).unwrap().state;
    assert_restored!(original, restored, "Yard Light circuit (by name)");
    eprintln!("[write_circuit_by_name] restored");

    client.disconnect().await.unwrap();
}

// ─── Light command (fire-and-forget) ────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_light_command() {
    let mut client = connect().await;

    // Light commands are fire-and-forget — no readable state to verify.
    // We verify the protocol accepts the command, then send Off to restore.
    eprintln!("[write_light_command] sending Party(5)");
    client.set_light_command(5).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    eprintln!("[write_light_command] sending Off(0) to restore");
    client.set_light_command(0).await.unwrap();
    eprintln!("[write_light_command] done — commands accepted");

    client.disconnect().await.unwrap();
}

// ─── Cancel delay (idempotent) ──────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn write_cancel_delay() {
    let mut client = connect().await;

    eprintln!("[write_cancel_delay] sending cancel delay");
    client.cancel_delay().await.unwrap();

    let status = client.get_status().await.unwrap();
    assert!(!status.bodies.is_empty(), "status should still be readable after cancel delay");
    eprintln!("[write_cancel_delay] done — adapter still responsive");

    client.disconnect().await.unwrap();
}

// ─── Helpers ────────────────────────────────────────────────────────────

fn fmt_mode(mode: i32) -> &'static str {
    match mode {
        0 => "Off",
        1 => "Solar",
        2 => "SolarPreferred",
        3 => "HeatPump",
        _ => "Unknown",
    }
}
