//! Thermostat cluster handler for Pentair Spa.
//!
//! Implements the Matter Thermostat cluster (0x0201) using the auto-generated
//! ClusterHandler trait from `rs_matter::import!(Thermostat)`.

use core::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;

use rs_matter::dm::{Cluster, Dataver, InvokeContext, ReadContext, WriteContext};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::tlv::Nullable;
use rs_matter::with;

use crate::clusters::thermostat::thermostat::{
    self, GetWeeklyScheduleRequest, SetWeeklyScheduleRequest, SetpointRaiseLowerRequest,
};
use crate::convert;
use crate::matter_bridge::{Command, SharedState};

pub struct SpaThermostatHandler {
    dataver: Dataver,
    shared: Arc<SharedState>,
    cmd_tx: mpsc::Sender<Command>,
    local_temp: Cell<i16>,
    heating_setpoint: Cell<i16>,
    system_mode: Cell<u8>,
    last_gen: Cell<u64>,
    /// Suppress sync_from_shared for setpoint after a local write,
    /// to avoid stale daemon state overwriting our optimistic update.
    setpoint_written_locally: Cell<bool>,
}

impl SpaThermostatHandler {
    pub fn new(
        dataver: Dataver,
        shared: Arc<SharedState>,
        cmd_tx: mpsc::Sender<Command>,
    ) -> Self {
        let (temp, setpoint, mode, gen) = {
            let s = shared.state.lock().unwrap();
            (s.spa_temp_matter, s.spa_setpoint_matter, s.spa_system_mode,
             shared.generation.load(Ordering::Acquire))
        };
        Self {
            dataver,
            shared,
            cmd_tx,
            local_temp: Cell::new(temp),
            heating_setpoint: Cell::new(setpoint),
            system_mode: Cell::new(mode),
            last_gen: Cell::new(gen),
            setpoint_written_locally: Cell::new(false),
        }
    }

    pub const fn adapt(self) -> thermostat::HandlerAdaptor<Self> {
        thermostat::HandlerAdaptor(self)
    }

    fn sync_from_shared(&self) {
        let current_gen = self.shared.generation.load(Ordering::Acquire);
        if current_gen != self.last_gen.get() {
            self.last_gen.set(current_gen);
            let s = self.shared.state.lock().unwrap();
            self.local_temp.set(s.spa_temp_matter);
            // Only sync setpoint from daemon if we haven't written locally,
            // OR if the daemon now agrees with our written value (confirming the write propagated).
            if self.setpoint_written_locally.get() {
                if s.spa_setpoint_matter == self.heating_setpoint.get() {
                    // Daemon caught up — resume normal sync
                    self.setpoint_written_locally.set(false);
                }
                // Otherwise keep our locally-written value
            } else {
                self.heating_setpoint.set(s.spa_setpoint_matter);
            }
            self.system_mode.set(s.spa_system_mode);
        }
    }
}

impl thermostat::ClusterHandler for SpaThermostatHandler {
    const CLUSTER: Cluster<'static> = thermostat::FULL_CLUSTER
        .with_revision(7)
        .with_features(0x01) // Heating only
        .with_attrs(with!(
            required;
            thermostat::AttributeId::LocalTemperature
                | thermostat::AttributeId::ControlSequenceOfOperation
                | thermostat::AttributeId::SystemMode
                | thermostat::AttributeId::OccupiedHeatingSetpoint
                | thermostat::AttributeId::AbsMinHeatSetpointLimit
                | thermostat::AttributeId::AbsMaxHeatSetpointLimit
                | thermostat::AttributeId::MinHeatSetpointLimit
                | thermostat::AttributeId::MaxHeatSetpointLimit
        ))
        .with_cmds(with!(
            thermostat::CommandId::SetpointRaiseLower
        ));

    fn dataver(&self) -> u32 {
        self.dataver.get()
    }

    fn dataver_changed(&self) {
        self.dataver.changed();
    }

    fn local_temperature(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<Nullable<i16>, Error> {
        self.sync_from_shared();
        Ok(Nullable::some(self.local_temp.get()))
    }

    fn control_sequence_of_operation(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<thermostat::ControlSequenceOfOperationEnum, Error> {
        Ok(thermostat::ControlSequenceOfOperationEnum::HeatingOnly)
    }

    fn system_mode(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<thermostat::SystemModeEnum, Error> {
        self.sync_from_shared();
        match self.system_mode.get() {
            0 => Ok(thermostat::SystemModeEnum::Off),
            4 => Ok(thermostat::SystemModeEnum::Heat),
            _ => Ok(thermostat::SystemModeEnum::Off),
        }
    }

    fn set_control_sequence_of_operation(
        &self,
        _ctx: impl WriteContext,
        _value: thermostat::ControlSequenceOfOperationEnum,
    ) -> Result<(), Error> {
        Err(Error::new(ErrorCode::InvalidAction))
    }

    fn set_system_mode(
        &self,
        _ctx: impl WriteContext,
        value: thermostat::SystemModeEnum,
    ) -> Result<(), Error> {
        match value {
            thermostat::SystemModeEnum::Off => {
                self.system_mode.set(0);
                let _ = self.cmd_tx.send(Command::SpaOff);
                Ok(())
            }
            thermostat::SystemModeEnum::Heat => {
                self.system_mode.set(4);
                let _ = self.cmd_tx.send(Command::SpaOn);
                Ok(())
            }
            _ => Err(Error::new(ErrorCode::InvalidAction)),
        }
    }

    // --- Optional attributes (overriding defaults for proper behavior) ---

    fn occupied_heating_setpoint(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<i16, Error> {
        self.sync_from_shared();
        let val = self.heating_setpoint.get();
        tracing::debug!(value = val, "thermostat: read OccupiedHeatingSetpoint");
        Ok(val)
    }

    fn set_occupied_heating_setpoint(
        &self,
        _ctx: impl WriteContext,
        value: i16,
    ) -> Result<(), Error> {
        tracing::info!(value = value, fahrenheit = convert::matter_to_fahrenheit(value), "thermostat: write OccupiedHeatingSetpoint");
        self.heating_setpoint.set(value);
        self.setpoint_written_locally.set(true);
        self.dataver.changed();
        let fahrenheit = convert::matter_to_fahrenheit(value);
        let _ = self.cmd_tx.send(Command::SetSpaSetpoint(fahrenheit));
        Ok(())
    }

    // Pentair spa: 60°F (1556) to 104°F (4000) in 0.01°C
    fn abs_min_heat_setpoint_limit(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<i16, Error> {
        Ok(convert::fahrenheit_to_matter(60)) // 60°F = 15.56°C
    }

    fn abs_max_heat_setpoint_limit(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<i16, Error> {
        Ok(convert::fahrenheit_to_matter(104)) // 104°F = 40.00°C
    }

    fn min_heat_setpoint_limit(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<i16, Error> {
        Ok(convert::fahrenheit_to_matter(60))
    }

    fn max_heat_setpoint_limit(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<i16, Error> {
        Ok(convert::fahrenheit_to_matter(104))
    }

    // --- Commands ---

    fn handle_setpoint_raise_lower(
        &self,
        _ctx: impl InvokeContext,
        request: SetpointRaiseLowerRequest<'_>,
    ) -> Result<(), Error> {
        let mode = request.mode()?;
        let amount = request.amount()?;

        match mode {
            thermostat::SetpointRaiseLowerModeEnum::Heat
            | thermostat::SetpointRaiseLowerModeEnum::Both => {
                let delta_matter = amount as i16 * 10; // 0.1°C steps → 0.01°C units
                let min = convert::fahrenheit_to_matter(60);
                let max = convert::fahrenheit_to_matter(104);
                let new_setpoint = self.heating_setpoint.get().saturating_add(delta_matter).clamp(min, max);
                self.heating_setpoint.set(new_setpoint);
                let fahrenheit = convert::matter_to_fahrenheit(new_setpoint);
                let _ = self.cmd_tx.send(Command::SetSpaSetpoint(fahrenheit));
                Ok(())
            }
            _ => Err(Error::new(ErrorCode::InvalidAction)),
        }
    }

    fn handle_set_weekly_schedule(
        &self,
        _ctx: impl InvokeContext,
        _request: SetWeeklyScheduleRequest<'_>,
    ) -> Result<(), Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }

    fn handle_get_weekly_schedule<P: rs_matter::tlv::TLVBuilderParent>(
        &self,
        _ctx: impl InvokeContext,
        _request: GetWeeklyScheduleRequest<'_>,
        _response: thermostat::GetWeeklyScheduleResponseBuilder<P>,
    ) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }

    fn handle_clear_weekly_schedule(
        &self,
        _ctx: impl InvokeContext,
    ) -> Result<(), Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }

    fn handle_set_active_schedule_request(
        &self,
        _ctx: impl InvokeContext,
        _request: thermostat::SetActiveScheduleRequestRequest<'_>,
    ) -> Result<(), Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }

    fn handle_set_active_preset_request(
        &self,
        _ctx: impl InvokeContext,
        _request: thermostat::SetActivePresetRequestRequest<'_>,
    ) -> Result<(), Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }

    fn handle_atomic_request<P: rs_matter::tlv::TLVBuilderParent>(
        &self,
        _ctx: impl InvokeContext,
        _request: thermostat::AtomicRequestRequest<'_>,
        _response: thermostat::AtomicResponseBuilder<P>,
    ) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }
}
