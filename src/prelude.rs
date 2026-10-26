pub use crate::admin::{
    BackupOptions, BackupReport, CompactReport, InspectReport, VerifyOptions, VerifyReport,
};
pub use crate::backend::Backend;
pub use crate::dataplane::{DumpReport, ExportReport, ImportReport, RestoreReport};
pub use crate::error::{Error, ErrorKind, MigrationError, Result};
pub use crate::migration::{
    FieldSummary, MigrationOptions, MigrationPlan, MigrationReport, MigrationStep, SchemaSummary,
    TableSummary,
};
pub use crate::net::{ClientEndpoint, Listen, NetworkAccess, DEFAULT_DATABASE};
pub use crate::options::{OpenOptions, RecoverOptions};
pub use crate::query::{
    avg, count, max, min, sum, Aggregate, Assignment, Field, Group, Join, Order, Predicate,
    SelectValue,
};
pub use crate::secret::{ClientCredentials, Credentials, Secret};
pub use crate::table::{Column, FieldType, Projection, Table, TableHandle};
pub use crate::{operations, schema};

#[cfg(feature = "client")]
pub use crate::client::ClientOptions;
#[cfg(feature = "host")]
pub use crate::host::{CredentialsSource, ServeOptions};
#[cfg(feature = "async")]
pub use crate::pool::{Pool, PoolOptions, Pooled};
