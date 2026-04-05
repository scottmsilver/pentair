use crate::api::routes::AppState;
use axum::{
    extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use tracing::debug;

pub async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.push_tx.subscribe();

    debug!("WebSocket client connected");

    if send_current_pool_state(&mut socket, &state).await.is_err() {
        debug!("WebSocket client disconnected before initial snapshot");
        return;
    }

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(crate::adapter::PushEvent::StatusChanged) => {
                        if send_current_pool_state(&mut socket, &state).await.is_err() {
                            break;  // Client disconnected
                        }
                    }
                    Ok(crate::adapter::PushEvent::MatterRecommission) => {
                        let json: Utf8Bytes = r#"{"command":"matter_recommission"}"#.into();
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,  // Broadcast channel closed or lagged
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(_)) => {}  // Ignore client messages for now
                    _ => break,  // Client disconnected
                }
            }
        }
    }

    debug!("WebSocket client disconnected");
}

async fn send_current_pool_state(socket: &mut WebSocket, state: &AppState) -> Result<(), ()> {
    let current = state.shared.read().await.pool_system();
    let Some(system) = current else {
        return Ok(());
    };

    let mut json_val = serde_json::to_value(&system).map_err(|_| ())?;
    // Inject matter status so all clients (HTTP and WS) get the same data
    let fabric_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".pentair")
        .join("matter-fabrics.bin");
    let commissioned = fabric_path.exists();
    json_val["matter"] = serde_json::json!({
        "commissioned": commissioned,
        "status_display": if commissioned { "Paired" } else { "Not paired" },
        "can_reset": commissioned,
        "pairing_code": if commissioned { None } else { Some(crate::api::routes::matter_manual_code()) },
        "pairing_qr_url": if commissioned { None } else { Some("/api/matter/qr") },
    });
    let json = serde_json::to_string(&json_val).map_err(|_| ())?;
    let bytes: Utf8Bytes = json.into();
    socket.send(Message::Text(bytes)).await.map_err(|_| ())
}
