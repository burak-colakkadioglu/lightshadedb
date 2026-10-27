use crate::error::{Error, ErrorCode, QuotaKind, RemoteDetail, Result};
use crate::net::{ClientEndpoint, IpNet, Listen, NetworkAccess};
use crate::secret::{ClientCredentials, Credential, Credentials, CredentialsSource, Role, Secret};
use crate::table::{FieldType, Value};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAGIC: [u8; 4] = *b"LSRP";
pub const VERSION: u32 = 6;
pub const MIN_VERSION: u32 = 5;
pub const MAX_FRAME: usize = 256 * 1024 * 1024;
pub const MAX_HANDSHAKE: usize = 64 * 1024;

#[doc(hidden)]
pub trait Requester {
    fn request(&mut self, index: u32, params: &[Value]) -> Result<Vec<Value>>;
    fn begin(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
}

#[derive(Debug)]
pub enum Stream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl Stream {
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.peer_addr().ok(),
            #[cfg(unix)]
            Self::Unix(_) => None,
        }
    }

    pub fn set_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        match self {
            Self::Tcp(stream) => {
                stream
                    .set_read_timeout(timeout)
                    .map_err(|source| Error::io("setting the TCP read timeout", source))?;
                stream
                    .set_write_timeout(timeout)
                    .map_err(|source| Error::io("setting the TCP write timeout", source))?;
            }
            #[cfg(unix)]
            Self::Unix(stream) => {
                stream
                    .set_read_timeout(timeout)
                    .map_err(|source| Error::io("setting the Unix read timeout", source))?;
                stream
                    .set_write_timeout(timeout)
                    .map_err(|source| Error::io("setting the Unix write timeout", source))?;
            }
        }
        Ok(())
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
        }
    }
}

pub fn connect(
    endpoint: &ClientEndpoint,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<(Stream, ClientEndpoint)> {
    match endpoint {
        ClientEndpoint::Tcp {
            host,
            port,
            database,
        } => {
            let addresses = (host.as_str(), *port)
                .to_socket_addrs()
                .map_err(|source| Error::io("resolving the server hostname", source))?;
            let mut last = None;
            for address in addresses {
                match TcpStream::connect_timeout(&address, connect_timeout) {
                    Ok(stream) => {
                        stream.set_nodelay(true).ok();
                        let stream = Stream::Tcp(stream);
                        stream.set_timeout(Some(io_timeout))?;
                        return Ok((
                            stream,
                            ClientEndpoint::tcp_db(host.clone(), *port, database.clone()),
                        ));
                    }
                    Err(error) => last = Some(error),
                }
            }
            Err(Error::io(
                "connecting to the Lightshade DB server",
                last.unwrap_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        "hostname resolved to no addresses",
                    )
                }),
            ))
        }
        ClientEndpoint::Path(path) => connect_path(path, connect_timeout, io_timeout),
    }
}

fn connect_path(
    path: &Path,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<(Stream, ClientEndpoint)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let metadata = std::fs::metadata(path)
            .map_err(|source| Error::io("reading the endpoint path", source))?;
        if metadata.file_type().is_socket() {
            let stream = std::os::unix::net::UnixStream::connect(path)
                .map_err(|source| Error::io("connecting to the Unix socket", source))?;
            let stream = Stream::Unix(stream);
            stream.set_timeout(Some(io_timeout))?;
            return Ok((stream, ClientEndpoint::path(path)));
        }
        if metadata.is_file() {
            let text = std::fs::read_to_string(path)
                .map_err(|source| Error::io("reading the endpoint file", source))?;
            let endpoint = ClientEndpoint::parse(text.trim())?;
            return connect(&endpoint, connect_timeout, io_timeout);
        }
        Err(Error::InvalidArgument {
            name: "endpoint".to_string(),
            reason: format!("`{}` is not a socket or endpoint file", path.display()),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, connect_timeout, io_timeout);
        Err(Error::Unsupported {
            feature: "unix-sockets",
        })
    }
}

pub enum Listener {
    Tcp {
        listener: TcpListener,
        access: NetworkAccess,
    },
    #[cfg(unix)]
    Unix {
        listener: std::os::unix::net::UnixListener,
        path: PathBuf,
    },
}

pub struct Bound {
    pub listener: Listener,
    pub endpoint: ClientEndpoint,
    pub endpoint_file: Option<PathBuf>,
    pub local_addr: Option<SocketAddr>,
    pub network_access: NetworkAccess,
    pub allowlist: Vec<IpNet>,
}

impl Listener {
    pub fn accept(&self) -> std::io::Result<Stream> {
        match self {
            Self::Tcp { listener, .. } => listener.accept().map(|(stream, _)| {
                stream.set_nodelay(true).ok();
                stream.set_nonblocking(false).ok();
                Stream::Tcp(stream)
            }),
            #[cfg(unix)]
            Self::Unix { listener, .. } => listener.accept().map(|(stream, _)| {
                stream.set_nonblocking(false).ok();
                Stream::Unix(stream)
            }),
        }
    }

    pub fn set_nonblocking(&self, value: bool) -> std::io::Result<()> {
        match self {
            Self::Tcp { listener, .. } => listener.set_nonblocking(value),
            #[cfg(unix)]
            Self::Unix { listener, .. } => listener.set_nonblocking(value),
        }
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        None
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Self::Unix { path, .. } = self {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn bind(listen: &Listen, database: &Path) -> Result<Bound> {
    listen.validate()?;
    match listen {
        Listen::Tcp {
            addr,
            access,
            allow,
        } => {
            let listener = TcpListener::bind(addr)
                .map_err(|source| Error::io("binding the TCP listener", source))?;
            let local = listener
                .local_addr()
                .map_err(|source| Error::io("reading the TCP listener address", source))?;
            listener.set_nonblocking(true).ok();
            let endpoint = ClientEndpoint::tcp(local.ip().to_string(), local.port());
            let endpoint_file = endpoint_path(database);
            Ok(Bound {
                listener: Listener::Tcp {
                    listener,
                    access: *access,
                },
                endpoint,
                endpoint_file: Some(endpoint_file),
                local_addr: Some(local),
                network_access: *access,
                allowlist: allow.clone(),
            })
        }
        #[cfg(unix)]
        Listen::Unix(path) => bind_unix(path.clone()),
        Listen::Derived => {
            #[cfg(unix)]
            {
                bind_unix(endpoint_path(database))
            }
            #[cfg(not(unix))]
            {
                let addr = SocketAddr::from(([127, 0, 0, 1], 0));
                bind(
                    &Listen::Tcp {
                        addr,
                        access: NetworkAccess::Local,
                        allow: Vec::new(),
                    },
                    database,
                )
            }
        }
    }
}

#[cfg(unix)]
fn bind_unix(path: PathBuf) -> Result<Bound> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(Error::io("removing the stale Unix socket", source)),
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|source| Error::io("creating the socket directory", source))?;
        }
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)
        .map_err(|source| Error::io("binding the Unix socket", source))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|source| Error::io("securing the Unix socket", source))?;
    listener.set_nonblocking(true).ok();
    Ok(Bound {
        listener: Listener::Unix {
            listener,
            path: path.clone(),
        },
        endpoint: ClientEndpoint::path(&path),
        endpoint_file: None,
        local_addr: None,
        network_access: NetworkAccess::Local,
        allowlist: Vec::new(),
    })
}

pub fn endpoint_path(database: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sock", database.display()))
}

pub fn write_endpoint_file(path: &Path, endpoint: &ClientEndpoint) -> Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| Error::io("writing the endpoint file", source))?;
    writeln!(file, "{endpoint}")
        .and_then(|_| file.flush())
        .map_err(|source| Error::io("writing the endpoint file", source))
}

pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(Error::Protocol {
            reason: "frame exceeds the 256 MiB limit".to_string(),
        });
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| writer.write_all(payload))
        .and_then(|_| writer.flush())
        .map_err(|source| Error::io("writing a wire frame", source))
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|source| Error::io("reading a wire frame length", source))?;
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME {
        return Err(Error::Protocol {
            reason: "frame exceeds the 256 MiB limit".to_string(),
        });
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|source| Error::io("reading a wire frame", source))?;
    Ok(payload)
}

pub fn encode_values(values: &[Value], buffer: &mut Vec<u8>) {
    put_u32(buffer, values.len() as u32);
    for value in values {
        put_value(value, buffer);
    }
}

pub fn decode_values(input: &mut std::io::Cursor<&[u8]>) -> Result<Vec<Value>> {
    let length = get_u32(input)? as usize;
    if length > 1_000_000 {
        return Err(Error::Protocol {
            reason: "parameter count is too large".to_string(),
        });
    }
    let mut values = Vec::with_capacity(length);
    for _ in 0..length {
        values.push(get_value(input)?);
    }
    Ok(values)
}

pub fn put_value(value: &Value, buffer: &mut Vec<u8>) {
    if !matches!(value, Value::Null(_)) {
        buffer.push(value.field_type().tag());
    }
    match value {
        Value::Null(ty) => {
            buffer.push(14);
            buffer.push(ty.tag());
        }
        Value::I8(value) => buffer.push(*value as u8),
        Value::I16(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::I32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::I64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U8(value) => buffer.push(*value),
        Value::U16(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::F32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::F64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::Bool(value) => buffer.push(*value as u8),
        Value::Text(value) => put_text(buffer, value),
        Value::Bytes(value) => put_bytes(buffer, value),
        Value::Uuid(value) => buffer.extend_from_slice(value),
    }
}

pub fn get_value(input: &mut std::io::Cursor<&[u8]>) -> Result<Value> {
    let tag = get_u8(input)?;
    if tag == 14 {
        let inner = get_u8(input)?;
        let ty = FieldType::from_tag(inner).ok_or_else(|| Error::Protocol {
            reason: format!("unknown null type tag {inner}"),
        })?;
        return Ok(Value::Null(ty));
    }
    let ty = FieldType::from_tag(tag).ok_or_else(|| Error::Protocol {
        reason: format!("unknown value type tag {tag}"),
    })?;
    Ok(match ty {
        FieldType::I8 => Value::I8(get_u8(input)? as i8),
        FieldType::I16 => Value::I16(i16::from_le_bytes(read_array(input)?)),
        FieldType::I32 => Value::I32(i32::from_le_bytes(read_array(input)?)),
        FieldType::I64 => Value::I64(i64::from_le_bytes(read_array(input)?)),
        FieldType::U8 => Value::U8(get_u8(input)?),
        FieldType::U16 => Value::U16(u16::from_le_bytes(read_array(input)?)),
        FieldType::U32 => Value::U32(u32::from_le_bytes(read_array(input)?)),
        FieldType::U64 => Value::U64(u64::from_le_bytes(read_array(input)?)),
        FieldType::F32 => Value::F32(f32::from_le_bytes(read_array(input)?)),
        FieldType::F64 => Value::F64(f64::from_le_bytes(read_array(input)?)),
        FieldType::Bool => Value::Bool(get_u8(input)? != 0),
        FieldType::Text => Value::Text(get_text(input)?),
        FieldType::Bytes => Value::Bytes(get_bytes(input)?),
        FieldType::Uuid => Value::Uuid(read_array(input)?),
    })
}

pub fn put_text(buffer: &mut Vec<u8>, value: &str) {
    buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buffer.extend_from_slice(value.as_bytes());
}

pub fn put_bytes(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buffer.extend_from_slice(value);
}

pub fn get_text(input: &mut std::io::Cursor<&[u8]>) -> Result<String> {
    let bytes = get_bytes(input)?;
    String::from_utf8(bytes).map_err(|_| Error::Protocol {
        reason: "text field is not valid UTF-8".to_string(),
    })
}

pub fn get_bytes(input: &mut std::io::Cursor<&[u8]>) -> Result<Vec<u8>> {
    let length = get_u32(input)? as usize;
    if length > MAX_FRAME {
        return Err(Error::Protocol {
            reason: "byte field is too large".to_string(),
        });
    }
    let start = input.position() as usize;
    let bytes = input.get_ref();
    if start + length > bytes.len() {
        return Err(Error::Protocol {
            reason: "truncated byte field".to_string(),
        });
    }
    let value = bytes[start..start + length].to_vec();
    input.set_position((start + length) as u64);
    Ok(value)
}

pub fn put_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

pub fn get_u32(input: &mut std::io::Cursor<&[u8]>) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array(input)?))
}

pub fn get_u64(input: &mut std::io::Cursor<&[u8]>) -> Result<u64> {
    Ok(u64::from_le_bytes(read_array(input)?))
}

fn get_u8(input: &mut std::io::Cursor<&[u8]>) -> Result<u8> {
    Ok(read_array::<1>(input)?[0])
}

fn read_array<const N: usize>(input: &mut std::io::Cursor<&[u8]>) -> Result<[u8; N]> {
    let start = input.position() as usize;
    let bytes = input.get_ref();
    if start + N > bytes.len() {
        return Err(Error::Protocol {
            reason: "truncated wire value".to_string(),
        });
    }
    let mut array = [0u8; N];
    array.copy_from_slice(&bytes[start..start + N]);
    input.set_position((start + N) as u64);
    Ok(array)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Operation,
    Begin,
    Commit,
    Rollback,
    WalMeta,
    WalChunk,
    Checkpoint,
}

impl RequestKind {
    pub fn tag(self) -> u8 {
        match self {
            Self::Operation => 0,
            Self::Begin => 1,
            Self::Commit => 2,
            Self::Rollback => 3,
            Self::WalMeta => 4,
            Self::WalChunk => 5,
            Self::Checkpoint => 6,
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => Self::Operation,
            1 => Self::Begin,
            2 => Self::Commit,
            3 => Self::Rollback,
            4 => Self::WalMeta,
            5 => Self::WalChunk,
            6 => Self::Checkpoint,
            _ => return None,
        })
    }

    pub fn carries_payload(self) -> bool {
        matches!(self, Self::Operation | Self::WalMeta | Self::WalChunk)
    }
}

pub fn request(
    stream: &mut Stream,
    kind: RequestKind,
    index: u32,
    params: &[Value],
) -> Result<Vec<Value>> {
    write_frame(stream, &encode_request(kind, index, params))?;
    let response = read_frame(stream)?;
    decode_response(&response)
}

pub fn pipeline(
    stream: &mut Stream,
    requests: &[(RequestKind, u32, Vec<Value>)],
) -> Result<Vec<Result<Vec<Value>, Error>>> {
    for (kind, index, params) in requests {
        write_frame(stream, &encode_request(*kind, *index, params))?;
    }
    let mut out = Vec::with_capacity(requests.len());
    for _ in 0..requests.len() {
        let frame = read_frame(stream)?;
        out.push(decode_response(&frame));
    }
    Ok(out)
}

pub fn read_request(stream: &mut Stream) -> Result<(RequestKind, u32, Vec<Value>)> {
    let frame = read_frame(stream)?;
    let mut input = std::io::Cursor::new(frame.as_slice());
    let tag = get_u8(&mut input)?;
    let kind = RequestKind::from_tag(tag).ok_or_else(|| Error::Protocol {
        reason: format!("unknown request kind {tag}"),
    })?;
    if kind.carries_payload() {
        let index = get_u32(&mut input)?;
        let params = decode_values(&mut input)?;
        Ok((kind, index, params))
    } else {
        Ok((kind, 0, Vec::new()))
    }
}

pub fn write_response_ok(stream: &mut Stream, values: &[Value]) -> Result<()> {
    let mut payload = vec![0u8];
    encode_values(values, &mut payload);
    write_frame(stream, &payload)
}

pub fn write_response_error(stream: &mut Stream, error: &Error) -> Result<()> {
    let mut payload = vec![1u8];
    put_error(&mut payload, error);
    write_frame(stream, &payload)
}

pub fn put_error(buffer: &mut Vec<u8>, error: &Error) {
    buffer.extend_from_slice(&error.code().as_u16().to_le_bytes());
    match error.remote_detail() {
        None => buffer.push(0),
        Some(RemoteDetail::Conflict { table }) => {
            buffer.push(1);
            put_text(buffer, &table);
        }
        Some(RemoteDetail::DuplicateKey { table }) => {
            buffer.push(2);
            put_text(buffer, &table);
        }
        Some(RemoteDetail::Quota { kind, limit }) => {
            buffer.push(3);
            buffer.push(match kind {
                QuotaKind::Rows => 0,
                QuotaKind::WalBytes => 1,
                QuotaKind::Connections => 2,
                QuotaKind::Transactions => 3,
            });
            buffer.extend_from_slice(&limit.to_le_bytes());
        }
        Some(RemoteDetail::Schema { table, field }) => {
            buffer.push(4);
            buffer.push(table.is_some() as u8);
            if let Some(table) = &table {
                put_text(buffer, table);
            }
            buffer.push(field.is_some() as u8);
            if let Some(field) = &field {
                put_text(buffer, field);
            }
        }
        Some(RemoteDetail::Auth) => buffer.push(5),
    }
    put_text(buffer, &error.to_string());
}

pub fn get_error(input: &mut std::io::Cursor<&[u8]>) -> Result<Error> {
    let code = ErrorCode::from_u16(u16::from_le_bytes(read_array(input)?));
    let tag = get_u8(input)?;
    let detail = match tag {
        1 => Some(RemoteDetail::Conflict {
            table: get_text(input)?,
        }),
        2 => Some(RemoteDetail::DuplicateKey {
            table: get_text(input)?,
        }),
        3 => {
            let kind = match get_u8(input)? {
                0 => QuotaKind::Rows,
                1 => QuotaKind::WalBytes,
                2 => QuotaKind::Connections,
                3 => QuotaKind::Transactions,
                _ => {
                    return Err(Error::Protocol {
                        reason: "unknown quota kind".to_string(),
                    })
                }
            };
            Some(RemoteDetail::Quota {
                kind,
                limit: get_u64(input)?,
            })
        }
        4 => {
            let table = (get_u8(input)? != 0).then(|| get_text(input)).transpose()?;
            let field = (get_u8(input)? != 0).then(|| get_text(input)).transpose()?;
            Some(RemoteDetail::Schema { table, field })
        }
        5 => Some(RemoteDetail::Auth),
        0 => None,
        other => {
            return Err(Error::Protocol {
                reason: format!("unknown remote error detail tag {other}"),
            })
        }
    };
    let message = get_text(input)?;
    Ok(match (code, detail) {
        (Some(code), Some(detail)) => Error::remote_with_detail(code, message, detail),
        (Some(code), None) => crate::error::reconstruct_remote(code, message, None),
        (None, _) => Error::Protocol {
            reason: format!("unknown remote error code: {message}"),
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRole {
    Reader,
    Writer,
    Admin,
}

impl SessionRole {
    pub fn can_write(self) -> bool {
        !matches!(self, Self::Reader)
    }

    fn from_role(role: Role) -> Self {
        match role {
            Role::Reader => Self::Reader,
            Role::Writer => Self::Writer,
            Role::Admin => Self::Admin,
        }
    }
}

pub fn encode_hello(
    workload: u64,
    credentials: &ClientCredentials,
    database: &str,
) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(MAX_HANDSHAKE);
    payload.extend_from_slice(&MAGIC);
    payload.extend_from_slice(&VERSION.to_le_bytes());
    payload.extend_from_slice(&workload.to_le_bytes());
    put_text(&mut payload, database);
    put_text(&mut payload, &credentials.id);
    put_text(&mut payload, credentials.secret.expose());
    if payload.len() > MAX_HANDSHAKE {
        return Err(Error::InvalidArgument {
            name: "credentials".to_string(),
            reason: "handshake exceeds 64 KiB".to_string(),
        });
    }
    Ok(payload)
}

pub fn decode_handshake_response(frame: &[u8]) -> Result<()> {
    let mut input = std::io::Cursor::new(frame);
    if get_u8(&mut input)? == 0 {
        return Ok(());
    }
    Err(get_error(&mut input)?)
}

pub fn encode_request(kind: RequestKind, index: u32, params: &[Value]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(kind.tag());
    if kind.carries_payload() {
        put_u32(&mut payload, index);
        encode_values(params, &mut payload);
    }
    payload
}

pub fn decode_response(frame: &[u8]) -> Result<Vec<Value>> {
    let mut input = std::io::Cursor::new(frame);
    let status = get_u8(&mut input)?;
    if status == 0 {
        return decode_values(&mut input);
    }
    Err(get_error(&mut input)?)
}

pub fn client_handshake(
    stream: &mut Stream,
    workload: u64,
    credentials: &ClientCredentials,
    database: &str,
) -> Result<()> {
    let payload = encode_hello(workload, credentials, database)?;
    write_frame(stream, &payload)?;
    let response = read_frame(stream)?;
    decode_handshake_response(&response)
}

pub struct Hello {
    pub workload: u64,
    pub database: String,
    pub id: String,
    pub secret: Secret,
    pub version: u32,
}

pub fn read_hello(stream: &mut Stream) -> Result<Hello> {
    let frame = read_frame(stream)?;
    if frame.len() > MAX_HANDSHAKE {
        return Err(Error::Protocol {
            reason: "handshake exceeds 64 KiB".to_string(),
        });
    }
    let mut input = std::io::Cursor::new(frame.as_slice());
    if read_array::<4>(&mut input)? != MAGIC {
        return Err(Error::Protocol {
            reason: "bad handshake magic".to_string(),
        });
    }
    let version = u32::from_le_bytes(read_array(&mut input)?);
    if version != VERSION && version != MIN_VERSION {
        return Err(Error::Protocol {
            reason: format!("unsupported protocol version {version}"),
        });
    }
    let workload = u64::from_le_bytes(read_array(&mut input)?);
    let database = get_text(&mut input)?;
    let id = get_text(&mut input)?;
    let secret = Secret::new(get_text(&mut input)?);
    if input.position() != input.get_ref().len() as u64 {
        return Err(Error::Protocol {
            reason: "handshake has trailing bytes".to_string(),
        });
    }
    Ok(Hello {
        workload,
        database,
        id,
        secret,
        version,
    })
}

pub fn authenticate(
    hello: &Hello,
    workload: u64,
    credentials: &Credentials,
) -> Result<SessionRole> {
    if hello.workload != workload {
        return Err(Error::Auth(crate::error::AuthError::Failed));
    }
    let Some(credential) = credentials.get(&hello.id) else {
        return Err(Error::Auth(crate::error::AuthError::Failed));
    };
    if !credential.secret.ct_eq(&hello.secret) {
        return Err(Error::Auth(crate::error::AuthError::Failed));
    }
    Ok(SessionRole::from_role(credential.role))
}

pub fn server_handshake(
    stream: &mut Stream,
    workload: u64,
    credentials: &Credentials,
) -> Result<(SessionRole, u32)> {
    let hello = read_hello(stream)?;
    let role = authenticate(&hello, workload, credentials)?;
    Ok((role, hello.version))
}

pub fn write_handshake_error(stream: &mut Stream, error: &Error) -> Result<()> {
    let mut payload = vec![1u8];
    put_error(&mut payload, error);
    write_frame(stream, &payload)
}

pub fn write_handshake_ok(stream: &mut Stream) -> Result<()> {
    write_frame(stream, &[0u8])
}

pub fn load_credentials(source: &CredentialsSource) -> Result<Credentials> {
    match source {
        CredentialsSource::Static(credentials) => Ok(credentials.clone()),
        CredentialsSource::File(path) => Credentials::load(path),
    }
}

pub fn credential_debug(credential: &Credential) -> String {
    format!("{} ({:?})", credential.id, credential.role)
}

pub fn allows_peer(access: NetworkAccess, allow: &[IpNet], stream: &Stream) -> bool {
    match stream.peer_addr() {
        None => true,
        Some(address) => {
            access.allows(address.ip())
                && (allow.is_empty() || allow.iter().any(|net| net.contains(address.ip())))
        }
    }
}

pub fn peer_ip(stream: &Stream) -> Option<IpAddr> {
    stream.peer_addr().map(|address| address.ip())
}

pub fn param(values: &[Value], index: usize) -> Result<&Value> {
    values.get(index).ok_or_else(|| Error::Protocol {
        reason: format!("request is missing parameter {index}"),
    })
}

macro_rules! expect_value {
    ($name:ident, $variant:ident, $ty:ty) => {
        pub fn $name(value: &Value) -> Result<$ty> {
            match value {
                Value::$variant(value) => Ok(*value),
                other => Err(Error::Protocol {
                    reason: format!(
                        "expected {} but the wire value is {}",
                        stringify!($variant),
                        other.field_type()
                    ),
                }),
            }
        }
    };
}

expect_value!(as_i8, I8, i8);
expect_value!(as_i16, I16, i16);
expect_value!(as_i32, I32, i32);
expect_value!(as_i64, I64, i64);
expect_value!(as_u8, U8, u8);
expect_value!(as_u16, U16, u16);
expect_value!(as_u32, U32, u32);
expect_value!(as_u64, U64, u64);
expect_value!(as_f32, F32, f32);
expect_value!(as_f64, F64, f64);
expect_value!(as_bool, Bool, bool);
expect_value!(as_uuid, Uuid, [u8; 16]);

pub fn as_text(value: &Value) -> Result<String> {
    match value {
        Value::Text(value) => Ok(value.clone()),
        other => Err(Error::Protocol {
            reason: format!("expected Text but the wire value is {}", other.field_type()),
        }),
    }
}

pub fn as_bytes(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Bytes(value) => Ok(value.clone()),
        other => Err(Error::Protocol {
            reason: format!(
                "expected Bytes but the wire value is {}",
                other.field_type()
            ),
        }),
    }
}

pub fn encode_rows(rows: &[Vec<Value>]) -> Vec<Value> {
    let mut values = Vec::with_capacity(1 + rows.iter().map(Vec::len).sum::<usize>());
    values.push(Value::U32(rows.len() as u32));
    for row in rows {
        values.extend(row.iter().cloned());
    }
    values
}

pub fn decode_rows(values: Vec<Value>, width: usize) -> Result<Vec<Vec<Value>>> {
    let mut iter = values.into_iter();
    let count = match iter.next() {
        Some(Value::U32(count)) => count as usize,
        _ => {
            return Err(Error::Protocol {
                reason: "row response has no count".to_string(),
            })
        }
    };
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        let mut row = Vec::with_capacity(width);
        for _ in 0..width {
            row.push(iter.next().ok_or_else(|| Error::Protocol {
                reason: "row response ended early".to_string(),
            })?);
        }
        rows.push(row);
    }
    Ok(rows)
}
