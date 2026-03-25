//! Integration tests for pentair-matter using a mock daemon HTTP server.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

/// Recorded POST request from the daemon client.
#[derive(Debug, Clone)]
struct RecordedPost {
    path: String,
    body: Option<Value>,
}

/// Mock daemon state shared across handlers.
#[derive(Default, Clone)]
struct MockDaemonState {
    posts: Arc<Mutex<Vec<RecordedPost>>>,
}

/// Sample pool system JSON matching the real daemon response.
fn sample_pool_system() -> Value {
    json!({
        "pool": {"on": false, "active": false, "temperature": 82, "setpoint": 59,
                 "heat_mode": "off", "heating": "off"},
        "spa": {"on": true, "active": true, "temperature": 103, "setpoint": 104,
                "heat_mode": "heat-pump", "heating": "heater",
                "accessories": {"jets": true}},
        "lights": {"on": true, "mode": "caribbean",
                   "available_modes": ["off","on","set","sync","swim","party",
                                       "romantic","caribbean","american","sunset",
                                       "royal","blue","green","red","white","purple"]},
        "auxiliaries": [],
        "pump": {"pump_type": "VS", "running": true, "watts": 1200, "rpm": 2700, "gpm": 45},
        "system": {"controller": "IntelliTouch", "firmware": "5.2", "temp_unit": "°F",
                   "air_temperature": 72, "freeze_protection": false, "pool_spa_shared_pump": true}
    })
}

async fn handle_get_pool() -> Json<Value> {
    Json(sample_pool_system())
}

async fn handle_post(
    State(state): State<MockDaemonState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    body: Option<Json<Value>>,
) -> Json<Value> {
    state.posts.lock().unwrap().push(RecordedPost {
        path: uri.path().to_string(),
        body: body.map(|b| b.0),
    });
    Json(json!({"ok": true}))
}

/// Start a mock daemon on a random port, return (addr, state).
async fn start_mock_daemon() -> (SocketAddr, MockDaemonState) {
    let state = MockDaemonState::default();
    let app = Router::new()
        .route("/api/pool", get(handle_get_pool))
        .route("/api/spa/on", post(handle_post))
        .route("/api/spa/off", post(handle_post))
        .route("/api/spa/jets/on", post(handle_post))
        .route("/api/spa/jets/off", post(handle_post))
        .route("/api/lights/on", post(handle_post))
        .route("/api/lights/off", post(handle_post))
        .route("/api/lights/mode", post(handle_post))
        .route("/api/spa/heat", post(handle_post))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

mod daemon_client_tests {
    use super::*;

    #[tokio::test]
    async fn get_pool_parses_response() {
        let (addr, _) = start_mock_daemon().await;
        let client =
            pentair_matter::daemon_client::DaemonClient::new(&format!("http://{}", addr));
        let pool = client.get_pool().await.unwrap();

        let spa = pool.spa.unwrap();
        assert!(spa.active);
        assert_eq!(spa.temperature, 103);
        assert_eq!(spa.setpoint, 104);
        assert_eq!(spa.heat_mode, "heat-pump");
        assert!(spa.accessories.get("jets").copied().unwrap_or(false));

        let lights = pool.lights.unwrap();
        assert!(lights.on);
        assert_eq!(lights.mode.as_deref(), Some("caribbean"));
        assert_eq!(lights.available_modes.len(), 16);
    }

    #[tokio::test]
    async fn post_spa_on_sends_correct_request() {
        let (addr, state) = start_mock_daemon().await;
        let client =
            pentair_matter::daemon_client::DaemonClient::new(&format!("http://{}", addr));

        client.post("/api/spa/on", None).await.unwrap();

        let posts = state.posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].path, "/api/spa/on");
        assert!(posts[0].body.is_none());
    }

    #[tokio::test]
    async fn post_spa_heat_sends_json_body() {
        let (addr, state) = start_mock_daemon().await;
        let client =
            pentair_matter::daemon_client::DaemonClient::new(&format!("http://{}", addr));

        client
            .post("/api/spa/heat", Some(json!({"setpoint": 104})))
            .await
            .unwrap();

        let posts = state.posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].path, "/api/spa/heat");
        assert_eq!(posts[0].body.as_ref().unwrap()["setpoint"], 104);
    }

    #[tokio::test]
    async fn post_lights_mode_sends_json_body() {
        let (addr, state) = start_mock_daemon().await;
        let client =
            pentair_matter::daemon_client::DaemonClient::new(&format!("http://{}", addr));

        client
            .post("/api/lights/mode", Some(json!({"mode": "caribbean"})))
            .await
            .unwrap();

        let posts = state.posts.lock().unwrap();
        assert_eq!(posts[0].path, "/api/lights/mode");
        assert_eq!(posts[0].body.as_ref().unwrap()["mode"], "caribbean");
    }
}

mod state_pipeline_tests {
    use super::*;

    #[test]
    fn full_state_pipeline() {
        // Parse daemon response → MatterState
        let json_str = serde_json::to_string(&sample_pool_system()).unwrap();
        let pool: pentair_matter::pool_types::PoolSystem =
            serde_json::from_str(&json_str).unwrap();

        let modes = pool.lights.as_ref().unwrap().available_modes.clone();
        let mode_map = pentair_matter::light_modes::LightModeMap::from_available_modes(&modes);
        let state = pentair_matter::state::MatterState::from_pool_system(&pool, &mode_map);

        // Verify the full pipeline
        assert!(state.spa_reachable);
        assert!(state.spa_on);
        assert_eq!(state.spa_temp_matter, 3944); // 103°F ≈ 39.44°C
        assert_eq!(state.spa_setpoint_matter, 4000); // 104°F = 40.00°C
        assert_eq!(state.spa_system_mode, 4); // Heat
        assert!(state.jets_on);
        assert!(state.lights_on);
        assert!(state.lights_reachable);
        // caribbean is at index 3 after filtering off/on/set/sync
        assert_eq!(state.light_mode_index, Some(3));
    }

    #[test]
    fn state_change_detection() {
        let pool1: pentair_matter::pool_types::PoolSystem =
            serde_json::from_value(sample_pool_system()).unwrap();
        let mut pool2 = pool1.clone();
        pool2.spa.as_mut().unwrap().temperature = 105;

        let modes = pool1.lights.as_ref().unwrap().available_modes.clone();
        let mode_map = pentair_matter::light_modes::LightModeMap::from_available_modes(&modes);

        let state1 = pentair_matter::state::MatterState::from_pool_system(&pool1, &mode_map);
        let state2 = pentair_matter::state::MatterState::from_pool_system(&pool2, &mode_map);

        assert_ne!(state1, state2);
        assert_ne!(state1.spa_temp_matter, state2.spa_temp_matter);
    }
}
