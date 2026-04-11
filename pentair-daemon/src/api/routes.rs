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
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::{info, warn, Level};

const INDEX_HTML: &str = include_str!("../../static/index.html");
const APPROVE_HTML: &str = include_str!("../../static/approve.html");
const MATTER_HTML: &str = include_str!("../../static/matter.html");

#[derive(Clone)]
pub struct AppState {
    pub shared: SharedState,
    pub cmd_tx: mpsc::Sender<AdapterCommand>,
    pub push_tx: broadcast::Sender<PushEvent>,
    pub devices: crate::devices::DeviceManager,
    pub scheduled_heat: SharedScheduledHeat,
    pub scenes: SceneStore,
    pub network_secret: String,
    pub public_ip: Option<String>,
    pub web: crate::config::WebConfig,
}

pub fn router(
    shared: SharedState,
    cmd_tx: mpsc::Sender<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    devices: crate::devices::DeviceManager,
    scheduled_heat: SharedScheduledHeat,
    scenes: SceneStore,
    network_secret: String,
    public_ip: Option<String>,
    web: crate::config::WebConfig,
) -> Router {
    let state = AppState {
        shared,
        cmd_tx,
        push_tx,
        devices,
        scheduled_heat,
        scenes,
        network_secret,
        public_ip,
        web,
    };

    Router::new()
        // ── Web UI ─────────────────────────────────────────────────
        .route("/", get(serve_ui))
        .route("/approve", get(serve_approve_page))
        .route("/matter", get(serve_matter_page))
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
        .route("/api/matter/recommission", post(matter_recommission))
        .route("/api/cancel-delay", post(cancel_delay))
        .route("/api/refresh", post(refresh))
        .route("/api/ws", get(super::websocket::ws_handler))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path(),
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO))
        )
}

// ── Web UI ──────────────────────────────────────────────────────────────

async fn serve_ui(State(state): State<AppState>) -> impl IntoResponse {
    let replacements = [
        ("{{FIREBASE_API_KEY}}", state.web.firebase.api_key.as_str()),
        ("{{FIREBASE_AUTH_DOMAIN}}", state.web.firebase.auth_domain.as_str()),
        ("{{FIREBASE_PROJECT_ID}}", state.web.firebase.project_id.as_str()),
        ("{{REMOTE_DOMAIN}}", state.web.remote_domain.as_str()),
    ];
    let mut html = INDEX_HTML.to_string();
    for (key, value) in &replacements {
        html = html.replace(key, value);
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
}

// GET endpoints - serve from cache

async fn get_pool(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = state.shared.read().await;
    match s.pool_system() {
        Some(pool) => {
            let mut json = serde_json::to_value(&pool).unwrap();
            // Inject Matter status as a daemon-owned display contract
            let fabric_path = dirs::home_dir()
                .unwrap_or_default()
                .join(".pentair")
                .join("matter-fabrics.bin");
            let commissioned = fabric_path.exists();
            json["matter"] = serde_json::json!({
                "commissioned": commissioned,
                "status_display": if commissioned { "Paired" } else { "Not paired" },
                "can_reset": commissioned,
                "pairing_code": if commissioned { None } else { Some(matter_manual_code()) },
                "pairing_qr_url": if commissioned { None } else { Some("/api/matter/qr") },
            });
            Json(json)
        }
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

    info!("{} heat request: setpoint={:?} mode={:?}", body_name, body.setpoint, body.mode);

    if let Some(setpoint) = body.setpoint {
        if !(40..=104).contains(&setpoint) {
            warn!("{} setpoint {} out of range (40-104)", body_name, setpoint);
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok": false, "error": "setpoint must be 40-104"})));
        }
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
            Ok(Err(e)) => {
                warn!("{} set heat setpoint to {} failed: {}", body_name, setpoint, e);
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"ok": false, "error": e})));
            }
            Err(_) => {
                warn!("{} set heat setpoint to {}: adapter unavailable", body_name, setpoint);
                return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"ok": false, "error": "adapter unavailable"})));
            }
            Ok(Ok(())) => info!("{} set heat setpoint to {}: ok", body_name, setpoint),
        }
    }

    if let Some(mode_str) = body.mode {
        let mode = match mode_str.as_str() {
            "off" => 0,
            "solar" => 1,
            "solar-preferred" => 2,
            "heat-pump" | "heater" => 3,
            _ => {
                warn!("{} unknown heat mode: {}", body_name, mode_str);
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
            Ok(Err(e)) => {
                warn!("{} set heat mode to {}: {}", body_name, mode_str, e);
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"ok": false, "error": e})));
            }
            Err(_) => {
                warn!("{} set heat mode to {}: adapter unavailable", body_name, mode_str);
                return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"ok": false, "error": "adapter unavailable"})));
            }
            Ok(Ok(())) => info!("{} set heat mode to {}: ok", body_name, mode_str),
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

pub fn matter_manual_code() -> String {
    std::env::var("MATTER_MANUAL_CODE").unwrap_or_else(|_| DEFAULT_MATTER_MANUAL_CODE.to_string())
}

fn generate_matter_qr_png() -> Result<Vec<u8>, String> {
    let setup_code = matter_setup_code();
    let qr = qrcode::QrCode::new(setup_code.as_bytes()).map_err(|e| format!("QR: {e}"))?;
    let image = qr.render::<image::Luma<u8>>().quiet_zone(true).build();
    let mut png_bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png_bytes, image::ImageFormat::Png).map_err(|e| format!("PNG: {e}"))?;
    Ok(png_bytes.into_inner())
}

async fn matter_qr() -> impl IntoResponse {
    match generate_matter_qr_png() {
        Ok(png) => (StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], png),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], e.into_bytes()),
    }
}

async fn matter_recommission(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state.push_tx.send(crate::adapter::PushEvent::MatterRecommission);
    tracing::info!("Matter recommission requested via API");
    (StatusCode::OK, "Matter bridge will enter commissioning mode")
}

async fn matter_info() -> Json<serde_json::Value> {
    let fabric_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".pentair")
        .join("matter-fabrics.bin");
    let commissioned = fabric_path.exists();
    Json(serde_json::json!({
        "setup_code": matter_setup_code(),
        "manual_code": matter_manual_code(),
        "discriminator": 3840,
        "passcode": 20202021,
        "vendor_id": "0xFFF1",
        "product_id": "0x8001",
        "commissioned": commissioned,
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

async fn serve_matter_page() -> impl IntoResponse {
    use base64::Engine;
    let qr_data_url = match generate_matter_qr_png() {
        Ok(png) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            format!("data:image/png;base64,{}", b64)
        }
        Err(_) => String::new(),
    };
    let html = MATTER_HTML
        .replace("/api/matter/qr", &qr_data_url)
        .replace("{{MANUAL_CODE}}", &matter_manual_code());
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html"), (header::CACHE_CONTROL, "no-store")], html)
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
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html"), (header::CACHE_CONTROL, "no-store")], html)
}

/// Validate that a redirect origin matches the configured remote domain.
/// Empty remote_domain rejects all origins (safe default for LAN-only use).
#[cfg(test)]
fn is_valid_redirect_origin(origin: &str, remote_domain: &str) -> bool {
    if remote_domain.is_empty() {
        return false;
    }
    let origin_url = url::Url::parse(origin).ok();
    origin_url
        .as_ref()
        .and_then(|u| u.host_str())
        .map(|h| h == remote_domain || h.ends_with(&format!(".{}", remote_domain)))
        .unwrap_or(false)
}

#[derive(Deserialize)]
struct ApproveForm {
    email: String,
}

async fn approve_redirect(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Form(form): axum::extract::Form<ApproveForm>,
) -> impl IntoResponse {
    if state.web.remote_domain.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            "Remote domain not configured".to_string(),
        );
    }
    // Proof of presence: verify the requester's public IP (from CF-Connecting-IP)
    // matches our own public IP, proving they're on the same home network.
    // Reject if the header is missing (blocks direct requests that bypass the tunnel).
    let client_ip = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok());
    let is_local = match (client_ip, state.public_ip.as_deref()) {
        (Some(cip), Some(pip)) => cip == pip,
        _ => false,
    };
    if !is_local {
        return (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            "You must be on the home network to approve access.".to_string(),
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
    // Build callback using the request's Host header so it routes back through
    // the same tunnel hostname (e.g., pool.oursilverfamily.com, not the bare domain).
    // Validate Host against remote_domain to prevent header injection redirecting
    // the signed callback to an attacker-controlled domain.
    let host = headers.get("host")
        .and_then(|v| v.to_str().ok())
        .filter(|h| {
            let rd = &state.web.remote_domain;
            *h == rd.as_str() || h.ends_with(&format!(".{}", rd))
        })
        .unwrap_or(&state.web.remote_domain);
    let callback = format!(
        "https://{}/api/approve-callback?email={}&ts={}&sig={}",
        host, email_enc, ts, sig,
    );
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, callback)],
        "".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_rejects_wrong_domain() {
        assert!(!is_valid_redirect_origin("https://evil.com/callback", "example.com"));
    }

    #[test]
    fn approve_accepts_configured_domain() {
        assert!(is_valid_redirect_origin("https://example.com/callback", "example.com"));
    }

    #[test]
    fn approve_accepts_subdomain() {
        assert!(is_valid_redirect_origin("https://pool.example.com/callback", "example.com"));
    }

    #[test]
    fn approve_rejects_all_when_domain_empty() {
        assert!(!is_valid_redirect_origin("https://example.com/callback", ""));
        assert!(!is_valid_redirect_origin("https://anything.com", ""));
        assert!(!is_valid_redirect_origin("https://localhost", ""));
    }

    #[test]
    fn approve_rejects_suffix_attack() {
        // "evilexample.com" ends with "example.com" as a string, but is not a subdomain
        assert!(!is_valid_redirect_origin("https://evilexample.com/callback", "example.com"));
    }

    #[test]
    fn serve_ui_substitutes_all_vars() {
        // Verify the template vars exist in the raw HTML
        let html = INDEX_HTML;
        assert!(html.contains("{{FIREBASE_API_KEY}}"));
        assert!(html.contains("{{FIREBASE_AUTH_DOMAIN}}"));
        assert!(html.contains("{{FIREBASE_PROJECT_ID}}"));
        assert!(html.contains("{{REMOTE_DOMAIN}}"));
    }

    #[test]
    fn approve_link_is_relative() {
        // The approve link must be a relative URL (no LAN IP) so it goes through the tunnel
        let html = INDEX_HTML;
        assert!(!html.contains("DAEMON_LOCAL"), "approve link should not reference DAEMON_LOCAL");
        assert!(html.contains("'/approve?email='"), "approve link should be a relative URL");
    }
}

#[cfg(test)]
mod approve_flow_tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_router(public_ip: Option<&str>) -> Router {
        let shared = crate::state::new_shared_state(
            vec![],
            crate::config::HeatingConfig::default(),
            crate::config::SpaHeatNotificationsConfig::default(),
            std::path::PathBuf::from("/dev/null"),
        );
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(1);
        let (push_tx, _) = tokio::sync::broadcast::channel(1);
        let devices = crate::devices::DeviceManager::load(std::path::PathBuf::from("/dev/null"));
        let scheduled_heat = crate::scheduled_heat::new_shared_scheduled_heat(
            std::path::PathBuf::from("/dev/null"),
        );
        let scenes = crate::scenes::SceneStore::new(vec![]);
        let web = crate::config::WebConfig {
            remote_domain: "example.com".to_string(),
            firebase: Default::default(),
        };
        router(
            shared, cmd_tx, push_tx, devices, scheduled_heat, scenes,
            "test-secret".to_string(),
            public_ip.map(|s| s.to_string()),
            web,
        )
    }

    fn approve_form_body(email: &str) -> String {
        format!(
            "email={}",
            percent_encoding::utf8_percent_encode(email, percent_encoding::NON_ALPHANUMERIC),
        )
    }

    #[tokio::test]
    async fn approve_rejects_missing_cf_connecting_ip() {
        let app = test_router(Some("1.2.3.4"));
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn approve_rejects_wrong_ip() {
        let app = test_router(Some("1.2.3.4"));
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", "9.9.9.9")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn approve_rejects_when_public_ip_unknown() {
        let app = test_router(None);
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", "1.2.3.4")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn approve_succeeds_when_ip_matches() {
        let app = test_router(Some("1.2.3.4"));
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", "1.2.3.4")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let location = resp.headers().get("location").unwrap().to_str().unwrap();
        // In tests the Host header is absent so it falls back to remote_domain
        assert!(location.starts_with("https://example.com/api/approve-callback?"),
            "callback should use host header or remote_domain fallback, got: {}", location);
        assert!(location.contains("email=user%40test%2Ecom"));
        assert!(location.contains("&ts="));
        assert!(location.contains("&sig="));
    }

    #[tokio::test]
    async fn approve_rejects_when_remote_domain_empty() {
        // Build a router with empty remote_domain
        let shared = crate::state::new_shared_state(
            vec![],
            crate::config::HeatingConfig::default(),
            crate::config::SpaHeatNotificationsConfig::default(),
            std::path::PathBuf::from("/dev/null"),
        );
        let (cmd_tx, _) = tokio::sync::mpsc::channel(1);
        let (push_tx, _) = tokio::sync::broadcast::channel(1);
        let devices = crate::devices::DeviceManager::load(std::path::PathBuf::from("/dev/null"));
        let scheduled_heat = crate::scheduled_heat::new_shared_scheduled_heat(
            std::path::PathBuf::from("/dev/null"),
        );
        let scenes = crate::scenes::SceneStore::new(vec![]);
        let web = crate::config::WebConfig {
            remote_domain: "".to_string(),
            firebase: Default::default(),
        };
        let app = router(
            shared, cmd_tx, push_tx, devices, scheduled_heat, scenes,
            "test-secret".to_string(), Some("1.2.3.4".to_string()), web,
        );
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", "1.2.3.4")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn approve_callback_signature_is_valid() {
        let app = test_router(Some("1.2.3.4"));
        let body = approve_form_body("user@test.com");
        let req = Request::post("/api/approve")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", "1.2.3.4")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let location = resp.headers().get("location").unwrap().to_str().unwrap();
        let url = url::Url::parse(location).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
        let email = params.get("email").unwrap();
        let ts = params.get("ts").unwrap();
        let sig = params.get("sig").unwrap();
        // Verify the HMAC matches what the daemon would produce
        let expected_sig = crate::network_secret::sign("test-secret", email, ts.parse().unwrap());
        assert_eq!(sig.as_ref(), expected_sig);
    }
}
