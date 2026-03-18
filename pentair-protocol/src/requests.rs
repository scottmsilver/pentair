use crate::action::Action;
use crate::codec::encode_message;
use crate::types::{encode_sl_array, encode_sl_datetime, encode_sl_string, SLDateTime};

/// The raw connection string sent before any framed messages.
pub const CONNECT_STRING: &[u8] = b"CONNECTSERVERHOST\r\n\r\n";

/// Build a payload from a sequence of i32 values encoded as little-endian.
fn payload_i32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Challenge request (action 14), empty payload.
pub fn build_challenge_request() -> Vec<u8> {
    encode_message(u16::from(Action::ChallengeRequest), &[])
}

/// Login request (action 27).
///
/// schema=348, connection_type=0, client_version="Android", password=16 zero bytes, process_id=2.
pub fn build_login_request() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&348i32.to_le_bytes()); // schema
    payload.extend_from_slice(&0i32.to_le_bytes()); // connection_type
    payload.extend(encode_sl_string("Android")); // client_version
    payload.extend(encode_sl_array(&[0u8; 16])); // password (16 zero bytes)
    payload.extend_from_slice(&2i32.to_le_bytes()); // process_id
    encode_message(u16::from(Action::LoginRequest), &payload)
}

/// Add client (action 12522).
///
/// controller_id=0, client_id (caller provides).
pub fn build_add_client(client_id: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, client_id]);
    encode_message(u16::from(Action::AddClient), &payload)
}

/// Remove client (action 12524).
///
/// controller_id=0, client_id.
pub fn build_remove_client(client_id: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, client_id]);
    encode_message(u16::from(Action::RemoveClient), &payload)
}

/// Ping (action 16), empty payload.
pub fn build_ping() -> Vec<u8> {
    encode_message(u16::from(Action::PingRequest), &[])
}

/// Get version (action 8120), empty payload.
pub fn build_get_version() -> Vec<u8> {
    encode_message(u16::from(Action::GetVersion), &[])
}

/// Get system time (action 8110), empty payload.
pub fn build_get_system_time() -> Vec<u8> {
    encode_message(u16::from(Action::GetSystemTime), &[])
}

/// Get status (action 12526).
///
/// param=0.
pub fn build_get_status() -> Vec<u8> {
    let payload = payload_i32s(&[0]);
    encode_message(u16::from(Action::GetStatus), &payload)
}

/// Get controller config (action 12532).
///
/// param1=0, param2=0.
pub fn build_get_controller_config() -> Vec<u8> {
    let payload = payload_i32s(&[0, 0]);
    encode_message(u16::from(Action::GetControllerConfig), &payload)
}

/// Button press / circuit control (action 12530).
///
/// controller_id=0, circuit_id (logical, will ADD 499 for wire), state (true=on, false=off).
pub fn build_button_press(circuit_id: i32, state: bool) -> Vec<u8> {
    let wire_circuit = circuit_id + 499;
    let state_val = if state { 1i32 } else { 0i32 };
    let payload = payload_i32s(&[0, wire_circuit, state_val]);
    encode_message(u16::from(Action::ButtonPress), &payload)
}

/// Set heat set point (action 12528).
///
/// controller_id=0, body_type (0=Pool, 1=Spa), temperature.
pub fn build_set_heat_setpoint(body_type: i32, temperature: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, body_type, temperature]);
    encode_message(u16::from(Action::SetHeatSetPoint), &payload)
}

/// Set heat mode (action 12538).
///
/// controller_id=0, body_type, heat_mode (0-3).
pub fn build_set_heat_mode(body_type: i32, heat_mode: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, body_type, heat_mode]);
    encode_message(u16::from(Action::SetHeatMode), &payload)
}

/// Set cool set point (action 12590).
///
/// controller_id=0, body_type, temperature.
pub fn build_set_cool_setpoint(body_type: i32, temperature: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, body_type, temperature]);
    encode_message(u16::from(Action::SetCoolSetPoint), &payload)
}

/// Color lights command (action 12556).
///
/// controller_id=0, command (LightCommand as i32).
pub fn build_color_lights_command(command: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, command]);
    encode_message(u16::from(Action::ColorLightsCommand), &payload)
}

/// Get chem data (action 12592).
///
/// controller_idx=0.
pub fn build_get_chem_data() -> Vec<u8> {
    let payload = payload_i32s(&[0]);
    encode_message(u16::from(Action::GetChemData), &payload)
}

/// Get SCG config (action 12572).
///
/// controller_idx=0.
pub fn build_get_scg_config() -> Vec<u8> {
    let payload = payload_i32s(&[0]);
    encode_message(u16::from(Action::GetScgConfig), &payload)
}

/// Set SCG config (action 12576).
///
/// controller_id=0, pool_output, spa_output, reserved_1=0, reserved_2=0.
pub fn build_set_scg_config(pool_output: i32, spa_output: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, pool_output, spa_output, 0, 0]);
    encode_message(u16::from(Action::SetScgConfig), &payload)
}

/// Get pump status (action 12584).
///
/// controller_idx=0, pump_index (0-indexed, sent as-is).
pub fn build_get_pump_status(pump_index: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, pump_index]);
    encode_message(u16::from(Action::GetPumpStatus), &payload)
}

/// Get schedule data (action 12542).
///
/// param=0, schedule_type (0=Recurring, 1=RunOnce).
pub fn build_get_schedule_data(schedule_type: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, schedule_type]);
    encode_message(u16::from(Action::GetScheduleData), &payload)
}

/// Add schedule event (action 12544).
///
/// param=0, schedule_type.
pub fn build_add_schedule_event(schedule_type: i32) -> Vec<u8> {
    let payload = payload_i32s(&[0, schedule_type]);
    encode_message(u16::from(Action::AddScheduleEvent), &payload)
}

/// Delete schedule event (action 12546).
///
/// param=0, schedule_id (logical, ADD 699 for wire).
pub fn build_delete_schedule_event(schedule_id: i32) -> Vec<u8> {
    let wire_schedule = schedule_id + 699;
    let payload = payload_i32s(&[0, wire_schedule]);
    encode_message(u16::from(Action::DeleteScheduleEvent), &payload)
}

/// Set schedule event (action 12548).
///
/// param=0, schedule_id (logical+699), circuit_id (logical+499),
/// start_time, stop_time, day_mask, flags=2, heat_cmd=4, heat_set_point.
pub fn build_set_schedule_event(
    schedule_id: i32,
    circuit_id: i32,
    start_time: i32,
    stop_time: i32,
    day_mask: i32,
    heat_set_point: i32,
) -> Vec<u8> {
    let wire_schedule = schedule_id + 699;
    let wire_circuit = circuit_id + 499;
    let payload = payload_i32s(&[
        0,              // param
        wire_schedule,  // schedule_id on wire
        wire_circuit,   // circuit_id on wire
        start_time,
        stop_time,
        day_mask,
        2,              // flags (default)
        4,              // heat_cmd (default)
        heat_set_point,
    ]);
    encode_message(u16::from(Action::SetScheduleEvent), &payload)
}

/// Get weather forecast (action 9807), empty payload.
pub fn build_get_weather_forecast() -> Vec<u8> {
    encode_message(u16::from(Action::GetWeatherForecast), &[])
}

/// Get history (action 12534).
///
/// controller_idx=0, start_time, end_time, sender_id.
pub fn build_get_history(
    start_time: &SLDateTime,
    end_time: &SLDateTime,
    sender_id: i32,
) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0i32.to_le_bytes()); // controller_idx
    payload.extend(encode_sl_datetime(start_time));
    payload.extend(encode_sl_datetime(end_time));
    payload.extend_from_slice(&sender_id.to_le_bytes());
    encode_message(u16::from(Action::GetHistory), &payload)
}

/// Cancel delay (action 12580).
///
/// param=0 (all delays).
pub fn build_cancel_delay() -> Vec<u8> {
    let payload = payload_i32s(&[0]);
    encode_message(u16::from(Action::CancelDelay), &payload)
}

/// Get all errors (action 12582), empty payload.
pub fn build_get_all_errors() -> Vec<u8> {
    encode_message(u16::from(Action::GetAllErrors), &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode_header, Cursor, HEADER_SIZE};

    // ── 1. Login request matches fixture ──────────────────────────────

    #[test]
    fn login_request_matches_fixture() {
        let expected = include_bytes!("../../test-fixtures/login_request.bin");
        let actual = build_login_request();
        assert_eq!(
            actual.as_slice(),
            &expected[..],
            "build_login_request() does not match login_request.bin fixture"
        );
    }

    // ── 2. Button press offset ────────────────────────────────────────

    #[test]
    fn button_press_applies_circuit_offset() {
        let msg = build_button_press(1, true);
        let payload = &msg[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);
        let controller_id = cursor.read_i32le().unwrap();
        let wire_circuit = cursor.read_i32le().unwrap();
        let state = cursor.read_i32le().unwrap();
        assert_eq!(controller_id, 0);
        assert_eq!(wire_circuit, 500, "circuit_id 1 should become 500 on wire (1+499)");
        assert_eq!(state, 1);
    }

    // ── 3. Delete schedule event offset ───────────────────────────────

    #[test]
    fn delete_schedule_event_applies_offset() {
        let msg = build_delete_schedule_event(1);
        let payload = &msg[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);
        let param = cursor.read_i32le().unwrap();
        let wire_schedule = cursor.read_i32le().unwrap();
        assert_eq!(param, 0);
        assert_eq!(wire_schedule, 700, "schedule_id 1 should become 700 on wire (1+699)");
    }

    // ── 4. Set schedule event offsets ─────────────────────────────────

    #[test]
    fn set_schedule_event_applies_both_offsets() {
        let msg = build_set_schedule_event(1, 2, 480, 960, 0x7F, 82);
        let payload = &msg[HEADER_SIZE..];
        let mut cursor = Cursor::new(payload);
        let param = cursor.read_i32le().unwrap();
        let wire_schedule = cursor.read_i32le().unwrap();
        let wire_circuit = cursor.read_i32le().unwrap();
        let start_time = cursor.read_i32le().unwrap();
        let stop_time = cursor.read_i32le().unwrap();
        let day_mask = cursor.read_i32le().unwrap();
        let flags = cursor.read_i32le().unwrap();
        let heat_cmd = cursor.read_i32le().unwrap();
        let heat_set_point = cursor.read_i32le().unwrap();

        assert_eq!(param, 0);
        assert_eq!(wire_schedule, 700, "schedule_id 1 should become 700 on wire (1+699)");
        assert_eq!(wire_circuit, 501, "circuit_id 2 should become 501 on wire (2+499)");
        assert_eq!(start_time, 480);
        assert_eq!(stop_time, 960);
        assert_eq!(day_mask, 0x7F);
        assert_eq!(flags, 2);
        assert_eq!(heat_cmd, 4);
        assert_eq!(heat_set_point, 82);
        assert_eq!(cursor.remaining(), 0);
    }

    // ── 5. All builders produce valid framed messages ─────────────────

    #[test]
    fn all_builders_produce_valid_frames() {
        let dt = SLDateTime {
            year: 2026,
            month: 3,
            day_of_week: 3,
            day: 18,
            hour: 10,
            minute: 0,
            second: 0,
            millisecond: 0,
        };

        let messages: Vec<(&str, Vec<u8>)> = vec![
            ("challenge_request", build_challenge_request()),
            ("login_request", build_login_request()),
            ("add_client", build_add_client(42000)),
            ("remove_client", build_remove_client(42000)),
            ("ping", build_ping()),
            ("get_version", build_get_version()),
            ("get_system_time", build_get_system_time()),
            ("get_status", build_get_status()),
            ("get_controller_config", build_get_controller_config()),
            ("button_press", build_button_press(1, true)),
            ("set_heat_setpoint", build_set_heat_setpoint(0, 82)),
            ("set_heat_mode", build_set_heat_mode(1, 3)),
            ("set_cool_setpoint", build_set_cool_setpoint(0, 78)),
            ("color_lights_command", build_color_lights_command(5)),
            ("get_chem_data", build_get_chem_data()),
            ("get_scg_config", build_get_scg_config()),
            ("set_scg_config", build_set_scg_config(50, 0)),
            ("get_pump_status", build_get_pump_status(0)),
            ("get_schedule_data", build_get_schedule_data(0)),
            ("add_schedule_event", build_add_schedule_event(1)),
            ("delete_schedule_event", build_delete_schedule_event(1)),
            (
                "set_schedule_event",
                build_set_schedule_event(1, 1, 480, 960, 0x7F, 82),
            ),
            ("get_weather_forecast", build_get_weather_forecast()),
            ("get_history", build_get_history(&dt, &dt, 1)),
            ("cancel_delay", build_cancel_delay()),
            ("get_all_errors", build_get_all_errors()),
        ];

        for (name, msg) in &messages {
            assert!(
                msg.len() >= HEADER_SIZE,
                "{}: message too short ({} bytes, need at least {})",
                name,
                msg.len(),
                HEADER_SIZE
            );

            let header = decode_header(msg)
                .unwrap_or_else(|e| panic!("{}: decode_header failed: {}", name, e));

            let actual_payload_len = msg.len() - HEADER_SIZE;
            assert_eq!(
                header.data_length as usize, actual_payload_len,
                "{}: header.data_length ({}) != actual payload length ({})",
                name, header.data_length, actual_payload_len
            );
        }
    }

    // ── 6. CONNECT_STRING ─────────────────────────────────────────────

    #[test]
    fn connect_string_value_and_length() {
        assert_eq!(CONNECT_STRING, b"CONNECTSERVERHOST\r\n\r\n");
        assert_eq!(CONNECT_STRING.len(), 21);
    }
}
