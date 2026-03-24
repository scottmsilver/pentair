mod backend;
mod commands;
mod connection;
mod output;

use clap::{Parser, Subcommand};

use crate::backend::{Backend, DaemonClient};

const DEFAULT_DAEMON_URL: &str = "http://localhost:8080";

#[derive(Parser)]
#[command(name = "pentair", about = "Pentair ScreenLogic pool controller")]
struct Cli {
    /// Adapter address (host:port). Overrides PENTAIR_HOST env and auto-discovery.
    #[arg(long, global = true, env = "PENTAIR_HOST")]
    host: Option<String>,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Connect directly to adapter (skip daemon)
    #[arg(long, global = true)]
    direct: bool,

    /// Daemon URL for non-direct mode
    #[arg(long, global = true, env = "PENTAIR_DAEMON_URL")]
    daemon_url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show pool status
    Status,
    /// Show firmware version
    Version,
    /// Discover adapters on the network
    Discover,
    /// Circuit control
    Circuit {
        #[command(subcommand)]
        action: CircuitAction,
    },
    /// Heat control
    Heat {
        #[command(subcommand)]
        action: HeatAction,
    },
    /// Light control
    Light {
        /// Light command (e.g., party, caribbean, blue, off)
        command: String,
    },
    /// Chemistry data
    Chem,
    /// Chlorinator config
    Chlor {
        #[command(subcommand)]
        action: Option<ChlorAction>,
    },
    /// Schedule management
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },
    /// Query controller history data
    History {
        /// Number of hours to look back from the controller's current time
        #[arg(long, default_value = "24")]
        hours: i64,
    },
    /// Pump status
    Pump {
        /// Pump index (0-7)
        #[arg(default_value = "0")]
        index: i32,
    },
    /// Weather forecast
    Weather,
    /// Cancel all delays
    CancelDelay,
    /// Send raw protocol message
    Raw {
        /// Action code (decimal)
        action: u16,
        /// Hex-encoded payload (optional)
        payload: Option<String>,
    },
}

#[derive(Subcommand)]
enum CircuitAction {
    /// List all circuits
    List,
    /// Turn a circuit on
    On {
        /// Circuit name or ID
        circuit: String,
    },
    /// Turn a circuit off
    Off {
        /// Circuit name or ID
        circuit: String,
    },
}

#[derive(Subcommand)]
enum HeatAction {
    /// Show heat status
    Status,
    /// Set heat set point
    Set {
        /// Body: pool or spa
        body: String,
        /// Temperature
        temp: i32,
    },
    /// Set heat mode
    Mode {
        /// Body: pool or spa
        body: String,
        /// Mode: off, solar, solar-preferred, heat-pump
        mode: String,
    },
    /// Set cool set point
    Cool {
        /// Body: pool or spa
        body: String,
        /// Temperature
        temp: i32,
    },
}

#[derive(Subcommand)]
enum ChlorAction {
    /// Set chlorinator output
    Set {
        /// Pool output percentage (0-100)
        pool: i32,
        /// Spa output percentage (0-100)
        spa: i32,
    },
}

#[derive(Subcommand)]
enum ScheduleAction {
    /// List schedules
    List {
        /// Type: recurring or runonce
        #[arg(default_value = "recurring")]
        schedule_type: String,
    },
    /// Add a new schedule
    Add {
        /// Type: recurring or runonce
        #[arg(default_value = "recurring")]
        schedule_type: String,
    },
    /// Delete a schedule
    Delete {
        /// Schedule ID
        id: i32,
    },
    /// Set schedule parameters
    Set {
        /// Schedule ID
        id: i32,
        /// Circuit name or ID
        circuit: String,
        /// Start time (HH:MM)
        start: String,
        /// Stop time (HH:MM)
        stop: String,
        /// Days (e.g., "MoTuWeThFrSaSu" or "daily" or "weekdays")
        days: String,
        /// Heat set point (optional)
        #[arg(long, default_value = "0")]
        heat: i32,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if let Err(e) = run(cli).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Discover => {
            commands::discover::run(cli.json).await?;
        }
        _ => {
            let mut backend = if cli.direct {
                connection::resolve_and_connect(cli.host.as_deref()).await?
            } else {
                let url = cli
                    .daemon_url
                    .unwrap_or_else(|| DEFAULT_DAEMON_URL.to_string());
                Backend::Daemon(DaemonClient::new(url))
            };
            let result = run_connected(&cli.command, &mut backend, cli.json).await;
            backend.disconnect().await?;
            result?;
        }
    }
    Ok(())
}

async fn run_connected(
    command: &Commands,
    backend: &mut Backend,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Status => commands::status::run(backend, json).await?,
        Commands::Version => commands::version::run(backend, json).await?,
        Commands::Circuit { action } => match action {
            CircuitAction::List => commands::circuit::list(backend, json).await?,
            CircuitAction::On { circuit } => {
                commands::circuit::set(backend, circuit, true, json).await?
            }
            CircuitAction::Off { circuit } => {
                commands::circuit::set(backend, circuit, false, json).await?
            }
        },
        Commands::Heat { action } => match action {
            HeatAction::Status => commands::heat::status(backend, json).await?,
            HeatAction::Set { body, temp } => {
                commands::heat::set(backend, body, *temp, json).await?
            }
            HeatAction::Mode { body, mode } => {
                commands::heat::mode(backend, body, mode, json).await?
            }
            HeatAction::Cool { body, temp } => {
                commands::heat::cool(backend, body, *temp, json).await?
            }
        },
        Commands::Light { command } => commands::light::run(backend, command, json).await?,
        Commands::Chem => commands::chem::run(backend, json).await?,
        Commands::Chlor { action } => match action {
            Some(ChlorAction::Set { pool, spa }) => {
                commands::chlor::set(backend, *pool, *spa, json).await?
            }
            None => commands::chlor::show(backend, json).await?,
        },
        Commands::Schedule { action } => match action {
            ScheduleAction::List { schedule_type } => {
                commands::schedule::list(backend, schedule_type, json).await?
            }
            ScheduleAction::Add { schedule_type } => {
                commands::schedule::add(backend, schedule_type, json).await?
            }
            ScheduleAction::Delete { id } => commands::schedule::delete(backend, *id, json).await?,
            ScheduleAction::Set {
                id,
                circuit,
                start,
                stop,
                days,
                heat,
            } => {
                commands::schedule::set(backend, *id, circuit, start, stop, days, *heat, json)
                    .await?
            }
        },
        Commands::History { hours } => commands::history::run(backend, *hours, json).await?,
        Commands::Pump { index } => commands::pump::run(backend, *index, json).await?,
        Commands::Weather => commands::weather::run(backend, json).await?,
        Commands::CancelDelay => commands::cancel::run(backend, json).await?,
        Commands::Raw { action, payload } => {
            commands::raw::run(backend, *action, payload.as_deref(), json).await?
        }
        Commands::Discover => unreachable!(),
    }
    Ok(())
}
