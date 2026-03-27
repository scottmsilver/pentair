mod clusters;
mod config;
mod convert;
mod daemon_client;
mod light_modes;
mod matter_bridge;
mod mode_select_handler;
mod pool_types;
mod state;
mod thermostat_handler;
mod ws_subscriber;

use std::sync::Arc;

use clap::Parser;
use config::Config;
use daemon_client::DaemonClient;
use light_modes::LightModeMap;
use matter_bridge::{Command, SharedState};
use state::MatterState;

#[derive(Parser)]
#[command(name = "pentair-matter", about = "Matter bridge for Pentair pool control")]
struct Cli {
    #[arg(long, env = "PENTAIR_DAEMON_URL")]
    daemon_url: Option<String>,
    #[arg(long)]
    discriminator: Option<u16>,
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// Delete fabric state and re-enter commissioning mode
    #[arg(long)]
    reset_fabric: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config = Config::load(cli.daemon_url, cli.discriminator, cli.config);

    if cli.reset_fabric {
        if config.fabric_path.exists() {
            std::fs::remove_file(&config.fabric_path).expect("failed to delete fabric file");
            tracing::info!(path = %config.fabric_path.display(), "Fabric state deleted — will enter commissioning mode");
        } else {
            tracing::info!(path = %config.fabric_path.display(), "No fabric file found — already in commissioning mode");
        }
    }

    tracing::info!(daemon_url = %config.daemon_url, "pentair-matter starting");

    let daemon = DaemonClient::new(&config.daemon_url);
    let shared = Arc::new(SharedState::new());

    // Fetch initial state so the bridge starts with real values
    let mode_map = match daemon.get_pool().await {
        Ok(ps) => {
            let modes = ps
                .lights
                .as_ref()
                .map(|l| l.available_modes.as_slice())
                .unwrap_or(&[]);
            let mm = LightModeMap::from_available_modes(modes);
            let initial = MatterState::from_pool_system(&ps, &mm);
            shared.update(initial);
            tracing::info!("Initial state loaded from daemon");
            mm
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch initial state — starting with defaults");
            LightModeMap::from_available_modes(&[])
        }
    };

    // Spawn the Matter bridge on a dedicated thread
    let (bridge_handle, cmd_rx) = matter_bridge::spawn_bridge(&config, shared.clone(), mode_map.clone());

    // Spawn the WebSocket subscriber to keep state in sync
    let ws_shared = shared.clone();
    let ws_mode_map = mode_map.clone();
    let ws_url = daemon.ws_url();
    tokio::spawn(async move {
        ws_subscriber::run_ws_subscriber(ws_url, ws_shared, ws_mode_map).await;
    });

    // Spawn the command dispatcher on a blocking thread (uses std::sync::mpsc::recv)
    let cmd_daemon = daemon.clone();
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(dispatch_commands(cmd_rx, cmd_daemon));
    });

    // Wait for bridge thread (or ctrl+c)
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received SIGINT, shutting down");
        }
        result = tokio::task::spawn_blocking(move || bridge_handle.join()) => {
            match result {
                Ok(Ok(Ok(()))) => tracing::info!("Matter bridge shut down cleanly"),
                Ok(Ok(Err(e))) => tracing::error!("Matter bridge error: {}", e),
                Ok(Err(_)) => tracing::error!("Matter bridge thread panicked"),
                Err(e) => tracing::error!("spawn_blocking error: {}", e),
            }
        }
    }
}

/// Process commands from the Matter thread by calling the daemon REST API.
async fn dispatch_commands(cmd_rx: std::sync::mpsc::Receiver<Command>, daemon: DaemonClient) {
    loop {
        // Use recv_timeout to avoid blocking the tokio thread permanently
        match cmd_rx.recv() {
            Ok(cmd) => {
                tracing::info!(command = ?cmd, "Dispatching command to daemon");
                let result = match cmd {
                    Command::SpaOn => daemon.post("/api/spa/on", None).await,
                    Command::SpaOff => daemon.post("/api/spa/off", None).await,
                    Command::JetsOn => daemon.post("/api/spa/jets/on", None).await,
                    Command::JetsOff => daemon.post("/api/spa/jets/off", None).await,
                    Command::LightsOn => daemon.post("/api/lights/on", None).await,
                    Command::LightsOff => daemon.post("/api/lights/off", None).await,
                    Command::SetSpaSetpoint(f) => {
                        daemon
                            .post(
                                "/api/spa/heat",
                                Some(serde_json::json!({"setpoint": f})),
                            )
                            .await
                    }
                    Command::SetLightMode(mode) => {
                        daemon
                            .post(
                                "/api/lights/mode",
                                Some(serde_json::json!({"mode": mode})),
                            )
                            .await
                    }
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "Command dispatch failed");
                }
            }
            Err(_) => {
                tracing::info!("Command channel closed, shutting down dispatcher");
                break;
            }
        }
    }
}
