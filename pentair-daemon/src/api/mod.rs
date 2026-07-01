pub mod routes;
pub mod websocket;

use crate::adapter::AdapterCommand;
use crate::adapter::PushEvent;
use crate::scheduled_heat::SharedScheduledHeat;
use crate::scenes::SceneStore;
use crate::state::SharedState;
use axum::Router;
use tokio::sync::{broadcast, mpsc};

#[allow(clippy::too_many_arguments)]
pub fn create_router(
    state: SharedState,
    cmd_tx: mpsc::Sender<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    devices: crate::devices::DeviceManager,
    scheduled_heat: SharedScheduledHeat,
    scenes: SceneStore,
    network_secret: String,
    public_ip: Option<String>,
    web: crate::config::WebConfig,
    comfort_plan: crate::config::ComfortPlanConfig,
) -> Router {
    routes::router(state, cmd_tx, push_tx, devices, scheduled_heat, scenes, network_secret, public_ip, web, comfort_plan)
}
