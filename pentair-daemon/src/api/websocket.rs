use axum::{
    extract::State,
    extract::ws::{WebSocket, WebSocketUpgrade, Message, Utf8Bytes},
    response::IntoResponse,
};
use crate::api::routes::AppState;
use tracing::debug;

pub async fn ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.push_tx.subscribe();

    debug!("WebSocket client connected");

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        let json = serde_json::to_string(&ev).unwrap_or_default();
                        let bytes: Utf8Bytes = json.into();
                        if socket.send(Message::Text(bytes)).await.is_err() {
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
