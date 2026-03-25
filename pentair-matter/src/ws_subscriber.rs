use std::sync::Arc;

use futures_util::StreamExt;
use tokio_tungstenite::connect_async;

use crate::light_modes::LightModeMap;
use crate::matter_bridge::SharedState;
use crate::pool_types::PoolSystem;
use crate::state::MatterState;

/// Subscribes to the daemon's WebSocket and updates shared state.
pub async fn run_ws_subscriber(
    ws_url: String,
    shared: Arc<SharedState>,
    mode_map: LightModeMap,
) {
    let mut backoff: u64 = 1;
    loop {
        tracing::info!(url = %ws_url, "connecting to daemon WebSocket");
        match connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("WebSocket connected");
                backoff = 1;
                let (_, mut read) = ws_stream.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                            match serde_json::from_str::<PoolSystem>(&text) {
                                Ok(pool) => {
                                    let new_state =
                                        MatterState::from_pool_system(&pool, &mode_map);
                                    shared.update(new_state);
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
                        _ => { tracing::trace!("ignoring non-text WS message"); }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "WebSocket connection failed");
            }
        }

        tracing::info!(backoff_secs = backoff, "reconnecting...");
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
