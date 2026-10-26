use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct OpenOptions {
    pub read_only: bool,
    pub create: bool,
    pub migrate: bool,
    pub checkpoint_bytes: u64,
    pub row_cache_bytes: u64,
    pub max_rows: u64,
    pub max_wal_bytes: u64,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            read_only: false,
            create: true,
            migrate: false,
            checkpoint_bytes: 8 * 1024 * 1024,
            row_cache_bytes: 64 * 1024 * 1024,
            max_rows: 0,
            max_wal_bytes: 0,
        }
    }
}

impl OpenOptions {
    pub fn from_env() -> Result<Self> {
        let mut options = Self::default();
        if let Some(value) = env_u64("LIGHTSHADE_CHECKPOINT_BYTES")? {
            options.checkpoint_bytes = value;
        }
        if let Some(value) = env_u64("LIGHTSHADE_ROW_CACHE_BYTES")? {
            options.row_cache_bytes = value;
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

#[derive(Clone, Debug, Default)]
pub struct RecoverOptions {
    pub discard_wal: bool,
}

pub(crate) fn env_u64(name: &str) -> Result<Option<u64>> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| Error::InvalidArgument {
                name: name.to_string(),
                reason: format!("`{value}` is not a non-negative integer"),
            }),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidArgument {
            name: name.to_string(),
            reason: "is not valid UTF-8".to_string(),
        }),
    }
}

pub(crate) fn env_bool(name: &str) -> Result<Option<bool>> {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "yes" | "on" => Ok(Some(true)),
            "0" | "false" | "no" | "off" => Ok(Some(false)),
            other => Err(Error::InvalidArgument {
                name: name.to_string(),
                reason: format!("`{other}` is not a boolean"),
            }),
        },
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidArgument {
            name: name.to_string(),
            reason: "is not valid UTF-8".to_string(),
        }),
    }
}
