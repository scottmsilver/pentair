//! Live read-only tests for the Pentair ScreenLogic adapter.
//!
//! These tests connect to real hardware but only read state — no mutations.
//! Safe to run at any time.
//!
//! Run with:
//!   PENTAIR_HOST=192.168.1.89 cargo test --test live_read -p pentair-client -- --ignored --test-threads=1 --nocapture

use pentair_client::client::Client;

async fn connect() -> Client {
    let addr = if let Ok(host) = std::env::var("PENTAIR_HOST") {
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
    };
    eprintln!("[live_read] connecting to {}", addr);
    Client::connect(&addr).await.expect("failed to connect")
}

#[tokio::test]
#[ignore]
async fn read_version() {
    let mut client = connect().await;
    let v = client.get_version().await.unwrap();
    assert!(
        v.version.contains("POOL"),
        "version should contain 'POOL': {}",
        v.version
    );
    eprintln!("[read_version] {}", v.version);
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_status_bodies() {
    let mut client = connect().await;
    let s = client.get_status().await.unwrap();
    assert_eq!(s.bodies.len(), 2, "expected 2 bodies (pool + spa)");
    assert_eq!(s.bodies[0].body_type, 0, "first body should be pool");
    assert_eq!(s.bodies[1].body_type, 1, "second body should be spa");
    // Temps should be reasonable (40-120°F)
    for body in &s.bodies {
        assert!(
            body.current_temp >= 40 && body.current_temp <= 120,
            "body {} temp {} out of range",
            body.body_type,
            body.current_temp
        );
        assert!(
            body.set_point >= 40 && body.set_point <= 104,
            "body {} setpoint {} out of range",
            body.body_type,
            body.set_point
        );
    }
    eprintln!(
        "[read_status_bodies] pool={}°F spa={}°F air={}°F",
        s.bodies[0].current_temp, s.bodies[1].current_temp, s.air_temp
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_status_circuits() {
    let mut client = connect().await;
    let s = client.get_status().await.unwrap();
    assert_eq!(s.circuits.len(), 7, "expected 7 circuits");
    // Circuit IDs should be 500-506
    for (i, c) in s.circuits.iter().enumerate() {
        assert_eq!(c.circuit_id, 500 + i as i32, "circuit {} has wrong id", i);
    }
    eprintln!(
        "[read_status_circuits] {} circuits, all IDs correct",
        s.circuits.len()
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_controller_config() {
    let mut client = connect().await;
    let c = client.get_controller_config().await.unwrap();
    assert_eq!(c.controller_id, 1);
    assert_eq!(c.controller_type, 1, "should be IntelliTouch");
    assert!(!c.is_celsius);
    assert_eq!(c.circuits.len(), 7);
    assert_eq!(c.colors.len(), 8);
    assert_eq!(c.equipment_flags.raw(), 24); // IntelliBrite + IntelliFlow_0
                                             // Verify known circuit names
    let names: Vec<&str> = c.circuits.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Pool"), "missing Pool circuit");
    assert!(names.contains(&"Spa"), "missing Spa circuit");
    assert!(names.contains(&"Lights"), "missing Lights circuit");
    eprintln!(
        "[read_controller_config] {} circuits, {} colors, flags=0x{:x}",
        c.circuits.len(),
        c.colors.len(),
        c.equipment_flags.raw()
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_pump_0_vs() {
    let mut client = connect().await;
    let p = client.get_pump_status(0).await.unwrap();
    assert_eq!(p.pump_type, 2, "pump 0 should be VS (type 2)");
    assert_eq!(p.circuits.len(), 8, "pump should have 8 circuit presets");
    // First circuit preset should have speed > 0
    assert!(
        p.circuits[0].speed > 0,
        "first preset should have nonzero speed"
    );
    assert!(p.circuits[0].is_rpm, "first preset should be RPM");
    eprintln!(
        "[read_pump_0_vs] type=VS, running={}, watts={}, rpm={}, gpm={}",
        p.is_running, p.watts, p.rpm, p.gpm
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_pumps_1_7_empty() {
    let mut client = connect().await;
    for i in 1..=7 {
        let p = client.get_pump_status(i).await.unwrap();
        assert_eq!(
            p.pump_type, 0,
            "pump {} should be type 0 (not installed)",
            i
        );
    }
    eprintln!("[read_pumps_1_7_empty] all 7 empty pumps confirmed");
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_chem_no_intellichem() {
    let mut client = connect().await;
    let c = client.get_chem_data().await.unwrap();
    assert!(c.is_valid, "sentinel should be 42 even without IntelliChem");
    assert_eq!(c.ph, 0.0);
    assert_eq!(c.orp, 0);
    assert_eq!(c.salt_ppm, 0);
    eprintln!(
        "[read_chem_no_intellichem] valid={}, all zeros (no IntelliChem)",
        c.is_valid
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_chlorinator() {
    let mut client = connect().await;
    let s = client.get_scg_config().await.unwrap();
    assert!(!s.installed, "chlorinator should not be installed");
    assert_eq!(s.pool_set_point, 50);
    eprintln!(
        "[read_chlorinator] installed={}, pool={}%, spa={}%",
        s.installed, s.pool_set_point, s.spa_set_point
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_schedules() {
    let mut client = connect().await;
    let recurring = client.get_schedule_data(0).await.unwrap();
    let runonce = client.get_schedule_data(1).await.unwrap();
    assert!(
        recurring.events.len() > 0,
        "should have recurring schedules"
    );
    assert_eq!(runonce.events.len(), 0, "should have no runonce schedules");
    // Verify first event has reasonable values
    let first = &recurring.events[0];
    assert!(first.schedule_id > 0, "schedule_id should be > 0");
    assert!(first.circuit_id > 0, "circuit_id should be > 0");
    eprintln!(
        "[read_schedules] {} recurring, {} runonce",
        recurring.events.len(),
        runonce.events.len()
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_weather() {
    let mut client = connect().await;
    let w = client.get_weather().await.unwrap();
    // Sunrise/sunset should be reasonable minutes from midnight
    assert!(
        w.sunrise > 300 && w.sunrise < 600,
        "sunrise {} not in expected range",
        w.sunrise
    );
    assert!(
        w.sunset > 900 && w.sunset < 1200,
        "sunset {} not in expected range",
        w.sunset
    );
    eprintln!(
        "[read_weather] sunrise={} ({:02}:{:02}), sunset={} ({:02}:{:02})",
        w.sunrise,
        w.sunrise / 60,
        w.sunrise % 60,
        w.sunset,
        w.sunset / 60,
        w.sunset % 60
    );
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn read_system_time() {
    let mut client = connect().await;
    let t = client.get_system_time().await.unwrap();
    assert!(t.time.year >= 2024 && t.time.year <= 2030);
    assert!(t.time.month >= 1 && t.time.month <= 12);
    assert!(t.time.day >= 1 && t.time.day <= 31);
    eprintln!(
        "[read_system_time] {}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.time.year, t.time.month, t.time.day, t.time.hour, t.time.minute, t.time.second
    );
    client.disconnect().await.unwrap();
}
