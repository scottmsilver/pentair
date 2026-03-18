use crate::error::ProtocolError;

/// Every request/response/push message in the ScreenLogic protocol is
/// identified by a 16-bit action code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Action {
    // ── Authentication & session ──────────────────────────────────────
    LoginFailure = 13,
    ChallengeRequest = 14,
    ChallengeResponse = 15,
    PingRequest = 16,
    PingResponse = 17,
    LoginRequest = 27,
    LoginResponse = 28,

    // ── Errors ────────────────────────────────────────────────────────
    UnknownCommand = 30,
    BadParameter = 31,

    // ── System time ───────────────────────────────────────────────────
    GetSystemTime = 8110,
    SystemTimeResponse = 8111,
    SetSystemTime = 8112,
    SetSystemTimeResponse = 8113,

    // ── Version ───────────────────────────────────────────────────────
    GetVersion = 8120,
    VersionResponse = 8121,

    // ── Weather ───────────────────────────────────────────────────────
    WeatherForecastChanged = 9806,
    GetWeatherForecast = 9807,
    WeatherForecastResponse = 9808,

    // ── Push notifications ────────────────────────────────────────────
    StatusChanged = 12500,
    ScheduleChanged = 12501,
    HistoryDataPush = 12502,
    RuntimeChanged = 12503,
    ColorUpdatePush = 12504,
    ChemistryChanged = 12505,

    // ── Client registration ───────────────────────────────────────────
    AddClient = 12522,
    AddClientResponse = 12523,
    RemoveClient = 12524,
    RemoveClientResponse = 12525,

    // ── Status & control ──────────────────────────────────────────────
    GetStatus = 12526,
    StatusResponse = 12527,
    SetHeatSetPoint = 12528,
    SetHeatSetPointResponse = 12529,
    ButtonPress = 12530,
    ButtonPressResponse = 12531,
    GetControllerConfig = 12532,
    ControllerConfigResponse = 12533,
    GetHistory = 12534,
    HistoryResponse = 12535,
    SetHeatMode = 12538,
    SetHeatModeResponse = 12539,

    // ── Schedules ─────────────────────────────────────────────────────
    GetScheduleData = 12542,
    ScheduleDataResponse = 12543,
    AddScheduleEvent = 12544,
    AddScheduleEventResponse = 12545,
    DeleteScheduleEvent = 12546,
    DeleteScheduleEventResponse = 12547,
    SetScheduleEvent = 12548,
    SetScheduleEventResponse = 12549,
    SetCircuitRuntime = 12550,
    SetCircuitRuntimeResponse = 12551,

    // ── Lights ────────────────────────────────────────────────────────
    ColorLightsCommand = 12556,
    ColorLightsCommandResponse = 12557,

    // ── Circuits & names ──────────────────────────────────────────────
    GetCircuitNames = 12560,
    CircuitNamesResponse = 12561,
    GetCustomNames = 12562,
    CustomNamesResponse = 12563,
    SetCustomName = 12564,
    SetCustomNameResponse = 12565,

    // ── Equipment config ──────────────────────────────────────────────
    GetEquipmentConfig = 12566,
    EquipmentConfigResponse = 12567,
    SetEquipmentConfig = 12568,
    SetEquipmentConfigResponse = 12569,

    // ── Salt chlorine generator ───────────────────────────────────────
    GetScgConfig = 12572,
    ScgConfigResponse = 12573,
    SetScgEnabled = 12574,
    SetScgEnabledResponse = 12575,
    SetScgConfig = 12576,
    SetScgConfigResponse = 12577,

    // ── Delays & errors ───────────────────────────────────────────────
    CancelDelay = 12580,
    CancelDelayResponse = 12581,
    GetAllErrors = 12582,
    AllErrorsResponse = 12583,

    // ── Pumps ─────────────────────────────────────────────────────────
    GetPumpStatus = 12584,
    PumpStatusResponse = 12585,
    SetPumpSpeed = 12586,
    SetPumpSpeedResponse = 12587,

    // ── Cooling ───────────────────────────────────────────────────────
    SetCoolSetPoint = 12590,
    SetCoolSetPointResponse = 12591,

    // ── Chemistry ─────────────────────────────────────────────────────
    GetChemData = 12592,
    ChemDataResponse = 12593,
    GetChemHistory = 12596,
    ChemHistoryResponse = 12597,

    // ── Gateway (discovery) ───────────────────────────────────────────
    GatewayRequest = 18003,
    GatewayResponse = 18004,
}

impl TryFrom<u16> for Action {
    type Error = ProtocolError;

    fn try_from(value: u16) -> Result<Self, ProtocolError> {
        match value {
            13 => Ok(Action::LoginFailure),
            14 => Ok(Action::ChallengeRequest),
            15 => Ok(Action::ChallengeResponse),
            16 => Ok(Action::PingRequest),
            17 => Ok(Action::PingResponse),
            27 => Ok(Action::LoginRequest),
            28 => Ok(Action::LoginResponse),
            30 => Ok(Action::UnknownCommand),
            31 => Ok(Action::BadParameter),
            8110 => Ok(Action::GetSystemTime),
            8111 => Ok(Action::SystemTimeResponse),
            8112 => Ok(Action::SetSystemTime),
            8113 => Ok(Action::SetSystemTimeResponse),
            8120 => Ok(Action::GetVersion),
            8121 => Ok(Action::VersionResponse),
            9806 => Ok(Action::WeatherForecastChanged),
            9807 => Ok(Action::GetWeatherForecast),
            9808 => Ok(Action::WeatherForecastResponse),
            12500 => Ok(Action::StatusChanged),
            12501 => Ok(Action::ScheduleChanged),
            12502 => Ok(Action::HistoryDataPush),
            12503 => Ok(Action::RuntimeChanged),
            12504 => Ok(Action::ColorUpdatePush),
            12505 => Ok(Action::ChemistryChanged),
            12522 => Ok(Action::AddClient),
            12523 => Ok(Action::AddClientResponse),
            12524 => Ok(Action::RemoveClient),
            12525 => Ok(Action::RemoveClientResponse),
            12526 => Ok(Action::GetStatus),
            12527 => Ok(Action::StatusResponse),
            12528 => Ok(Action::SetHeatSetPoint),
            12529 => Ok(Action::SetHeatSetPointResponse),
            12530 => Ok(Action::ButtonPress),
            12531 => Ok(Action::ButtonPressResponse),
            12532 => Ok(Action::GetControllerConfig),
            12533 => Ok(Action::ControllerConfigResponse),
            12534 => Ok(Action::GetHistory),
            12535 => Ok(Action::HistoryResponse),
            12538 => Ok(Action::SetHeatMode),
            12539 => Ok(Action::SetHeatModeResponse),
            12542 => Ok(Action::GetScheduleData),
            12543 => Ok(Action::ScheduleDataResponse),
            12544 => Ok(Action::AddScheduleEvent),
            12545 => Ok(Action::AddScheduleEventResponse),
            12546 => Ok(Action::DeleteScheduleEvent),
            12547 => Ok(Action::DeleteScheduleEventResponse),
            12548 => Ok(Action::SetScheduleEvent),
            12549 => Ok(Action::SetScheduleEventResponse),
            12550 => Ok(Action::SetCircuitRuntime),
            12551 => Ok(Action::SetCircuitRuntimeResponse),
            12556 => Ok(Action::ColorLightsCommand),
            12557 => Ok(Action::ColorLightsCommandResponse),
            12560 => Ok(Action::GetCircuitNames),
            12561 => Ok(Action::CircuitNamesResponse),
            12562 => Ok(Action::GetCustomNames),
            12563 => Ok(Action::CustomNamesResponse),
            12564 => Ok(Action::SetCustomName),
            12565 => Ok(Action::SetCustomNameResponse),
            12566 => Ok(Action::GetEquipmentConfig),
            12567 => Ok(Action::EquipmentConfigResponse),
            12568 => Ok(Action::SetEquipmentConfig),
            12569 => Ok(Action::SetEquipmentConfigResponse),
            12572 => Ok(Action::GetScgConfig),
            12573 => Ok(Action::ScgConfigResponse),
            12574 => Ok(Action::SetScgEnabled),
            12575 => Ok(Action::SetScgEnabledResponse),
            12576 => Ok(Action::SetScgConfig),
            12577 => Ok(Action::SetScgConfigResponse),
            12580 => Ok(Action::CancelDelay),
            12581 => Ok(Action::CancelDelayResponse),
            12582 => Ok(Action::GetAllErrors),
            12583 => Ok(Action::AllErrorsResponse),
            12584 => Ok(Action::GetPumpStatus),
            12585 => Ok(Action::PumpStatusResponse),
            12586 => Ok(Action::SetPumpSpeed),
            12587 => Ok(Action::SetPumpSpeedResponse),
            12590 => Ok(Action::SetCoolSetPoint),
            12591 => Ok(Action::SetCoolSetPointResponse),
            12592 => Ok(Action::GetChemData),
            12593 => Ok(Action::ChemDataResponse),
            12596 => Ok(Action::GetChemHistory),
            12597 => Ok(Action::ChemHistoryResponse),
            18003 => Ok(Action::GatewayRequest),
            18004 => Ok(Action::GatewayResponse),
            _ => Err(ProtocolError::InvalidAction(value)),
        }
    }
}

impl From<Action> for u16 {
    fn from(action: Action) -> u16 {
        action as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every known code round-trips through u16 -> Action -> u16.
    #[test]
    fn roundtrip_all_known_codes() {
        let codes: &[(u16, Action)] = &[
            (13, Action::LoginFailure),
            (14, Action::ChallengeRequest),
            (15, Action::ChallengeResponse),
            (16, Action::PingRequest),
            (17, Action::PingResponse),
            (27, Action::LoginRequest),
            (28, Action::LoginResponse),
            (30, Action::UnknownCommand),
            (31, Action::BadParameter),
            (8110, Action::GetSystemTime),
            (8111, Action::SystemTimeResponse),
            (8112, Action::SetSystemTime),
            (8113, Action::SetSystemTimeResponse),
            (8120, Action::GetVersion),
            (8121, Action::VersionResponse),
            (9806, Action::WeatherForecastChanged),
            (9807, Action::GetWeatherForecast),
            (9808, Action::WeatherForecastResponse),
            (12500, Action::StatusChanged),
            (12501, Action::ScheduleChanged),
            (12502, Action::HistoryDataPush),
            (12503, Action::RuntimeChanged),
            (12504, Action::ColorUpdatePush),
            (12505, Action::ChemistryChanged),
            (12522, Action::AddClient),
            (12523, Action::AddClientResponse),
            (12524, Action::RemoveClient),
            (12525, Action::RemoveClientResponse),
            (12526, Action::GetStatus),
            (12527, Action::StatusResponse),
            (12528, Action::SetHeatSetPoint),
            (12529, Action::SetHeatSetPointResponse),
            (12530, Action::ButtonPress),
            (12531, Action::ButtonPressResponse),
            (12532, Action::GetControllerConfig),
            (12533, Action::ControllerConfigResponse),
            (12534, Action::GetHistory),
            (12535, Action::HistoryResponse),
            (12538, Action::SetHeatMode),
            (12539, Action::SetHeatModeResponse),
            (12542, Action::GetScheduleData),
            (12543, Action::ScheduleDataResponse),
            (12544, Action::AddScheduleEvent),
            (12545, Action::AddScheduleEventResponse),
            (12546, Action::DeleteScheduleEvent),
            (12547, Action::DeleteScheduleEventResponse),
            (12548, Action::SetScheduleEvent),
            (12549, Action::SetScheduleEventResponse),
            (12550, Action::SetCircuitRuntime),
            (12551, Action::SetCircuitRuntimeResponse),
            (12556, Action::ColorLightsCommand),
            (12557, Action::ColorLightsCommandResponse),
            (12560, Action::GetCircuitNames),
            (12561, Action::CircuitNamesResponse),
            (12562, Action::GetCustomNames),
            (12563, Action::CustomNamesResponse),
            (12564, Action::SetCustomName),
            (12565, Action::SetCustomNameResponse),
            (12566, Action::GetEquipmentConfig),
            (12567, Action::EquipmentConfigResponse),
            (12568, Action::SetEquipmentConfig),
            (12569, Action::SetEquipmentConfigResponse),
            (12572, Action::GetScgConfig),
            (12573, Action::ScgConfigResponse),
            (12574, Action::SetScgEnabled),
            (12575, Action::SetScgEnabledResponse),
            (12576, Action::SetScgConfig),
            (12577, Action::SetScgConfigResponse),
            (12580, Action::CancelDelay),
            (12581, Action::CancelDelayResponse),
            (12582, Action::GetAllErrors),
            (12583, Action::AllErrorsResponse),
            (12584, Action::GetPumpStatus),
            (12585, Action::PumpStatusResponse),
            (12586, Action::SetPumpSpeed),
            (12587, Action::SetPumpSpeedResponse),
            (12590, Action::SetCoolSetPoint),
            (12591, Action::SetCoolSetPointResponse),
            (12592, Action::GetChemData),
            (12593, Action::ChemDataResponse),
            (12596, Action::GetChemHistory),
            (12597, Action::ChemHistoryResponse),
            (18003, Action::GatewayRequest),
            (18004, Action::GatewayResponse),
        ];

        for &(code, expected) in codes {
            let parsed = Action::try_from(code).expect(&format!("code {} should parse", code));
            assert_eq!(parsed, expected, "code {} parsed to wrong variant", code);
            assert_eq!(u16::from(parsed), code, "variant {:?} encoded wrong", expected);
        }
    }

    #[test]
    fn unknown_code_returns_error() {
        let result = Action::try_from(9999u16);
        assert!(result.is_err());
        match result.unwrap_err() {
            ProtocolError::InvalidAction(code) => assert_eq!(code, 9999),
            other => panic!("expected InvalidAction, got {:?}", other),
        }
    }
}
