//! Matter bridge for Pentair pool system.
//!
//! Runs the rs-matter stack on a dedicated OS thread (using `futures_lite::future::block_on`),
//! exposing three bridged endpoints:
//!   - Endpoint 2: Spa (OnOff)
//!   - Endpoint 3: Jets (OnOff)
//!   - Endpoint 4: Lights (OnOff)
//!
//! Endpoint 0 = root node, Endpoint 1 = aggregator.
//!
//! Communication with the tokio side happens via:
//! - `std::sync::mpsc::Sender<Command>` for matter→tokio commands
//! - `SharedState` (Arc<Mutex<MatterState>> + AtomicBool dirty flag) for tokio→matter state

use core::cell::Cell;
use core::pin::pin;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use embassy_futures::select::{select, select4};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Duration;

use rs_matter::crypto::{default_crypto, Crypto};
use rs_matter::dm::clusters::desc::{self, ClusterHandler as _};
use rs_matter::dm::clusters::level_control::LevelControlHooks;
use rs_matter::dm::clusters::net_comm::NetworkType;
use rs_matter::dm::clusters::on_off::{
    self, EffectVariantEnum, OnOffHooks, OutOfBandMessage, StartUpOnOffEnum,
};
use rs_matter::dm::devices::test::{DAC_PRIVKEY, TEST_DEV_ATT};
use rs_matter::dm::devices::{DEV_TYPE_AGGREGATOR, DEV_TYPE_BRIDGED_NODE, DEV_TYPE_ON_OFF_LIGHT};
use rs_matter::dm::DeviceType;

/// Matter Thermostat device type (Matter App Cluster Spec §9.5, ID 0x0301).
/// Not defined in rs-matter's built-in device list.
const DEV_TYPE_THERMOSTAT: DeviceType = DeviceType {
    dtype: 0x0301,
    drev: 2,
};
use rs_matter::dm::endpoints;
use rs_matter::dm::events::DefaultEvents;
use rs_matter::dm::networks::unix::UnixNetifs;
use rs_matter::dm::subscriptions::DefaultSubscriptions;
use rs_matter::dm::IMBuffer;
use rs_matter::dm::{
    Async, AsyncHandler, AsyncMetadata, Cluster, DataModel, Dataver, EmptyHandler, Endpoint,
    EpClMatcher, InvokeContext, Node, ReadContext,
};
use rs_matter::error::Error;
use rs_matter::pairing::qr::QrTextType;
use rs_matter::pairing::DiscoveryCapabilities;
use rs_matter::persist::{Psm, NO_NETWORKS};
use rs_matter::respond::DefaultResponder;
use rs_matter::sc::pase::MAX_COMM_WINDOW_TIMEOUT_SECS;
use rs_matter::tlv::{Nullable, TLVBuilderParent, Utf8StrBuilder};
use rs_matter::transport::MATTER_SOCKET_BIND_ADDR;
use rs_matter::utils::select::Coalesce;
use rs_matter::utils::storage::pooled::PooledBuffers;
use rs_matter::{clusters, devices, with, Matter, MATTER_PORT};

use rs_matter::dm::clusters::basic_info::BasicInfoConfig;
use rs_matter::dm::devices::test::TEST_DEV_COMM;

pub use rs_matter::dm::clusters::decl::bridged_device_basic_information::{
    self, ClusterHandler as _, KeepActiveRequest,
};

use crate::clusters::mode_select::mode_select as mode_select_decl;
use crate::clusters::thermostat::thermostat as thermostat_decl;
use crate::config::Config;
use crate::mode_select_handler::LightModeSelectHandler;
use crate::state::MatterState;
use crate::thermostat_handler::SpaThermostatHandler;

// ---------------------------------------------------------------------------
// Commands sent from Matter thread to tokio thread
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Command {
    SpaOn,
    SpaOff,
    JetsOn,
    JetsOff,
    LightsOn,
    LightsOff,
    SetSpaSetpoint(i32), // Fahrenheit
    SetLightMode(String),
}

// ---------------------------------------------------------------------------
// Shared state between threads
// ---------------------------------------------------------------------------

pub struct SharedState {
    pub state: Mutex<MatterState>,
    /// Generation counter incremented on each state update.
    /// Consumers track their last-seen generation to detect changes.
    pub generation: AtomicU64,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MatterState::default()),
            generation: AtomicU64::new(0),
        }
    }

    pub fn update(&self, new_state: MatterState) {
        let mut s = self.state.lock().unwrap();
        if *s != new_state {
            *s = new_state;
            self.generation.fetch_add(1, Ordering::Release);
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge entry point
// ---------------------------------------------------------------------------

/// Start the Matter bridge on a dedicated OS thread.
///
/// Returns a `JoinHandle` and a command receiver for the tokio side to process.
pub fn spawn_bridge(
    config: &Config,
    shared: Arc<SharedState>,
    mode_map: crate::light_modes::LightModeMap,
) -> (
    std::thread::JoinHandle<Result<(), BridgeError>>,
    mpsc::Receiver<Command>,
) {
    let discriminator = config.discriminator;
    let fabric_path = config.fabric_path.clone();
    let (cmd_tx, cmd_rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("matter-bridge".into())
        .stack_size(550 * 1024)
        .spawn(move || run_matter(discriminator, fabric_path, shared, cmd_tx, mode_map))
        .expect("failed to spawn matter-bridge thread");

    (handle, cmd_rx)
}

/// Errors from the Matter bridge.
#[derive(Debug)]
pub enum BridgeError {
    Matter(Error),
    Io(std::io::Error),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Matter(e) => write!(f, "Matter error: {:?}", e),
            BridgeError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl From<Error> for BridgeError {
    fn from(e: Error) -> Self {
        BridgeError::Matter(e)
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        BridgeError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Device info
// ---------------------------------------------------------------------------

// TODO(v2): Replace test VID/PID/DAC with CSA-assigned credentials for Matter certification.
// Test defaults are recognized by chip-tool and Google Home in development mode only.
const DEV_DET: BasicInfoConfig<'static> = BasicInfoConfig {
    vid: 0xFFF1,
    pid: 0x8001,
    hw_ver: 1,
    hw_ver_str: "1",
    sw_ver: 1,
    sw_ver_str: "0.1.0",
    serial_no: "PENTAIR-001",
    product_name: "Pentair Pool Bridge",
    vendor_name: "Pentair-Matter",
    device_name: "PentairPool",
    ..BasicInfoConfig::new()
};

// ---------------------------------------------------------------------------
// Bridge topology
// ---------------------------------------------------------------------------

// Thermostat CLUSTER constant for use in the static Node definition.
const SPA_THERMOSTAT_CLUSTER: Cluster<'static> = thermostat_decl::FULL_CLUSTER
    .with_revision(7)
    .with_features(0x01) // Heating only
    .with_attrs(with!(
        required;
        thermostat_decl::AttributeId::LocalTemperature
            | thermostat_decl::AttributeId::ControlSequenceOfOperation
            | thermostat_decl::AttributeId::SystemMode
            | thermostat_decl::AttributeId::OccupiedHeatingSetpoint
            | thermostat_decl::AttributeId::AbsMinHeatSetpointLimit
            | thermostat_decl::AttributeId::AbsMaxHeatSetpointLimit
            | thermostat_decl::AttributeId::MinHeatSetpointLimit
            | thermostat_decl::AttributeId::MaxHeatSetpointLimit
    ))
    .with_cmds(with!(
        thermostat_decl::CommandId::SetpointRaiseLower
    ));

// ModeSelect CLUSTER constant for use in the static Node definition.
const LIGHTS_MODE_SELECT_CLUSTER: Cluster<'static> = mode_select_decl::FULL_CLUSTER
    .with_revision(2)
    .with_features(0)
    .with_attrs(with!(required))
    .with_cmds(with!(mode_select_decl::CommandId::ChangeToMode));

/// Endpoint 0 = root, 1 = aggregator, 2 = spa, 3 = jets, 4 = lights
const NODE: Node<'static> = Node {
    id: 0,
    endpoints: &[
        endpoints::root_endpoint(NetworkType::Ethernet),
        // Aggregator
        Endpoint {
            id: 1,
            device_types: devices!(DEV_TYPE_AGGREGATOR),
            clusters: clusters!(desc::DescHandler::CLUSTER),
        },
        // Spa (Thermostat + OnOff + Bridged)
        Endpoint {
            id: 2,
            device_types: devices!(DEV_TYPE_THERMOSTAT, DEV_TYPE_BRIDGED_NODE),
            clusters: clusters!(
                desc::DescHandler::CLUSTER,
                BridgedHandler::CLUSTER,
                PentairOnOffHooks::CLUSTER,
                SPA_THERMOSTAT_CLUSTER
            ),
        },
        // Jets (OnOff + Bridged)
        Endpoint {
            id: 3,
            device_types: devices!(DEV_TYPE_ON_OFF_LIGHT, DEV_TYPE_BRIDGED_NODE),
            clusters: clusters!(
                desc::DescHandler::CLUSTER,
                BridgedHandler::CLUSTER,
                PentairOnOffHooks::CLUSTER
            ),
        },
        // Lights (OnOff + ModeSelect + Bridged)
        Endpoint {
            id: 4,
            device_types: devices!(DEV_TYPE_ON_OFF_LIGHT, DEV_TYPE_BRIDGED_NODE),
            clusters: clusters!(
                desc::DescHandler::CLUSTER,
                BridgedHandler::CLUSTER,
                PentairOnOffHooks::CLUSTER,
                LIGHTS_MODE_SELECT_CLUSTER
            ),
        },
    ],
};

// ---------------------------------------------------------------------------
// OnOff hooks for Pentair endpoints
// ---------------------------------------------------------------------------

/// Which device this OnOff hooks instance controls.
#[derive(Clone, Copy, Debug)]
enum DeviceRole {
    Spa,
    Jets,
    Lights,
}

/// OnOff hooks that read state from SharedState and send commands via mpsc.
struct PentairOnOffHooks {
    role: DeviceRole,
    on: Cell<bool>,
    last_gen: Cell<u64>,
    shared: Arc<SharedState>,
    cmd_tx: mpsc::Sender<Command>,
}

impl PentairOnOffHooks {
    fn new(role: DeviceRole, shared: Arc<SharedState>, cmd_tx: mpsc::Sender<Command>) -> Self {
        let (initial, gen) = {
            let s = shared.state.lock().unwrap();
            let on = match role {
                DeviceRole::Spa => s.spa_on,
                DeviceRole::Jets => s.jets_on,
                DeviceRole::Lights => s.lights_on,
            };
            (on, shared.generation.load(Ordering::Acquire))
        };
        Self {
            role,
            on: Cell::new(initial),
            last_gen: Cell::new(gen),
            shared,
            cmd_tx,
        }
    }
}

use rs_matter::dm::clusters::decl::on_off as on_off_cluster;

impl OnOffHooks for PentairOnOffHooks {
    const CLUSTER: Cluster<'static> = on_off_cluster::FULL_CLUSTER
        .with_revision(6)
        .with_attrs(with!(
            required;
            on_off_cluster::AttributeId::OnOff
        ))
        .with_cmds(with!(
            on_off_cluster::CommandId::Off
                | on_off_cluster::CommandId::On
                | on_off_cluster::CommandId::Toggle
        ));

    fn on_off(&self) -> bool {
        self.on.get()
    }

    fn set_on_off(&self, on: bool) {
        self.on.set(on);
        let cmd = match (self.role, on) {
            (DeviceRole::Spa, true) => Command::SpaOn,
            (DeviceRole::Spa, false) => Command::SpaOff,
            (DeviceRole::Jets, true) => Command::JetsOn,
            (DeviceRole::Jets, false) => Command::JetsOff,
            (DeviceRole::Lights, true) => Command::LightsOn,
            (DeviceRole::Lights, false) => Command::LightsOff,
        };
        if let Err(e) = self.cmd_tx.send(cmd) {
            tracing::error!(role = ?self.role, error = %e, "failed to send command to daemon");
        }
    }

    fn start_up_on_off(&self) -> Nullable<StartUpOnOffEnum> {
        Nullable::none()
    }

    fn set_start_up_on_off(&self, _value: Nullable<StartUpOnOffEnum>) -> Result<(), Error> {
        Ok(())
    }

    async fn handle_off_with_effect(&self, _effect: EffectVariantEnum) {
        // No effects supported
    }

    /// Polls shared state for changes and notifies the handler.
    async fn run<F: Fn(OutOfBandMessage)>(&self, notify: F) {
        loop {
            embassy_time::Timer::after(Duration::from_millis(200)).await;
            let current_gen = self.shared.generation.load(Ordering::Acquire);
            if current_gen != self.last_gen.get() {
                self.last_gen.set(current_gen);
                let new_on = {
                    let s = self.shared.state.lock().unwrap();
                    match self.role {
                        DeviceRole::Spa => s.spa_on,
                        DeviceRole::Jets => s.jets_on,
                        DeviceRole::Lights => s.lights_on,
                    }
                };
                if new_on != self.on.get() {
                    self.on.set(new_on);
                    notify(OutOfBandMessage::Update);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bridged Device Basic Information handler
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct BridgedHandler {
    dataver: Dataver,
    reachable: bool,
}

impl BridgedHandler {
    const fn new(dataver: Dataver) -> Self {
        Self {
            dataver,
            reachable: true,
        }
    }

    const fn adapt(self) -> bridged_device_basic_information::HandlerAdaptor<Self> {
        bridged_device_basic_information::HandlerAdaptor(self)
    }
}

impl bridged_device_basic_information::ClusterHandler for BridgedHandler {
    const CLUSTER: Cluster<'static> = bridged_device_basic_information::FULL_CLUSTER
        .with_features(0)
        .with_attrs(with!(required))
        .with_cmds(with!());

    fn dataver(&self) -> u32 {
        self.dataver.get()
    }

    fn dataver_changed(&self) {
        self.dataver.changed();
    }

    fn reachable(&self, _ctx: impl ReadContext) -> Result<bool, Error> {
        Ok(self.reachable)
    }

    fn unique_id<P: TLVBuilderParent>(
        &self,
        _ctx: impl ReadContext,
        _builder: Utf8StrBuilder<P>,
    ) -> Result<P, Error> {
        Err(Error::new(rs_matter::error::ErrorCode::InvalidAction))
    }

    fn handle_keep_active(
        &self,
        _ctx: impl InvokeContext,
        _request: KeepActiveRequest<'_>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Data model handler assembly
// ---------------------------------------------------------------------------

fn dm_handler<'a, OH: OnOffHooks, LH: LevelControlHooks>(
    mut rand: impl rand::RngCore + Copy,
    spa_on_off: &'a on_off::OnOffHandler<'a, OH, LH>,
    spa_thermostat: &'a thermostat_decl::HandlerAdaptor<SpaThermostatHandler>,
    jets_on_off: &'a on_off::OnOffHandler<'a, OH, LH>,
    lights_on_off: &'a on_off::OnOffHandler<'a, OH, LH>,
    lights_mode_select: &'a mode_select_decl::HandlerAdaptor<LightModeSelectHandler>,
) -> impl AsyncMetadata + AsyncHandler + 'a {
    (
        NODE,
        endpoints::with_eth(
            &(),
            &UnixNetifs,
            rand,
            endpoints::with_sys(
                &false,
                rand,
                EmptyHandler
                    // Aggregator (ep 1)
                    .chain(
                        EpClMatcher::new(Some(1), Some(desc::DescHandler::CLUSTER.id)),
                        Async(
                            desc::DescHandler::new_aggregator(Dataver::new_rand(&mut rand)).adapt(),
                        ),
                    )
                    // Spa (ep 2)
                    .chain(
                        EpClMatcher::new(Some(2), Some(desc::DescHandler::CLUSTER.id)),
                        Async(desc::DescHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(2), Some(BridgedHandler::CLUSTER.id)),
                        Async(BridgedHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(2), Some(PentairOnOffHooks::CLUSTER.id)),
                        on_off::HandlerAsyncAdaptor(spa_on_off),
                    )
                    .chain(
                        EpClMatcher::new(Some(2), Some(SPA_THERMOSTAT_CLUSTER.id)),
                        Async(spa_thermostat),
                    )
                    // Jets (ep 3)
                    .chain(
                        EpClMatcher::new(Some(3), Some(desc::DescHandler::CLUSTER.id)),
                        Async(desc::DescHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(3), Some(BridgedHandler::CLUSTER.id)),
                        Async(BridgedHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(3), Some(PentairOnOffHooks::CLUSTER.id)),
                        on_off::HandlerAsyncAdaptor(jets_on_off),
                    )
                    // Lights (ep 4)
                    .chain(
                        EpClMatcher::new(Some(4), Some(desc::DescHandler::CLUSTER.id)),
                        Async(desc::DescHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(4), Some(BridgedHandler::CLUSTER.id)),
                        Async(BridgedHandler::new(Dataver::new_rand(&mut rand)).adapt()),
                    )
                    .chain(
                        EpClMatcher::new(Some(4), Some(PentairOnOffHooks::CLUSTER.id)),
                        on_off::HandlerAsyncAdaptor(lights_on_off),
                    )
                    .chain(
                        EpClMatcher::new(Some(4), Some(LIGHTS_MODE_SELECT_CLUSTER.id)),
                        Async(lights_mode_select),
                    ),
            ),
        ),
    )
}

// ---------------------------------------------------------------------------
// Matter thread entry point
// ---------------------------------------------------------------------------

fn run_matter(
    discriminator: u16,
    fabric_path: std::path::PathBuf,
    shared: Arc<SharedState>,
    cmd_tx: mpsc::Sender<Command>,
    mode_map: crate::light_modes::LightModeMap,
) -> Result<(), BridgeError> {
    let mut dev_comm = TEST_DEV_COMM.clone();
    dev_comm.discriminator = discriminator;

    let matter = Matter::new_default(&DEV_DET, dev_comm, &TEST_DEV_ATT, MATTER_PORT);
    matter.initialize_transport_buffers()?;

    let buffers = PooledBuffers::<10, NoopRawMutex, IMBuffer>::new(0);
    let subscriptions = DefaultSubscriptions::new();
    let crypto = default_crypto::<NoopRawMutex, _>(rand::thread_rng(), DAC_PRIVKEY);
    let mut rng = crypto.rand()?;
    let events = DefaultEvents::new_default();

    // Create OnOff handlers for each endpoint
    let spa_hooks = PentairOnOffHooks::new(DeviceRole::Spa, shared.clone(), cmd_tx.clone());
    let spa_on_off = on_off::OnOffHandler::new_standalone(
        Dataver::new_rand(&mut rng),
        2,
        spa_hooks,
    );

    let jets_hooks = PentairOnOffHooks::new(DeviceRole::Jets, shared.clone(), cmd_tx.clone());
    let jets_on_off = on_off::OnOffHandler::new_standalone(
        Dataver::new_rand(&mut rng),
        3,
        jets_hooks,
    );

    // Thermostat for spa
    let spa_thermostat = SpaThermostatHandler::new(
        Dataver::new_rand(&mut rng),
        shared.clone(),
        cmd_tx.clone(),
    ).adapt();

    let lights_hooks = PentairOnOffHooks::new(DeviceRole::Lights, shared.clone(), cmd_tx.clone());
    let lights_on_off = on_off::OnOffHandler::new_standalone(
        Dataver::new_rand(&mut rng),
        4,
        lights_hooks,
    );

    // ModeSelect for lights (adapted before passing to dm_handler)
    let lights_mode_select = LightModeSelectHandler::new(
        Dataver::new_rand(&mut rng),
        mode_map,
        shared,
        cmd_tx,
    ).adapt();

    let dm = DataModel::new(
        &matter,
        &crypto,
        &buffers,
        &subscriptions,
        Some(&events),
        dm_handler(rng, &spa_on_off, &spa_thermostat, &jets_on_off, &lights_on_off, &lights_mode_select),
    );

    let responder = DefaultResponder::new(&dm);
    let mut respond = pin!(responder.run::<4, 4>());
    let mut dm_job = pin!(dm.run());

    let socket = async_io::Async::<UdpSocket>::bind(MATTER_SOCKET_BIND_ADDR)?;
    let mut mdns = pin!(run_mdns(&matter, &crypto, &dm));
    let mut transport = pin!(matter.run(&crypto, &socket, &socket));

    let mut psm: Psm<4096> = Psm::new();
    // PSM expects a file path, not a directory. Ensure parent dir exists.
    if let Some(parent) = fabric_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    psm.load(&fabric_path, &matter, NO_NETWORKS, Some(&events))?;

    if !matter.is_commissioned() {
        tracing::info!("Device is not commissioned. Printing pairing info...");
        matter.print_standard_qr_text(DiscoveryCapabilities::IP)?;
        matter.print_standard_qr_code(QrTextType::Unicode, DiscoveryCapabilities::IP)?;
        matter.open_basic_comm_window(MAX_COMM_WINDOW_TIMEOUT_SECS, &crypto, &dm)?;
    } else {
        tracing::info!("Device is already commissioned.");
    }

    let mut persist = pin!(psm.run(&fabric_path, &matter, NO_NETWORKS, Some(&events)));

    tracing::info!(
        port = MATTER_PORT,
        discriminator = discriminator,
        "Matter bridge running (3 endpoints: spa, jets, lights)"
    );

    let all = select4(
        &mut transport,
        &mut mdns,
        &mut persist,
        select(&mut respond, &mut dm_job).coalesce(),
    );

    futures_lite::future::block_on(all.coalesce())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Built-in mDNS responder
// ---------------------------------------------------------------------------

async fn run_mdns<C: Crypto>(
    matter: &Matter<'_>,
    crypto: C,
    notify: &dyn rs_matter::dm::ChangeNotify,
) -> Result<(), Error> {
    run_builtin_mdns(matter, crypto, notify).await
}

async fn run_builtin_mdns<C: Crypto>(
    matter: &Matter<'_>,
    crypto: C,
    notify: &dyn rs_matter::dm::ChangeNotify,
) -> Result<(), Error> {
    use rs_matter::transport::network::{Ipv4Addr, Ipv6Addr};

    fn initialize_network() -> Result<(Ipv4Addr, Ipv6Addr, u32), Error> {
        use nix::net::if_::InterfaceFlags;
        use nix::sys::socket::SockaddrIn6;
        use rs_matter::error::ErrorCode;

        let interfaces = || {
            nix::ifaddrs::getifaddrs().expect("getifaddrs syscall failed").filter(|ia| {
                ia.flags
                    .contains(InterfaceFlags::IFF_UP | InterfaceFlags::IFF_BROADCAST)
                    && !ia
                        .flags
                        .intersects(InterfaceFlags::IFF_LOOPBACK | InterfaceFlags::IFF_POINTOPOINT)
            })
        };

        let (iname, ip, ipv6) = interfaces()
            .filter_map(|ia| {
                ia.address
                    .and_then(|addr| addr.as_sockaddr_in6().map(SockaddrIn6::ip))
                    .map(|ipv6| (ia.interface_name, ipv6))
            })
            .filter_map(|(iname, ipv6)| {
                interfaces()
                    .filter(|ia2| ia2.interface_name == iname)
                    .find_map(|ia2| {
                        ia2.address
                            .and_then(|addr| addr.as_sockaddr_in().map(|addr| addr.ip().into()))
                            .map(|ip: std::net::Ipv4Addr| (iname.clone(), ip, ipv6))
                    })
            })
            .next()
            .ok_or_else(|| {
                tracing::error!("Cannot find network interface suitable for mDNS broadcasting");
                ErrorCode::StdIoError
            })?;

        tracing::info!(
            interface = %iname,
            ipv4 = %ip,
            ipv6 = %ipv6,
            "Using network interface for mDNS"
        );

        Ok((ip.octets().into(), ipv6.octets().into(), 0))
    }

    let (ipv4_addr, ipv6_addr, interface) = initialize_network()?;

    use rs_matter::transport::network::mdns::builtin::{BuiltinMdnsResponder, Host};
    use rs_matter::transport::network::mdns::{
        MDNS_IPV4_BROADCAST_ADDR, MDNS_IPV6_BROADCAST_ADDR, MDNS_SOCKET_DEFAULT_BIND_ADDR,
    };
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_only_v6(false)?;
    socket.bind(&MDNS_SOCKET_DEFAULT_BIND_ADDR.into())?;
    let socket = async_io::Async::<UdpSocket>::new_nonblocking(socket.into())?;

    socket
        .get_ref()
        .join_multicast_v6(&MDNS_IPV6_BROADCAST_ADDR, interface)?;
    socket
        .get_ref()
        .join_multicast_v4(&MDNS_IPV4_BROADCAST_ADDR, &ipv4_addr)?;

    BuiltinMdnsResponder::new(matter, crypto, notify)
        .run(
            &socket,
            &socket,
            &Host {
                id: 0,
                hostname: "pentair-pool",
                ip: ipv4_addr,
                ipv6: ipv6_addr,
            },
            Some(ipv4_addr),
            Some(interface),
        )
        .await
}
