pub mod routes;
pub mod websocket;

use axum::Router;
use crate::adapter::AdapterCommand;
use crate::state::SharedState;
use tokio::sync::{broadcast, mpsc};
use crate::adapter::PushEvent;

pub fn create_router(
    state: SharedState,
    cmd_tx: mpsc::Sender<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    devices: crate::devices::DeviceManager,
) -> Router {
    routes::router(state, cmd_tx, push_tx, devices)
}
