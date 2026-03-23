mod adapter;
mod api;
mod config;
mod devices;
mod fcm;
mod heat;
mod state;

use std::path::PathBuf;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Load config
    let config_path = std::env::var("PENTAIR_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("pentair.toml"));
    let config = config::Config::load(&config_path).unwrap_or_default();

    info!("starting pentair-daemon, binding to {}", config.bind);

    let heating_history_path = resolve_history_path(&config.heating.history_path);
    let state = state::new_shared_state(
        config.associations.spa.clone(),
        config.heating.clone(),
        heating_history_path,
    );
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(32);
    let (push_tx, _push_rx) = tokio::sync::broadcast::channel(64);

    // Load device token store
    let devices_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pentair")
        .join("devices.json");
    let devices = devices::DeviceManager::load(devices_path);

    // Create FCM sender (None if not configured)
    let fcm_sender = fcm::FcmSender::new(
        config.fcm.project_id.clone(),
        &config.fcm.service_account,
        devices.clone(),
    )
    .map(std::sync::Arc::new);

    // Start adapter task
    let adapter_state = state.clone();
    let adapter_host = config.adapter_host.clone();
    let push_tx_adapter = push_tx.clone();
    tokio::spawn(async move {
        adapter::run_adapter(
            adapter_host,
            adapter_state,
            cmd_rx,
            push_tx_adapter,
            fcm_sender,
        )
        .await;
    });

    // Start HTTP server
    let router = api::create_router(state, cmd_tx, push_tx, devices);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    let local_addr = listener.local_addr()?;
    info!("listening on {}", config.bind);

    if !local_addr.ip().is_loopback() {
        // Advertise via mDNS for app discovery.
        let mdns = mdns_sd::ServiceDaemon::new().expect("failed to start mDNS");
        let hostname = hostname::get()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let service_info = mdns_sd::ServiceInfo::new(
            "_pentair._tcp.local.",
            "Pentair Pool",
            &format!("{}.local.", hostname),
            "",
            local_addr.port(),
            None,
        )
        .expect("failed to create mDNS service")
        .enable_addr_auto();
        mdns.register(service_info)
            .expect("failed to register mDNS service");
        info!(
            "mDNS: advertising _pentair._tcp on port {} as {}.local.",
            local_addr.port(),
            hostname
        );
    } else {
        info!(
            "mDNS: skipped because listener is loopback-only ({})",
            local_addr
        );
    }

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("shutting down");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

fn resolve_history_path(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped);
    }

    PathBuf::from(raw)
}
