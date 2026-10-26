use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use zeroize::Zeroize;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn ct_eq(&self, other: &Secret) -> bool {
        use subtle::ConstantTimeEq;
        if self.0.len() != other.0.len() {
            return false;
        }
        self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Reader,
    Writer,
    Admin,
}

#[derive(Clone, Debug)]
pub struct Credential {
    pub id: String,
    pub secret: Secret,
    pub role: Role,
}

#[derive(Clone, Debug, Default)]
pub struct Credentials {
    entries: BTreeMap<String, Credential>,
}

impl Credentials {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, credential: Credential) -> Option<Credential> {
        self.entries.insert(credential.id.clone(), credential)
    }

    pub fn get(&self, id: &str) -> Option<&Credential> {
        self.entries.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Credential)> {
        self.entries
            .iter()
            .map(|(id, credential)| (id.as_str(), credential))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn load(path: &std::path::Path) -> crate::Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|source| crate::Error::io("reading credentials", source))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| crate::Error::InvalidArgument {
            name: "credentials".to_string(),
            reason: "file is not valid UTF-8".to_string(),
        })?;
        let mut credentials = Self::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let id = parts.next().ok_or_else(|| crate::Error::InvalidArgument {
                name: "credentials".to_string(),
                reason: format!("line {} is empty", index + 1),
            })?;
            let role = match parts.next() {
                Some("reader") => Role::Reader,
                Some("writer") => Role::Writer,
                Some("admin") => Role::Admin,
                _ => {
                    return Err(crate::Error::InvalidArgument {
                        name: "credentials".to_string(),
                        reason: format!("line {} has an invalid role", index + 1),
                    })
                }
            };
            let secret = parts.next().ok_or_else(|| crate::Error::InvalidArgument {
                name: "credentials".to_string(),
                reason: format!("line {} has no secret", index + 1),
            })?;
            if parts.next().is_some() {
                return Err(crate::Error::InvalidArgument {
                    name: "credentials".to_string(),
                    reason: format!("line {} has trailing fields", index + 1),
                });
            }
            credentials.insert(Credential {
                id: id.to_string(),
                secret: Secret::new(secret),
                role,
            });
        }
        Ok(credentials)
    }
}

#[derive(Clone, Debug)]
pub struct ClientCredentials {
    pub id: String,
    pub secret: Secret,
}

impl ClientCredentials {
    pub fn new(id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            secret: Secret::new(secret),
        }
    }

    pub fn from_env() -> crate::Result<Self> {
        let id = std::env::var("LIGHTSHADE_CREDENTIAL_ID").map_err(|_| {
            crate::Error::InvalidArgument {
                name: "LIGHTSHADE_CREDENTIAL_ID".to_string(),
                reason: "is not set".to_string(),
            }
        })?;
        let secret = std::env::var("LIGHTSHADE_CREDENTIAL_SECRET").map_err(|_| {
            crate::Error::InvalidArgument {
                name: "LIGHTSHADE_CREDENTIAL_SECRET".to_string(),
                reason: "is not set".to_string(),
            }
        })?;
        Ok(Self::new(id, secret))
    }
}

#[derive(Clone, Debug)]
pub enum CredentialsSource {
    File(PathBuf),
    Static(Credentials),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secrets() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert!(!format!("{secret:?}").contains("hunter2"));
        let credentials = Credentials {
            entries: BTreeMap::from([(
                "ada".to_string(),
                Credential {
                    id: "ada".to_string(),
                    secret: Secret::new("hunter2"),
                    role: Role::Admin,
                },
            )]),
        };
        let text = format!("{credentials:?}");
        assert!(!text.contains("hunter2"));
        assert!(text.contains("ada"));
    }

    #[test]
    fn constant_time_equality_compares_contents() {
        assert!(Secret::new("abc").ct_eq(&Secret::new("abc")));
        assert!(!Secret::new("abc").ct_eq(&Secret::new("abd")));
        assert!(!Secret::new("abc").ct_eq(&Secret::new("ab")));
    }
}
