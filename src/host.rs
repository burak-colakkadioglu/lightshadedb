use crate::error::{Error, Result};
use crate::net::{Listen, NetworkAccess};
use crate::options::env_u64;
pub use crate::secret::CredentialsSource;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub listen: Listen,
    pub credentials: CredentialsSource,
    pub credentials_overrides: std::collections::HashMap<String, CredentialsSource>,
    pub tenant: Option<String>,
    pub audit_log: Option<PathBuf>,
    pub idle_timeout: Duration,
    pub handshake_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub lock_timeout: Duration,
    pub max_connections: usize,
    pub max_transactions: usize,
    pub max_rows: u64,
    pub max_wal_bytes: u64,
    pub group_commit: Duration,
    pub credentials_reload: Option<Duration>,
    pub handle_signals: bool,
}

impl ServeOptions {
    pub fn new(credentials: CredentialsSource) -> Self {
        Self {
            listen: Listen::Derived,
            credentials,
            credentials_overrides: std::collections::HashMap::new(),
            tenant: None,
            audit_log: None,
            idle_timeout: Duration::from_secs(30),
            handshake_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(10),
            lock_timeout: Duration::from_secs(10),
            max_connections: 128,
            max_transactions: 128,
            max_rows: 0,
            max_wal_bytes: 0,
            group_commit: Duration::from_millis(1),
            credentials_reload: Some(Duration::from_secs(1)),
            handle_signals: false,
        }
    }

    pub fn from_env() -> Result<Self> {
        let credentials = std::env::var("LIGHTSHADE_CREDENTIALS")
            .map(|path| CredentialsSource::File(PathBuf::from(path)))
            .map_err(|_| Error::InvalidArgument {
                name: "LIGHTSHADE_CREDENTIALS".to_string(),
                reason: "is not set".to_string(),
            })?;
        let mut options = Self::new(credentials);
        options.handle_signals = true;
        if let Ok(value) = std::env::var("LIGHTSHADE_LISTEN") {
            options.listen = parse_listen(&value)?;
        }
        if let Ok(value) = std::env::var("LIGHTSHADE_NETWORK_ACCESS") {
            let access = value.parse::<NetworkAccess>()?;
            options.listen = apply_access(options.listen, access)?;
        }
        if let Some(value) = env_u64("LIGHTSHADE_MAX_ROWS")? {
            options.max_rows = value;
        }
        if let Some(value) = env_u64("LIGHTSHADE_MAX_WAL_BYTES")? {
            options.max_wal_bytes = value;
        }
        Ok(options)
    }
}

fn parse_listen(value: &str) -> Result<Listen> {
    if let Some(rest) = value.strip_prefix("unix://") {
        #[cfg(unix)]
        {
            return Ok(Listen::Unix(PathBuf::from(rest)));
        }
        #[cfg(not(unix))]
        {
            let _ = rest;
            return Err(Error::Unsupported {
                feature: "unix-sockets",
            });
        }
    }
    if let Some(rest) = value.strip_prefix("tcp://") {
        let addr = rest.parse().map_err(|_| Error::InvalidArgument {
            name: "LIGHTSHADE_LISTEN".to_string(),
            reason: format!("`{rest}` is not a socket address"),
        })?;
        let access = infer_access(addr)?;
        return Ok(Listen::Tcp {
            addr,
            access,
            allow: Vec::new(),
        });
    }
    Err(Error::InvalidArgument {
        name: "LIGHTSHADE_LISTEN".to_string(),
        reason: "expected tcp://HOST:PORT or unix:///path".to_string(),
    })
}

fn infer_access(addr: std::net::SocketAddr) -> Result<NetworkAccess> {
    if addr.ip().is_loopback() {
        return Ok(NetworkAccess::Local);
    }
    if addr.ip().is_unspecified() {
        return Err(Error::InvalidArgument {
            name: "LIGHTSHADE_NETWORK_ACCESS".to_string(),
            reason: "a wildcard bind requires an explicit access policy".to_string(),
        });
    }
    let policy = NetworkAccess::LocalNetwork;
    if policy.allows(addr.ip()) {
        Ok(policy)
    } else {
        Err(Error::InvalidArgument {
            name: "LIGHTSHADE_NETWORK_ACCESS".to_string(),
            reason: "a public bind requires an explicit access policy".to_string(),
        })
    }
}

fn apply_access(listen: Listen, access: NetworkAccess) -> Result<Listen> {
    match listen {
        Listen::Tcp { addr, allow, .. } => {
            let listen = Listen::Tcp {
                addr,
                access,
                allow,
            };
            listen.validate()?;
            Ok(listen)
        }
        other => Ok(other),
    }
}
