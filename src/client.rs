use crate::error::Result;
use crate::net::ConnectionSpec;
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

    pub fn from_spec_or_env(spec: &ConnectionSpec) -> Result<(crate::net::ClientEndpoint, Self)> {
        let mut options = match Self::from_env() {
            Ok(options) => options,
            Err(error) => match &spec.credentials {
                Some(credentials) => Self::new(credentials.clone()),
                None => return Err(error),
            },
        };
        spec.apply_to(
            &mut options.credentials,
            &mut options.connect_timeout,
            &mut options.io_timeout,
        );
        Ok((spec.endpoint.clone(), options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{ClientEndpoint, ConnectionSpec};

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn take(keys: &[&'static str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    const ID: &str = "LIGHTSHADE_CREDENTIAL_ID";
    const SECRET: &str = "LIGHTSHADE_CREDENTIAL_SECRET";

    #[test]
    fn spec_or_env_resolves_credentials() {
        let _guard = EnvGuard::take(&[ID, SECRET]);

        std::env::set_var(ID, "env-id");
        std::env::set_var(SECRET, "env-secret");
        let spec = ConnectionSpec::parse("lightshadedb://url-id:url-secret@db:9000").unwrap();
        let (endpoint, options) = ClientOptions::from_spec_or_env(&spec).unwrap();
        assert!(matches!(endpoint, ClientEndpoint::Tcp { .. }));
        assert_eq!(options.credentials.id, "url-id");
        assert_eq!(options.credentials.secret.expose(), "url-secret");

        let spec = ConnectionSpec::parse("lightshadedb://db:9000").unwrap();
        let (_, options) = ClientOptions::from_spec_or_env(&spec).unwrap();
        assert_eq!(options.credentials.id, "env-id");

        std::env::remove_var(ID);
        std::env::remove_var(SECRET);
        let spec = ConnectionSpec::parse("lightshadedb://url-id:url-secret@db:9000").unwrap();
        let (_, options) = ClientOptions::from_spec_or_env(&spec).unwrap();
        assert_eq!(options.credentials.id, "url-id");

        let spec = ConnectionSpec::parse("lightshadedb://db:9000").unwrap();
        assert!(ClientOptions::from_spec_or_env(&spec).is_err());
    }
}
