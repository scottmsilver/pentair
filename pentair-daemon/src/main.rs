mod config;
mod state;
mod adapter;
mod api;

use std::path::PathBuf;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    // Load config
    let config_path = std::env::var("PENTAIR_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("pentair.toml"));
    let config = config::Config::load(&config_path).unwrap_or_default();

    info!("starting pentair-daemon, binding to {}", config.bind);

    let state = state::new_shared_state(config.associations.spa.clone());
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(32);
    let (push_tx, _push_rx) = tokio::sync::broadcast::channel(64);

    // Start adapter task
    let adapter_state = state.clone();
    let adapter_host = config.adapter_host.clone();
    let push_tx_adapter = push_tx.clone();
    tokio::spawn(async move {
        adapter::run_adapter(adapter_host, adapter_state, cmd_rx, push_tx_adapter).await;
    });

    // Start HTTP server
    let router = api::create_router(state, cmd_tx, push_tx);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    info!("listening on {}", config.bind);

    // Advertise via mDNS for app discovery
    let mdns = mdns_sd::ServiceDaemon::new().expect("failed to start mDNS");
    let hostname = hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let bind_port = listener.local_addr()?.port();
    let service_info = mdns_sd::ServiceInfo::new(
        "_pentair._tcp.local.",
        "Pentair Pool",
        &format!("{}.local.", hostname),
        "",
        bind_port,
        None,
    )
    .expect("failed to create mDNS service");
    mdns.register(service_info)
        .expect("failed to register mDNS service");
    info!("mDNS: advertising _pentair._tcp on port {}", bind_port);

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
