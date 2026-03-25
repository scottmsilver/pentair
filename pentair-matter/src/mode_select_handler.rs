//! ModeSelect cluster handler for Pentair IntelliBrite lights.

use core::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;

use rs_matter::dm::{ArrayAttributeRead, Cluster, Dataver, InvokeContext, ReadContext};
use rs_matter::error::{Error, ErrorCode};
use rs_matter::tlv::{Nullable, TLVBuilderParent, Utf8StrBuilder};
use rs_matter::with;

use crate::clusters::mode_select::mode_select::{
    self, ChangeToModeRequest, ModeOptionStructArrayBuilder, ModeOptionStructBuilder,
};
use crate::light_modes::LightModeMap;
use crate::matter_bridge::{Command, SharedState};

pub struct LightModeSelectHandler {
    dataver: Dataver,
    mode_map: LightModeMap,
    current_mode: Cell<u8>,
    last_gen: Cell<u64>,
    mode_written_locally: Cell<bool>,
    shared: Arc<SharedState>,
    cmd_tx: mpsc::Sender<Command>,
}

impl LightModeSelectHandler {
    pub fn new(
        dataver: Dataver,
        mode_map: LightModeMap,
        shared: Arc<SharedState>,
        cmd_tx: mpsc::Sender<Command>,
    ) -> Self {
        let (initial_mode, gen) = {
            let s = shared.state.lock().unwrap();
            (s.light_mode_index.unwrap_or(0), shared.generation.load(Ordering::Acquire))
        };
        Self {
            dataver,
            mode_map,
            current_mode: Cell::new(initial_mode),
            last_gen: Cell::new(gen),
            mode_written_locally: Cell::new(false),
            shared,
            cmd_tx,
        }
    }

    pub const fn adapt(self) -> mode_select::HandlerAdaptor<Self> {
        mode_select::HandlerAdaptor(self)
    }
}

impl mode_select::ClusterHandler for LightModeSelectHandler {
    const CLUSTER: Cluster<'static> = mode_select::FULL_CLUSTER
        .with_revision(2)
        .with_features(0)
        .with_attrs(with!(required))
        .with_cmds(with!(mode_select::CommandId::ChangeToMode));

    fn dataver(&self) -> u32 {
        self.dataver.get()
    }

    fn dataver_changed(&self) {
        self.dataver.changed();
    }

    fn description<P: TLVBuilderParent>(
        &self,
        _ctx: impl ReadContext,
        builder: Utf8StrBuilder<P>,
    ) -> Result<P, Error> {
        builder.set("Pool Lights")
    }

    fn standard_namespace(
        &self,
        _ctx: impl ReadContext,
    ) -> Result<Nullable<u16>, Error> {
        Ok(Nullable::none())
    }

    fn supported_modes<P: TLVBuilderParent>(
        &self,
        _ctx: impl ReadContext,
        builder: ArrayAttributeRead<
            ModeOptionStructArrayBuilder<P>,
            ModeOptionStructBuilder<P>,
        >,
    ) -> Result<P, Error> {
        match builder {
            ArrayAttributeRead::ReadAll(builder) => {
                let mut b = builder;
                for (idx, name) in self.mode_map.iter() {
                    b = b
                        .push()?
                        .label(name)?
                        .mode(idx)?
                        .semantic_tags()?.end()? // end array
                        .end()?; // end struct
                }
                b.end()
            }
            ArrayAttributeRead::ReadOne(index, builder) => {
                let idx = index as u8;
                if let Some(name) = self.mode_map.name_by_index(idx) {
                    builder
                        .label(name)?
                        .mode(idx)?
                        .semantic_tags()?.end()? // end array
                        .end() // end struct
                } else {
                    Err(ErrorCode::ConstraintError.into())
                }
            }
            ArrayAttributeRead::ReadNone(builder) => builder.end(),
        }
    }

    fn current_mode(&self, _ctx: impl ReadContext) -> Result<u8, Error> {
        let current_gen = self.shared.generation.load(Ordering::Acquire);
        if current_gen != self.last_gen.get() {
            self.last_gen.set(current_gen);
            if let Some(idx) = self.shared.state.lock().unwrap().light_mode_index {
                if self.mode_written_locally.get() {
                    if idx == self.current_mode.get() {
                        self.mode_written_locally.set(false);
                    }
                    // Keep locally-written value until daemon catches up
                } else {
                    self.current_mode.set(idx);
                }
            }
        }
        Ok(self.current_mode.get())
    }

    fn handle_change_to_mode(
        &self,
        _ctx: impl InvokeContext,
        request: ChangeToModeRequest<'_>,
    ) -> Result<(), Error> {
        let new_mode = request.new_mode()?;
        match self.mode_map.name_by_index(new_mode) {
            Some(name) => {
                self.current_mode.set(new_mode);
                self.mode_written_locally.set(true);
                self.dataver.changed();
                let _ = self.cmd_tx.send(Command::SetLightMode(name.to_string()));
                Ok(())
            }
            None => Err(Error::new(ErrorCode::InvalidAction)),
        }
    }
}
