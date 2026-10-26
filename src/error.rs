use crate::table::FieldType;
use std::path::PathBuf;
use std::time::Duration;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("database is corrupt: {0}")]
    Corrupt(#[from] Corruption),
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),
    #[error("quota exceeded: {kind} limit {limit}")]
    Quota { kind: QuotaKind, limit: u64 },
    #[error("write conflict on table `{table}`; transaction rolled back")]
    Conflict { table: String },
    #[error("duplicate primary key in table `{table}`")]
    DuplicateKey { table: String },
    #[error("database is read-only")]
    ReadOnly,
    #[error("database is locked: {path}")]
    Locked { path: PathBuf },
    #[error("database is busy")]
    Busy,
    #[error("not found: {path}")]
    NotFound { path: PathBuf },
    #[error("authentication error: {0}")]
    Auth(#[from] AuthError),
    #[error("protocol error: {reason}")]
    Protocol { reason: String },
    #[error("feature `{feature}` is not enabled")]
    Unsupported { feature: &'static str },
    #[error("invalid argument `{name}`: {reason}")]
    InvalidArgument { name: String, reason: String },
    #[error("database handle failed: {reason}")]
    Failed { reason: String },
    #[error("migration error: {0}")]
    Migration(#[from] MigrationError),
    #[error("remote error: {0}")]
    Remote(Box<RemoteError>),
    #[error("internal error: {reason}")]
    Internal { reason: String },
}

impl Error {
    pub fn io(context: &'static str, source: std::io::Error) -> Self {
        Self::Io { context, source }
    }

    pub fn remote(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Remote(Box::new(RemoteError {
            code,
            message: message.into(),
            detail: None,
        }))
    }

    pub fn remote_with_detail(
        code: ErrorCode,
        message: impl Into<String>,
        detail: RemoteDetail,
    ) -> Self {
        reconstruct_remote(code, message.into(), Some(detail))
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Io { .. } => ErrorCode::Io,
            Self::Corrupt(_) => ErrorCode::Corrupt,
            Self::Schema(_) => ErrorCode::Schema,
            Self::Quota { .. } => ErrorCode::Quota,
            Self::Conflict { .. } => ErrorCode::Conflict,
            Self::DuplicateKey { .. } => ErrorCode::DuplicateKey,
            Self::ReadOnly => ErrorCode::ReadOnly,
            Self::Locked { .. } => ErrorCode::Locked,
            Self::Busy => ErrorCode::Busy,
            Self::NotFound { .. } => ErrorCode::NotFound,
            Self::Auth(_) => ErrorCode::Auth,
            Self::Protocol { .. } => ErrorCode::Protocol,
            Self::Unsupported { .. } => ErrorCode::Unsupported,
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::Failed { .. } => ErrorCode::Failed,
            Self::Migration(_) => ErrorCode::Migration,
            Self::Remote(_) => ErrorCode::Remote,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self.code() {
            ErrorCode::Io => ErrorKind::Io,
            ErrorCode::Corrupt => ErrorKind::Corruption,
            ErrorCode::Schema => ErrorKind::Schema,
            ErrorCode::Migration => ErrorKind::Migration,
            ErrorCode::Quota => ErrorKind::Quota,
            ErrorCode::Conflict => ErrorKind::Conflict,
            ErrorCode::DuplicateKey => ErrorKind::DuplicateKey,
            ErrorCode::ReadOnly => ErrorKind::ReadOnly,
            ErrorCode::Locked => ErrorKind::Locked,
            ErrorCode::Busy => ErrorKind::Busy,
            ErrorCode::NotFound => ErrorKind::NotFound,
            ErrorCode::Auth => ErrorKind::Auth,
            ErrorCode::Protocol => ErrorKind::Protocol,
            ErrorCode::Unsupported => ErrorKind::Unsupported,
            ErrorCode::InvalidArgument => ErrorKind::InvalidArgument,
            ErrorCode::Failed => ErrorKind::Failed,
            ErrorCode::Remote => ErrorKind::Remote,
            ErrorCode::Internal => ErrorKind::Internal,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Conflict { .. } | Self::Busy => true,
            Self::Remote(remote) => {
                matches!(remote.code, ErrorCode::Conflict | ErrorCode::Busy)
            }
            _ => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Busy => Some(Duration::from_millis(50)),
            Self::Quota { .. } => Some(Duration::from_secs(1)),
            Self::Remote(remote) if remote.code == ErrorCode::Busy => {
                Some(Duration::from_millis(50))
            }
            _ => None,
        }
    }

    pub fn remote_detail(&self) -> Option<RemoteDetail> {
        match self {
            Self::Conflict { table } => Some(RemoteDetail::Conflict {
                table: table.clone(),
            }),
            Self::DuplicateKey { table } => Some(RemoteDetail::DuplicateKey {
                table: table.clone(),
            }),
            Self::Quota { kind, limit } => Some(RemoteDetail::Quota {
                kind: *kind,
                limit: *limit,
            }),
            Self::Schema(SchemaError::UnknownField { table, field }) => {
                Some(RemoteDetail::Schema {
                    table: Some(table.clone()),
                    field: Some(field.clone()),
                })
            }
            Self::Schema(SchemaError::UnknownTable { name }) => Some(RemoteDetail::Schema {
                table: Some(name.clone()),
                field: None,
            }),
            Self::Auth(_) => Some(RemoteDetail::Auth),
            _ => None,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.kind() {
            ErrorKind::Corruption => 3,
            ErrorKind::Conflict => 4,
            ErrorKind::Auth => 5,
            ErrorKind::Quota => 6,
            ErrorKind::Unsupported => 7,
            ErrorKind::Migration => 8,
            _ => 1,
        }
    }
}

pub(crate) fn reconstruct_remote(
    code: ErrorCode,
    message: String,
    detail: Option<RemoteDetail>,
) -> Error {
    match detail {
        Some(RemoteDetail::Conflict { table }) => Error::Conflict { table },
        Some(RemoteDetail::DuplicateKey { table }) => Error::DuplicateKey { table },
        Some(RemoteDetail::Quota { kind, limit }) => Error::Quota { kind, limit },
        Some(RemoteDetail::Schema { table, field }) => {
            let error = match (table, field) {
                (table, Some(field)) => SchemaError::UnknownField {
                    table: table.unwrap_or_default(),
                    field,
                },
                (Some(table), None) => SchemaError::UnknownTable { name: table },
                (None, None) => SchemaError::UnknownTable {
                    name: "<remote>".to_string(),
                },
            };
            Error::Schema(error)
        }
        Some(RemoteDetail::Auth) => Error::Auth(AuthError::Failed),
        None => match code {
            ErrorCode::Auth => Error::Auth(AuthError::Failed),
            ErrorCode::ReadOnly => Error::ReadOnly,
            ErrorCode::Busy => Error::Busy,
            ErrorCode::Conflict => Error::Conflict {
                table: "<remote>".to_string(),
            },
            ErrorCode::DuplicateKey => Error::DuplicateKey {
                table: "<remote>".to_string(),
            },
            _ => Error::Remote(Box::new(RemoteError {
                code,
                message,
                detail: None,
            })),
        },
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Corruption {
    #[error("bad snapshot or WAL magic")]
    BadMagic,
    #[error("unsupported format version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("checksum mismatch at lsn {lsn}")]
    ChecksumMismatch { lsn: u64 },
    #[error("torn WAL tail")]
    TornWalTail,
    #[error("truncated snapshot or WAL")]
    Truncated,
    #[error("invalid encoding")]
    InvalidEncoding,
    #[error("snapshot moved during read")]
    SnapshotMoved,
    #[error("stored schema does not match this build")]
    SchemaMismatch,
    #[error("stored table `{table}` does not match this build")]
    TableMismatch { table: String },
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    #[error("duplicate table `{name}`")]
    DuplicateTable { name: String },
    #[error("duplicate field `{table}.{field}`")]
    DuplicateField { table: String, field: String },
    #[error("duplicate operation `{name}`")]
    DuplicateOperation { name: String },
    #[error("table `{table}` has no primary key")]
    MissingPrimaryKey { table: String },
    #[error("table `{table}` has more than one primary key")]
    MultiplePrimaryKeys { table: String },
    #[error("primary key `{table}.{field}` cannot have a default")]
    PrimaryKeyDefault { table: String, field: String },
    #[error("unknown table `{name}`")]
    UnknownTable { name: String },
    #[error("unknown field `{table}.{field}`")]
    UnknownField { table: String, field: String },
    #[error("field `{table}.{field}` is {expected} but the value is {found}")]
    TypeMismatch {
        table: String,
        field: String,
        expected: FieldType,
        found: FieldType,
    },
    #[error("new field `{table}.{field}` needs a default")]
    MissingDefault { table: String, field: String },
    #[error("field `{table}.{field}` of type {ty} cannot have a default")]
    UnsupportedDefault {
        table: String,
        field: String,
        ty: FieldType,
    },
    #[error("WAL uses a legacy operation encoding")]
    LegacyWal,
    #[error("invalid rename from `{from}` to `{to}`")]
    InvalidRename { from: String, to: String },
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    #[error("authentication failed")]
    Failed,
    #[error("authentication expired")]
    Expired,
    #[error("permission denied")]
    PermissionDenied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QuotaKind {
    #[error("rows")]
    Rows,
    #[error("WAL bytes")]
    WalBytes,
    #[error("connections")]
    Connections,
    #[error("transactions")]
    Transactions,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrationError {
    #[error("migration required: {additions} additions and {removals} removals")]
    Required {
        destructive: bool,
        additions: usize,
        removals: usize,
    },
    #[error("destructive migration requires allow_destructive")]
    DestructiveNotAllowed,
    #[error("field `{table}.{field}` needs a conversion from {from} to {to}")]
    ConversionRequired {
        table: String,
        field: String,
        from: FieldType,
        to: FieldType,
    },
    #[error("new field `{table}.{field}` needs a default")]
    MissingDefault { table: String, field: String },
    #[error("conversion for `{table}.{field}` failed: {reason}")]
    ConversionFailed {
        table: String,
        field: String,
        reason: String,
    },
    #[error("backup failed at `{path}`: {source}")]
    Backup {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("migration was interrupted; the original database is unchanged")]
    Interrupted,
    #[error("stored schema changed after the migration plan was produced")]
    StalePlan,
    #[error("migration cannot change the primary key of `{table}`")]
    PrimaryKeyChanged { table: String },
    #[error("rename hint `{from}` -> `{to}` collides with another declaration")]
    RenameConflict { from: String, to: String },
    #[error("field `{table}.{field}` cannot become non-nullable; backfill stored NULLs first")]
    NullabilityChanged { table: String, field: String },
}

#[derive(Debug, thiserror::Error)]
#[error("remote error {code:?}: {message}")]
pub struct RemoteError {
    pub code: ErrorCode,
    pub message: String,
    pub detail: Option<RemoteDetail>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RemoteDetail {
    #[error("conflict on `{table}`")]
    Conflict { table: String },
    #[error("duplicate key in `{table}`")]
    DuplicateKey { table: String },
    #[error("quota {kind} limit {limit}")]
    Quota { kind: QuotaKind, limit: u64 },
    #[error("schema mismatch")]
    Schema {
        table: Option<String>,
        field: Option<String>,
    },
    #[error("authentication failed")]
    Auth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    Io,
    Corruption,
    Schema,
    Migration,
    Quota,
    Conflict,
    DuplicateKey,
    ReadOnly,
    Locked,
    Busy,
    NotFound,
    Auth,
    Protocol,
    Unsupported,
    InvalidArgument,
    Failed,
    Remote,
    Internal,
}

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    Io = 1,
    Corrupt = 2,
    Schema = 3,
    Quota = 4,
    Conflict = 5,
    DuplicateKey = 6,
    ReadOnly = 7,
    Locked = 8,
    Busy = 9,
    NotFound = 10,
    Auth = 11,
    Protocol = 12,
    Unsupported = 13,
    InvalidArgument = 14,
    Failed = 15,
    Remote = 16,
    Internal = 17,
    Migration = 18,
}

impl ErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::Io,
            2 => Self::Corrupt,
            3 => Self::Schema,
            4 => Self::Quota,
            5 => Self::Conflict,
            6 => Self::DuplicateKey,
            7 => Self::ReadOnly,
            8 => Self::Locked,
            9 => Self::Busy,
            10 => Self::NotFound,
            11 => Self::Auth,
            12 => Self::Protocol,
            13 => Self::Unsupported,
            14 => Self::InvalidArgument,
            15 => Self::Failed,
            16 => Self::Remote,
            17 => Self::Internal,
            18 => Self::Migration,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_round_trip() {
        for value in 1..=18 {
            let code = ErrorCode::from_u16(value).unwrap();
            assert_eq!(code.as_u16(), value);
        }
        assert_eq!(ErrorCode::from_u16(0), None);
        assert_eq!(ErrorCode::from_u16(19), None);
    }

    #[test]
    fn known_remote_details_reconstruct_local_variants() {
        assert!(matches!(
            Error::remote_with_detail(
                ErrorCode::Conflict,
                "conflict",
                RemoteDetail::Conflict {
                    table: "messages".into()
                }
            ),
            Error::Conflict { table } if table == "messages"
        ));
        assert!(matches!(
            Error::remote_with_detail(
                ErrorCode::Quota,
                "quota",
                RemoteDetail::Quota {
                    kind: QuotaKind::Rows,
                    limit: 10
                }
            ),
            Error::Quota {
                kind: QuotaKind::Rows,
                limit: 10
            }
        ));
    }

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(Error::Corrupt(Corruption::BadMagic).exit_code(), 3);
        assert_eq!(Error::Conflict { table: "t".into() }.exit_code(), 4);
        assert_eq!(Error::Auth(AuthError::Failed).exit_code(), 5);
        assert_eq!(
            Error::Quota {
                kind: QuotaKind::Rows,
                limit: 1
            }
            .exit_code(),
            6
        );
        assert_eq!(Error::Unsupported { feature: "x" }.exit_code(), 7);
        assert_eq!(Error::Migration(MigrationError::Interrupted).exit_code(), 8);
        assert_eq!(Error::Busy.exit_code(), 1);
    }
}
