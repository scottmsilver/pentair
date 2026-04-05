//! ThermostatUserInterfaceConfiguration cluster handler.
//!
//! Tells Google Home to display temperatures in Fahrenheit.
//! Cluster 0x0204, using the auto-generated ClusterHandler trait.

use rs_matter::dm::{Cluster, Dataver, ReadContext, WriteContext};
use rs_matter::error::Error;
use rs_matter::with;

use crate::clusters::thermostat_ui::thermostat_user_interface_configuration::{
    self, KeypadLockoutEnum, TemperatureDisplayModeEnum,
};

pub struct ThermostatUiHandler {
    dataver: Dataver,
}

impl ThermostatUiHandler {
    pub fn new(dataver: Dataver) -> Self {
        Self { dataver }
    }

    pub const fn adapt(
        self,
    ) -> thermostat_user_interface_configuration::HandlerAdaptor<Self> {
        thermostat_user_interface_configuration::HandlerAdaptor(self)
    }
}

impl thermostat_user_interface_configuration::ClusterHandler for ThermostatUiHandler {
    const CLUSTER: Cluster<'static> = thermostat_user_interface_configuration::FULL_CLUSTER
        .with_revision(2)
        .with_features(0)
        .with_attrs(with!(required))
        .with_cmds(with!());

    fn dataver(&self) -> u32 {
        self.dataver.get()
    }

    fn dataver_changed(&self) {
        self.dataver.changed();
    }

    fn temperature_display_mode(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<TemperatureDisplayModeEnum, Error> {
        Ok(TemperatureDisplayModeEnum::Fahrenheit)
    }

    fn set_temperature_display_mode(
        &self,
        _ctx: impl WriteContext,
        _value: TemperatureDisplayModeEnum,
    ) -> Result<(), Error> {
        // Read-only — always Fahrenheit
        Ok(())
    }

    fn keypad_lockout(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<KeypadLockoutEnum, Error> {
        Ok(KeypadLockoutEnum::NoLockout)
    }

    fn set_keypad_lockout(
        &self,
        _ctx: impl WriteContext,
        _value: KeypadLockoutEnum,
    ) -> Result<(), Error> {
        Ok(())
    }
}
