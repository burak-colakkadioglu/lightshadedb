mod admin;
mod maintenance;
mod replay;
mod snapshot;
mod storage;
mod table_storage;
mod ttl;

pub(crate) use admin::*;
pub(crate) use maintenance::*;
pub(crate) use replay::*;
pub(crate) use storage::*;
pub(crate) use table_storage::*;
pub(crate) use ttl::*;

use crate::ir::*;
use replay::{emit_replay, emit_replay_effects};
use snapshot::{emit_from_snapshot, emit_snapshot};

pub(crate) fn emit_persistence_methods(
    out: &mut String,
    workload: &Workload,
) -> Result<(), String> {
    emit_snapshot(out, workload)?;
    emit_from_snapshot(out, workload)?;
    emit_replay(out, workload)?;
    emit_replay_effects(out, workload)?;
    Ok(())
}
