use crate::codec::Cursor;
use crate::equipment::EquipmentFlags;
use crate::error::Result;
use crate::types::{decode_sl_datetime, decode_sl_string, SLDateTime};

// ── Version ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionResponse {
    pub version: String,
}

pub fn parse_version(payload: &[u8]) -> Result<VersionResponse> {
    let mut cursor = Cursor::new(payload);
    let version = decode_sl_string(&mut cursor)?;
    Ok(VersionResponse { version })
}

// ── System Time ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemTimeResponse {
    pub time: SLDateTime,
    pub adjust_for_dst: bool,
}

pub fn parse_system_time(payload: &[u8]) -> Result<SystemTimeResponse> {
    let mut cursor = Cursor::new(payload);
    let time = decode_sl_datetime(&mut cursor)?;
    let dst_raw = cursor.read_i32le()?;
    Ok(SystemTimeResponse {
        time,
        adjust_for_dst: dst_raw != 0,
    })
}

// ── Challenge ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeResponse {
    pub challenge: String,
}

pub fn parse_challenge(payload: &[u8]) -> Result<ChallengeResponse> {
    let mut cursor = Cursor::new(payload);
    let challenge = decode_sl_string(&mut cursor)?;
    Ok(ChallengeResponse { challenge })
}

// ── Pool Status ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodyStatus {
    pub body_type: i32,
    pub current_temp: i32,
    pub heat_status: i32,
    pub set_point: i32,
    pub cool_set_point: i32,
    pub heat_mode: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CircuitStatus {
    pub circuit_id: i32,
    pub state: bool,
    pub color_set: u8,
    pub color_pos: u8,
    pub color_stagger: u8,
    pub delay: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineChemistry {
    pub ph: i32,
    pub orp: i32,
    pub saturation: i32,
    pub salt_ppm: i32,
    pub ph_tank: i32,
    pub orp_tank: i32,
    pub alarms: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolStatus {
    pub panel_mode: i32,
    pub freeze_mode: bool,
    pub remotes: u8,
    pub pool_delay: u8,
    pub spa_delay: u8,
    pub cleaner_delay: u8,
    pub air_temp: i32,
    pub bodies: Vec<BodyStatus>,
    pub circuits: Vec<CircuitStatus>,
    pub chemistry: InlineChemistry,
}

pub fn parse_pool_status(payload: &[u8]) -> Result<PoolStatus> {
    let mut cursor = Cursor::new(payload);

    let panel_mode = cursor.read_i32le()?;
    let freeze_byte = cursor.read_u8()?;
    let freeze_mode = (freeze_byte & 0x08) != 0;
    let remotes = cursor.read_u8()?;
    let pool_delay = cursor.read_u8()?;
    let spa_delay = cursor.read_u8()?;
    let cleaner_delay = cursor.read_u8()?;
    cursor.skip(3)?; // unknown bytes at offsets 9-11

    let air_temp = cursor.read_i32le()?;

    let body_count = cursor.read_i32le()?.min(2) as usize;
    let mut bodies = Vec::with_capacity(body_count);
    for _ in 0..body_count {
        bodies.push(BodyStatus {
            body_type: cursor.read_i32le()?,
            current_temp: cursor.read_i32le()?,
            heat_status: cursor.read_i32le()?,
            set_point: cursor.read_i32le()?,
            cool_set_point: cursor.read_i32le()?,
            heat_mode: cursor.read_i32le()?,
        });
    }

    let circuit_count = (cursor.read_i32le()?.max(0) as usize).min(cursor.remaining() / 12);
    let mut circuits = Vec::with_capacity(circuit_count);
    for _ in 0..circuit_count {
        let circuit_id = cursor.read_i32le()?;
        let state_raw = cursor.read_i32le()?;
        let color_set = cursor.read_u8()?;
        let color_pos = cursor.read_u8()?;
        let color_stagger = cursor.read_u8()?;
        let delay = cursor.read_u8()?;
        circuits.push(CircuitStatus {
            circuit_id,
            state: state_raw > 0,
            color_set,
            color_pos,
            color_stagger,
            delay,
        });
    }

    let chemistry = InlineChemistry {
        ph: cursor.read_i32le()?,
        orp: cursor.read_i32le()?,
        saturation: cursor.read_i32le()?,
        salt_ppm: cursor.read_i32le()?,
        ph_tank: cursor.read_i32le()?,
        orp_tank: cursor.read_i32le()?,
        alarms: cursor.read_i32le()?,
    };

    Ok(PoolStatus {
        panel_mode,
        freeze_mode,
        remotes,
        pool_delay,
        spa_delay,
        cleaner_delay,
        air_temp,
        bodies,
        circuits,
        chemistry,
    })
}

// ── Controller Config ───────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CircuitConfig {
    pub circuit_id: i32,
    pub name: String,
    pub name_index: u8,
    pub function: u8,
    pub interface: u8,
    pub flags: u8,
    pub color_set: u8,
    pub color_pos: u8,
    pub color_stagger: u8,
    pub device_id: u8,
    pub default_runtime: u16,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColorInfo {
    pub name: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ControllerConfig {
    pub controller_id: i32,
    pub pool_min_setpoint: u8,
    pub pool_max_setpoint: u8,
    pub spa_min_setpoint: u8,
    pub spa_max_setpoint: u8,
    pub is_celsius: bool,
    pub controller_type: u8,
    pub hw_type: u8,
    pub controller_data: u8,
    pub equipment_flags: EquipmentFlags,
    pub generic_circ_name: String,
    pub circuits: Vec<CircuitConfig>,
    pub colors: Vec<ColorInfo>,
    pub pump_circ_array: [u8; 8],
    pub interface_tab_flags: i32,
    pub show_alarms: i32,
}

pub fn parse_controller_config(payload: &[u8]) -> Result<ControllerConfig> {
    let mut cursor = Cursor::new(payload);

    let controller_id_raw = cursor.read_i32le()?;
    let controller_id = controller_id_raw - 99;

    let pool_min_setpoint = cursor.read_u8()?;
    let pool_max_setpoint = cursor.read_u8()?;
    let spa_min_setpoint = cursor.read_u8()?;
    let spa_max_setpoint = cursor.read_u8()?;
    let is_celsius = cursor.read_u8()? != 0;
    let controller_type = cursor.read_u8()?;
    let hw_type = cursor.read_u8()?;
    let controller_data = cursor.read_u8()?;

    let eq_flags_raw = cursor.read_i32le()? as u32;
    let equipment_flags = EquipmentFlags::from_raw(eq_flags_raw);

    let generic_circ_name = decode_sl_string(&mut cursor)?;

    let circuit_count = (cursor.read_i32le()?.max(0) as usize).min(cursor.remaining() / 16);
    let mut circuits = Vec::with_capacity(circuit_count);
    for _ in 0..circuit_count {
        let circuit_id = cursor.read_i32le()?;
        let name = decode_sl_string(&mut cursor)?;
        let name_index = cursor.read_u8()?;
        let function = cursor.read_u8()?;
        let interface = cursor.read_u8()?;
        let flags = cursor.read_u8()?;
        let color_set = cursor.read_u8()?;
        let color_pos = cursor.read_u8()?;
        let color_stagger = cursor.read_u8()?;
        let device_id = cursor.read_u8()?;
        let default_runtime = cursor.read_u16le()?;
        cursor.skip(2)?; // unknown trailing bytes

        circuits.push(CircuitConfig {
            circuit_id,
            name,
            name_index,
            function,
            interface,
            flags,
            color_set,
            color_pos,
            color_stagger,
            device_id,
            default_runtime,
        });
    }

    let color_count = (cursor.read_i32le()?.max(0) as usize).min(cursor.remaining() / 16);
    let mut colors = Vec::with_capacity(color_count);
    for _ in 0..color_count {
        let name = decode_sl_string(&mut cursor)?;
        let r = (cursor.read_i32le()? & 0xFF) as u8;
        let g = (cursor.read_i32le()? & 0xFF) as u8;
        let b = (cursor.read_i32le()? & 0xFF) as u8;
        colors.push(ColorInfo { name, r, g, b });
    }

    let pump_bytes = cursor.read_bytes(8)?;
    let mut pump_circ_array = [0u8; 8];
    pump_circ_array.copy_from_slice(pump_bytes);

    let interface_tab_flags = cursor.read_i32le()?;
    let show_alarms = cursor.read_i32le()?;

    Ok(ControllerConfig {
        controller_id,
        pool_min_setpoint,
        pool_max_setpoint,
        spa_min_setpoint,
        spa_max_setpoint,
        is_celsius,
        controller_type,
        hw_type,
        controller_data,
        equipment_flags,
        generic_circ_name,
        circuits,
        colors,
        pump_circ_array,
        interface_tab_flags,
        show_alarms,
    })
}

// ── Chemistry Data ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChemData {
    pub is_valid: bool,
    pub ph: f32,
    pub orp: u16,
    pub ph_set_point: f32,
    pub orp_set_point: u16,
    pub ph_dose_time: u32,
    pub orp_dose_time: u32,
    pub ph_dose_volume: u16,
    pub orp_dose_volume: u16,
    pub ph_supply_level: u8,
    pub orp_supply_level: u8,
    pub saturation: f32,
    pub calcium: u16,
    pub cyanuric_acid: u16,
    pub alkalinity: u16,
    pub salt_ppm: u16,
    pub water_temp: u8,
    pub alarms: u8,
    pub alerts: u8,
    pub dose_flags: u8,
    pub config_flags: u8,
    pub fw_minor: u8,
    pub fw_major: u8,
    pub balance_flags: u8,
}

pub fn parse_chem_data(payload: &[u8]) -> Result<ChemData> {
    let mut cursor = Cursor::new(payload);

    let sentinel = cursor.read_u32le()?;
    let is_valid = sentinel == 42;

    cursor.skip(1)?; // unknown byte

    let ph_raw = cursor.read_u16be()?;
    let orp = cursor.read_u16be()?;
    let ph_set_raw = cursor.read_u16be()?;
    let orp_set_point = cursor.read_u16be()?;
    let ph_dose_time = cursor.read_u32be()?;
    let orp_dose_time = cursor.read_u32be()?;
    let ph_dose_volume = cursor.read_u16be()?;
    let orp_dose_volume = cursor.read_u16be()?;
    let ph_supply_level = cursor.read_u8()?;
    let orp_supply_level = cursor.read_u8()?;

    let sat_raw = cursor.read_u8()?;
    let saturation = if sat_raw & 0x80 != 0 {
        -((256 - sat_raw as i16) as f32) / 100.0
    } else {
        sat_raw as f32 / 100.0
    };

    let calcium = cursor.read_u16be()?;
    let cyanuric_acid = cursor.read_u16be()?;
    let alkalinity = cursor.read_u16be()?;

    let salt_raw = cursor.read_u8()?;
    let salt_ppm = salt_raw as u16 * 50;

    cursor.skip(1)?; // probe_is_celsius

    let water_temp = cursor.read_u8()?;
    let alarms = cursor.read_u8()?;
    let alerts = cursor.read_u8()?;
    let dose_flags = cursor.read_u8()?;
    let config_flags = cursor.read_u8()?;
    let fw_minor = cursor.read_u8()?;
    let fw_major = cursor.read_u8()?;
    let balance_flags = cursor.read_u8()?;

    Ok(ChemData {
        is_valid,
        ph: ph_raw as f32 / 100.0,
        orp,
        ph_set_point: ph_set_raw as f32 / 100.0,
        orp_set_point,
        ph_dose_time,
        orp_dose_time,
        ph_dose_volume,
        orp_dose_volume,
        ph_supply_level,
        orp_supply_level,
        saturation,
        calcium,
        cyanuric_acid,
        alkalinity,
        salt_ppm,
        water_temp,
        alarms,
        alerts,
        dose_flags,
        config_flags,
        fw_minor,
        fw_major,
        balance_flags,
    })
}

// ── SCG Config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScgConfig {
    pub installed: bool,
    pub status: i32,
    pub pool_set_point: i32,
    pub spa_set_point: i32,
    pub salt_ppm: i32,
    pub flags: i32,
    pub super_chlor_timer: i32,
}

pub fn parse_scg_config(payload: &[u8]) -> Result<ScgConfig> {
    let mut cursor = Cursor::new(payload);

    let installed_raw = cursor.read_i32le()?;
    let status = cursor.read_i32le()?;
    let pool_set_point = cursor.read_i32le()?;
    let spa_set_point = cursor.read_i32le()?;
    let salt_ppm = cursor.read_i32le()?;
    let flags = cursor.read_i32le()?;
    let super_chlor_timer = cursor.read_i32le()?;

    Ok(ScgConfig {
        installed: installed_raw != 0,
        status,
        pool_set_point,
        spa_set_point,
        salt_ppm,
        flags,
        super_chlor_timer,
    })
}

// ── Pump Status ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PumpCircuit {
    pub circuit_id: u32,
    pub speed: u32,
    pub is_rpm: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PumpStatus {
    pub pump_type: u32,
    pub is_running: bool,
    pub watts: u32,
    pub rpm: u32,
    pub unknown_1: u32,
    pub gpm: u32,
    pub unknown_2: u32,
    pub circuits: Vec<PumpCircuit>,
}

pub fn parse_pump_status(payload: &[u8]) -> Result<PumpStatus> {
    if payload.is_empty() {
        return Ok(PumpStatus {
            pump_type: 0,
            is_running: false,
            watts: 0,
            rpm: 0,
            unknown_1: 0,
            gpm: 0,
            unknown_2: 0,
            circuits: Vec::new(),
        });
    }

    let mut cursor = Cursor::new(payload);

    let pump_type = cursor.read_u32le()?;
    if pump_type == 0 {
        return Ok(PumpStatus {
            pump_type: 0,
            is_running: false,
            watts: 0,
            rpm: 0,
            unknown_1: 0,
            gpm: 0,
            unknown_2: 0,
            circuits: Vec::new(),
        });
    }

    let is_running_raw = cursor.read_u32le()?;
    let watts = cursor.read_u32le()?;
    let rpm = cursor.read_u32le()?;
    let unknown_1 = cursor.read_u32le()?;
    let gpm = cursor.read_u32le()?;
    let unknown_2 = cursor.read_u32le()?;

    let mut circuits = Vec::with_capacity(8);
    for _ in 0..8 {
        let circuit_id = cursor.read_u32le()?;
        let speed = cursor.read_u32le()?;
        let is_rpm_raw = cursor.read_u32le()?;
        circuits.push(PumpCircuit {
            circuit_id,
            speed,
            is_rpm: is_rpm_raw != 0,
        });
    }

    Ok(PumpStatus {
        pump_type,
        is_running: is_running_raw != 0,
        watts,
        rpm,
        unknown_1,
        gpm,
        unknown_2,
        circuits,
    })
}

// ── Schedule Data ───────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScheduleEvent {
    pub schedule_id: u32,
    pub circuit_id: u32,
    pub start_time: u32,
    pub stop_time: u32,
    pub day_mask: u32,
    pub flags: u32,
    pub heat_cmd: u32,
    pub heat_set_point: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScheduleData {
    pub events: Vec<ScheduleEvent>,
}

pub fn parse_schedule_data(payload: &[u8]) -> Result<ScheduleData> {
    let mut cursor = Cursor::new(payload);

    let event_count = (cursor.read_u32le()? as usize).min(cursor.remaining() / 32);
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        let schedule_id_raw = cursor.read_u32le()?;
        let circuit_id_raw = cursor.read_u32le()?;
        let start_time = cursor.read_u32le()?;
        let stop_time = cursor.read_u32le()?;
        let day_mask = cursor.read_u32le()?;
        let flags = cursor.read_u32le()?;
        let heat_cmd = cursor.read_u32le()?;
        let heat_set_point = cursor.read_u32le()?;

        events.push(ScheduleEvent {
            schedule_id: schedule_id_raw.wrapping_sub(699),
            circuit_id: circuit_id_raw.wrapping_sub(499),
            start_time,
            stop_time,
            day_mask,
            flags,
            heat_cmd,
            heat_set_point,
        });
    }

    Ok(ScheduleData { events })
}

// ── Weather Response ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForecastDay {
    pub time: SLDateTime,
    pub high_temp: i32,
    pub low_temp: i32,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeatherResponse {
    pub version: i32,
    pub zip_code: String,
    pub last_update: SLDateTime,
    pub last_request: SLDateTime,
    pub date_text: String,
    pub description: String,
    pub current_temp: i32,
    pub humidity: i32,
    pub wind: String,
    pub pressure: i32,
    pub dew_point: i32,
    pub wind_chill: i32,
    pub visibility: i32,
    pub forecast_days: Vec<ForecastDay>,
    pub sunrise: i32,
    pub sunset: i32,
}

pub fn parse_weather(payload: &[u8]) -> Result<WeatherResponse> {
    let mut cursor = Cursor::new(payload);

    let version = cursor.read_i32le()?;
    let zip_code = decode_sl_string(&mut cursor)?;
    let last_update = decode_sl_datetime(&mut cursor)?;
    let last_request = decode_sl_datetime(&mut cursor)?;
    let date_text = decode_sl_string(&mut cursor)?;
    let description = decode_sl_string(&mut cursor)?;
    let current_temp = cursor.read_i32le()?;
    let humidity = cursor.read_i32le()?;
    let wind = decode_sl_string(&mut cursor)?;
    let pressure = cursor.read_i32le()?;
    let dew_point = cursor.read_i32le()?;
    let wind_chill = cursor.read_i32le()?;
    let visibility = cursor.read_i32le()?;

    let num_days = cursor.read_i32le()? as usize;
    let mut forecast_days = Vec::with_capacity(num_days);
    for _ in 0..num_days {
        let time = decode_sl_datetime(&mut cursor)?;
        let high_temp = cursor.read_i32le()?;
        let low_temp = cursor.read_i32le()?;
        let text = decode_sl_string(&mut cursor)?;
        forecast_days.push(ForecastDay {
            time,
            high_temp,
            low_temp,
            text,
        });
    }

    let sunrise = cursor.read_i32le()?;
    let sunset = cursor.read_i32le()?;

    Ok(WeatherResponse {
        version,
        zip_code,
        last_update,
        last_request,
        date_text,
        description,
        current_temp,
        humidity,
        wind,
        pressure,
        dew_point,
        wind_chill,
        visibility,
        forecast_days,
        sunrise,
        sunset,
    })
}

// ── Discovery Response ──────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveryResponse {
    pub ip: [u8; 4],
    pub port: u16,
    pub gateway_type: u8,
    pub gateway_subtype: u8,
    pub adapter_name: String,
}

pub fn parse_discovery(payload: &[u8]) -> Result<DiscoveryResponse> {
    let mut cursor = Cursor::new(payload);

    cursor.skip(4)?; // check_digit

    let ip0 = cursor.read_u8()?;
    let ip1 = cursor.read_u8()?;
    let ip2 = cursor.read_u8()?;
    let ip3 = cursor.read_u8()?;

    let port = cursor.read_u16le()?;
    let gateway_type = cursor.read_u8()?;
    let gateway_subtype = cursor.read_u8()?;

    let remaining = cursor.remaining();
    let name_bytes = cursor.read_bytes(remaining)?;
    // Null-terminated string
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
    let adapter_name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();

    Ok(DiscoveryResponse {
        ip: [ip0, ip1, ip2, ip3],
        port,
        gateway_type,
        gateway_subtype,
        adapter_name,
    })
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::HEADER_SIZE;

    #[test]
    fn fixture_version_response() {
        let data = include_bytes!("../../test-fixtures/version_response.bin");
        assert_eq!(data.len(), 64);
        let payload = &data[HEADER_SIZE..];
        let resp = parse_version(payload).unwrap();
        assert_eq!(resp.version, "POOL: 5.2 Build 738.0 Rel");
    }

    #[test]
    fn fixture_system_time_response() {
        let data = include_bytes!("../../test-fixtures/system_time_response.bin");
        assert_eq!(data.len(), 28);
        let payload = &data[HEADER_SIZE..];
        let resp = parse_system_time(payload).unwrap();
        assert_eq!(resp.time.year, 2026);
        assert_eq!(resp.time.month, 3);
        assert_eq!(resp.time.day, 17);
        assert!(resp.adjust_for_dst);
    }

    #[test]
    fn fixture_status_response() {
        let data = include_bytes!("../../test-fixtures/status_response.bin");
        assert_eq!(data.len(), 192);
        let payload = &data[HEADER_SIZE..];
        let status = parse_pool_status(payload).unwrap();

        assert_eq!(status.panel_mode, 1);
        assert!(!status.freeze_mode);
        assert_eq!(status.air_temp, 68);

        // 2 bodies: pool and spa
        assert_eq!(status.bodies.len(), 2);
        assert_eq!(status.bodies[0].body_type, 0); // Pool
        assert_eq!(status.bodies[0].current_temp, 105);
        assert_eq!(status.bodies[1].body_type, 1); // Spa
        assert_eq!(status.bodies[1].current_temp, 103);

        // Circuits
        assert_eq!(status.circuits.len(), 7);
        assert_eq!(status.circuits[0].circuit_id, 500);
        assert_eq!(status.circuits[6].circuit_id, 506);
        // All circuits off in this capture
        for circ in &status.circuits {
            assert!(!circ.state);
        }
    }

    #[test]
    fn fixture_controller_config_response() {
        let data = include_bytes!("../../test-fixtures/controller_config_response.bin");
        assert_eq!(data.len(), 468);
        let payload = &data[HEADER_SIZE..];
        let config = parse_controller_config(payload).unwrap();

        assert_eq!(config.controller_id, 1); // 100 - 99
        assert!(!config.is_celsius);
        assert_eq!(config.pool_min_setpoint, 40);
        assert_eq!(config.pool_max_setpoint, 104);
        assert_eq!(config.spa_min_setpoint, 40);
        assert_eq!(config.spa_max_setpoint, 104);
        assert_eq!(config.controller_type, 1);
        assert_eq!(config.hw_type, 0);

        // Equipment flags
        assert_eq!(config.equipment_flags.raw(), 24);
        assert!(config.equipment_flags.has_intellibrite());
        assert!(config.equipment_flags.has_intelliflo_0());
        assert!(!config.equipment_flags.has_chlorinator());

        // Circuits
        assert_eq!(config.circuits.len(), 7);
        assert_eq!(config.circuits[0].name, "Spa");
        assert_eq!(config.circuits[0].circuit_id, 500);
        assert_eq!(config.circuits[0].function, 1); // Spa
        assert_eq!(config.circuits[1].name, "Lights");
        assert_eq!(config.circuits[1].function, 16); // IntelliBrite
        assert_eq!(config.circuits[5].name, "Pool");
        assert_eq!(config.circuits[5].function, 2); // Pool
        for circ in &config.circuits {
            assert_eq!(circ.default_runtime, 720);
        }

        // Colors
        assert_eq!(config.colors.len(), 8);
        assert_eq!(config.colors[0].name, "White");
        assert_eq!(config.colors[0].r, 255);
        assert_eq!(config.colors[0].g, 255);
        assert_eq!(config.colors[0].b, 255);

        assert_eq!(config.generic_circ_name, "Water Features");
        assert_eq!(config.interface_tab_flags, 127);
        assert_eq!(config.show_alarms, 0);
    }

    #[test]
    fn fixture_chem_data_response() {
        let data = include_bytes!("../../test-fixtures/chem_data_response.bin");
        assert_eq!(data.len(), 56);
        let payload = &data[HEADER_SIZE..];
        let chem = parse_chem_data(payload).unwrap();

        assert!(chem.is_valid); // sentinel == 42
        assert_eq!(chem.ph, 0.0);
        assert_eq!(chem.orp, 0);
        assert_eq!(chem.ph_set_point, 0.0);
        assert_eq!(chem.orp_set_point, 0);
        assert_eq!(chem.ph_dose_time, 0);
        assert_eq!(chem.orp_dose_time, 0);
        assert_eq!(chem.salt_ppm, 0);
        assert_eq!(chem.water_temp, 0);
    }

    #[test]
    fn fixture_scg_config_response() {
        let data = include_bytes!("../../test-fixtures/scg_config_response.bin");
        assert_eq!(data.len(), 36);
        let payload = &data[HEADER_SIZE..];
        let scg = parse_scg_config(payload).unwrap();

        assert!(!scg.installed);
        assert_eq!(scg.status, 1);
        assert_eq!(scg.pool_set_point, 50);
        assert_eq!(scg.spa_set_point, 0);
        assert_eq!(scg.salt_ppm, 0);
        assert_eq!(scg.flags, 0);
        assert_eq!(scg.super_chlor_timer, 0);
    }

    #[test]
    fn fixture_pump_status_0_response() {
        let data = include_bytes!("../../test-fixtures/pump_status_0_response.bin");
        assert_eq!(data.len(), 132);
        let payload = &data[HEADER_SIZE..];
        let pump = parse_pump_status(payload).unwrap();

        assert_eq!(pump.pump_type, 2); // VS
        assert!(!pump.is_running);
        assert_eq!(pump.watts, 0);
        assert_eq!(pump.rpm, 0);
        assert_eq!(pump.unknown_1, 0);
        assert_eq!(pump.gpm, 0);
        assert_eq!(pump.unknown_2, 255);
        assert_eq!(pump.circuits.len(), 8);

        // First circuit: id=6, speed=2000, is_rpm=true
        assert_eq!(pump.circuits[0].circuit_id, 6);
        assert_eq!(pump.circuits[0].speed, 2000);
        assert!(pump.circuits[0].is_rpm);
    }

    #[test]
    fn fixture_pump_status_1_response_empty() {
        let data = include_bytes!("../../test-fixtures/pump_status_1_response.bin");
        assert_eq!(data.len(), 8);
        // Header only, no payload
        let payload = &data[HEADER_SIZE..];
        assert!(payload.is_empty());
        let pump = parse_pump_status(payload).unwrap();

        assert_eq!(pump.pump_type, 0);
        assert!(!pump.is_running);
        assert!(pump.circuits.is_empty());
    }

    #[test]
    fn fixture_schedule_recurring_response() {
        let data = include_bytes!("../../test-fixtures/schedule_recurring_response.bin");
        assert_eq!(data.len(), 940);
        let payload = &data[HEADER_SIZE..];
        let sched = parse_schedule_data(payload).unwrap();

        // 932 bytes payload: 4 (count) + 29 * 32 = 932
        assert_eq!(sched.events.len(), 29);

        // First event: raw schedule_id=700, adjusted=1; raw circuit_id=514, adjusted=15
        assert_eq!(sched.events[0].schedule_id, 1); // 700 - 699
        assert_eq!(sched.events[0].circuit_id, 15); // 514 - 499
    }

    #[test]
    fn fixture_schedule_runonce_response() {
        let data = include_bytes!("../../test-fixtures/schedule_runonce_response.bin");
        assert_eq!(data.len(), 12);
        let payload = &data[HEADER_SIZE..];
        let sched = parse_schedule_data(payload).unwrap();

        assert_eq!(sched.events.len(), 0);
    }

    #[test]
    fn fixture_weather_response() {
        let data = include_bytes!("../../test-fixtures/weather_response.bin");
        assert_eq!(data.len(), 96);
        let payload = &data[HEADER_SIZE..];
        let weather = parse_weather(payload).unwrap();

        assert_eq!(weather.version, 0);
        assert_eq!(weather.forecast_days.len(), 0);
        assert_eq!(weather.sunrise, 354);
        assert_eq!(weather.sunset, 1070);
    }

    #[test]
    fn fixture_discovery_response() {
        let data = include_bytes!("../../test-fixtures/discovery_response.bin");
        assert_eq!(data.len(), 40);
        // Discovery is UDP, no message header to skip
        let resp = parse_discovery(data).unwrap();

        assert_eq!(resp.ip, [192, 168, 1, 89]);
        assert_eq!(resp.port, 80);
        assert_eq!(resp.gateway_type, 2);
        assert_eq!(resp.gateway_subtype, 12);
        assert_eq!(resp.adapter_name, "Pentair: 21-A9-F0");
    }

    // ── Unit tests for edge cases ───────────────────────────────────────

    #[test]
    fn chem_data_saturation_positive() {
        // saturation byte = 0x32 (50) -> 0.50
        // Build a minimal valid chem_data payload (sentinel=42, then fields)
        let mut payload = Vec::new();
        payload.extend_from_slice(&42u32.to_le_bytes()); // sentinel
        payload.push(0); // unknown
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph_set_point
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp_set_point
        payload.extend_from_slice(&0u32.to_be_bytes()); // ph_dose_time
        payload.extend_from_slice(&0u32.to_be_bytes()); // orp_dose_time
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph_dose_volume
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp_dose_volume
        payload.push(0); // ph_supply_level
        payload.push(0); // orp_supply_level
        payload.push(50); // saturation = 50 -> 0.50
        payload.extend_from_slice(&0u16.to_be_bytes()); // calcium
        payload.extend_from_slice(&0u16.to_be_bytes()); // cyanuric_acid
        payload.extend_from_slice(&0u16.to_be_bytes()); // alkalinity
        payload.push(0); // salt
        payload.push(0); // probe_is_celsius
        payload.push(0); // water_temp
        payload.push(0); // alarms
        payload.push(0); // alerts
        payload.push(0); // dose_flags
        payload.push(0); // config_flags
        payload.push(0); // fw_minor
        payload.push(0); // fw_major
        payload.push(0); // balance_flags

        let chem = parse_chem_data(&payload).unwrap();
        assert!((chem.saturation - 0.50).abs() < f32::EPSILON);
    }

    #[test]
    fn chem_data_saturation_negative() {
        // saturation byte = 0xCE (206) -> bit 7 set -> -(256-206)/100 = -0.50
        let mut payload = Vec::new();
        payload.extend_from_slice(&42u32.to_le_bytes()); // sentinel
        payload.push(0); // unknown
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph_set_point
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp_set_point
        payload.extend_from_slice(&0u32.to_be_bytes()); // ph_dose_time
        payload.extend_from_slice(&0u32.to_be_bytes()); // orp_dose_time
        payload.extend_from_slice(&0u16.to_be_bytes()); // ph_dose_volume
        payload.extend_from_slice(&0u16.to_be_bytes()); // orp_dose_volume
        payload.push(0); // ph_supply_level
        payload.push(0); // orp_supply_level
        payload.push(0xCE); // saturation = 206 -> -0.50
        payload.extend_from_slice(&0u16.to_be_bytes()); // calcium
        payload.extend_from_slice(&0u16.to_be_bytes()); // cyanuric_acid
        payload.extend_from_slice(&0u16.to_be_bytes()); // alkalinity
        payload.push(0); // salt
        payload.push(0); // probe_is_celsius
        payload.push(0); // water_temp
        payload.push(0); // alarms
        payload.push(0); // alerts
        payload.push(0); // dose_flags
        payload.push(0); // config_flags
        payload.push(0); // fw_minor
        payload.push(0); // fw_major
        payload.push(0); // balance_flags

        let chem = parse_chem_data(&payload).unwrap();
        assert!((chem.saturation - (-0.50)).abs() < f32::EPSILON);
    }

    #[test]
    fn pump_status_type_zero_returns_early() {
        // pump_type=0 should return early with empty fields
        let payload = 0u32.to_le_bytes();
        let pump = parse_pump_status(&payload).unwrap();
        assert_eq!(pump.pump_type, 0);
        assert!(pump.circuits.is_empty());
    }

    #[test]
    fn schedule_data_empty() {
        let payload = 0u32.to_le_bytes();
        let sched = parse_schedule_data(&payload).unwrap();
        assert!(sched.events.is_empty());
    }

    #[test]
    fn version_empty_string() {
        // SLString with length 0
        let payload = 0u32.to_le_bytes();
        let resp = parse_version(&payload).unwrap();
        assert_eq!(resp.version, "");
    }

    #[test]
    fn system_time_dst_false() {
        // Build 20 bytes: 16 byte datetime + 0 for dst
        let mut payload = vec![0u8; 16]; // zero datetime
        payload.extend_from_slice(&0i32.to_le_bytes()); // adjust_for_dst = false
        let resp = parse_system_time(&payload).unwrap();
        assert!(!resp.adjust_for_dst);
    }

    #[test]
    fn pool_status_freeze_mode_set() {
        // Build a minimal status payload with freeze_mode bit set
        let mut payload = Vec::new();
        payload.extend_from_slice(&1i32.to_le_bytes()); // panel_mode
        payload.push(0x08); // freeze_mode byte with bit 3 set
        payload.push(0); // remotes
        payload.push(0); // pool_delay
        payload.push(0); // spa_delay
        payload.push(0); // cleaner_delay
        payload.extend_from_slice(&[0, 0, 0]); // skip 3
        payload.extend_from_slice(&72i32.to_le_bytes()); // air_temp
        payload.extend_from_slice(&0i32.to_le_bytes()); // body_count = 0
        payload.extend_from_slice(&0i32.to_le_bytes()); // circuit_count = 0
        // 7 × i32 for chemistry
        for _ in 0..7 {
            payload.extend_from_slice(&0i32.to_le_bytes());
        }
        let status = parse_pool_status(&payload).unwrap();
        assert!(status.freeze_mode);
        assert_eq!(status.air_temp, 72);
    }
}
