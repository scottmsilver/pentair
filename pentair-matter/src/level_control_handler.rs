//! Fixed-brightness LevelControl handler for pool lights.
//!
//! Pool lights don't dim — they only change color modes. This handler
//! reports full brightness (254) and accepts but ignores level commands.
//! Required by Google Home for Extended Color Light device type.

use rs_matter::dm::{Cluster, Dataver, InvokeContext, ReadContext, WriteContext};
use rs_matter::error::Error;
use rs_matter::tlv::Nullable;
use rs_matter::with;

use std::sync::mpsc;

use crate::clusters::level_control::level_control::{
    self, MoveRequest, MoveToLevelRequest, StepRequest,
    MoveToLevelWithOnOffRequest, MoveWithOnOffRequest, StepWithOnOffRequest,
};
use crate::matter_bridge::Command;

pub struct FixedLevelHandler {
    dataver: Dataver,
    cmd_tx: mpsc::Sender<Command>,
}

impl FixedLevelHandler {
    pub fn new(dataver: Dataver, cmd_tx: mpsc::Sender<Command>) -> Self {
        Self { dataver, cmd_tx }
    }

    pub const fn adapt(self) -> level_control::HandlerAdaptor<Self> {
        level_control::HandlerAdaptor(self)
    }
}

impl level_control::ClusterHandler for FixedLevelHandler {
    const CLUSTER: Cluster<'static> = level_control::FULL_CLUSTER
        .with_revision(5)
        .with_features(0x03) // OnOff + Lighting
        .with_attrs(with!(required; level_control::AttributeId::StartUpCurrentLevel))
        .with_cmds(with!(
            level_control::CommandId::MoveToLevel
                | level_control::CommandId::Move
                | level_control::CommandId::Step
                | level_control::CommandId::Stop
                | level_control::CommandId::MoveToLevelWithOnOff
                | level_control::CommandId::MoveWithOnOff
                | level_control::CommandId::StepWithOnOff
                | level_control::CommandId::StopWithOnOff
        ));

    fn dataver(&self) -> u32 { self.dataver.get() }
    fn dataver_changed(&self) { self.dataver.changed(); }

    fn current_level(&self, _ctx: impl ReadContext) -> Result<Nullable<u8>, Error> {
        Ok(Nullable::some(254)) // Always full brightness
    }

    fn options(&self, _ctx: impl ReadContext) -> Result<level_control::OptionsBitmap, Error> {
        Ok(level_control::OptionsBitmap::empty())
    }

    fn set_options(&self, _ctx: impl WriteContext, _value: level_control::OptionsBitmap) -> Result<(), Error> {
        Ok(())
    }

    fn on_level(&self, _ctx: impl ReadContext) -> Result<Nullable<u8>, Error> {
        Ok(Nullable::some(254))
    }

    fn set_on_level(&self, _ctx: impl WriteContext, _value: Nullable<u8>) -> Result<(), Error> {
        Ok(())
    }

    fn start_up_current_level(&self, _ctx: impl ReadContext) -> Result<Nullable<u8>, Error> {
        Ok(Nullable::some(254))
    }

    fn set_start_up_current_level(&self, _ctx: impl WriteContext, _value: Nullable<u8>) -> Result<(), Error> {
        Ok(())
    }

    // All level commands are accepted but ignored — pool lights don't dim
    fn handle_move_to_level(&self, _ctx: impl InvokeContext, _req: MoveToLevelRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move(&self, _ctx: impl InvokeContext, _req: MoveRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_step(&self, _ctx: impl InvokeContext, _req: StepRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_stop(&self, _ctx: impl InvokeContext, _req: level_control::StopRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move_to_level_with_on_off(&self, _ctx: impl InvokeContext, req: MoveToLevelWithOnOffRequest<'_>) -> Result<(), Error> {
        // Level 0 = off, any other level = on (pool lights don't dim)
        let level = req.level()?;
        let cmd = if level == 0 { Command::LightsOff } else { Command::LightsOn };
        if let Err(e) = self.cmd_tx.send(cmd) {
            tracing::error!("Failed to send lights command: {e}");
        }
        Ok(())
    }
    fn handle_move_with_on_off(&self, _ctx: impl InvokeContext, _req: MoveWithOnOffRequest<'_>) -> Result<(), Error> {
        let _ = self.cmd_tx.send(Command::LightsOn);
        Ok(())
    }
    fn handle_step_with_on_off(&self, _ctx: impl InvokeContext, _req: StepWithOnOffRequest<'_>) -> Result<(), Error> {
        let _ = self.cmd_tx.send(Command::LightsOn);
        Ok(())
    }
    fn handle_stop_with_on_off(&self, _ctx: impl InvokeContext, _req: level_control::StopWithOnOffRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move_to_closest_frequency(&self, _ctx: impl InvokeContext, _req: level_control::MoveToClosestFrequencyRequest<'_>) -> Result<(), Error> { Ok(()) }
}
