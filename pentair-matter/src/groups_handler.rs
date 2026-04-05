//! Stub Groups cluster handler. Required by Extended Color Light device type.

use rs_matter::dm::{Cluster, Dataver, InvokeContext, ReadContext};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::with;

use crate::clusters::groups::groups::{self, NameSupportBitmap};

pub struct GroupsHandler {
    dataver: Dataver,
}

impl GroupsHandler {
    pub fn new(dataver: Dataver) -> Self {
        Self { dataver }
    }
    pub const fn adapt(self) -> groups::HandlerAdaptor<Self> {
        groups::HandlerAdaptor(self)
    }
}

impl groups::ClusterHandler for GroupsHandler {
    const CLUSTER: Cluster<'static> = groups::FULL_CLUSTER
        .with_revision(4)
        .with_features(0)
        .with_attrs(with!(required))
        .with_cmds(with!(
            groups::CommandId::AddGroup
                | groups::CommandId::ViewGroup
                | groups::CommandId::GetGroupMembership
                | groups::CommandId::RemoveGroup
                | groups::CommandId::RemoveAllGroups
                | groups::CommandId::AddGroupIfIdentifying
        ));

    fn dataver(&self) -> u32 { self.dataver.get() }
    fn dataver_changed(&self) { self.dataver.changed(); }

    fn name_support(&self, _ctx: impl ReadContext) -> Result<NameSupportBitmap, Error> {
        Ok(NameSupportBitmap::empty())
    }

    fn handle_add_group<P: rs_matter::tlv::TLVBuilderParent>(&self, _ctx: impl InvokeContext, _req: groups::AddGroupRequest<'_>, _resp: groups::AddGroupResponseBuilder<P>) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }
    fn handle_view_group<P: rs_matter::tlv::TLVBuilderParent>(&self, _ctx: impl InvokeContext, _req: groups::ViewGroupRequest<'_>, _resp: groups::ViewGroupResponseBuilder<P>) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }
    fn handle_get_group_membership<P: rs_matter::tlv::TLVBuilderParent>(&self, _ctx: impl InvokeContext, _req: groups::GetGroupMembershipRequest<'_>, _resp: groups::GetGroupMembershipResponseBuilder<P>) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }
    fn handle_remove_group<P: rs_matter::tlv::TLVBuilderParent>(&self, _ctx: impl InvokeContext, _req: groups::RemoveGroupRequest<'_>, _resp: groups::RemoveGroupResponseBuilder<P>) -> Result<P, Error> {
        Err(Error::new(ErrorCode::InvalidCommand))
    }
    fn handle_remove_all_groups(&self, _ctx: impl InvokeContext) -> Result<(), Error> { Ok(()) }
    fn handle_add_group_if_identifying(&self, _ctx: impl InvokeContext, _req: groups::AddGroupIfIdentifyingRequest<'_>) -> Result<(), Error> { Ok(()) }
}
