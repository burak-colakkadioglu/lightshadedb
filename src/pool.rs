use crate::error::{Error, Result};
use crate::net::ClientEndpoint;
use crate::secret::ClientCredentials;
use crate::table::Value;
use crate::wire::{self, RequestKind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct PoolOptions {
    pub credentials: ClientCredentials,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_connections: usize,
    pub max_retries: u32,
}

impl PoolOptions {
    pub fn new(credentials: ClientCredentials) -> Self {
        Self {
            credentials,
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_secs(30),
            max_connections: 16,
            max_retries: 3,
        }
    }
}

fn internal_error(reason: impl Into<String>) -> Error {
    Error::Internal {
        reason: reason.into(),
    }
}

#[derive(Clone, Debug)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

#[derive(Debug)]
struct PoolInner {
    endpoint: ClientEndpoint,
    workload: u64,
    options: PoolOptions,
    semaphore: Arc<tokio::sync::Semaphore>,
    idle: Mutex<Vec<AsyncConn>>,
}

impl Pool {
    pub async fn new(
        endpoint: ClientEndpoint,
        workload: u64,
        options: PoolOptions,
    ) -> Result<Self> {
        if options.max_connections == 0 {
            return Err(Error::InvalidArgument {
                name: "max_connections".to_string(),
                reason: "the pool needs at least one connection".to_string(),
            });
        }
        let pool = Self {
            inner: Arc::new(PoolInner {
                endpoint,
                workload,
                options: options.clone(),
                semaphore: Arc::new(tokio::sync::Semaphore::new(options.max_connections)),
                idle: Mutex::new(Vec::new()),
            }),
        };
        let _ = pool.get().await?;
        Ok(pool)
    }

    pub fn endpoint(&self) -> &ClientEndpoint {
        &self.inner.endpoint
    }

    pub async fn request(&self, index: u32, params: &[Value]) -> Result<Vec<Value>> {
        let mut attempts = 0;
        loop {
            let pooled = self.get().await?;
            match pooled.request(RequestKind::Operation, index, params).await {
                Ok(values) => return Ok(values),
                Err(error) if error.is_retryable() && attempts < self.inner.options.max_retries => {
                    attempts += 1;
                    drop(pooled);
                    if let Some(delay) = error.retry_after() {
                        tokio::time::sleep(delay).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn get(&self) -> Result<Pooled> {
        let permit = self
            .inner
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| internal_error("connection pool shut down".to_string()))?;
        let conn = self
            .inner
            .idle
            .lock()
            .map_err(|_| internal_error("connection pool lock failed".to_string()))?
            .pop();
        let conn = match conn {
            Some(conn) => conn,
            None => self.dial().await?,
        };
        Ok(Pooled {
            conn: tokio::sync::Mutex::new(Some(conn)),
            pool: self.clone(),
            _permit: permit,
        })
    }

    async fn dial(&self) -> Result<AsyncConn> {
        connect_async(
            &self.inner.endpoint,
            &self.inner.options.credentials,
            self.inner.workload,
            &self.inner.options,
        )
        .await
    }
}

pub struct Pooled {
    conn: tokio::sync::Mutex<Option<AsyncConn>>,
    pool: Pool,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Pooled {
    pub fn defunct(&self) {
        if let Ok(mut slot) = self.conn.try_lock() {
            slot.take();
        }
    }
    pub async fn request(
        &self,
        kind: RequestKind,
        index: u32,
        params: &[Value],
    ) -> Result<Vec<Value>> {
        let mut slot = self.conn.lock().await;
        let Some(conn) = slot.as_mut() else {
            return Err(internal_error("pooled connection is defunct".to_string()));
        };
        let payload = wire::encode_request(kind, index, params);
        let frame = match tokio::time::timeout(
            self.pool.inner.options.io_timeout,
            conn.round_trip(&payload),
        )
        .await
        {
            Ok(Ok(frame)) => frame,
            Ok(Err(error)) => {
                slot.take();
                return Err(error);
            }
            Err(_) => {
                slot.take();
                return Err(Error::io(
                    "request timed out",
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "request timed out"),
                ));
            }
        };
        wire::decode_response(&frame)
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.conn.try_lock() {
            if let Some(conn) = slot.take() {
                if let Ok(mut idle) = self.pool.inner.idle.try_lock() {
                    idle.push(conn);
                }
            }
        }
    }
}

#[derive(Debug)]
struct AsyncConn {
    stream: AsyncStream,
}

#[derive(Debug)]
enum AsyncStream {
    Tcp(tokio::net::TcpStream),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

impl AsyncConn {
    async fn round_trip(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        write_frame_async(&mut self.stream, payload).await?;
        read_frame_async(&mut self.stream).await
    }
}

fn writer(stream: &mut AsyncStream) -> &mut (dyn tokio::io::AsyncWrite + Unpin + Send) {
    match stream {
        AsyncStream::Tcp(stream) => stream,
        #[cfg(unix)]
        AsyncStream::Unix(stream) => stream,
    }
}

fn reader(stream: &mut AsyncStream) -> &mut (dyn tokio::io::AsyncRead + Unpin + Send) {
    match stream {
        AsyncStream::Tcp(stream) => stream,
        #[cfg(unix)]
        AsyncStream::Unix(stream) => stream,
    }
}

async fn write_frame_async(stream: &mut AsyncStream, payload: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;
    if payload.len() > wire::MAX_FRAME {
        return Err(Error::Protocol {
            reason: "frame exceeds the 256 MiB limit".to_string(),
        });
    }
    let writer = writer(stream);
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .await
        .map_err(|source| Error::io("writing a wire frame", source))?;
    writer
        .write_all(payload)
        .await
        .map_err(|source| Error::io("writing a wire frame", source))?;
    writer
        .flush()
        .await
        .map_err(|source| Error::io("writing a wire frame", source))
}

async fn read_frame_async(stream: &mut AsyncStream) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt as _;
    let mut header = [0u8; 4];
    reader(stream)
        .read_exact(&mut header)
        .await
        .map_err(|source| Error::io("reading a wire frame length", source))?;
    let length = u32::from_le_bytes(header) as usize;
    if length > wire::MAX_FRAME {
        return Err(Error::Protocol {
            reason: "frame exceeds the 256 MiB limit".to_string(),
        });
    }
    let mut payload = vec![0u8; length];
    reader(stream)
        .read_exact(&mut payload)
        .await
        .map_err(|source| Error::io("reading a wire frame", source))?;
    Ok(payload)
}

async fn connect_async(
    endpoint: &ClientEndpoint,
    credentials: &ClientCredentials,
    workload: u64,
    options: &PoolOptions,
) -> Result<AsyncConn> {
    match endpoint {
        ClientEndpoint::Tcp {
            host,
            port,
            database,
        } => {
            let mut last = None;
            let addresses = tokio::net::lookup_host((host.as_str(), *port))
                .await
                .map_err(|source| Error::io("resolving the server hostname", source))?;
            for address in addresses {
                let dialed = tokio::time::timeout(
                    options.connect_timeout,
                    tokio::net::TcpStream::connect(address),
                )
                .await;
                match dialed {
                    Ok(Ok(tcp)) => {
                        tcp.set_nodelay(true).ok();
                        let mut stream = AsyncStream::Tcp(tcp);
                        handshake_async(
                            &mut stream,
                            workload,
                            credentials,
                            database,
                            options.io_timeout,
                        )
                        .await?;
                        return Ok(AsyncConn { stream });
                    }
                    Ok(Err(error)) => last = Some(error),
                    Err(_) => {
                        last = Some(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "connection timed out",
                        ));
                    }
                }
            }
            Err(Error::io(
                "connecting to the Lightshade server",
                last.unwrap_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        "hostname resolved to no addresses",
                    )
                }),
            ))
        }
        ClientEndpoint::Path(path) => {
            connect_path_async(path, credentials, workload, options).await
        }
    }
}

async fn connect_path_async(
    path: &std::path::Path,
    credentials: &ClientCredentials,
    workload: u64,
    options: &PoolOptions,
) -> Result<AsyncConn> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|source| Error::io("reading the endpoint path", source))?;
        if metadata.file_type().is_socket() {
            let dialed = tokio::time::timeout(
                options.connect_timeout,
                tokio::net::UnixStream::connect(path),
            )
            .await
            .map_err(|_| {
                Error::io(
                    "connecting to the Unix socket",
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timed out"),
                )
            })?
            .map_err(|source| Error::io("connecting to the Unix socket", source))?;
            let mut stream = AsyncStream::Unix(dialed);
            handshake_async(
                &mut stream,
                workload,
                credentials,
                crate::net::DEFAULT_DATABASE,
                options.io_timeout,
            )
            .await?;
            return Ok(AsyncConn { stream });
        }
        if metadata.is_file() {
            let text = tokio::fs::read_to_string(path)
                .await
                .map_err(|source| Error::io("reading the endpoint file", source))?;
            let endpoint = ClientEndpoint::parse(text.trim())?;
            return Box::pin(connect_async(&endpoint, credentials, workload, options)).await;
        }
        return Err(Error::InvalidArgument {
            name: "endpoint".to_string(),
            reason: format!("`{}` is not a socket or endpoint file", path.display()),
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (path, credentials, workload, options);
        Err(Error::Unsupported {
            feature: "unix-sockets",
        })
    }
}

async fn handshake_async(
    stream: &mut AsyncStream,
    workload: u64,
    credentials: &ClientCredentials,
    database: &str,
    io_timeout: Duration,
) -> Result<()> {
    let payload = wire::encode_hello(workload, credentials, database)?;
    let round_trip = async {
        write_frame_async(stream, &payload).await?;
        read_frame_async(stream).await
    };
    let frame = tokio::time::timeout(io_timeout, round_trip)
        .await
        .map_err(|_| {
            Error::io(
                "handshake timed out",
                std::io::Error::new(std::io::ErrorKind::TimedOut, "handshake timed out"),
            )
        })??;
    wire::decode_handshake_response(&frame)
}
