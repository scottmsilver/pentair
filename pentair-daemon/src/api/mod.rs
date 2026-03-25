pub mod routes;
pub mod websocket;

use crate::adapter::AdapterCommand;
use crate::adapter::PushEvent;
use crate::scenes::SceneStore;
use crate::state::SharedState;
use axum::Router;
use tokio::sync::{broadcast, mpsc};

pub fn create_router(
    state: SharedState,
    cmd_tx: mpsc::Sender<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    devices: crate::devices::DeviceManager,
    scenes: SceneStore,
) -> Router {
    routes::router(state, cmd_tx, push_tx, devices, scenes)
}
