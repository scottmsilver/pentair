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
                    Ok(_ev) => {
                        if send_current_pool_state(&mut socket, &state).await.is_err() {
                            break;  // Client disconnected
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

    let json = serde_json::to_string(&system).map_err(|_| ())?;
    let bytes: Utf8Bytes = json.into();
    socket.send(Message::Text(bytes)).await.map_err(|_| ())
}
