use crate::state::SharedState;
use pentair_client::client::Client;
use pentair_client::discovery::discover;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};
use std::time::Duration;

/// Commands sent from the API to the adapter task
pub enum AdapterCommand {
    SetCircuit { circuit_id: i32, state: bool, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    SetHeatSetpoint { body_type: i32, temp: i32, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    SetHeatMode { body_type: i32, mode: i32, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    SetCoolSetpoint { body_type: i32, temp: i32, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    SetLightCommand { command: i32, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    SetScgConfig { pool: i32, spa: i32, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    CancelDelay { reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    RefreshAll { reply: tokio::sync::oneshot::Sender<Result<(), String>> },
}

/// Push events sent to WebSocket subscribers
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum PushEvent {
    StatusChanged,
    ChemistryChanged,
    ConfigChanged,
}

pub async fn run_adapter(
    adapter_host: String,
    state: SharedState,
    mut cmd_rx: mpsc::Receiver<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!("connecting to adapter...");

        let addr = if adapter_host.is_empty() {
            // Auto-discover
            match discover().await {
                Ok(resp) => {
                    let addr = format!("{}.{}.{}.{}:{}", resp.ip[0], resp.ip[1], resp.ip[2], resp.ip[3], resp.port);
                    info!("discovered adapter at {}", addr);
                    addr
                }
                Err(e) => {
                    error!("discovery failed: {}, retrying in {:?}", e, backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            }
        } else {
            let host = &adapter_host;
            if host.contains(':') { host.to_string() } else { format!("{}:80", host) }
        };

        match Client::connect(&addr).await {
            Ok(mut client) => {
                info!("connected to adapter at {}", addr);
                backoff = Duration::from_secs(1);  // Reset backoff on success

                // Initial data load
                refresh_all(&mut client, &state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);

                // Main loop: handle commands and periodic refresh
                let mut refresh_interval = tokio::time::interval(Duration::from_secs(30));
                let mut ping_interval = tokio::time::interval(Duration::from_secs(60));

                loop {
                    tokio::select! {
                        Some(cmd) = cmd_rx.recv() => {
                            let result = handle_command(&mut client, &state, &push_tx, cmd).await;
                            if result.is_err() {
                                error!("adapter connection lost during command");
                                break;
                            }
                        }
                        _ = refresh_interval.tick() => {
                            if let Err(e) = refresh_status(&mut client, &state).await {
                                error!("refresh failed: {}", e);
                                break;
                            }
                            let _ = push_tx.send(PushEvent::StatusChanged);
                        }
                        _ = ping_interval.tick() => {
                            if let Err(e) = client.ping().await {
                                error!("ping failed: {}", e);
                                break;
                            }
                        }
                    }
                }

                // Try graceful disconnect
                let _ = client.disconnect().await;
            }
            Err(e) => {
                error!("connection failed: {}, retrying in {:?}", e, backoff);
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn handle_command(
    client: &mut Client,
    state: &SharedState,
    push_tx: &broadcast::Sender<PushEvent>,
    cmd: AdapterCommand,
) -> Result<(), ()> {
    match cmd {
        AdapterCommand::SetCircuit { circuit_id, state: on, reply } => {
            let result = client.set_circuit(circuit_id, on).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_status(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetHeatSetpoint { body_type, temp, reply } => {
            let result = client.set_heat_setpoint(body_type, temp).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_status(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetHeatMode { body_type, mode, reply } => {
            let result = client.set_heat_mode(body_type, mode).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_status(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetCoolSetpoint { body_type, temp, reply } => {
            let result = client.set_cool_setpoint(body_type, temp).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok { Ok(()) } else { Err(()) }
        }
        AdapterCommand::SetLightCommand { command, reply } => {
            let result = client.set_light_command(command).await;
            let ok = result.is_ok();
            if ok {
                // Track fire-and-forget light mode in daemon state.
                let mode_name = match command {
                    0 => "off", 1 => "on", 2 => "set", 3 => "sync",
                    4 => "swim", 5 => "party", 6 => "romantic", 7 => "caribbean",
                    8 => "american", 9 => "sunset", 10 => "royal", 11 => "save",
                    12 => "recall", 13 => "blue", 14 => "green", 15 => "red",
                    16 => "white", 17 => "purple", _ => "unknown",
                };
                state.write().await.light_mode = Some(mode_name.to_string());
            }
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok { Ok(()) } else { Err(()) }
        }
        AdapterCommand::SetScgConfig { pool, spa, reply } => {
            let result = client.set_scg_config(pool, spa).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok { Ok(()) } else { Err(()) }
        }
        AdapterCommand::CancelDelay { reply } => {
            let result = client.cancel_delay().await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok { Ok(()) } else { Err(()) }
        }
        AdapterCommand::RefreshAll { reply } => {
            refresh_all(client, state).await;
            let _ = push_tx.send(PushEvent::StatusChanged);
            let _ = reply.send(Ok(()));
            Ok(())
        }
    }
}

async fn refresh_all(client: &mut Client, state: &SharedState) {
    let _ = refresh_status(client, state).await;

    if let Ok(config) = client.get_controller_config().await {
        state.write().await.config = Some(config);
    }
    if let Ok(chem) = client.get_chem_data().await {
        state.write().await.chem = Some(chem);
    }
    if let Ok(scg) = client.get_scg_config().await {
        state.write().await.scg = Some(scg);
    }
    if let Ok(version) = client.get_version().await {
        state.write().await.version = Some(version);
    }
    // All pumps (needed for topology discovery)
    for i in 0..8 {
        if let Ok(pump) = client.get_pump_status(i).await {
            state.write().await.pumps[i as usize] = Some(pump);
        }
    }

    // Rebuild semantic model + circuit map after full refresh
    state.write().await.rebuild_semantic();
}

async fn refresh_status(client: &mut Client, state: &SharedState) -> Result<(), pentair_client::error::ClientError> {
    let status = client.get_status().await?;
    state.write().await.status = Some(status);
    Ok(())
}
