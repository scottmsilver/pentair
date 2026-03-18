use crate::error::ProtocolError;

/// Water body selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BodyType {
    Pool = 0,
    Spa = 1,
}

impl TryFrom<i32> for BodyType {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(BodyType::Pool),
            1 => Ok(BodyType::Spa),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "BodyType",
                value,
            }),
        }
    }
}

/// Heating mode for a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HeatMode {
    Off = 0,
    Solar = 1,
    SolarPreferred = 2,
    HeatPump = 3,
}

impl TryFrom<i32> for HeatMode {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(HeatMode::Off),
            1 => Ok(HeatMode::Solar),
            2 => Ok(HeatMode::SolarPreferred),
            3 => Ok(HeatMode::HeatPump),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "HeatMode",
                value,
            }),
        }
    }
}

/// Current heating status reported by the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HeatStatus {
    Off = 0,
    Solar = 1,
    Heater = 2,
    Both = 3,
}

impl TryFrom<i32> for HeatStatus {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(HeatStatus::Off),
            1 => Ok(HeatStatus::Solar),
            2 => Ok(HeatStatus::Heater),
            3 => Ok(HeatStatus::Both),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "HeatStatus",
                value,
            }),
        }
    }
}

/// Color-light preset commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LightCommand {
    Off = 0,
    On = 1,
    Set = 2,
    Sync = 3,
    Swim = 4,
    Party = 5,
    Romantic = 6,
    Caribbean = 7,
    American = 8,
    Sunset = 9,
    Royal = 10,
    Save = 11,
    Recall = 12,
    Blue = 13,
    Green = 14,
    Red = 15,
    White = 16,
    Purple = 17,
}

impl TryFrom<i32> for LightCommand {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(LightCommand::Off),
            1 => Ok(LightCommand::On),
            2 => Ok(LightCommand::Set),
            3 => Ok(LightCommand::Sync),
            4 => Ok(LightCommand::Swim),
            5 => Ok(LightCommand::Party),
            6 => Ok(LightCommand::Romantic),
            7 => Ok(LightCommand::Caribbean),
            8 => Ok(LightCommand::American),
            9 => Ok(LightCommand::Sunset),
            10 => Ok(LightCommand::Royal),
            11 => Ok(LightCommand::Save),
            12 => Ok(LightCommand::Recall),
            13 => Ok(LightCommand::Blue),
            14 => Ok(LightCommand::Green),
            15 => Ok(LightCommand::Red),
            16 => Ok(LightCommand::White),
            17 => Ok(LightCommand::Purple),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "LightCommand",
                value,
            }),
        }
    }
}

/// What a circuit controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CircuitFunction {
    Generic = 0,
    Spa = 1,
    Pool = 2,
    MasterCleaner = 5,
    Light = 7,
    SAMLight = 9,
    SALLight = 10,
    PhotonGen = 11,
    ColorWheel = 12,
    Valve = 13,
    Spillway = 14,
    IntelliBrite = 16,
    Floor = 17,
    ColorLogic = 19,
}

impl TryFrom<i32> for CircuitFunction {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(CircuitFunction::Generic),
            1 => Ok(CircuitFunction::Spa),
            2 => Ok(CircuitFunction::Pool),
            5 => Ok(CircuitFunction::MasterCleaner),
            7 => Ok(CircuitFunction::Light),
            9 => Ok(CircuitFunction::SAMLight),
            10 => Ok(CircuitFunction::SALLight),
            11 => Ok(CircuitFunction::PhotonGen),
            12 => Ok(CircuitFunction::ColorWheel),
            13 => Ok(CircuitFunction::Valve),
            14 => Ok(CircuitFunction::Spillway),
            16 => Ok(CircuitFunction::IntelliBrite),
            17 => Ok(CircuitFunction::Floor),
            19 => Ok(CircuitFunction::ColorLogic),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "CircuitFunction",
                value,
            }),
        }
    }
}

/// Pump hardware type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum PumpType {
    None = 0,
    /// Variable Flow
    VF = 1,
    /// Variable Speed
    VS = 2,
    /// Variable Speed / Flow
    VSF = 3,
}

impl TryFrom<i32> for PumpType {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(PumpType::None),
            1 => Ok(PumpType::VF),
            2 => Ok(PumpType::VS),
            3 => Ok(PumpType::VSF),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "PumpType",
                value,
            }),
        }
    }
}

/// Schedule recurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ScheduleType {
    Recurring = 0,
    RunOnce = 1,
}

impl TryFrom<i32> for ScheduleType {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(ScheduleType::Recurring),
            1 => Ok(ScheduleType::RunOnce),
            _ => Err(ProtocolError::UnknownVariant {
                kind: "ScheduleType",
                value,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_type_valid() {
        assert_eq!(BodyType::try_from(0).unwrap(), BodyType::Pool);
        assert_eq!(BodyType::try_from(1).unwrap(), BodyType::Spa);
    }

    #[test]
    fn body_type_invalid() {
        let err = BodyType::try_from(99).unwrap_err();
        match err {
            ProtocolError::UnknownVariant { kind, value } => {
                assert_eq!(kind, "BodyType");
                assert_eq!(value, 99);
            }
            other => panic!("expected UnknownVariant, got {:?}", other),
        }
    }

    #[test]
    fn heat_mode_valid() {
        assert_eq!(HeatMode::try_from(0).unwrap(), HeatMode::Off);
        assert_eq!(HeatMode::try_from(1).unwrap(), HeatMode::Solar);
        assert_eq!(HeatMode::try_from(2).unwrap(), HeatMode::SolarPreferred);
        assert_eq!(HeatMode::try_from(3).unwrap(), HeatMode::HeatPump);
    }

    #[test]
    fn heat_mode_invalid() {
        assert!(HeatMode::try_from(4).is_err());
    }

    #[test]
    fn heat_status_valid() {
        assert_eq!(HeatStatus::try_from(0).unwrap(), HeatStatus::Off);
        assert_eq!(HeatStatus::try_from(1).unwrap(), HeatStatus::Solar);
        assert_eq!(HeatStatus::try_from(2).unwrap(), HeatStatus::Heater);
        assert_eq!(HeatStatus::try_from(3).unwrap(), HeatStatus::Both);
    }

    #[test]
    fn heat_status_invalid() {
        assert!(HeatStatus::try_from(-1).is_err());
    }

    #[test]
    fn light_command_all_valid() {
        let expected = [
            (0, LightCommand::Off),
            (1, LightCommand::On),
            (2, LightCommand::Set),
            (3, LightCommand::Sync),
            (4, LightCommand::Swim),
            (5, LightCommand::Party),
            (6, LightCommand::Romantic),
            (7, LightCommand::Caribbean),
            (8, LightCommand::American),
            (9, LightCommand::Sunset),
            (10, LightCommand::Royal),
            (11, LightCommand::Save),
            (12, LightCommand::Recall),
            (13, LightCommand::Blue),
            (14, LightCommand::Green),
            (15, LightCommand::Red),
            (16, LightCommand::White),
            (17, LightCommand::Purple),
        ];
        for (val, variant) in expected {
            assert_eq!(LightCommand::try_from(val).unwrap(), variant);
        }
    }

    #[test]
    fn light_command_invalid() {
        assert!(LightCommand::try_from(18).is_err());
    }

    #[test]
    fn circuit_function_all_valid() {
        let expected = [
            (0, CircuitFunction::Generic),
            (1, CircuitFunction::Spa),
            (2, CircuitFunction::Pool),
            (5, CircuitFunction::MasterCleaner),
            (7, CircuitFunction::Light),
            (9, CircuitFunction::SAMLight),
            (10, CircuitFunction::SALLight),
            (11, CircuitFunction::PhotonGen),
            (12, CircuitFunction::ColorWheel),
            (13, CircuitFunction::Valve),
            (14, CircuitFunction::Spillway),
            (16, CircuitFunction::IntelliBrite),
            (17, CircuitFunction::Floor),
            (19, CircuitFunction::ColorLogic),
        ];
        for (val, variant) in expected {
            assert_eq!(
                CircuitFunction::try_from(val).unwrap(),
                variant,
                "CircuitFunction value {}",
                val,
            );
        }
    }

    #[test]
    fn circuit_function_invalid_gaps() {
        // Values that fall in gaps should fail
        for val in [3, 4, 6, 8, 15, 18] {
            assert!(
                CircuitFunction::try_from(val).is_err(),
                "value {} should be invalid",
                val,
            );
        }
    }

    #[test]
    fn pump_type_valid() {
        assert_eq!(PumpType::try_from(0).unwrap(), PumpType::None);
        assert_eq!(PumpType::try_from(1).unwrap(), PumpType::VF);
        assert_eq!(PumpType::try_from(2).unwrap(), PumpType::VS);
        assert_eq!(PumpType::try_from(3).unwrap(), PumpType::VSF);
    }

    #[test]
    fn pump_type_invalid() {
        assert!(PumpType::try_from(4).is_err());
    }

    #[test]
    fn schedule_type_valid() {
        assert_eq!(ScheduleType::try_from(0).unwrap(), ScheduleType::Recurring);
        assert_eq!(ScheduleType::try_from(1).unwrap(), ScheduleType::RunOnce);
    }

    #[test]
    fn schedule_type_invalid() {
        assert!(ScheduleType::try_from(2).is_err());
    }
}
