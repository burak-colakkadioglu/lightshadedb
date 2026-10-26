use crate::error::{Error, Result};
use crate::secret::ClientCredentials;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NetworkAccess {
    Local,
    LocalNetwork,
    External,
}

impl NetworkAccess {
    pub fn allows(self, peer: IpAddr) -> bool {
        match self {
            Self::External => true,
            Self::Local => is_loopback(peer),
            Self::LocalNetwork => is_local_network(peer),
        }
    }
}

fn is_loopback(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
    }
}

fn is_local_network(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_private_v4(mapped);
            }
            v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

fn is_private_v4(v4: Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local()
}

impl fmt::Display for NetworkAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Local => "local",
            Self::LocalNetwork => "local-network",
            Self::External => "external",
        })
    }
}

impl FromStr for NetworkAccess {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "local" => Ok(Self::Local),
            "local-network" | "local_network" | "lan" => Ok(Self::LocalNetwork),
            "external" | "public" => Ok(Self::External),
            other => Err(Error::InvalidArgument {
                name: "network_access".to_string(),
                reason: format!("`{other}` is not one of `local`, `local-network`, or `external`"),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpNet {
    V4 { addr: u32, prefix: u8 },
    V6 { addr: u128, prefix: u8 },
}

impl IpNet {
    pub fn contains(self, peer: IpAddr) -> bool {
        match (self, peer) {
            (Self::V4 { addr, prefix }, IpAddr::V4(peer)) => {
                let peer: u32 = peer.into();
                if prefix == 0 {
                    return true;
                }
                let shift = 32 - prefix;
                (addr >> shift) == (peer >> shift)
            }
            (Self::V6 { addr, prefix }, IpAddr::V6(peer)) => {
                let peer: u128 = peer.into();
                if prefix == 0 {
                    return true;
                }
                let shift = 128 - prefix;
                (addr >> shift) == (peer >> shift)
            }
            (Self::V4 { .. }, IpAddr::V6(peer)) => peer
                .to_ipv4_mapped()
                .is_some_and(|mapped| self.contains(IpAddr::V4(mapped))),
            _ => false,
        }
    }
}

impl FromStr for IpNet {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(Error::InvalidArgument {
                name: "allow".to_string(),
                reason: "network is empty".to_string(),
            });
        }
        let (address, prefix) = match value.rsplit_once('/') {
            Some((address, prefix)) => {
                let prefix = prefix.parse::<u8>().map_err(|_| Error::InvalidArgument {
                    name: "allow".to_string(),
                    reason: format!("`{value}` has an invalid prefix length"),
                })?;
                (address, Some(prefix))
            }
            None => (value, None),
        };
        if address.is_empty() || address.contains('%') {
            return Err(Error::InvalidArgument {
                name: "allow".to_string(),
                reason: format!("`{value}` is not a valid IP network"),
            });
        }
        if let Ok(parsed) = address.parse::<Ipv4Addr>() {
            let addr: u32 = parsed.into();
            let prefix = prefix.unwrap_or(32);
            if prefix > 32 {
                return Err(Error::InvalidArgument {
                    name: "allow".to_string(),
                    reason: format!("`{value}` has an invalid prefix length"),
                });
            }
            return Ok(Self::V4 { addr, prefix });
        }
        match address.parse::<std::net::Ipv6Addr>() {
            Ok(parsed) => {
                let prefix = prefix.unwrap_or(128);
                if prefix > 128 {
                    return Err(Error::InvalidArgument {
                        name: "allow".to_string(),
                        reason: format!("`{value}` has an invalid prefix length"),
                    });
                }
                Ok(Self::V6 {
                    addr: parsed.into(),
                    prefix,
                })
            }
            Err(_) => Err(Error::InvalidArgument {
                name: "allow".to_string(),
                reason: format!("`{value}` is not a valid IP network"),
            }),
        }
    }
}

impl fmt::Display for IpNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::V4 { addr, prefix } => {
                write!(
                    f,
                    "{}.{}.{}.{}/{prefix}",
                    (addr >> 24) & 255,
                    (addr >> 16) & 255,
                    (addr >> 8) & 255,
                    addr & 255
                )
            }
            Self::V6 { addr, prefix } => {
                write!(f, "{}/{prefix}", IpAddr::V6(addr.into()))
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEndpoint {
    Path(PathBuf),
    Tcp {
        host: String,
        port: u16,
        database: String,
    },
}

pub const DEFAULT_DATABASE: &str = "default";

impl ClientEndpoint {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    pub fn tcp(host: impl Into<String>, port: u16) -> Self {
        Self::tcp_db(host, port, DEFAULT_DATABASE)
    }

    pub fn tcp_db(host: impl Into<String>, port: u16, database: impl Into<String>) -> Self {
        Self::Tcp {
            host: host.into(),
            port,
            database: database.into(),
        }
    }

    pub fn database(&self) -> &str {
        match self {
            Self::Path(_) => DEFAULT_DATABASE,
            Self::Tcp { database, .. } => database,
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        if let Some(rest) = value.strip_prefix("tcp://") {
            let (host_port, database) = match rest.split_once('/') {
                Some((host_port, database)) => (host_port, database.to_string()),
                None => (rest, DEFAULT_DATABASE.to_string()),
            };
            if database.is_empty() {
                return Err(Error::InvalidArgument {
                    name: "endpoint".to_string(),
                    reason: "TCP endpoint database name is empty".to_string(),
                });
            }
            let (host, port) = split_host_port(host_port)?;
            return Ok(Self::tcp_db(host, port, database));
        }
        if let Some(rest) = value.strip_prefix("unix://") {
            if rest.is_empty() {
                return Err(Error::InvalidArgument {
                    name: "endpoint".to_string(),
                    reason: "unix endpoint has an empty path".to_string(),
                });
            }
            return Ok(Self::path(rest));
        }
        if value.is_empty() {
            return Err(Error::InvalidArgument {
                name: "endpoint".to_string(),
                reason: "endpoint is empty".to_string(),
            });
        }
        Ok(Self::path(value))
    }

    pub fn as_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Tcp { .. } => None,
        }
    }
}

fn split_host_port(value: &str) -> Result<(String, u16)> {
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| Error::InvalidArgument {
            name: "endpoint".to_string(),
            reason: "TCP endpoint needs HOST:PORT".to_string(),
        })?;
    if host.is_empty() {
        return Err(Error::InvalidArgument {
            name: "endpoint".to_string(),
            reason: "TCP endpoint has an empty host".to_string(),
        });
    }
    let port = port.parse::<u16>().map_err(|_| Error::InvalidArgument {
        name: "endpoint".to_string(),
        reason: format!("`{port}` is not a TCP port"),
    })?;
    let host = if host.starts_with('[') && host.ends_with(']') {
        host[1..host.len() - 1].to_string()
    } else {
        host.to_string()
    };
    Ok((host, port))
}

impl Listen {
    pub fn parse_tcp(value: &str, access: NetworkAccess) -> Result<Self> {
        let rest = value
            .strip_prefix("tcp://")
            .ok_or_else(|| Error::InvalidArgument {
                name: "listen".to_string(),
                reason: format!("`{value}` must look like `tcp://HOST:PORT`"),
            })?;
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| Error::InvalidArgument {
                name: "listen".to_string(),
                reason: format!("`{value}` must look like `tcp://HOST:PORT`"),
            })?;
        let port = port.parse::<u16>().map_err(|_| Error::InvalidArgument {
            name: "listen".to_string(),
            reason: format!("`{value}` has an invalid port"),
        })?;
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|source| Error::io("resolving the listen hostname", source))?
            .next()
            .ok_or_else(|| Error::InvalidArgument {
                name: "listen".to_string(),
                reason: format!("`{value}` resolved to no addresses"),
            })?;
        Ok(Self::Tcp {
            addr,
            access,
            allow: Vec::new(),
        })
    }

    pub fn allowlist(&self) -> &[IpNet] {
        match self {
            Self::Tcp { allow, .. } => allow,
            _ => &[],
        }
    }

    pub fn with_allowlist(mut self, networks: &[&str]) -> Result<Self> {
        let mut allow = Vec::with_capacity(networks.len());
        for network in networks {
            allow.push(network.parse()?);
        }
        match &mut self {
            Self::Tcp { allow: slot, .. } => *slot = allow,
            _ => {
                return Err(Error::InvalidArgument {
                    name: "listen".to_string(),
                    reason: "an IP allowlist requires a TCP listener".to_string(),
                })
            }
        }
        Ok(self)
    }
}

impl fmt::Display for ClientEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => write!(f, "{}", path.display()),
            Self::Tcp {
                host,
                port,
                database,
            } if database == DEFAULT_DATABASE => {
                if host.contains(':') {
                    write!(f, "tcp://[{host}]:{port}")
                } else {
                    write!(f, "tcp://{host}:{port}")
                }
            }
            Self::Tcp {
                host,
                port,
                database,
            } => {
                if host.contains(':') {
                    write!(f, "tcp://[{host}]:{port}/{database}")
                } else {
                    write!(f, "tcp://{host}:{port}/{database}")
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum Listen {
    Derived,
    #[cfg(unix)]
    Unix(PathBuf),
    Tcp {
        addr: SocketAddr,
        access: NetworkAccess,
        allow: Vec<IpNet>,
    },
}

impl Listen {
    pub fn tcp_local(port: u16) -> Self {
        Self::Tcp {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            access: NetworkAccess::Local,
            allow: Vec::new(),
        }
    }

    pub fn tcp_local_network(addr: SocketAddr) -> Self {
        Self::Tcp {
            addr,
            access: NetworkAccess::LocalNetwork,
            allow: Vec::new(),
        }
    }

    pub fn tcp_external(addr: SocketAddr) -> Self {
        Self::Tcp {
            addr,
            access: NetworkAccess::External,
            allow: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let Self::Tcp { addr, access, .. } = self else {
            if !self.allowlist().is_empty() {
                return Err(Error::InvalidArgument {
                    name: "listen".to_string(),
                    reason: "an IP allowlist requires a TCP listener".to_string(),
                });
            }
            return Ok(());
        };
        match access {
            NetworkAccess::Local if !addr.ip().is_loopback() => Err(Error::InvalidArgument {
                name: "listen".to_string(),
                reason: format!(
                    "`{addr}` is not a loopback address; use local-network or external"
                ),
            }),
            NetworkAccess::LocalNetwork => {
                let ip = addr.ip();
                let wildcard = ip.is_unspecified();
                if !wildcard && !is_local_network(ip) {
                    return Err(Error::InvalidArgument {
                        name: "listen".to_string(),
                        reason: format!(
                            "`{ip}` is not a loopback, private, link-local, or unique-local address"
                        ),
                    });
                }
                Ok(())
            }
            NetworkAccess::Local | NetworkAccess::External => Ok(()),
        }
    }

    pub fn network_access(&self) -> Option<NetworkAccess> {
        match self {
            Self::Tcp { access, .. } => Some(*access),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionSpec {
    pub endpoint: ClientEndpoint,
    pub credentials: Option<ClientCredentials>,
    pub connect_timeout: Option<Duration>,
    pub io_timeout: Option<Duration>,
}

impl ConnectionSpec {
    pub fn parse(url: &str) -> Result<Self> {
        let rest = url.strip_prefix("lightshadedb://").ok_or_else(|| {
            Error::InvalidArgument {
                name: "url".to_string(),
                reason: "must look like lightshadedb://[id:secret@]host:port[/dbname][?params] or lightshadedb://[id:secret@]/socket/path".to_string(),
            }
        })?;
        if rest.contains('#') {
            return Err(Error::InvalidArgument {
                name: "url".to_string(),
                reason: "fragments (`#...`) are not supported".to_string(),
            });
        }
        let (head, query) = match rest.split_once('?') {
            Some((head, query)) => (head, Some(query)),
            None => (rest, None),
        };
        let (userinfo, hostpart) = match head.rsplit_once('@') {
            Some((userinfo, hostpart)) => (Some(userinfo), hostpart),
            None => (None, head),
        };
        let credentials = match userinfo {
            Some(userinfo) => Some(parse_userinfo(userinfo)?),
            None => None,
        };
        let endpoint = if hostpart.starts_with('/') {
            if hostpart.len() < 2 {
                return Err(Error::InvalidArgument {
                    name: "url".to_string(),
                    reason: "unix socket path is empty".to_string(),
                });
            }
            ClientEndpoint::path(decode_pct(hostpart, "socket path")?)
        } else {
            let (authority, database) = match hostpart.split_once('/') {
                Some((authority, database)) => {
                    if database.is_empty() {
                        return Err(Error::InvalidArgument {
                            name: "url".to_string(),
                            reason: "database name is empty".to_string(),
                        });
                    }
                    (authority, decode_pct(database, "database name")?)
                }
                None => (hostpart, DEFAULT_DATABASE.to_string()),
            };
            let (host, port) = split_host_port(authority)?;
            ClientEndpoint::tcp_db(host, port, database)
        };
        let mut spec = Self {
            endpoint,
            credentials,
            connect_timeout: None,
            io_timeout: None,
        };
        if let Some(query) = query {
            parse_query(query, &mut spec)?;
        }
        Ok(spec)
    }

    pub fn apply_to(
        &self,
        credentials: &mut ClientCredentials,
        connect_timeout: &mut Duration,
        io_timeout: &mut Duration,
    ) {
        if let Some(spec_credentials) = &self.credentials {
            *credentials = spec_credentials.clone();
        }
        if let Some(timeout) = self.connect_timeout {
            *connect_timeout = timeout;
        }
        if let Some(timeout) = self.io_timeout {
            *io_timeout = timeout;
        }
    }
}

fn parse_userinfo(userinfo: &str) -> Result<ClientCredentials> {
    let (id, secret) = userinfo
        .split_once(':')
        .ok_or_else(|| Error::InvalidArgument {
            name: "url".to_string(),
            reason: "userinfo must look like id:secret".to_string(),
        })?;
    let id = decode_pct(id, "credential id")?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::InvalidArgument {
            name: "url".to_string(),
            reason: "credential id must be 1-64 ASCII letters, digits, `.`, `_`, or `-`"
                .to_string(),
        });
    }
    let secret = decode_pct(secret, "credential secret")?;
    if secret.is_empty() {
        return Err(Error::InvalidArgument {
            name: "url".to_string(),
            reason: "credential secret is empty".to_string(),
        });
    }
    Ok(ClientCredentials::new(id, secret))
}

fn decode_pct(value: &str, what: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(Error::InvalidArgument {
                    name: "url".to_string(),
                    reason: format!("{what} has a truncated `%` escape"),
                });
            }
            let high = hex_value(bytes[index + 1], what)?;
            let low = hex_value(bytes[index + 2], what)?;
            out.push(high << 4 | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::InvalidArgument {
        name: "url".to_string(),
        reason: format!("{what} is not valid UTF-8"),
    })
}

fn hex_value(byte: u8, what: &str) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::InvalidArgument {
            name: "url".to_string(),
            reason: format!("{what} has an invalid `%` escape"),
        }),
    }
}

fn parse_query(query: &str, spec: &mut ConnectionSpec) -> Result<()> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(|| Error::InvalidArgument {
            name: "url".to_string(),
            reason: format!("query parameter `{pair}` must look like `name=value`"),
        })?;
        match key {
            "connect_timeout" => {
                spec.connect_timeout = Some(parse_timeout(key, value)?);
            }
            "io_timeout" => {
                spec.io_timeout = Some(parse_timeout(key, value)?);
            }
            "sslmode" => {
                return Err(Error::InvalidArgument {
                    name: "url".to_string(),
                    reason: "sslmode is not supported: Lightshade DB speaks its own protocol, not PostgreSQL wire; terminate TLS in front of the server".to_string(),
                });
            }
            "timezone" => {
                return Err(Error::InvalidArgument {
                    name: "url".to_string(),
                    reason: "timezone is not supported: Lightshade DB has no time-zone-aware field types; store instants as UTC".to_string(),
                });
            }
            _ => {
                return Err(Error::InvalidArgument {
                    name: "url".to_string(),
                    reason: format!("unknown query parameter `{key}`"),
                });
            }
        }
    }
    Ok(())
}

fn parse_timeout(key: &str, value: &str) -> Result<Duration> {
    value
        .parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| Error::InvalidArgument {
            name: "url".to_string(),
            reason: format!("query parameter `{key}` must be a whole number of seconds"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn local_allows_only_loopback() {
        assert!(NetworkAccess::Local.allows(ip("127.0.0.1")));
        assert!(NetworkAccess::Local.allows(ip("::1")));
        assert!(NetworkAccess::Local.allows(ip("::ffff:127.0.0.1")));
        assert!(!NetworkAccess::Local.allows(ip("192.168.1.10")));
        assert!(!NetworkAccess::Local.allows(ip("8.8.8.8")));
    }

    #[test]
    fn local_network_covers_private_and_link_local() {
        for address in [
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.10",
            "169.254.2.3",
            "fd00::10",
            "fe80::1",
            "::ffff:192.168.1.10",
        ] {
            assert!(
                NetworkAccess::LocalNetwork.allows(ip(address)),
                "{address} should be local-network"
            );
        }
        for address in [
            "8.8.8.8",
            "172.32.0.1",
            "100.64.0.1",
            "203.0.113.7",
            "2001:db8::1",
        ] {
            assert!(
                !NetworkAccess::LocalNetwork.allows(ip(address)),
                "{address} should not be local-network"
            );
        }
    }

    #[test]
    fn external_allows_everything() {
        assert!(NetworkAccess::External.allows(ip("8.8.8.8")));
        assert!(NetworkAccess::External.allows(ip("2001:db8::1")));
    }

    #[test]
    fn ip_networks_parse_and_match() {
        let lan: IpNet = "10.0.7.0/24".parse().unwrap();
        assert_eq!(lan.to_string(), "10.0.7.0/24");
        assert!(lan.contains("10.0.7.99".parse().unwrap()));
        assert!(!lan.contains("10.0.8.1".parse().unwrap()));
        assert!(!lan.contains("8.8.8.8".parse().unwrap()));

        let host: IpNet = "192.168.1.50".parse().unwrap();
        assert_eq!(host.to_string(), "192.168.1.50/32");
        assert!(host.contains("192.168.1.50".parse().unwrap()));
        assert!(!host.contains("192.168.1.51".parse().unwrap()));

        let v6: IpNet = "fd00::/8".parse().unwrap();
        assert!(v6.contains("fd00::1".parse().unwrap()));
        assert!(!v6.contains("2001:db8::1".parse().unwrap()));

        assert!(lan.contains("::ffff:10.0.7.5".parse().unwrap()));

        for bad in [
            "",
            "10.0.0.0/33",
            "10.0.0.0/",
            "/24",
            "999.0.0.1",
            "example.com",
            "fe80::1%eth0",
            "::1/129",
        ] {
            assert!(bad.parse::<IpNet>().is_err(), "{bad}");
        }
    }

    #[test]
    fn endpoint_parsing_round_trips() {
        assert_eq!(
            ClientEndpoint::parse("tcp://192.168.1.10:9000").unwrap(),
            ClientEndpoint::tcp("192.168.1.10", 9000)
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://[::1]:9000").unwrap(),
            ClientEndpoint::tcp("::1", 9000)
        );
        assert_eq!(
            ClientEndpoint::parse("unix:///run/lightshade.sock").unwrap(),
            ClientEndpoint::path("/run/lightshade.sock")
        );
        assert_eq!(
            ClientEndpoint::parse("/run/lightshade.sock").unwrap(),
            ClientEndpoint::path("/run/lightshade.sock")
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://[fd00::10]:9000")
                .unwrap()
                .to_string(),
            "tcp://[fd00::10]:9000"
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://localhost:9000")
                .unwrap()
                .to_string(),
            "tcp://localhost:9000"
        );
        assert!(ClientEndpoint::parse("tcp://host").is_err());
        assert!(ClientEndpoint::parse("tcp://:9000").is_err());
        assert!(ClientEndpoint::parse("tcp://host:notaport").is_err());
        assert!(ClientEndpoint::parse("").is_err());
    }

    #[test]
    fn endpoints_carry_a_database_name() {
        assert_eq!(
            ClientEndpoint::parse("tcp://db.internal:9000/c1").unwrap(),
            ClientEndpoint::tcp_db("db.internal", 9000, "c1")
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://[::1]:9000/orders").unwrap(),
            ClientEndpoint::tcp_db("::1", 9000, "orders")
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://db.internal:9000/c1")
                .unwrap()
                .to_string(),
            "tcp://db.internal:9000/c1"
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://db.internal:9000").unwrap(),
            ClientEndpoint::tcp("db.internal", 9000)
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://db.internal:9000/c1")
                .unwrap()
                .database(),
            "c1"
        );
        assert_eq!(
            ClientEndpoint::parse("tcp://db.internal:9000")
                .unwrap()
                .database(),
            DEFAULT_DATABASE
        );
        assert_eq!(
            ClientEndpoint::parse("/run/lightshade.sock")
                .unwrap()
                .database(),
            DEFAULT_DATABASE
        );
        assert!(ClientEndpoint::parse("tcp://host:9000/").is_err());
    }

    #[test]
    fn listen_addresses_parse_with_a_policy() {
        let listen = Listen::parse_tcp("tcp://127.0.0.1:7101", NetworkAccess::Local).unwrap();
        match listen {
            Listen::Tcp { addr, access, .. } => {
                assert_eq!(addr.port(), 7101);
                assert!(addr.ip().is_loopback());
                assert_eq!(access, NetworkAccess::Local);
            }
            _ => panic!("expected a TCP listen address"),
        }
        assert!(Listen::parse_tcp("127.0.0.1:7101", NetworkAccess::Local).is_err());
        assert!(Listen::parse_tcp("tcp://host:notaport", NetworkAccess::Local).is_err());
    }

    #[test]
    fn listen_policy_is_validated() {
        assert!(Listen::tcp_local(9000).validate().is_ok());
        assert!(
            Listen::tcp_local_network("192.168.1.10:9000".parse().unwrap())
                .validate()
                .is_ok()
        );
        assert!(Listen::tcp_local_network("0.0.0.0:9000".parse().unwrap())
            .validate()
            .is_ok());
        assert!(Listen::tcp_local_network("8.8.8.8:9000".parse().unwrap())
            .validate()
            .is_err());
        assert!(Listen::Tcp {
            addr: "0.0.0.0:9000".parse().unwrap(),
            access: NetworkAccess::Local,
            allow: Vec::new(),
        }
        .validate()
        .is_err());
        assert!(Listen::tcp_external("0.0.0.0:9000".parse().unwrap())
            .validate()
            .is_ok());
    }

    #[test]
    fn access_policy_parses() {
        assert_eq!(
            "local".parse::<NetworkAccess>().unwrap(),
            NetworkAccess::Local
        );
        assert_eq!(
            "local-network".parse::<NetworkAccess>().unwrap(),
            NetworkAccess::LocalNetwork
        );
        assert_eq!(
            "external".parse::<NetworkAccess>().unwrap(),
            NetworkAccess::External
        );
        assert!("publicly".parse::<NetworkAccess>().is_err());
    }

    #[test]
    fn connection_urls_parse() {
        let spec =
            ConnectionSpec::parse("lightshadedb://bench:s3cret@db.internal:9000/orders").unwrap();
        assert_eq!(
            spec.endpoint,
            ClientEndpoint::tcp_db("db.internal", 9000, "orders")
        );
        let credentials = spec.credentials.unwrap();
        assert_eq!(credentials.id, "bench");
        assert_eq!(credentials.secret.expose(), "s3cret");
        assert!(spec.connect_timeout.is_none());

        let spec = ConnectionSpec::parse("lightshadedb://db.internal:9000").unwrap();
        assert_eq!(spec.endpoint, ClientEndpoint::tcp("db.internal", 9000));
        assert!(spec.credentials.is_none());

        let spec = ConnectionSpec::parse("lightshadedb://[::1]:9000/orders").unwrap();
        assert_eq!(spec.endpoint, ClientEndpoint::tcp_db("::1", 9000, "orders"));

        let spec = ConnectionSpec::parse("lightshadedb:///run/lightshade.sock").unwrap();
        assert_eq!(spec.endpoint, ClientEndpoint::path("/run/lightshade.sock"));

        let spec =
            ConnectionSpec::parse("lightshadedb://bench:p%40ss%2Fword@db.internal:9000").unwrap();
        assert_eq!(spec.credentials.unwrap().secret.expose(), "p@ss/word");

        let spec = ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?connect_timeout=3&io_timeout=30",
        )
        .unwrap();
        assert_eq!(
            spec.connect_timeout,
            Some(std::time::Duration::from_secs(3))
        );
        assert_eq!(spec.io_timeout, Some(std::time::Duration::from_secs(30)));

        assert!(ConnectionSpec::parse("postgres://bench:s3cret@db:9000/orders").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://db.internal").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://db.internal:9000/").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://:9000/orders").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://bench@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://bench:@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://:s3cret@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://bench:xx@db.internal:9000#frag").is_err());
        assert!(ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?sslmode=require"
        )
        .is_err());
        assert!(ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?timezone=UTC"
        )
        .is_err());
        assert!(ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?bogus=1"
        )
        .is_err());
    }

    #[test]
    fn connection_url_edge_cases() {
        assert!(ConnectionSpec::parse("lightshadedb://").is_err());
        assert!(ConnectionSpec::parse("lightshadedb:///").is_err());
        assert!(ConnectionSpec::parse("tcp://db.internal:9000/orders").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://db.internal").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://db.internal:notaport").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://[::1]/orders").is_err());

        let spec = ConnectionSpec::parse("lightshadedb://u%41ser:s3cret@db.internal:9000").unwrap();
        assert_eq!(spec.credentials.unwrap().id, "uAser");
        assert!(ConnectionSpec::parse("lightshadedb://%ZZ:sec@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://id:se%2@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://id:sec%@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://%FF:sec@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse(&format!(
            "lightshadedb://{}:sec@db.internal:9000",
            "a".repeat(65)
        ))
        .is_err());
        assert!(ConnectionSpec::parse("lightshadedb://a%20b:sec@db.internal:9000").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://a/b:sec@db.internal:9000").is_err());

        let spec = ConnectionSpec::parse("lightshadedb://id:sec@/run/x.sock").unwrap();
        assert_eq!(spec.endpoint, ClientEndpoint::path("/run/x.sock"));
        assert_eq!(spec.credentials.unwrap().id, "id");

        let spec = ConnectionSpec::parse("lightshadedb://db.internal:9000/my%20db").unwrap();
        assert_eq!(spec.endpoint.database(), "my db");

        assert!(ConnectionSpec::parse("lightshadedb://db.internal:9000?").is_err());
        assert!(ConnectionSpec::parse("lightshadedb://db.internal:9000?flag").is_err());
        assert!(
            ConnectionSpec::parse("lightshadedb://db.internal:9000?connect_timeout=x").is_err()
        );
        let spec =
            ConnectionSpec::parse("lightshadedb://db.internal:9000?connect_timeout=0").unwrap();
        assert_eq!(
            spec.connect_timeout,
            Some(std::time::Duration::from_secs(0))
        );
        let spec =
            ConnectionSpec::parse("lightshadedb://db.internal:9000?io_timeout=5&io_timeout=9")
                .unwrap();
        assert_eq!(spec.io_timeout, Some(std::time::Duration::from_secs(9)));
        let error = ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?sslmode=disable",
        )
        .unwrap_err();
        assert!(error.to_string().contains("sslmode"));
        let error = ConnectionSpec::parse(
            "lightshadedb://bench:s3cret@db.internal:9000/orders?timezone=UTC",
        )
        .unwrap_err();
        assert!(error.to_string().contains("timezone"));
    }

    #[test]
    fn connection_spec_overrides_options() {
        let base_credentials = crate::secret::ClientCredentials::new("env", "env-secret");
        let spec = ConnectionSpec::parse("lightshadedb://db.internal:9000").unwrap();
        let mut credentials = base_credentials.clone();
        let mut connect_timeout = std::time::Duration::from_secs(5);
        let mut io_timeout = std::time::Duration::from_secs(30);
        spec.apply_to(&mut credentials, &mut connect_timeout, &mut io_timeout);
        assert_eq!(credentials.id, "env");
        assert_eq!(connect_timeout, std::time::Duration::from_secs(5));

        let spec =
            ConnectionSpec::parse("lightshadedb://url:id@db.internal:9000?io_timeout=7").unwrap();
        let mut credentials = base_credentials;
        let mut connect_timeout = std::time::Duration::from_secs(5);
        let mut io_timeout = std::time::Duration::from_secs(30);
        spec.apply_to(&mut credentials, &mut connect_timeout, &mut io_timeout);
        assert_eq!(credentials.id, "url");
        assert_eq!(connect_timeout, std::time::Duration::from_secs(5));
        assert_eq!(io_timeout, std::time::Duration::from_secs(7));
    }

    #[test]
    fn connection_spec_debug_redacts_secrets() {
        let spec = ConnectionSpec::parse("lightshadedb://bench:hunter2@db.internal:9000").unwrap();
        let text = format!("{spec:?}");
        assert!(!text.contains("hunter2"));
        assert!(text.contains("bench"));
    }
}
