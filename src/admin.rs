use crate::table::{Column, FieldType};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct VerifyOptions {
    pub tolerate_torn_tail: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            tolerate_torn_tail: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TableReport {
    pub name: String,
    pub rows: u64,
}

#[derive(Clone, Debug)]
pub struct VerifyReport {
    pub ok: bool,
    pub lsn: u64,
    pub schema_fingerprint: u64,
    pub tables: Vec<TableReport>,
    pub rows: u64,
    pub snapshot_bytes: u64,
    pub wal_bytes: u64,
    pub wal_records: u64,
    pub torn_tail: bool,
    pub duration: Duration,
}

#[derive(Clone, Debug)]
pub struct FieldInspect {
    pub name: String,
    pub ty: FieldType,
    pub primary_key: bool,
    pub has_default: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug)]
pub struct TableInspect {
    pub name: String,
    pub rows: u64,
    pub fields: Vec<FieldInspect>,
}

#[derive(Clone, Debug)]
pub struct InspectReport {
    pub path: PathBuf,
    pub format_version: u32,
    pub wal_version: u32,
    pub schema_fingerprint: u64,
    pub workload_fingerprint: u64,
    pub lsn: u64,
    pub rows: u64,
    pub snapshot_bytes: u64,
    pub wal_bytes: u64,
    pub tables: Vec<TableInspect>,
}

#[derive(Clone, Debug, Default)]
pub struct BackupOptions {
    pub overwrite: bool,
}

#[derive(Clone, Debug)]
pub struct BackupReport {
    pub lsn: u64,
    pub rows: u64,
    pub bytes: u64,
    pub destination: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CompactReport {
    pub lsn: u64,
    pub rows: u64,
    pub snapshot_before: u64,
    pub snapshot_after: u64,
    pub wal_before: u64,
    pub wal_after: u64,
}

#[derive(Clone, Debug)]
pub struct RecoverReport {
    pub lsn: u64,
    pub wal_records: u64,
    pub discarded_wal: bool,
    pub rows: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct CheckpointReport {
    pub lsn: u64,
    pub snapshot_bytes: u64,
}

pub fn column(name: &'static str, ty: FieldType, primary_key: bool, nullable: bool) -> Column {
    Column {
        name,
        ty,
        primary_key,
        nullable,
    }
}
