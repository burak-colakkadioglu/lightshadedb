#![forbid(unsafe_code)]

pub mod admin;
pub mod backend;
pub mod dataplane;
pub mod error;
pub mod migration;
pub mod net;
pub mod options;
#[cfg(feature = "async")]
pub mod pool;
pub mod prelude;
pub mod query;
pub mod secret;
pub mod table;
pub mod wire;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "host")]
pub mod host;

pub use backend::Backend;
pub use error::{
    AuthError, Corruption, Error, ErrorCode, ErrorKind, MigrationError, QuotaKind, RemoteDetail,
    RemoteError, Result, SchemaError,
};
pub use lightshade_macros::{operations, registry, schema};
pub use migration::{
    FieldSummary, MigrationOptions, MigrationPlan, MigrationReport, MigrationStep, SchemaSummary,
    TableSummary,
};
pub use net::{ClientEndpoint, Listen, NetworkAccess, DEFAULT_DATABASE};
pub use options::{OpenOptions, RecoverOptions};
pub use query::{
    avg, count, max, min, sum, Aggregate, Assignment, Field, Group, Join, Order, Predicate,
    SelectValue,
};
pub use secret::{ClientCredentials, Credentials, Secret};
pub use table::{Column, FieldType, Projection, Table, TableHandle};

#[cfg(feature = "client")]
pub use client::ClientOptions;
#[cfg(feature = "host")]
pub use host::{CredentialsSource, ServeOptions};
