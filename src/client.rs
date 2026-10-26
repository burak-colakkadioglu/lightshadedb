use crate::error::Result;
use crate::secret::ClientCredentials;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub credentials: ClientCredentials,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
}

impl ClientOptions {
    pub fn new(credentials: ClientCredentials) -> Self {
        Self {
            credentials,
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(30),
        }
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self::new(ClientCredentials::from_env()?))
    }
}
