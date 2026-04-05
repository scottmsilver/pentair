//! Stub Identify cluster handler. Required by Extended Color Light device type.

use rs_matter::dm::{Cluster, Dataver, InvokeContext, ReadContext, WriteContext};
use rs_matter::error::Error;
use rs_matter::with;

use crate::clusters::identify::identify::{self, IdentifyTypeEnum};

pub struct IdentifyHandler {
    dataver: Dataver,
}

impl IdentifyHandler {
    pub fn new(dataver: Dataver) -> Self {
        Self { dataver }
    }
    pub const fn adapt(self) -> identify::HandlerAdaptor<Self> {
        identify::HandlerAdaptor(self)
    }
}

impl identify::ClusterHandler for IdentifyHandler {
    const CLUSTER: Cluster<'static> = identify::FULL_CLUSTER
        .with_revision(4)
        .with_features(0)
        .with_attrs(with!(required))
        .with_cmds(with!(identify::CommandId::Identify));

    fn dataver(&self) -> u32 { self.dataver.get() }
    fn dataver_changed(&self) { self.dataver.changed(); }

    fn identify_time(&self, _ctx: impl ReadContext) -> Result<u16, Error> { Ok(0) }
    fn set_identify_time(&self, _ctx: impl WriteContext, _value: u16) -> Result<(), Error> { Ok(()) }
    fn identify_type(&self, _ctx: impl ReadContext) -> Result<IdentifyTypeEnum, Error> {
        Ok(IdentifyTypeEnum::None)
    }

    fn handle_identify(&self, _ctx: impl InvokeContext, _req: identify::IdentifyRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_trigger_effect(&self, _ctx: impl InvokeContext, _req: identify::TriggerEffectRequest<'_>) -> Result<(), Error> { Ok(()) }
}
