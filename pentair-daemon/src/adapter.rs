use crate::fcm::FcmSender;
use crate::state::SharedState;
use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, NaiveDateTime, Timelike};
use pentair_client::client::Client;
use pentair_client::discovery::discover;
use pentair_protocol::semantic::PoolSystem;
use pentair_protocol::types::SLDateTime;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};

/// Commands sent from the API to the adapter task
pub enum AdapterCommand {
    SetCircuit {
        circuit_id: i32,
        state: bool,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetHeatSetpoint {
        body_type: i32,
        temp: i32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetHeatMode {
        body_type: i32,
        mode: i32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetCoolSetpoint {
        body_type: i32,
        temp: i32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetLightCommand {
        command: i32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetScgConfig {
        pool: i32,
        spa: i32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    CancelDelay {
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    RefreshAll {
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}

/// Push triggers sent to WebSocket subscribers.
///
/// The websocket API now sends full semantic `PoolSystem` snapshots, so the
/// broadcast channel only needs a generic "state changed" trigger.
#[derive(Debug, Clone)]
pub enum PushEvent {
    StatusChanged,
}

pub async fn run_adapter(
    adapter_host: String,
    state: SharedState,
    mut cmd_rx: mpsc::Receiver<AdapterCommand>,
    push_tx: broadcast::Sender<PushEvent>,
    fcm: Option<Arc<FcmSender>>,
) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!("connecting to adapter...");

        let addr = if adapter_host.is_empty() {
            // Auto-discover
            match discover().await {
                Ok(resp) => {
                    let addr = format!(
                        "{}.{}.{}.{}:{}",
                        resp.ip[0], resp.ip[1], resp.ip[2], resp.ip[3], resp.port
                    );
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
            if host.contains(':') {
                host.to_string()
            } else {
                format!("{}:80", host)
            }
        };

        match Client::connect(&addr).await {
            Ok(mut client) => {
                info!("connected to adapter at {}", addr);
                backoff = Duration::from_secs(1); // Reset backoff on success

                // Initial data load
                refresh_all(&mut client, &state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);

                let mut previous_system: Option<PoolSystem> = state.read().await.pool_system();

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
                            // Check for transitions after commands too
                            let current = state.read().await.pool_system();
                            detect_transitions(&previous_system, &current, &fcm).await;
                            previous_system = current;
                        }
                        _ = refresh_interval.tick() => {
                            if let Err(e) = refresh_runtime_state(&mut client, &state).await {
                                error!("refresh failed: {}", e);
                                break;
                            }
                            let _ = push_tx.send(PushEvent::StatusChanged);
                            let current = state.read().await.pool_system();
                            detect_transitions(&previous_system, &current, &fcm).await;
                            previous_system = current;
                        }
                        _ = ping_interval.tick() => {
                            if let Err(e) = client.ping().await {
                                error!("ping failed: {}", e);
                                break;
                            }
                        }
                    }
                }

                // Connection lost — notify
                if let Some(ref sender) = fcm {
                    sender
                        .send("Connection Lost", "Lost connection to pool adapter")
                        .await;
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
        AdapterCommand::SetCircuit {
            circuit_id,
            state: on,
            reply,
        } => {
            let result = client.set_circuit(circuit_id, on).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_runtime_state(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetHeatSetpoint {
            body_type,
            temp,
            reply,
        } => {
            let result = client.set_heat_setpoint(body_type, temp).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_runtime_state(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetHeatMode {
            body_type,
            mode,
            reply,
        } => {
            let result = client.set_heat_mode(body_type, mode).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                let _ = refresh_runtime_state(client, state).await;
                let _ = push_tx.send(PushEvent::StatusChanged);
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetCoolSetpoint {
            body_type,
            temp,
            reply,
        } => {
            let result = client.set_cool_setpoint(body_type, temp).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetLightCommand { command, reply } => {
            let result = client.set_light_command(command).await;
            let ok = result.is_ok();
            if ok {
                // Track fire-and-forget light mode in daemon state.
                let mode_name = match command {
                    0 => "off",
                    1 => "on",
                    2 => "set",
                    3 => "sync",
                    4 => "swim",
                    5 => "party",
                    6 => "romantic",
                    7 => "caribbean",
                    8 => "american",
                    9 => "sunset",
                    10 => "royal",
                    11 => "save",
                    12 => "recall",
                    13 => "blue",
                    14 => "green",
                    15 => "red",
                    16 => "white",
                    17 => "purple",
                    _ => "unknown",
                };
                state.write().await.light_mode = Some(mode_name.to_string());
            }
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::SetScgConfig { pool, spa, reply } => {
            let result = client.set_scg_config(pool, spa).await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                Ok(())
            } else {
                Err(())
            }
        }
        AdapterCommand::CancelDelay { reply } => {
            let result = client.cancel_delay().await;
            let ok = result.is_ok();
            let _ = reply.send(result.map_err(|e| e.to_string()));
            if ok {
                Ok(())
            } else {
                Err(())
            }
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
    let _ = refresh_runtime_state(client, state).await;

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

    // Rebuild semantic model + circuit map after full refresh
    state.write().await.refresh_semantic_state();
    backfill_last_reliable_from_history(client, state).await;
}

async fn refresh_runtime_state(
    client: &mut Client,
    state: &SharedState,
) -> Result<(), pentair_client::error::ClientError> {
    let status = client.get_status().await?;
    {
        let mut guard = state.write().await;
        guard.status = Some(status);
    }

    refresh_pumps(client, state).await;

    let mut guard = state.write().await;
    guard.refresh_semantic_state();
    Ok(())
}

async fn refresh_pumps(client: &mut Client, state: &SharedState) {
    for i in 0..8 {
        if let Ok(pump) = client.get_pump_status(i).await {
            state.write().await.pumps[i as usize] = Some(pump);
        }
    }
}

async fn backfill_last_reliable_from_history(client: &mut Client, state: &SharedState) {
    let Ok(system_time) = client.get_system_time().await else {
        warn!("failed to fetch controller time for history backfill");
        return;
    };
    let Ok(end) = sl_to_naive(&system_time.time) else {
        warn!("failed to parse controller time for history backfill");
        return;
    };
    let start = end - ChronoDuration::hours(48);
    let Ok(start_sl) = naive_to_sl(&start) else {
        warn!("failed to build history start time for backfill");
        return;
    };

    let Ok(history) = client
        .get_history(&start_sl, &system_time.time, client.client_id())
        .await
    else {
        warn!("failed to fetch controller history for backfill");
        return;
    };

    let pool_spa_shared_pump = state
        .read()
        .await
        .pool_system()
        .map(|system| system.system.pool_spa_shared_pump)
        .unwrap_or(false);

    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    state
        .write()
        .await
        .heat
        .seed_last_reliable_from_controller_history(
            &history,
            &system_time.time,
            now_unix_ms,
            pool_spa_shared_pump,
        );
}

fn sl_to_naive(time: &SLDateTime) -> Result<NaiveDateTime, ()> {
    let Some(date) = NaiveDate::from_ymd_opt(time.year as i32, time.month as u32, time.day as u32)
    else {
        return Err(());
    };
    date.and_hms_milli_opt(
        time.hour as u32,
        time.minute as u32,
        time.second as u32,
        time.millisecond as u32,
    )
    .ok_or(())
}

fn naive_to_sl(dt: &NaiveDateTime) -> Result<SLDateTime, ()> {
    Ok(SLDateTime {
        year: dt.year().try_into().map_err(|_| ())?,
        month: dt.month().try_into().map_err(|_| ())?,
        day_of_week: dt
            .weekday()
            .num_days_from_sunday()
            .try_into()
            .map_err(|_| ())?,
        day: dt.day().try_into().map_err(|_| ())?,
        hour: dt.hour().try_into().map_err(|_| ())?,
        minute: dt.minute().try_into().map_err(|_| ())?,
        second: dt.second().try_into().map_err(|_| ())?,
        millisecond: dt
            .and_utc()
            .timestamp_subsec_millis()
            .try_into()
            .map_err(|_| ())?,
    })
}

// ─── FCM event detection ────────────────────────────────────────────────

fn spa_is_ready(system: &PoolSystem) -> bool {
    system
        .spa
        .as_ref()
        .map_or(false, |spa| spa.on && spa.temperature >= spa.setpoint)
}

async fn detect_transitions(
    previous: &Option<PoolSystem>,
    current: &Option<PoolSystem>,
    fcm: &Option<Arc<FcmSender>>,
) {
    let sender = match fcm {
        Some(s) => s,
        None => return,
    };
    let (prev, curr) = match (previous, current) {
        (Some(p), Some(c)) => (p, c),
        _ => return,
    };

    // Spa ready: spa on + temp >= setpoint, transitioning from not-ready to ready
    if spa_is_ready(curr) && !spa_is_ready(prev) {
        if let Some(ref spa) = curr.spa {
            sender
                .send(
                    "Spa Ready",
                    &format!(
                        "Spa has reached {}{}.",
                        spa.temperature, curr.system.temp_unit
                    ),
                )
                .await;
        }
    }

    // Freeze protection activated
    if curr.system.freeze_protection && !prev.system.freeze_protection {
        sender
            .send("Freeze Protection", "Freeze protection has been activated.")
            .await;
    }

    // Heater started (spa)
    if let (Some(prev_spa), Some(curr_spa)) = (&prev.spa, &curr.spa) {
        if prev_spa.heating == "off" && curr_spa.heating != "off" {
            sender
                .send(
                    "Spa Heater Started",
                    &format!("Spa heater is now active ({}).", curr_spa.heating),
                )
                .await;
        }
    }

    // Heater started (pool)
    if let (Some(prev_pool), Some(curr_pool)) = (&prev.pool, &curr.pool) {
        if prev_pool.heating == "off" && curr_pool.heating != "off" {
            sender
                .send(
                    "Pool Heater Started",
                    &format!("Pool heater is now active ({}).", curr_pool.heating),
                )
                .await;
        }
    }
}
