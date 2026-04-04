use crate::adapter::{AdapterCommand, PushEvent};
use crate::scheduled_heat::{self, SharedScheduledHeat};
use crate::scenes::{self, SceneStore};
use crate::state::SharedState;
use axum::{
    extract::Path,
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc};

const INDEX_HTML: &str = include_str!("../../static/index.html");
const APPROVE_HTML: &str = include_str!("../../static/approve.html");

#[derive(Clone)]
pub struct AppState {
    pub shared: SharedState,
    pub cmd_tx: mpsc::Sender<AdapterCommand>,
    pub push_tx: broadcast::Sender<PushEvent>,
    pub devices: crate::devices::DeviceManager,
    pub scheduled_heat: SharedScheduledHeat,
    pub scenes: SceneStore,
    pub network_secret: String,
    pub daemon_local: String,
}

pub fn router(
    shared: SharedState,
    cmd_tx: mpsc::Sender<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    devices: crate::devices::DeviceManager,
    scheduled_heat: SharedScheduledHeat,
    scenes: SceneStore,
    network_secret: String,
    daemon_local: String,
) -> Router {
    let state = AppState {
        shared,
        cmd_tx,
        push_tx,
        devices,
        scheduled_heat,
        scenes,
        network_secret,
        daemon_local,
    };

    Router::new()
        // ── Web UI ─────────────────────────────────────────────────
        .route("/", get(serve_ui))
        .route("/approve", get(serve_approve_page))
        .route("/api/approve", post(approve_redirect))
        // ── Semantic API (primary — use these) ──────────────────────
        .route("/api/pool", get(get_pool))
        .route("/api/pool/on", post(pool_on))
        .route("/api/pool/off", post(pool_off))
        .route("/api/pool/heat", post(pool_heat))
        .route("/api/spa/on", post(spa_on))
        .route("/api/spa/off", post(spa_off))
        .route("/api/spa/heat", post(spa_heat))
        .route("/api/spa/heat-at", post(spa_heat_at).get(get_spa_heat_at).delete(delete_spa_heat_at))
        .route("/api/spa/jets/on", post(jets_on))
        .route("/api/spa/jets/off", post(jets_off))
        .route("/api/lights/on", post(lights_on))
        .route("/api/lights/off", post(lights_off))
        .route("/api/lights/mode", post(lights_mode))
        .route("/api/auxiliary/{id}/on", post(aux_on))
        .route("/api/auxiliary/{id}/off", post(aux_off))
        .route("/api/goodnight", post(goodnight))
        // ── Scenes API ──────────────────────────────────────────────
        .route("/api/scenes", get(list_scenes))
        .route("/api/scenes/{name}", post(trigger_scene))
        // ── Raw API (for debugging / advanced use) ──────────────────
        .route("/api/raw/status", get(get_status))
        .route("/api/raw/config", get(get_config))
        .route("/api/raw/version", get(get_version))
        .route("/api/raw/chem", get(get_chem))
        .route("/api/raw/chlor", get(get_chlor))
        .route("/api/raw/pumps/{index}", get(get_pump))
        .route("/api/raw/circuits/{id}", post(set_circuit))
        .route("/api/raw/heat/setpoint", post(set_heat_setpoint))
        .route("/api/raw/heat/mode", post(set_heat_mode))
        .route("/api/raw/heat/cool", post(set_cool_setpoint))
        .route("/api/raw/lights", post(set_light))
        .route("/api/raw/chlor/set", post(set_chlor))
        .route("/api/devices/register", post(register_device))
        .route("/api/matter/qr", get(matter_qr))
        .route("/api/matter/info", get(matter_info))
        .route("/api/cancel-delay", post(cancel_delay))
        .route("/api/refresh", post(refresh))
        .route("/api/ws", get(super::websocket::ws_handler))
        .with_state(state)
}

// ── Web UI ──────────────────────────────────────────────────────────────

async fn serve_ui(State(state): State<AppState>) -> impl IntoResponse {
    let html = INDEX_HTML.replace("{{DAEMON_LOCAL}}", &state.daemon_local);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html")],
        html,
    )
}

// GET endpoints - serve from cache

async fn get_pool(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    match s.pool_system() {
        Some(pool) => Json(serde_json::to_value(&pool).unwrap()),
        None => Json(serde_json::json!({"error": "pool data not yet available"})),
    }
}

async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    Json(serde_json::to_value(&s.status).unwrap_or(serde_json::Value::Null))
}

async fn get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    Json(serde_json::to_value(&s.config).unwrap_or(serde_json::Value::Null))
}

async fn get_version(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    Json(serde_json::to_value(&s.version).unwrap_or(serde_json::Value::Null))
}

async fn get_chem(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    Json(serde_json::to_value(&s.chem).unwrap_or(serde_json::Value::Null))
}

async fn get_chlor(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    Json(serde_json::to_value(&s.scg).unwrap_or(serde_json::Value::Null))
}

async fn get_pump(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    let pump = s.pumps.get(index).and_then(|p| p.as_ref());
    Json(serde_json::to_value(&pump).unwrap_or(serde_json::Value::Null))
}

// POST endpoints - dispatch commands

#[derive(Deserialize)]
struct CircuitRequest {
    state: bool,
}

async fn set_circuit(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<CircuitRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetCircuit {
            circuit_id: id,
            state: body.state,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize)]
struct HeatSetpointRequest {
    body_type: i32,
    temperature: i32,
}

async fn set_heat_setpoint(
    State(state): State<AppState>,
    Json(body): Json<HeatSetpointRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetHeatSetpoint {
            body_type: body.body_type,
            temp: body.temperature,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize)]
struct HeatModeRequest {
    body_type: i32,
    mode: i32,
}

async fn set_heat_mode(
    State(state): State<AppState>,
    Json(body): Json<HeatModeRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetHeatMode {
            body_type: body.body_type,
            mode: body.mode,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize)]
struct CoolSetpointRequest {
    body_type: i32,
    temperature: i32,
}

async fn set_cool_setpoint(
    State(state): State<AppState>,
    Json(body): Json<CoolSetpointRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetCoolSetpoint {
            body_type: body.body_type,
            temp: body.temperature,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize)]
struct LightRequest {
    command: i32,
}

async fn set_light(
    State(state): State<AppState>,
    Json(body): Json<LightRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetLightCommand {
            command: body.command,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize)]
struct ChlorRequest {
    pool: i32,
    spa: i32,
}

async fn set_chlor(
    State(state): State<AppState>,
    Json(body): Json<ChlorRequest>,
) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetScgConfig {
            pool: body.pool,
            spa: body.spa,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

async fn cancel_delay(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::CancelDelay { reply: tx })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

async fn refresh(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::RefreshAll { reply: tx })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

// ── Device registration ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    token: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    live_activity_token: Option<String>,
}

async fn register_device(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state
        .devices
        .register(body.token, body.platform, body.live_activity_token)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": e})),
        ),
    }
}

// ── Semantic route handlers ─────────────────────────────────────────────

/// Helper: resolve a semantic ID to a circuit and send a SetCircuit command.
async fn set_semantic_circuit(state: &AppState, id: &str, on: bool) -> Json<serde_json::Value> {
    let circuit_id = {
        let s = state.shared.read().await;
        s.resolve_circuit(id)
    };
    let Some(circuit_id) = circuit_id else {
        return Json(serde_json::json!({"ok": false, "error": format!("unknown device: {}", id)}));
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetCircuit {
            circuit_id,
            state: on,
            reply: tx,
        })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

#[derive(Deserialize, Default)]
struct OnRequest {
    /// Optional: set the heat setpoint when turning on.
    #[serde(default)]
    setpoint: Option<i32>,
}

async fn pool_on(
    State(state): State<AppState>,
    body: Option<Json<OnRequest>>,
) -> impl IntoResponse {
    if let Some(Json(req)) = &body {
        if let Some(sp) = req.setpoint {
            let r = apply_heat(
                &state,
                "pool",
                HeatRequest {
                    setpoint: Some(sp),
                    mode: None,
                },
            )
            .await;
            if r.1.0.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                return r;
            }
        }
    }
    (StatusCode::OK, set_semantic_circuit(&state, "pool", true).await)
}

async fn pool_off(State(state): State<AppState>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, "pool", false).await
}

async fn spa_on(
    State(state): State<AppState>,
    body: Option<Json<OnRequest>>,
) -> impl IntoResponse {
    if let Some(Json(req)) = &body {
        if let Some(sp) = req.setpoint {
            let r = apply_heat(
                &state,
                "spa",
                HeatRequest {
                    setpoint: Some(sp),
                    mode: None,
                },
            )
            .await;
            if r.1.0.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                return r;
            }
        }
    }
    (StatusCode::OK, set_semantic_circuit(&state, "spa", true).await)
}

async fn spa_off(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Turn off both spa and jets (jets alone with pool mode is weird)
    let _ = set_semantic_circuit(&state, "jets", false).await;
    set_semantic_circuit(&state, "spa", false).await
}

async fn jets_on(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Jets are a spa accessory — ensure spa is on first.
    // (Without spa, the valve routes water elsewhere and jets are pointless.)
    let spa_on = {
        let s = state.shared.read().await;
        s.pool_system()
            .and_then(|p| p.spa.map(|s| s.on))
            .unwrap_or(false)
    };
    if !spa_on {
        let result = set_semantic_circuit(&state, "spa", true).await;
        // If spa failed to turn on, don't proceed with jets.
        if let Some(ok) = result.0.get("ok") {
            if ok.as_bool() != Some(true) {
                return result;
            }
        }
        // Give the controller a moment to switch the valve.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    set_semantic_circuit(&state, "jets", true).await
}

async fn jets_off(State(state): State<AppState>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, "jets", false).await
}

async fn lights_on(State(state): State<AppState>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, "lights", true).await
}

async fn lights_off(State(state): State<AppState>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, "lights", false).await
}

#[derive(Deserialize)]
struct HeatRequest {
    #[serde(default)]
    setpoint: Option<i32>,
    #[serde(default)]
    mode: Option<String>,
}

async fn apply_heat(
    state: &AppState,
    body_name: &str,
    body: HeatRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let body_type = match body_name {
        "pool" => 0,
        "spa" => 1,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok": false, "error": "unknown body"}))),
    };

    if let Some(setpoint) = body.setpoint {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = state
            .cmd_tx
            .send(AdapterCommand::SetHeatSetpoint {
                body_type,
                temp: setpoint,
                reply: tx,
            })
            .await;
        match rx.await {
            Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"ok": false, "error": e}))),
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"ok": false, "error": "adapter unavailable"}))),
            _ => {}
        }
    }

    if let Some(mode_str) = body.mode {
        let mode = match mode_str.as_str() {
            "off" => 0,
            "solar" => 1,
            "solar-preferred" => 2,
            "heat-pump" | "heater" => 3,
            _ => {
                return (StatusCode::BAD_REQUEST, Json(
                    serde_json::json!({"ok": false, "error": format!("unknown heat mode: {}", mode_str)}),
                ))
            }
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = state
            .cmd_tx
            .send(AdapterCommand::SetHeatMode {
                body_type,
                mode,
                reply: tx,
            })
            .await;
        match rx.await {
            Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"ok": false, "error": e}))),
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"ok": false, "error": "adapter unavailable"}))),
            _ => {}
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}

async fn pool_heat(
    State(state): State<AppState>,
    Json(body): Json<HeatRequest>,
) -> impl IntoResponse {
    apply_heat(&state, "pool", body).await
}

async fn spa_heat(
    State(state): State<AppState>,
    Json(body): Json<HeatRequest>,
) -> impl IntoResponse {
    apply_heat(&state, "spa", body).await
}

/// Map a light mode name to its protocol command code.
fn light_mode_to_command(mode: &str) -> Option<i32> {
    match mode {
        "off" => Some(0),
        "on" => Some(1),
        "set" => Some(2),
        "sync" => Some(3),
        "swim" => Some(4),
        "party" => Some(5),
        "romantic" => Some(6),
        "caribbean" => Some(7),
        "american" => Some(8),
        "sunset" => Some(9),
        "royal" => Some(10),
        "blue" => Some(13),
        "green" => Some(14),
        "red" => Some(15),
        "white" => Some(16),
        "purple" => Some(17),
        _ => None,
    }
}

#[derive(Deserialize)]
struct LightModeRequest {
    mode: String,
}

async fn lights_mode(
    State(state): State<AppState>,
    Json(body): Json<LightModeRequest>,
) -> Json<serde_json::Value> {
    let Some(command) = light_mode_to_command(&body.mode) else {
        return Json(
            serde_json::json!({"ok": false, "error": format!("unknown light mode: {}", body.mode)}),
        );
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state
        .cmd_tx
        .send(AdapterCommand::SetLightCommand { command, reply: tx })
        .await;
    match rx.await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})),
        Ok(Err(e)) => Json(serde_json::json!({"ok": false, "error": e})),
        Err(_) => Json(serde_json::json!({"ok": false, "error": "adapter disconnected"})),
    }
}

async fn aux_on(State(state): State<AppState>, Path(id): Path<String>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, &id, true).await
}

async fn aux_off(State(state): State<AppState>, Path(id): Path<String>) -> Json<serde_json::Value> {
    set_semantic_circuit(&state, &id, false).await
}

// ── Goodnight (turn off user-initiated things) ───────────────────────────

async fn goodnight(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Acquire the scene execution lock to prevent interleaving with other commands
    let _guard = state.scenes.exec_lock.lock().await;

    let mut errors = Vec::new();

    // Only turn off devices that exist and are on
    let (spa_on, lights_on) = {
        let s = state.shared.read().await;
        match s.pool_system() {
            Some(ps) => (
                ps.spa.as_ref().is_some_and(|s| s.on),
                ps.lights.as_ref().is_some_and(|l| l.on),
            ),
            None => (false, false),
        }
    };

    if spa_on {
        let _ = set_semantic_circuit(&state, "jets", false).await;
        let result = set_semantic_circuit(&state, "spa", false).await;
        if !result.0.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            errors.push("spa");
        }
    }

    if lights_on {
        let result = set_semantic_circuit(&state, "lights", false).await;
        if !result.0.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            errors.push("lights");
        }
    }

    if errors.is_empty() {
        Json(serde_json::json!({"ok": true}))
    } else {
        Json(serde_json::json!({"ok": false, "error": format!("failed to turn off: {}", errors.join(", "))}))
    }
}

// ── Matter commissioning ─────────────────────────────────────────────────

/// Default Matter setup code (test credentials: discriminator 3840, passcode 20202021, VID 0xFFF1, PID 0x8001).
/// Override at runtime with the MATTER_SETUP_CODE environment variable.
const DEFAULT_MATTER_SETUP_CODE: &str = "MT:-24J0AFN00KA064IJ3P04A5D08CIH28QIB2OJKJ1K-XS0";

/// Default manual pairing code for the test credentials.
/// Override at runtime with the MATTER_MANUAL_CODE environment variable.
const DEFAULT_MATTER_MANUAL_CODE: &str = "3497-0112-332";

fn matter_setup_code() -> String {
    std::env::var("MATTER_SETUP_CODE").unwrap_or_else(|_| DEFAULT_MATTER_SETUP_CODE.to_string())
}

fn matter_manual_code() -> String {
    std::env::var("MATTER_MANUAL_CODE").unwrap_or_else(|_| DEFAULT_MATTER_MANUAL_CODE.to_string())
}

async fn matter_qr() -> impl IntoResponse {
    let setup_code = matter_setup_code();
    let qr = match qrcode::QrCode::new(setup_code.as_bytes()) {
        Ok(q) => q,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                format!("failed to generate QR code: {}", e).into_bytes(),
            );
        }
    };
    let image = qr.render::<image::Luma<u8>>().quiet_zone(true).build();
    let mut png_bytes = std::io::Cursor::new(Vec::new());
    if let Err(e) = image.write_to(&mut png_bytes, image::ImageFormat::Png) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            format!("failed to encode PNG: {}", e).into_bytes(),
        );
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        png_bytes.into_inner(),
    )
}

async fn matter_info() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "setup_code": matter_setup_code(),
        "manual_code": matter_manual_code(),
        "discriminator": 3840,
        "passcode": 20202021,
        "vendor_id": "0xFFF1",
        "product_id": "0x8001",
    }))
}

#[cfg(test)]
mod matter_tests {
    use super::*;

    #[test]
    fn default_setup_code_is_valid() {
        // The default setup code should be a valid Matter QR string starting with "MT:"
        assert!(DEFAULT_MATTER_SETUP_CODE.starts_with("MT:"));
        assert!(DEFAULT_MATTER_SETUP_CODE.len() > 10);
    }

    #[test]
    fn default_manual_code_is_valid() {
        // Manual pairing codes are digit groups separated by hyphens
        assert!(DEFAULT_MATTER_MANUAL_CODE.chars().all(|c| c.is_ascii_digit() || c == '-'));
        assert_eq!(DEFAULT_MATTER_MANUAL_CODE.replace('-', "").len(), 11);
    }

    #[test]
    fn qr_code_generates_valid_png() {
        let qr = qrcode::QrCode::new(DEFAULT_MATTER_SETUP_CODE.as_bytes()).unwrap();
        let image = qr.render::<image::Luma<u8>>().quiet_zone(true).build();
        let mut png_bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png_bytes, image::ImageFormat::Png).unwrap();
        let bytes = png_bytes.into_inner();

        // Should be a non-trivial PNG (header + data)
        assert!(bytes.len() > 100, "PNG too small: {} bytes", bytes.len());
        // PNG magic bytes
        assert_eq!(&bytes[..4], &[0x89, 0x50, 0x4E, 0x47], "not a valid PNG");
    }

    #[test]
    fn qr_code_dimensions_are_reasonable() {
        let qr = qrcode::QrCode::new(DEFAULT_MATTER_SETUP_CODE.as_bytes()).unwrap();
        let image = qr.render::<image::Luma<u8>>().quiet_zone(true).build();
        let (w, h) = image.dimensions();
        // QR code should be square and between 100-1000 pixels
        assert_eq!(w, h, "QR code should be square");
        assert!(w >= 100, "QR code too small: {}x{}", w, h);
        assert!(w <= 1000, "QR code too large: {}x{}", w, h);
    }

    #[tokio::test]
    async fn matter_info_returns_expected_fields() {
        let Json(info) = matter_info().await;
        assert_eq!(info["setup_code"], DEFAULT_MATTER_SETUP_CODE);
        assert_eq!(info["manual_code"], DEFAULT_MATTER_MANUAL_CODE);
        assert_eq!(info["discriminator"], 3840);
        assert_eq!(info["passcode"], 20202021);
        assert!(info["vendor_id"].as_str().unwrap().starts_with("0x"));
        assert!(info["product_id"].as_str().unwrap().starts_with("0x"));
    }

    #[tokio::test]
    async fn matter_qr_returns_png_response() {
        let response = matter_qr().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1_000_000).await.unwrap();
        // Verify PNG magic bytes
        assert_eq!(&body[..4], &[0x89, 0x50, 0x4E, 0x47]);
    }
}

// ── Scheduled heating (heat-at) ─────────────────────────────────────────

async fn spa_heat_at(
    State(state): State<AppState>,
    Json(body): Json<scheduled_heat::HeatAtRequest>,
) -> impl IntoResponse {
    // Read current spa state from the heat estimator
    let (current_temp, learned_rate, configured_rate) = {
        let s = state.shared.read().await;
        let air_temp_f = s.status.as_ref().map(|st| st.air_temp as f64);
        (
            s.heat.spa_last_reliable_temp(),
            s.heat.spa_learned_rate_f_per_hour(air_temp_f),
            s.heat.spa_configured_rate_f_per_hour(),
        )
    };

    match scheduled_heat::create_schedule(
        &state.scheduled_heat,
        &body,
        current_temp,
        learned_rate,
        configured_rate,
        state.shared.clone(),
        state.cmd_tx.clone(),
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": e})),
        ),
    }
}

async fn get_spa_heat_at(State(state): State<AppState>) -> Json<serde_json::Value> {
    let resp = scheduled_heat::get_schedule(&state.scheduled_heat).await;
    Json(serde_json::to_value(&resp).unwrap())
}

async fn delete_spa_heat_at(State(state): State<AppState>) -> Json<serde_json::Value> {
    let had = scheduled_heat::cancel_schedule(&state.scheduled_heat).await;
    Json(serde_json::json!({"ok": true, "had_schedule": had}))
}

// ── Scene handlers ──────────────────────────────────────────────────────

async fn list_scenes(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(state.scenes.list()).unwrap())
}

async fn trigger_scene(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(scene) = state.scenes.find(&name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"ok": false, "error": format!("unknown scene: {}", name)})),
        );
    };

    let scene = scene.clone();
    // Serialize scene execution to prevent command interleaving against hardware.
    let _guard = state.scenes.exec_lock.lock().await;
    let result = scenes::execute_scene(&scene, |target, action, value| {
        let state = state.clone();
        async move { execute_scene_command(&state, &target, &action, value.as_deref()).await }
    })
    .await;

    // Partial failures still return 200 with per-command results in the body.
    (StatusCode::OK, Json(serde_json::to_value(&result).unwrap()))
}

/// Execute a single scene command by dispatching through the same code paths
/// as the REST API handlers. This ensures scenes behave identically to manual
/// API calls (e.g., jets auto-enables spa, spa-off disables jets, etc.).
async fn execute_scene_command(
    state: &AppState,
    target: &str,
    action: &str,
    value: Option<&str>,
) -> Result<(), String> {
    match (target, action) {
        ("spa", "on") => {
            let result = set_semantic_circuit(state, "spa", true).await;
            json_to_result(&result.0)
        }
        ("spa", "off") => {
            // Match spa_off behavior: turn off jets first, then spa
            let _ = set_semantic_circuit(state, "jets", false).await;
            let result = set_semantic_circuit(state, "spa", false).await;
            json_to_result(&result.0)
        }
        ("pool", "on") => {
            let result = set_semantic_circuit(state, "pool", true).await;
            json_to_result(&result.0)
        }
        ("pool", "off") => {
            let result = set_semantic_circuit(state, "pool", false).await;
            json_to_result(&result.0)
        }
        ("jets", "on") => {
            // Match jets_on behavior: ensure spa is on first
            let spa_on = {
                let s = state.shared.read().await;
                s.pool_system()
                    .and_then(|p| p.spa.map(|s| s.on))
                    .unwrap_or(false)
            };
            if !spa_on {
                let result = set_semantic_circuit(state, "spa", true).await;
                if let Err(e) = json_to_result(&result.0) {
                    return Err(e);
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            let result = set_semantic_circuit(state, "jets", true).await;
            json_to_result(&result.0)
        }
        ("jets", "off") => {
            let result = set_semantic_circuit(state, "jets", false).await;
            json_to_result(&result.0)
        }
        ("lights", "on") => {
            let result = set_semantic_circuit(state, "lights", true).await;
            json_to_result(&result.0)
        }
        ("lights", "off") => {
            let result = set_semantic_circuit(state, "lights", false).await;
            json_to_result(&result.0)
        }
        ("spa", "heat") | ("pool", "heat") => {
            let setpoint = value
                .ok_or_else(|| format!("{} heat requires a value (setpoint)", target))?
                .parse::<i32>()
                .map_err(|e| format!("invalid setpoint: {}", e))?;
            let body = HeatRequest {
                setpoint: Some(setpoint),
                mode: None,
            };
            let (_, Json(resp)) = apply_heat(state, target, body).await;
            json_to_result(&resp)
        }
        ("lights", "mode") => {
            let mode = value.ok_or_else(|| "lights mode requires a value".to_string())?;
            let command = light_mode_to_command(mode)
                .ok_or_else(|| format!("unknown light mode: {}", mode))?;
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = state
                .cmd_tx
                .send(AdapterCommand::SetLightCommand { command, reply: tx })
                .await;
            match rx.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("adapter disconnected".to_string()),
            }
        }
        // Auxiliary circuits by name
        (id, "on") => {
            let result = set_semantic_circuit(state, id, true).await;
            json_to_result(&result.0)
        }
        (id, "off") => {
            let result = set_semantic_circuit(state, id, false).await;
            json_to_result(&result.0)
        }
        _ => Err(format!("unsupported command: {} {}", target, action)),
    }
}

/// Extract Ok/Err from the standard JSON response format `{"ok": bool, "error": "..."}`.
fn json_to_result(json: &serde_json::Value) -> Result<(), String> {
    if json.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        let error = json
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error")
            .to_string();
        Err(error)
    }
}

// ── LAN Approval ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ApproveQuery {
    email: Option<String>,
    origin: Option<String>,
}

async fn serve_approve_page(
    axum::extract::Query(q): axum::extract::Query<ApproveQuery>,
) -> impl IntoResponse {
    let email = q.email.unwrap_or_default();
    let origin = q.origin.unwrap_or_default();
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let html = APPROVE_HTML
        .replace("{{EMAIL}}", &esc(&email))
        .replace("{{ORIGIN}}", &esc(&origin));
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], html)
}

#[derive(Deserialize)]
struct ApproveForm {
    email: String,
    origin: String,
}

async fn approve_redirect(
    State(state): State<AppState>,
    axum::extract::Form(form): axum::extract::Form<ApproveForm>,
) -> impl IntoResponse {
    // Validate origin to prevent open redirect
    let origin_url = url::Url::parse(&form.origin).ok();
    let valid_origin = origin_url
        .as_ref()
        .and_then(|u| u.host_str())
        .map(|h| h == "oursilverfamily.com" || h.ends_with(".oursilverfamily.com"))
        .unwrap_or(false);
    if !valid_origin {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            "Invalid origin".to_string(),
        );
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let sig = crate::network_secret::sign(&state.network_secret, &form.email, ts);
    let email_enc = percent_encoding::utf8_percent_encode(
        &form.email, percent_encoding::NON_ALPHANUMERIC,
    );
    let callback = format!(
        "{}/api/approve-callback?email={}&ts={}&sig={}",
        form.origin, email_enc, ts, sig,
    );
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, callback)],
        "".to_string(),
    )
}
