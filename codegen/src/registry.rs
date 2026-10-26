use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryMode {
    Embedded,
    Host,
    Client,
}

impl RegistryMode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "embedded" => Some(Self::Embedded),
            "host" => Some(Self::Host),
            "client" => Some(Self::Client),
            _ => None,
        }
    }

    pub fn feature(&self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Host => "host",
            Self::Client => "client",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseEntry {
    pub id: String,
    pub schema: String,
    pub mode: RegistryMode,
    pub path: Option<String>,
    pub listen: Option<String>,
    pub access: Option<String>,
    pub allow: Vec<String>,
    pub credentials_file: Option<String>,
    pub endpoint: Option<String>,
    pub credential_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub databases: Vec<DatabaseEntry>,
}

impl Registry {
    pub fn env_prefix(id: &str) -> String {
        let mut prefix = String::from("LIGHTSHADE_");
        for character in id.chars() {
            if character.is_ascii_alphanumeric() {
                prefix.push(character.to_ascii_uppercase());
            } else {
                prefix.push('_');
            }
        }
        prefix
    }
}

fn invalid(message: impl Into<String>) -> String {
    message.into()
}

pub fn parse_registry(source: &str) -> Result<Registry, String> {
    let value: toml::Value = toml::from_str(source)
        .map_err(|error| invalid(format!("invalid lightshade.toml: {error}")))?;
    let table = value
        .as_table()
        .ok_or_else(|| invalid("lightshade.toml must have table entries"))?;
    let databases = table
        .get("database")
        .and_then(|value| value.as_table())
        .ok_or_else(|| invalid("lightshade.toml needs at least one [database.<id>] table"))?;
    if databases.is_empty() {
        return Err(invalid(
            "lightshade.toml needs at least one [database.<id>] table",
        ));
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(databases.len());
    for (id, entry) in databases {
        if id.is_empty() || id.chars().any(char::is_whitespace) {
            return Err(invalid(format!(
                "database id `{id}` must be non-empty without whitespace"
            )));
        }
        if !seen.insert(id.clone()) {
            return Err(invalid(format!("duplicate database id `{id}`")));
        }
        let entry = entry
            .as_table()
            .ok_or_else(|| invalid(format!("database `{id}` must be a table")))?;
        let get = |key: &str| {
            entry
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        for key in entry.keys() {
            let known = matches!(
                key.as_str(),
                "schema"
                    | "mode"
                    | "path"
                    | "listen"
                    | "access"
                    | "credentials_file"
                    | "endpoint"
                    | "credentials"
                    | "allow"
            );
            if !known {
                return Err(invalid(format!("database `{id}` has unknown key `{key}`")));
            }
        }
        let schema = get("schema")
            .filter(|schema| !schema.is_empty())
            .ok_or_else(|| invalid(format!("database `{id}` needs `schema = \"...\"`")))?;
        let mode = get("mode")
            .and_then(|mode| RegistryMode::parse(&mode))
            .ok_or_else(|| {
                invalid(format!(
                    "database `{id}` needs `mode = \"embedded\"`, `\"host\"`, or `\"client\"`"
                ))
            })?;
        let mut parsed = DatabaseEntry {
            id: id.clone(),
            schema,
            mode,
            path: get("path"),
            listen: get("listen"),
            access: get("access"),
            credentials_file: get("credentials_file"),
            endpoint: get("endpoint"),
            credential_id: None,
            allow: Vec::new(),
        };
        if let Some(credentials) = entry.get("credentials") {
            let table = credentials
                .as_table()
                .ok_or_else(|| invalid(format!("database `{id}` `credentials` must be a table")))?;
            for key in table.keys() {
                if key != "id" {
                    return Err(invalid(format!(
                        "database `{id}` credentials only accept `id` (the secret comes from the environment)"
                    )));
                }
            }
            parsed.credential_id = table
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
        if let Some(allow) = entry.get("allow") {
            let list = allow.as_array().ok_or_else(|| {
                invalid(format!(
                    "database `{}` `allow` must be a list of CIDR networks",
                    id
                ))
            })?;
            for network in list {
                let network = network.as_str().ok_or_else(|| {
                    invalid(format!("database `{}` `allow` entries must be strings", id))
                })?;
                validate_cidr(network).map_err(|message| {
                    invalid(format!(
                        "database `{}` has an invalid allowlist entry: {message}",
                        id
                    ))
                })?;
                parsed.allow.push(network.to_string());
            }
        }
        entries.push(parsed);
    }
    let registry = Registry { databases: entries };
    validate_registry(&registry)?;
    Ok(registry)
}

pub fn validate_registry(registry: &Registry) -> Result<(), String> {
    let mut fields = HashSet::new();
    for entry in &registry.databases {
        if !entry.allow.is_empty() && entry.mode != RegistryMode::Host {
            return Err(invalid(format!(
                "database `{}` `allow` needs `mode = \"host\"`: only listeners filter peers",
                entry.id
            )));
        }
        match entry.mode {
            RegistryMode::Embedded => {
                if entry.path.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "embedded database `{}` needs `path = \"...\"`",
                        entry.id
                    )));
                }
            }
            RegistryMode::Host => {
                if entry.path.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "hosted database `{}` needs `path = \"...\"`",
                        entry.id
                    )));
                }
                if entry.listen.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "hosted database `{}` needs `listen = \"tcp://HOST:PORT\"`",
                        entry.id
                    )));
                }
                if entry.credentials_file.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "hosted database `{}` needs `credentials_file = \"...\"`",
                        entry.id
                    )));
                }
                if let Some(access) = &entry.access {
                    if !matches!(access.as_str(), "local" | "local-network" | "external") {
                        return Err(invalid(format!(
                            "database `{}` has unknown access policy `{access}`",
                            entry.id
                        )));
                    }
                }
            }
            RegistryMode::Client => {
                if entry.endpoint.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "client database `{}` needs `endpoint = \"...\"`",
                        entry.id
                    )));
                }
                if entry.credential_id.as_deref().unwrap_or("").is_empty() {
                    return Err(invalid(format!(
                        "client database `{}` needs `[database.{}.credentials]` with `id = \"...\"`",
                        entry.id, entry.id
                    )));
                }
            }
        }
        let field = field_name(&entry.id);
        if !fields.insert(field.clone()) {
            return Err(invalid(format!(
                "database id `{}` collides with another id after field-name sanitizing (`{field}`)",
                entry.id
            )));
        }
    }
    Ok(())
}

pub fn field_name(id: &str) -> String {
    let mut out = String::new();
    for character in id.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.starts_with(|character: char| character.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out.push_str("database");
    }
    out
}

pub fn validate_cidr(value: &str) -> Result<(), String> {
    let invalid = |message: String| message;
    if value.is_empty() {
        return Err(invalid("network is empty".to_string()));
    }
    let (address, prefix) = match value.rsplit_once('/') {
        Some((address, prefix)) => {
            let prefix = prefix
                .parse::<u32>()
                .map_err(|_| invalid(format!("`{value}` has an invalid prefix length")))?;
            (address, Some(prefix))
        }
        None => (value, None),
    };
    if address.is_empty() || address.contains('%') {
        return Err(invalid(format!("`{value}` is not a valid IP network")));
    }
    if address.parse::<std::net::Ipv4Addr>().is_ok() {
        let prefix = prefix.unwrap_or(32);
        if prefix > 32 {
            return Err(invalid(format!("`{value}` has an invalid prefix length")));
        }
        return Ok(());
    }
    if address.parse::<std::net::Ipv6Addr>().is_ok() {
        let prefix = prefix.unwrap_or(128);
        if prefix > 128 {
            return Err(invalid(format!("`{value}` has an invalid prefix length")));
        }
        return Ok(());
    }
    Err(invalid(format!("`{value}` is not a valid IP network")))
}

pub fn split_listen(value: &str) -> Result<(String, u16), String> {
    let rest = value.strip_prefix("tcp://").ok_or_else(|| {
        invalid(format!(
            "listen address `{value}` must look like `tcp://HOST:PORT`"
        ))
    })?;
    let (host, port) = rest.rsplit_once(':').ok_or_else(|| {
        invalid(format!(
            "listen address `{value}` must look like `tcp://HOST:PORT`"
        ))
    })?;
    if host.is_empty() {
        return Err(invalid(format!(
            "listen address `{value}` has an empty host"
        )));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| invalid(format!("listen address `{value}` has an invalid port")))?;
    let host = if host.starts_with('[') && host.ends_with(']') {
        host[1..host.len() - 1].to_string()
    } else {
        host.to_string()
    };
    Ok((host, port))
}

pub fn schemas_in(registry: &Registry) -> Vec<String> {
    let mut schemas = Vec::new();
    let mut seen = HashSet::new();
    for entry in &registry.databases {
        if seen.insert(entry.schema.clone()) {
            schemas.push(entry.schema.clone());
        }
    }
    schemas
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[database.a1]
schema = "store"
mode = "embedded"
path = "data/a1.lsdb"

[database."b-1"]
schema = "store"
mode = "host"
path = "data/b1.lsdb"
listen = "tcp://127.0.0.1:7101"
access = "local-network"
credentials_file = "creds/b1.toml"

[database.c1]
schema = "analytics"
mode = "client"
endpoint = "tcp://db.internal:7101/c1"

[database.c1.credentials]
id = "reader"
"#;

    #[test]
    fn parses_all_modes_and_sanitizes_fields() {
        let registry = parse_registry(EXAMPLE).unwrap();
        assert_eq!(registry.databases.len(), 3);
        assert_eq!(registry.databases[0].mode, RegistryMode::Embedded);
        assert_eq!(
            registry.databases[1].access.as_deref(),
            Some("local-network")
        );
        assert_eq!(
            registry.databases[2].credential_id.as_deref(),
            Some("reader")
        );
        assert_eq!(field_name("b-1"), "b_1");
        assert_eq!(field_name("123"), "_123");
        assert_eq!(Registry::env_prefix("orders-eu"), "LIGHTSHADE_ORDERS_EU");
        assert_eq!(
            schemas_in(&registry),
            vec!["store".to_string(), "analytics".to_string()]
        );
    }

    #[test]
    fn rejects_bad_registries() {
        assert!(parse_registry("").is_err());
        assert!(parse_registry("[other.x]\nmode = \"embedded\"\n").is_err());
        assert!(parse_registry("[database.a1]\nschema = \"store\"\nmode = \"bogus\"\n").is_err());
        assert!(
            parse_registry("[database.a1]\nschema = \"store\"\nmode = \"embedded\"\n").is_err()
        );
        assert!(parse_registry(
            "[database.a1]\nschema = \"store\"\nmode = \"host\"\nlisten = \"tcp://127.0.0.1:1\"\n"
        )
        .is_err());
        assert!(parse_registry(
            "[database.a1]\nschema = \"store\"\nmode = \"client\"\nendpoint = \"tcp://h:1\"\n"
        )
        .is_err());
        assert!(parse_registry("[database.a-1]\nschema = \"x\"\nmode = \"embedded\"\npath = \"a\"\n[database.a_1]\nschema = \"x\"\nmode = \"embedded\"\npath = \"b\"\n").is_err());
        assert!(parse_registry(
            "[database.a1]\nschema = \"store\"\nmode = \"embedded\"\npath = \"a\"\nbogus = 1\n"
        )
        .is_err());
    }

    #[test]
    fn allowlists_validate_cidr_syntax() {
        for valid in [
            "10.0.7.0/24",
            "192.168.1.50",
            "127.0.0.1/32",
            "0.0.0.0/0",
            "fd00::/8",
            "::1",
            "::/0",
        ] {
            assert!(validate_cidr(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "10.0.0.0/33",
            "10.0.0.0/-1",
            "10.0.0.0/",
            "/24",
            "999.0.0.1",
            "example.com",
            "fe80::1%eth0",
            "::1/129",
            "10.0.0.1/24/24",
        ] {
            assert!(validate_cidr(invalid).is_err(), "{invalid}");
        }
        assert!(parse_registry("[database.a]\nschema = \"s\"\nmode = \"host\"\npath = \"a\"\nlisten = \"tcp://127.0.0.1:1\"\ncredentials_file = \"c\"\nallow = [\"10.0.0.0/8\", \"::1\"]\n").is_ok());
        assert!(parse_registry("[database.a]\nschema = \"s\"\nmode = \"host\"\npath = \"a\"\nlisten = \"tcp://127.0.0.1:1\"\ncredentials_file = \"c\"\nallow = [\"nope\"]\n").is_err());
        assert!(parse_registry("[database.a]\nschema = \"s\"\nmode = \"host\"\npath = \"a\"\nlisten = \"tcp://127.0.0.1:1\"\ncredentials_file = \"c\"\nallow = \"10.0.0.0/8\"\n").is_err());
        assert!(parse_registry("[database.a]\nschema = \"s\"\nmode = \"embedded\"\npath = \"a\"\nallow = [\"10.0.0.0/8\"]\n").is_err());
    }

    #[test]
    fn splits_listen_addresses() {
        assert_eq!(
            split_listen("tcp://127.0.0.1:7101").unwrap(),
            ("127.0.0.1".to_string(), 7101)
        );
        assert_eq!(
            split_listen("tcp://[::1]:7101").unwrap(),
            ("::1".to_string(), 7101)
        );
        assert!(split_listen("127.0.0.1:7101").is_err());
        assert!(split_listen("tcp://host:notaport").is_err());
    }
}
