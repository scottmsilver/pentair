mod adapter;
mod api;
mod apns;
mod config;
mod devices;
mod fcm;
mod heat;
mod network_secret;
mod scheduled_heat;
mod scenes;
mod spa_notifications;
mod state;

use std::path::PathBuf;
use tracing::{info, warn};

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
    let config = match config::Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to parse config {:?}: {}, using defaults", config_path, e);
            config::Config::default()
        }
    };

    info!("starting pentair-daemon, binding to {}", config.bind);

    let heating_history_path = resolve_history_path(&config.heating.history_path);
    let state = state::new_shared_state(
        config.associations.spa.clone(),
        config.heating.clone(),
        config.notifications.spa_heat.clone(),
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

    // Load scheduled heat state (persisted timers)
    let scheduled_heat_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pentair")
        .join("scheduled-heat.json");
    let scheduled_heat = scheduled_heat::new_shared_scheduled_heat(scheduled_heat_path);

    // Create FCM sender (None if not configured)
    let fcm_sender = fcm::FcmSender::new(
        config.fcm.project_id.clone(),
        &config.fcm.service_account,
        devices.clone(),
    )
    .map(std::sync::Arc::new);

    // Create APNs sender (None if not configured)
    let apns_sender = apns::ApnsSender::new(&config.apns, devices.clone())
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
            apns_sender,
        )
        .await;
    });

    // Resume any persisted heat schedule
    scheduled_heat::spawn_heat_timer_full(
        scheduled_heat.clone(),
        state.clone(),
        cmd_tx.clone(),
    )
    .await;

    // Resolve scenes (use configured scenes, or defaults if none configured)
    let scene_store = scenes::SceneStore::new(scenes::resolve_scenes(&config.scenes));
    info!("loaded {} scene(s)", scene_store.list().len());

    // Load or generate network secret for LAN-based remote access approval
    let network_secret = network_secret::load_or_create();

    // Discover public IP for proof-of-presence in the approval flow.
    // When a user approves remote access via the tunnel, we compare their
    // CF-Connecting-IP to our public IP to verify they're on the home network.
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let public_ip = match http.get("https://api.ipify.org").send().await {
        Ok(resp) => match resp.text().await {
            Ok(ip) => {
                let ip = ip.trim().to_string();
                info!("public IP for approval flow: {}", ip);
                Some(ip)
            }
            Err(e) => {
                warn!("failed to read public IP response: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("failed to discover public IP: {}", e);
            None
        }
    };

    // Start HTTP server
    let router = api::create_router(state, cmd_tx, push_tx, devices, scheduled_heat, scene_store, network_secret, public_ip, config.web.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    let local_addr = listener.local_addr()?;
    info!("listening on {}", config.bind);

    if !local_addr.ip().is_loopback() {
        // Advertise via mDNS for app discovery.
        match mdns_sd::ServiceDaemon::new() {
            Ok(mdns) => {
                let hostname = hostname::get()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                match mdns_sd::ServiceInfo::new(
                    "_pentair._tcp.local.",
                    "Pentair Pool",
                    &format!("{}.local.", hostname),
                    "",
                    local_addr.port(),
                    None,
                ) {
                    Ok(service_info) => {
                        let service_info = service_info.enable_addr_auto();
                        if let Err(e) = mdns.register(service_info) {
                            warn!("mDNS: failed to register service: {}, continuing without mDNS", e);
                        } else {
                            info!(
                                "mDNS: advertising _pentair._tcp on port {} as {}.local.",
                                local_addr.port(),
                                hostname
                            );
                        }
                    }
                    Err(e) => {
                        warn!("mDNS: failed to create service info: {}, continuing without mDNS", e);
                    }
                }
            }
            Err(e) => {
                warn!("mDNS: failed to start daemon: {}, continuing without mDNS", e);
            }
        }
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
