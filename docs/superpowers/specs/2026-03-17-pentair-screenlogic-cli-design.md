# Pentair ScreenLogic Rust CLI — Design Spec

## Overview

A Rust workspace implementing the Pentair ScreenLogic over IP protocol as a command-line tool for full pool/spa control. This is the first Rust implementation of this protocol. The target system is an IntelliTouch controller with IntelliBrite lights and IntelliFlow pump, reachable at a ScreenLogic protocol adapter on the local network.

Reference implementations cross-checked: node-screenlogic (parnic/node-screenlogic), screenlogicpy (dieselrabbit/screenlogicpy). All parse orders and field types verified against both. Where they disagree, screenlogicpy is preferred (actively maintained, author has hardware).

## Architecture

Three-crate Cargo workspace:

```
pentair/
  Cargo.toml                 # workspace root
  pentair-protocol/          # wire protocol: types, encode, decode (no IO)
  pentair-client/            # async TCP/UDP client (tokio)
  pentair-cli/               # clap-based binary
```

### Why three crates

- `pentair-protocol` has no IO dependencies — testable with byte slices, reusable in embedded or WASM contexts
- `pentair-client` owns all networking — swappable transport, single place for connection lifecycle
- `pentair-cli` is a thin layer mapping user commands to client calls and formatting output

---

## Crate: `pentair-protocol`

Pure data types and serialization. No `tokio`, no `std::net`.

### Wire format

All values little-endian unless otherwise noted (see ChemData exception). Messages have an 8-byte header:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 2 | header_id (u16 LE) — client-assigned opaque value, typically 0 |
| 2 | 2 | action (u16 LE) — the message/command code |
| 4 | 4 | data_length (u32 LE) — byte count of payload |
| 8.. | N | payload |

The protocol PDF calls bytes 0-1 "MSG CD 1" and bytes 2-3 "MSG CD 2". node-screenlogic calls byte 0-1 `senderId` — it is an opaque u16 set by the client (defaults to 0). It is NOT related to the "Sender ID" parameter in AddClient/RemoveClient payloads. For this implementation, we always set it to 0.

### Encoded types

**SLString**: `[len: u32 LE][bytes: len][zero-padding to 4-byte alignment]`
**SLArray**: `[len: u32 LE][bytes: len][zero-padding to 4-byte alignment]`
**SLDateTime**: 8 consecutive i16 LE fields: year, month (1-based), day_of_week, day, hour, minute, second, millisecond. Total: 16 bytes.

### Action code convention

The protocol follows a request/response pattern: client sends action code N, server responds with N+1.

```rust
pub enum Action {
    // Control messages — requests
    ChallengeRequest = 14,          // response = 15
    PingRequest = 16,               // response = 17
    LoginRequest = 27,              // response = 28

    // Control messages — responses/errors
    LoginFailure = 13,
    ChallengeResponse = 15,
    PingResponse = 17,
    LoginResponse = 28,
    UnknownCommand = 30,
    BadParameter = 31,

    // Version
    VersionRequest = 8120,          // response = 8121
    VersionResponse = 8121,

    // System time
    GetSystemTime = 8110,           // response = 8111
    SystemTimeResponse = 8111,
    SetSystemTime = 8112,           // response = 8113
    SetSystemTimeResponse = 8113,

    // Pool messages — unsolicited push
    StatusChanged = 12500,
    ScheduleChanged = 12501,
    RuntimeChanged = 12503,
    ChemDataChanged = 12505,

    // Pool messages — requests
    AddClient = 12522,
    RemoveClient = 12524,
    GetStatus = 12526,
    SetHeatSetPoint = 12528,
    ButtonPress = 12530,
    GetControllerConfig = 12532,
    GetHistory = 12534,
    SetHeatMode = 12538,
    GetScheduleData = 12542,
    AddScheduleEvent = 12544,
    DeleteScheduleEvent = 12546,
    SetScheduleEvent = 12548,
    SetCircuitRunTime = 12550,
    ColorLightsCommand = 12556,
    GetCustomNames = 12562,
    SetCustomName = 12564,
    GetEquipmentConfig = 12566,
    SetEquipmentConfig = 12568,
    GetScgConfig = 12572,
    SetScgEnabled = 12574,
    SetScgConfig = 12576,
    CancelDelay = 12580,
    GetPumpStatus = 12584,
    SetCoolSetPoint = 12590,
    GetChemData = 12592,
    GetChemHistory = 12596,

    // Pool messages — responses (request + 1)
    AddClientResponse = 12523,
    RemoveClientResponse = 12525,
    GetStatusResponse = 12527,
    SetHeatSetPointResponse = 12529,
    ButtonPressResponse = 12531,
    GetControllerConfigResponse = 12533,
    GetHistoryResponse = 12535,
    SetHeatModeResponse = 12539,
    GetScheduleDataResponse = 12543,
    AddScheduleEventResponse = 12545,
    DeleteScheduleEventResponse = 12547,
    SetScheduleEventResponse = 12549,
    SetCircuitRunTimeResponse = 12551,
    ColorLightsCommandResponse = 12557,
    GetCustomNamesResponse = 12563,
    SetCustomNameResponse = 12565,
    GetEquipmentConfigResponse = 12567,
    SetEquipmentConfigResponse = 12569,
    GetScgConfigResponse = 12573,
    SetScgEnabledResponse = 12575,
    SetScgConfigResponse = 12577,
    CancelDelayResponse = 12581,
    GetPumpStatusResponse = 12585,
    SetPumpSpeedResponse = 12587,
    SetCoolSetPointResponse = 12591,
    GetChemDataResponse = 12593,
    GetChemHistoryResponse = 12597,

    // Weather
    WeatherForecastRequest = 9807,
    WeatherForecastChanged = 9806,
    WeatherForecastResponse = 9808,

    // Gateway (remote connection)
    GatewayRequest = 18003,
    GatewayResponse = 18004,
}
```

### Domain enums

```rust
pub enum BodyType { Pool = 0, Spa = 1 }

pub enum HeatMode { Off = 0, Solar = 1, SolarPreferred = 2, HeatPump = 3, DontChange = 4 }

pub enum HeatStatus { Off = 0, Solar = 1, Heater = 2, Both = 3 }

pub enum LightCommand {
    Off = 0, On = 1, Set = 2, Sync = 3, Swim = 4,
    Party = 5, Romantic = 6, Caribbean = 7, American = 8,
    Sunset = 9, Royal = 10, Save = 11, Recall = 12,
    Blue = 13, Green = 14, Red = 15, White = 16,
    Purple = 17, Thumper = 18, NextMode = 19,
    Reset = 20, Hold = 21,
}

pub enum CircuitFunction {
    Generic = 0, Spa = 1, Pool = 2, MasterCleaner = 5,
    Light = 7, IntelliBrite = 16, MagicStream = 17,
}

pub enum PumpType { None = 0, VF = 1, VS = 2, VSF = 3 }

pub enum ScheduleType { Recurring = 0, RunOnce = 1 }
```

### Parsed response types

#### Controller Config (action 12533)

Parse order, field-by-field from node-screenlogic `decodeControllerConfig`:

```rust
pub struct ControllerConfig {
    pub controller_id: u32,        // i32le, subtract 99 from wire value
    pub pool_min_set_point: u8,    // u8
    pub pool_max_set_point: u8,    // u8
    pub spa_min_set_point: u8,     // u8
    pub spa_max_set_point: u8,     // u8
    pub is_celsius: bool,          // u8, nonzero = celsius
    pub controller_type: u8,       // u8
    pub hw_type: u8,               // u8
    pub controller_data: u8,       // u8
    pub equipment: EquipmentFlags, // i32le (4 bytes!)
    pub generic_circuit_name: String, // SLString
    pub circuits: Vec<CircuitDefinition>, // i32le count, then per-circuit
    pub colors: Vec<ColorDefinition>,    // i32le count, then per-color
    pub pump_circ_array: [u8; 8],  // 8 x u8
    pub interface_tab_flags: u32,  // i32le
    pub show_alarms: u32,          // i32le
}
```

Wire order per circuit (after SLString name):
```
nameIndex: u8
function: u8
interface: u8
freeze: u8
colorSet: u8
colorPos: u8
colorStagger: u8
deviceId: u8
eggTimer: u16le
(2 padding bytes skipped)
```
Total: 12 bytes of metadata per circuit after name.

nameIndex normalization: `< 101 ? +1 : +99` (for mapping default vs custom names).

```rust
pub struct CircuitDefinition {
    pub id: u32,                   // i32le, subtract 499 from wire value
    pub name: String,              // SLString
    pub name_index: u8,            // u8 (normalized: < 101 ? +1 : +99)
    pub function: u8,              // u8
    pub interface: u8,             // u8
    pub freeze: bool,              // u8
    pub color_set: u8,             // u8
    pub color_pos: u8,             // u8
    pub color_stagger: u8,         // u8
    pub device_id: u8,             // u8
    pub egg_timer: u16,            // u16le
    // 2 padding bytes on wire (not stored)
}

pub struct ColorDefinition {
    pub name: String,              // SLString
    pub r: u8,                     // i32le masked to 0xFF
    pub g: u8,                     // i32le masked to 0xFF
    pub b: u8,                     // i32le masked to 0xFF
}
```

Controller type detection:
- 13 or 14: EasyTouch
- 13 with `(hw_type & 4) != 0`: EasyTouch Lite
- 5: Dual Body (IntelliTouch i5+3S)
- 252 with hw_type 2: Chem2
- Not 10, 13, or 14: IntelliTouch

#### Equipment State / Pool Status (action 12527)

**This is the most complex response.** Exact parse order from `decodeEquipmentStateResponse`:

```rust
pub struct PoolStatus {
    pub panel_mode: i32,           // i32le
    pub freeze_mode: bool,         // u8, extract bit 3 (& 0x08) per screenlogicpy
    pub remotes: u8,               // u8
    pub pool_delay: u8,            // u8
    pub spa_delay: u8,             // u8
    pub cleaner_delay: u8,         // u8
    // 3 unknown bytes skipped on wire
    pub air_temp: i32,             // i32le
    pub bodies: Vec<BodyStatus>,   // i32le count (clamped to max 2), then per-body
    pub circuits: Vec<CircuitStatus>, // i32le count, then per-circuit
    // Inline chemistry data follows circuits:
    pub ph: f64,                   // i32le / 100.0
    pub orp: i32,                  // i32le (mV)
    pub saturation: f64,           // i32le / 100.0
    pub salt_ppm: i32,             // i32le * 50
    pub ph_tank: i32,              // i32le
    pub orp_tank: i32,             // i32le
    pub alarms: i32,               // i32le
}
```

Wire order:
```
1. panelMode: i32le
2. freezeMode: u8
3. remotes: u8
4. poolDelay: u8
5. spaDelay: u8
6. cleanerDelay: u8
7. (3 bytes padding — skip)
8. airTemp: i32le
9. bodyCount: i32le (clamp to max 2)
10. [per body: 6 x i32le = 24 bytes each]
11. circuitCount: i32le
12. [per circuit: i32le id + i32le state + u8 colorSet + u8 colorPos + u8 colorStagger + u8 delay = 12 bytes each]
13. pH: i32le (divide by 100)
14. orp: i32le
15. saturation: i32le (divide by 100)
16. saltPPM: i32le (multiply by 50)
17. pHTank: i32le
18. orpTank: i32le
19. alarms: i32le
```

```rust
pub struct BodyStatus {
    pub body_type: BodyType,       // i32le (0=pool, 1=spa); bodies indexed by this, not iteration order
    pub current_temp: i32,         // i32le
    pub heat_status: HeatStatus,   // i32le
    pub set_point: i32,            // i32le
    pub cool_set_point: i32,       // i32le
    pub heat_mode: HeatMode,       // i32le
}
// Per body = 6 x i32le = 24 bytes

pub struct CircuitStatus {
    pub id: u32,                   // i32le, subtract 499 from wire value
    pub state: bool,               // i32le (0=off, 1=on)
    pub color_set: u8,             // u8
    pub color_pos: u8,             // u8
    pub color_stagger: u8,         // u8
    pub delay: u8,                 // u8
}
// Per circuit = 4 + 4 + 1 + 1 + 1 + 1 = 12 bytes
```

#### Chemistry Data (action 12593) — BIG-ENDIAN exception

Most fields use **big-endian** encoding. This is the only known exception to the LE convention. Field layout verified against screenlogicpy (which decodes all fields that node-screenlogic skips as "unknown").

Wire parse order (offset-based, from screenlogicpy):

```
 0: sentinel: u32le (should be 42 for valid data)
 4: (1 unknown byte — skip)
 5: pH: u16BE (divide by 100, e.g. 752 = 7.52)
 7: orp: u16BE (mV)
 9: phSetPoint: u16BE (divide by 100)
11: orpSetPoint: u16BE
13: phDoseTime: u32BE (seconds)
17: orpDoseTime: u32BE (seconds)
21: phDoseVolume: u16BE (mL)
23: orpDoseVolume: u16BE (mL)
25: phSupplyLevel: u8 (tank level)
26: orpSupplyLevel: u8 (tank level)
27: saturation: u8 (signed: if val & 0x80, val = -(256 - val); then /100.0)
28: calcium: u16BE
30: cyanuricAcid: u16BE
32: alkalinity: u16BE
34: saltPPM: u8 (multiply by 50) — NOTE: screenlogicpy reads u8, node-screenlogic reads u16LE
35: probeIsCelsius: u8 (nonzero = Celsius)
36: waterTemp: u8
37: alarms: u8 (bitfield: flow, pH high/low, ORP high/low, supply, probe fault)
38: alerts: u8 (bitfield: pH lockout, pH/ORP limit, invalid setup, chlorinator comm)
39: doseFlags: u8 (bits 4-5 = pH dose state, bits 6-7 = ORP dose state)
40: configFlags: u8
41: fwMinor: u8 (IntelliChem firmware minor version)
42: fwMajor: u8 (IntelliChem firmware major version)
43: balanceFlags: u8 (bit 0 = corrosive, bit 1 = scaling)
44-46: (3 unknown bytes — skip)
```

Dose states: 0=Dosing, 1=Mixing, 2=Monitoring

```rust
pub struct ChemData {
    pub valid: bool,               // sentinel == 42
    pub ph: f64,                   // u16BE / 100.0
    pub orp: u16,                  // u16BE (mV)
    pub ph_set_point: f64,         // u16BE / 100.0
    pub orp_set_point: u16,        // u16BE
    pub ph_dose_time: u32,         // u32BE (seconds)
    pub orp_dose_time: u32,        // u32BE (seconds)
    pub ph_dose_volume: u16,       // u16BE (mL)
    pub orp_dose_volume: u16,      // u16BE (mL)
    pub ph_supply_level: u8,       // u8
    pub orp_supply_level: u8,      // u8
    pub saturation: f64,           // u8 signed / 100.0
    pub calcium: u16,              // u16BE
    pub cyanuric_acid: u16,        // u16BE
    pub alkalinity: u16,           // u16BE
    pub salt_ppm: u32,             // u8 * 50 (per screenlogicpy)
    pub probe_is_celsius: bool,    // u8
    pub temperature: u8,           // u8
    pub alarms: u8,                // u8 bitfield
    pub alerts: u8,                // u8 bitfield
    pub dose_flags: u8,            // u8 (pH dose state bits 4-5, ORP dose state bits 6-7)
    pub config_flags: u8,          // u8
    pub fw_version: String,        // "{fwMajor}.{fwMinor:03}" from 2 u8 fields
    pub is_corrosive: bool,        // balanceFlags bit 0
    pub is_scaling: bool,          // balanceFlags bit 1
}
```

#### SCG / Chlorinator Config (action 12573)

All fields i32le, from `decodeIntellichlorConfig`:

```
1. installed: i32le (1 = true)
2. status: i32le
3. poolSetPoint: i32le (% output)
4. spaSetPoint: i32le (% output)
5. salt: i32le (multiply by 50 for PPM)
6. flags: i32le
7. superChlorTimer: i32le
```

```rust
pub struct ScgConfig {
    pub installed: bool,           // i32le, 1=true
    pub status: u32,               // i32le
    pub pool_set_point: u32,       // i32le (% chlor output)
    pub spa_set_point: u32,        // i32le (% chlor output)
    pub salt_ppm: u32,             // i32le * 50
    pub flags: u32,                // i32le
    pub super_chlor_timer: u32,    // i32le
}
```

#### Pump Status (action 12585)

All fields u32le, from `decodePumpStatus`:

```
1. pumpType: u32le (0=none, 1=VF, 2=VS, 3=VSF)
2. isRunning: u32le (0=off, 1 or 0xFFFFFFFF=on)
3. pumpWatts: u32le
4. pumpRPMs: u32le
5. pumpUnknown1: u32le (always 0 — skip)
6. pumpGPMs: u32le
7. pumpUnknown2: u32le (always 255 — skip)
8. [8 pump circuits, each: circuitId(u32le), speed(u32le), isRPMs(u32le)]
```

```rust
pub struct PumpStatus {
    pub pump_type: PumpType,       // u32le
    pub is_running: bool,          // u32le
    pub watts: u32,                // u32le
    pub rpm: u32,                  // u32le
    pub gpm: u32,                  // u32le (after skipping unknown1)
    // unknown2 skipped
    pub circuits: Vec<PumpCircuit>, // always 8 entries
}

pub struct PumpCircuit {
    pub circuit_id: u32,           // u32le (wire value, not offset-adjusted)
    pub speed: u32,                // u32le
    pub is_rpm: bool,              // u32le (nonzero = RPM, 0 = GPM)
}
```

#### Schedule Data (action 12543)

From `decodeGetScheduleMessage`:

```
1. eventCount: u32le
2. [per event: 8 x u32le]
```

```rust
pub struct Schedule {
    pub id: u32,                   // u32le, subtract 699 from wire value
    pub circuit_id: u32,           // u32le, subtract 499 from wire value
    pub start_time: u32,           // u32le (minutes from midnight)
    pub stop_time: u32,            // u32le (minutes from midnight)
    pub day_mask: u32,             // u32le (Mon=0x01..Sun=0x40)
    pub flags: u32,                // u32le
    pub heat_cmd: u32,             // u32le
    pub heat_set_point: u32,       // u32le
}
```

Schedule types (passed as request parameter, not in response):
- `Recurring = 0` — regular recurring schedule
- `RunOnce = 1` — one-time / egg timer schedule

Outbound schedule operations:
- GetSchedules: payload = `i32le 0`, `i32le schedType`
- AddSchedule: payload = `i32le 0`, `i32le schedType`
- DeleteSchedule: payload = `i32le controllerId`, `i32le (schedId + 699)`
- SetSchedule: `i32le 0`, `i32le (scheduleId+699)`, `i32le (circuitId+499)`, `i32le startTime`, `i32le stopTime`, `i32le dayMask`, `i32le flags`, `i32le heatCmd`, `i32le heatSetPoint`

#### Version (action 8121)

Request: action 8120, no payload.
Response: `readSLString()` — firmware version string (e.g., "POOL: 5.2 Build 736.0 Rel...")

#### System Time (action 8111)

Response: `SLDateTime` (16 bytes) + `i32le adjustForDST` (1=true).

#### Weather Forecast (action 9808)

```
1. version: i32le
2. zip: SLString
3. lastUpdate: SLDateTime
4. lastRequest: SLDateTime
5. dateText: SLString
6. text: SLString
7. currentTemperature: i32le
8. humidity: i32le
9. wind: SLString
10. pressure: i32le
11. dewPoint: i32le
12. windChill: i32le
13. visibility: i32le
14. numDays: i32le
15. [per day: SLDateTime + highTemp(i32le) + lowTemp(i32le) + text(SLString)]
16. sunrise: i32le
17. sunset: i32le
```

### Equipment flags

```rust
bitflags! {
    pub struct EquipmentFlags: u32 {
        const SOLAR              = 0x0001;  // bit 0
        const SOLAR_HEAT_PUMP    = 0x0002;  // bit 1
        const INTELLICHLOR       = 0x0004;  // bit 2
        const INTELLIBRITE       = 0x0008;  // bit 3
        const INTELLIFLOW_0      = 0x0010;  // bit 4
        const INTELLIFLOW_1      = 0x0020;  // bit 5
        const INTELLIFLOW_2      = 0x0040;  // bit 6
        const INTELLIFLOW_3      = 0x0080;  // bit 7
        const INTELLIFLOW_4      = 0x0100;  // bit 8
        const INTELLIFLOW_5      = 0x0200;  // bit 9
        const INTELLIFLOW_6      = 0x0400;  // bit 10
        const INTELLIFLOW_7      = 0x0800;  // bit 11
        const NO_SPECIAL_LIGHTS  = 0x1000;  // bit 12
        const HEATPUMP_HAS_COOL  = 0x2000;  // bit 13
        const MAGICSTREAM        = 0x4000;  // bit 14
        const INTELLICHEM        = 0x8000;  // bit 15
    }
}
```

### Encoding/decoding

A `codec` module with:

```rust
pub fn encode_message(action: u16, payload: &[u8]) -> Vec<u8>  // header_id always 0
pub fn decode_header(buf: &[u8]) -> Result<MessageHeader>
pub fn encode_sl_string(s: &str) -> Vec<u8>
pub fn decode_sl_string(buf: &[u8], offset: &mut usize) -> Result<String>
pub fn encode_sl_array(data: &[u8]) -> Vec<u8>
pub fn decode_sl_array(buf: &[u8], offset: &mut usize) -> Result<Vec<u8>>
pub fn encode_sl_datetime(dt: &SLDateTime) -> Vec<u8>
pub fn decode_sl_datetime(buf: &[u8], offset: &mut usize) -> Result<SLDateTime>
pub fn read_i32le(buf: &[u8], offset: &mut usize) -> Result<i32>
pub fn read_u32le(buf: &[u8], offset: &mut usize) -> Result<u32>
pub fn read_u16le(buf: &[u8], offset: &mut usize) -> Result<u16>
pub fn read_u16be(buf: &[u8], offset: &mut usize) -> Result<u16>
pub fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8>
pub fn skip(offset: &mut usize, n: usize)
```

Request builder functions. All accept **logical IDs** and apply wire offsets (+499 for circuits, +699 for schedules, -1 for pumps) internally:

```rust
pub fn build_login_request(client_version: &str, password: &[u8; 16]) -> Vec<u8>
pub fn build_challenge_request() -> Vec<u8>
pub fn build_version_request() -> Vec<u8>
pub fn build_ping() -> Vec<u8>
pub fn build_add_client(controller_id: u32, client_id: u32) -> Vec<u8>
pub fn build_remove_client(controller_id: u32, client_id: u32) -> Vec<u8>
pub fn build_get_status_request() -> Vec<u8>
pub fn build_get_controller_config() -> Vec<u8>
pub fn build_button_press(circuit_id: u32, state: bool) -> Vec<u8>      // applies +499
pub fn build_set_heat_setpoint(body: BodyType, temp: u32) -> Vec<u8>
pub fn build_set_heat_mode(body: BodyType, mode: HeatMode) -> Vec<u8>
pub fn build_set_cool_setpoint(body: BodyType, temp: u32) -> Vec<u8>
pub fn build_light_command(cmd: LightCommand) -> Vec<u8>
pub fn build_set_circuit_runtime(circuit_id: u32, runtime: u32) -> Vec<u8> // applies +499
pub fn build_get_pump_status(pump_id: u32) -> Vec<u8>                     // sends pump_id - 1
pub fn build_get_schedule_data(sched_type: ScheduleType) -> Vec<u8>
pub fn build_add_schedule(sched_type: ScheduleType) -> Vec<u8>
pub fn build_delete_schedule(controller_id: u32, sched_id: u32) -> Vec<u8> // applies +699
pub fn build_set_schedule(schedule: &Schedule) -> Vec<u8>                   // applies +699/+499
pub fn build_get_chem_data() -> Vec<u8>
pub fn build_get_scg_config() -> Vec<u8>
pub fn build_set_scg_output(pool: u32, spa: u32) -> Vec<u8>
pub fn build_set_scg_enabled(enabled: bool) -> Vec<u8>
pub fn build_cancel_delay() -> Vec<u8>
pub fn build_get_weather() -> Vec<u8>
pub fn build_get_system_time() -> Vec<u8>
```

Response parser functions. All return **logical IDs** with wire offsets removed:

```rust
pub fn parse_controller_config(data: &[u8]) -> Result<ControllerConfig>
pub fn parse_pool_status(data: &[u8]) -> Result<PoolStatus>
pub fn parse_chem_data(data: &[u8]) -> Result<ChemData>
pub fn parse_scg_config(data: &[u8]) -> Result<ScgConfig>
pub fn parse_pump_status(data: &[u8]) -> Result<PumpStatus>
pub fn parse_schedule_data(data: &[u8]) -> Result<Vec<Schedule>>
pub fn parse_version(data: &[u8]) -> Result<String>
pub fn parse_system_time(data: &[u8]) -> Result<(SLDateTime, bool)>
pub fn parse_challenge(data: &[u8]) -> Result<String>
```

### Dependencies

- `bitflags`
- `thiserror`

---

## Crate: `pentair-client`

Async networking layer.

### Discovery

```rust
pub struct Adapter {
    pub name: String,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub gateway_type: u8,
    pub gateway_subtype: u8,
}

pub async fn discover(timeout: Duration) -> Result<Vec<Adapter>>
```

UDP broadcast `[1,0,0,0,0,0,0,0]` to `255.255.255.255:1444`. Response is ~40 bytes. Confirmed format from live testing:

```
check_digit: i32le at offset 0 (should be 2)
ip: 4 raw bytes at offsets 4-7
port: u16le at offset 8
gateway_type: u8 at offset 10
gateway_subtype: u8 at offset 11
name: null-terminated ASCII at offset 12+
```

Note: node-screenlogic's `SLGatewayDataMessage` parser (gatewayFound, licenseOK, ipAddr as SLString, etc.) is for the **remote gateway** response (action 18004), NOT the UDP discovery response.

### Client

```rust
pub struct Client {
    stream: TcpStream,   // tokio
    controller_id: u32,
    client_id: u32,
}

impl Client {
    pub async fn connect(addr: SocketAddr) -> Result<Self>
    pub async fn login(&mut self) -> Result<()>
    pub async fn get_version(&mut self) -> Result<String>
    pub async fn get_status(&mut self) -> Result<PoolStatus>
    pub async fn get_controller_config(&mut self) -> Result<ControllerConfig>
    pub async fn set_circuit(&mut self, circuit_id: u32, on: bool) -> Result<()>
    pub async fn set_heat_setpoint(&mut self, body: BodyType, temp: u32) -> Result<()>
    pub async fn set_heat_mode(&mut self, body: BodyType, mode: HeatMode) -> Result<()>
    pub async fn set_cool_setpoint(&mut self, body: BodyType, temp: u32) -> Result<()>
    pub async fn set_light_command(&mut self, cmd: LightCommand) -> Result<()>
    pub async fn get_chem_data(&mut self) -> Result<ChemData>
    pub async fn get_scg_config(&mut self) -> Result<ScgConfig>
    pub async fn set_scg_output(&mut self, pool: u32, spa: u32) -> Result<()>
    pub async fn set_scg_enabled(&mut self, enabled: bool) -> Result<()>
    pub async fn get_schedule_data(&mut self, sched_type: ScheduleType) -> Result<Vec<Schedule>>
    pub async fn add_schedule(&mut self, sched_type: ScheduleType) -> Result<()>
    pub async fn delete_schedule(&mut self, sched_id: u32) -> Result<()>
    pub async fn set_schedule(&mut self, schedule: &Schedule) -> Result<()>
    pub async fn get_pump_status(&mut self, pump_id: u32) -> Result<PumpStatus>
    pub async fn cancel_delay(&mut self) -> Result<()>
    pub async fn get_weather(&mut self) -> Result<WeatherForecast>
    pub async fn ping(&mut self) -> Result<()>
    pub async fn send_raw(&mut self, action: u16, payload: &[u8]) -> Result<(u16, Vec<u8>)>
}
```

### Connection lifecycle

1. TCP connect to `addr`
2. Send `b"CONNECTSERVERHOST\r\n\r\n"` (raw bytes, NOT SLMessage framed)
3. Send challenge request (action 14, empty payload)
4. Receive challenge response (action 15, contains challenge string)
5. Send login message (action 27): schema(i32le) 348, conn_type(i32le) 0, version(SLString) "Android", password(SLArray) 16 zero bytes for local, proc_id(i32le) 2
6. Expect login response (action 28). Action 13 = login failure.
7. Send AddClient (action 12522): controller_id(i32le), client_id(i32le)
8. Expect AddClient response (action 12523)

For passwordless LAN connections, the challenge response can be ignored and password sent as 16 zero bytes. If password support is added later: encrypt password using AES-ECB with challenge string as key material.

### Disconnect lifecycle

1. Send RemoveClient (action 12524): controller_id(i32le), client_id(i32le)
2. Close TCP connection

### Message send/receive

- `send_message(action, payload)` — encode 8-byte header + payload, write to stream
- `recv_message()` — read 8-byte header, then read `data_length` bytes, return `(action, data)`

TCP framing note: a single TCP read may contain partial messages or multiple messages. Buffer until `data_length` bytes received.

### Keepalive

Action 16 (ping), response = action 17. Keepalive interval: node-screenlogic uses 30s, screenlogicpy uses 300s (5 min). Server disconnects after ~10 min of inactivity. For CLI one-shot commands, no keepalive needed. For future long-lived connections, 60s is a safe middle ground.

### Dependencies

- `tokio` (net, io, time)
- `pentair-protocol`
- `thiserror`

---

## Crate: `pentair-cli`

### Commands

```
pentair discover
pentair version                           # firmware version
pentair status
pentair circuit list
pentair circuit on <id|name>
pentair circuit off <id|name>
pentair heat status
pentair heat set <pool|spa> <temp>
pentair heat mode <pool|spa> <off|solar|solar-preferred|heat>
pentair heat cool <pool|spa> <temp>
pentair light <command>
pentair chem
pentair chlor
pentair chlor set <pool%> <spa%>
pentair schedule list [--type recurring|runonce]
pentair schedule add <recurring|runonce>
pentair schedule delete <id>
pentair schedule set <id> <circuit> <start> <stop> <days> [--heat-cmd N] [--heat-setpoint N]
pentair pump <id>
pentair weather
pentair cancel-delay
pentair raw <action> [hex_payload]
```

### Connection resolution

1. `--host <ip[:port]>` flag (highest priority)
2. `PENTAIR_HOST` env var
3. Auto-discovery via UDP broadcast

### Output

Plain text, human-readable by default. `--json` flag for machine-readable output (structs derive `serde::Serialize`).

Status example:

```
Air: 105°F  Freeze: OFF

Pool:  72°F  Set: 82°F  Heat: Off
Spa:   0°F  Set: 100°F  Heat: Off

Circuits:
  [1] Spa              OFF
  [2] Lights           OFF
  [3] Water Feature    OFF
  [4] Jets             OFF
  [5] Floor Cleaner    OFF
  [6] Pool             OFF
  [7] Yard Light       OFF
```

### Dependencies

- `clap` (derive)
- `tokio` (rt-multi-thread, macros)
- `serde`, `serde_json` (for --json output)
- `pentair-client`
- `anyhow`

---

## Error handling

Custom error types per crate using `thiserror`:

- `pentair-protocol`: `ProtocolError` — parse failures, invalid data, unknown action codes, buffer too short
- `pentair-client`: `ClientError` — connection refused, timeout, login failure, unexpected response action, wraps `ProtocolError`
- `pentair-cli`: uses `anyhow` for top-level error reporting

---

## Testing strategy

- `pentair-protocol`: unit tests with captured byte payloads from the live system. Roundtrip encode/decode tests for all message types.
- `pentair-client`: integration tests against the real controller (gated behind `#[cfg(feature = "live-test")]` or `#[ignore]`)
- `pentair-cli`: manual testing against the live system

---

## Known protocol details (from node-screenlogic / live testing)

### Wire offsets
- Circuit IDs: wire = logical + 499
- Schedule IDs: wire = logical + 699
- Controller ID: wire = logical + 99
- Pump IDs: wire = logical - 1 (0-indexed on wire)

### Salt PPM
- Always stored as raw value on wire, multiply by 50 for actual PPM. Appears in three places: equipment state (i32le), chem data (u8), and SCG config (i32le).

### Field type surprises
- Equipment flags is a u32 (not u8 as the protocol PDF implies)
- For firmware <= 736, mask equipment flags with 0xFFFF (bit 16+ not valid on older firmware). screenlogicpy defines HYBRID_HEATER = 0x10000 for newer firmware.
- In PoolStatus, freeze_mode/remotes/delays are u8 followed by 3 unknown bytes (not u32 as the PDF suggests)
- freeze_mode: extract bit 3 (`& 0x08`), not simple nonzero (per screenlogicpy)
- ChemData uses big-endian for most fields (unique exception to LE convention)
- ChemData salt: screenlogicpy reads as u8, node-screenlogic reads as u16LE — test against live hardware to confirm

### Connection details
- Keepalive: action 16/17. node-screenlogic uses 30s, screenlogicpy uses 300s (5 min). Server disconnects after ~10 min idle.
- Discovery response is ~40 bytes, includes adapter name after the 12 documented bytes
- The full connection lifecycle includes AddClient/RemoveClient for proper session management
- AddClient: use random client_id in range 32767-65535 (per screenlogicpy)
- Password for local connections: screenlogicpy sends ASCII "0000000000000000" as SLString; node-screenlogic sends 16 null bytes as SLArray. Both work for passwordless local connections.
- Remote connections use AES-ECB encryption of challenge string with password as key.

### Push message convention
- Client-initiated message IDs: 0-32766
- Adapter-initiated (push) message IDs: 32767+
- Known push codes: STATUS_CHANGED (12500), SCHEDULE_CHANGED (12501), RUNTIME_CHANGED (12503), COLOR_UPDATE (12504), CHEMISTRY_CHANGED (12505)

### Unknown/undocumented bytes
- PoolStatus: 3 unknown bytes after cleanerDelay
- PumpStatus: unknown1 (always 0) after RPMs, unknown2 (always 255) after GPMs. Filter pump state values with high bit set (`& 0x80000000` — per screenlogicpy).
- ChemData: 1 unknown byte at offset 4, 3 unknown trailing bytes at offsets 44-46
- ControllerConfig: 2 unknown bytes after each circuit's eggTimer/defaultRuntime

### Discrepancies between implementations
| Field | node-screenlogic | screenlogicpy | Notes |
|-------|-----------------|---------------|-------|
| Chem salt | u16LE | u8 | Test on live hardware |
| Chem "12 unknown" | Skip | phDoseTime(u32BE), orpDoseTime(u32BE), phDoseVolume(u16BE), orpDoseVolume(u16BE) | screenlogicpy decoded them |
| Chem after temp | "2 unknown" | 9 bytes: alarms, alerts, doseFlags, configFlags, fwMinor, fwMajor, balance, 3 unknown | screenlogicpy decoded them |
| Keepalive | 30s | 300s (5 min) | Server timeout ~10 min |
| Login password | SLArray, 16 null bytes | SLString, "0000000000000000" | Both work locally |
| freeze_mode | Nonzero | Bit 3 (& 0x08) | screenlogicpy more precise |
| Equipment flags fw<=736 | Not handled | Mask with 0xFFFF | Version safety |

## Out of scope

- Daemon/long-running mode with push subscriptions
- Web UI or REST API
- Remote (non-LAN) connections via Pentair dispatcher
- Home Assistant integration
- RS-485 direct bus communication
