use super::{argument_expr, line};
use crate::ir::{Body, Returns, Ty, Workload};

const SERVER_AUTH_RUNTIME: &str = include_str!("server_auth.runtime");

pub(crate) fn emit_protocol(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(out, r#"const __LS_PROTOCOL_MAGIC: [u8; 4] = *b"LSRP";"#);
    line(out, "const __LS_PROTOCOL_VERSION: u32 = 3;");
    line(out, "const __LS_PROTOCOL_MAX: usize = 256 * 1024 * 1024;");
    line(out, "const __LS_HANDSHAKE_MAX: usize = 64 * 1024;");
    line(out, "");
    line(out, "#[cfg(unix)]");
    line(out, "static __LS_SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);");
    line(out, "");
    line(out, "#[cfg(unix)]");
    line(out, "extern \"C\" fn __ls_shutdown_handler(_: i32) {");
    line(
        out,
        "    __LS_SHUTDOWN.store(true, std::sync::atomic::Ordering::SeqCst);",
    );
    line(out, "}");
    line(out, "");
    line(out, "#[cfg(unix)]");
    line(out, "fn __ls_install_shutdown_handler() {");
    line(
        out,
        "    extern \"C\" { fn signal(__ls_signal: i32, __ls_handler: usize) -> usize; }",
    );
    line(out, "    unsafe {");
    line(out, "        signal(15, __ls_shutdown_handler as usize);");
    line(out, "        signal(2, __ls_shutdown_handler as usize);");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "#[cfg(not(unix))]");
    line(out, "fn __ls_install_shutdown_handler() {}");
    line(out, "");
    line(out, "fn __ls_shutdown_requested() -> bool {");
    line(out, "    #[cfg(unix)]");
    line(out, "    {");
    line(
        out,
        "        return __LS_SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst);",
    );
    line(out, "    }");
    line(out, "    #[cfg(not(unix))]");
    line(out, "    {");
    line(out, "        false");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "#[cfg(unix)]");
    line(out, "enum __LsListener {");
    line(out, "    Unix(std::os::unix::net::UnixListener),");
    line(out, "    Tcp(std::net::TcpListener),");
    line(out, "}");
    line(out, "#[cfg(windows)]");
    line(out, "enum __LsListener {");
    line(out, "    Tcp(std::net::TcpListener),");
    line(out, "}");
    line(out, "enum __LsStream {");
    line(out, "    #[cfg(unix)]");
    line(out, "    Unix(std::os::unix::net::UnixStream),");
    line(out, "    Tcp(std::net::TcpStream),");
    line(out, "}");
    line(out, "impl io::Read for __LsStream {");
    line(
        out,
        "    fn read(&mut self, __ls_buffer: &mut [u8]) -> io::Result<usize> {",
    );
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(
        out,
        "            __LsStream::Unix(__ls_stream) => __ls_stream.read(__ls_buffer),",
    );
    line(
        out,
        "            __LsStream::Tcp(__ls_stream) => __ls_stream.read(__ls_buffer),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "impl io::Write for __LsStream {");
    line(
        out,
        "    fn write(&mut self, __ls_buffer: &[u8]) -> io::Result<usize> {",
    );
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(
        out,
        "            __LsStream::Unix(__ls_stream) => __ls_stream.write(__ls_buffer),",
    );
    line(
        out,
        "            __LsStream::Tcp(__ls_stream) => __ls_stream.write(__ls_buffer),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "    fn flush(&mut self) -> io::Result<()> {");
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(
        out,
        "            __LsStream::Unix(__ls_stream) => __ls_stream.flush(),",
    );
    line(
        out,
        "            __LsStream::Tcp(__ls_stream) => __ls_stream.flush(),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "impl __LsStream {");
    line(
        out,
        "    fn set_read_timeout(&self, __ls_timeout: Option<std::time::Duration>) -> io::Result<()> {",
    );
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(
        out,
        "            __LsStream::Unix(__ls_stream) => __ls_stream.set_read_timeout(__ls_timeout),",
    );
    line(
        out,
        "            __LsStream::Tcp(__ls_stream) => __ls_stream.set_read_timeout(__ls_timeout),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "");
    line(
        out,
        "    fn set_timeouts(&self, __ls_timeout: std::time::Duration) -> io::Result<()> {",
    );
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(out, "            __LsStream::Unix(__ls_stream) => {");
    line(
        out,
        "                let _ = __ls_stream.set_read_timeout(Some(__ls_timeout));",
    );
    line(
        out,
        "                let _ = __ls_stream.set_write_timeout(Some(__ls_timeout));",
    );
    line(out, "                Ok(())");
    line(out, "            }");
    line(out, "            __LsStream::Tcp(__ls_stream) => {");
    line(
        out,
        "                __ls_stream.set_read_timeout(Some(__ls_timeout))?;",
    );
    line(
        out,
        "                __ls_stream.set_write_timeout(Some(__ls_timeout))",
    );
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    out.push_str(SERVER_AUTH_RUNTIME);
    line(out, "");
    line(out, "fn __ls_cli_loopback(__ls_address: &str) -> bool {");
    line(
        out,
        "    let __ls_host = match __ls_address.rsplit_once(':') {",
    );
    line(out, "        Some((__ls_host, _)) => __ls_host,");
    line(out, "        None => __ls_address,");
    line(out, "    };");
    line(
        out,
        "    matches!(__ls_host, \"127.0.0.1\" | \"localhost\" | \"[::1]\" | \"::1\")",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_remove_endpoint(__ls_socket: &Path) -> io::Result<()> {",
    );
    line(out, "    match std::fs::symlink_metadata(__ls_socket) {");
    line(out, "        Ok(__ls_metadata) if __ls_metadata.file_type().is_dir() => Err(io::Error::other(\"server endpoint path is a directory\")),");
    line(out, "        Ok(_) => std::fs::remove_file(__ls_socket),");
    line(
        out,
        "        Err(__ls_error) if __ls_error.kind() == io::ErrorKind::NotFound => Ok(()),",
    );
    line(out, "        Err(__ls_error) => Err(__ls_error),");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_store_endpoint(__ls_socket: &Path, __ls_text: &str) -> io::Result<()> {",
    );
    line(out, "    __ls_cli_remove_endpoint(__ls_socket)?;");
    line(
        out,
        "    let mut __ls_file = __ls_private_options().write(true).create_new(true).open(__ls_socket)?;",
    );
    line(out, "    __ls_file.write_all(__ls_text.as_bytes())?;");
    line(out, "    __ls_file.sync_all()?;");
    line(out, "    __ls_sync_parent(__ls_socket)");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_read_endpoint(__ls_socket: &Path) -> Option<String> {",
    );
    line(
        out,
        "    let __ls_metadata = std::fs::symlink_metadata(__ls_socket).ok()?;",
    );
    line(
        out,
        "    if __ls_metadata.file_type().is_symlink() || !__ls_metadata.file_type().is_file() { return None; }",
    );
    line(out, "    #[cfg(unix)]");
    line(out, "    {");
    line(out, "        use std::os::unix::fs::PermissionsExt;");
    line(
        out,
        "        if __ls_metadata.permissions().mode() & 0o077 != 0 { return None; }",
    );
    line(out, "    }");
    line(out, "    std::fs::read_to_string(__ls_socket).ok()");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_listen(__ls_socket: &Path) -> io::Result<__LsListener> {",
    );
    line(
        out,
        "    let __ls_requested = std::env::var(\"LIGHTSHADE_LISTEN\").ok();",
    );
    line(out, "#[cfg(unix)]");
    line(
        out,
        "    if let Some(__ls_address) = __ls_requested.as_deref() {",
    );
    line(
        out,
        "        let __ls_listener = std::net::TcpListener::bind(__ls_address)?;",
    );
    line(out, "        let __ls_local = __ls_listener.local_addr()?;");
    line(
        out,
        "        __ls_cli_store_endpoint(__ls_socket, &__ls_local.to_string())?;",
    );
    line(out, "        return Ok(__LsListener::Tcp(__ls_listener));");
    line(out, "    }");
    line(out, "#[cfg(unix)]");
    line(out, "    {");
    line(out, "        __ls_cli_remove_endpoint(__ls_socket)?;");
    line(
        out,
        "        let __ls_listener = std::os::unix::net::UnixListener::bind(__ls_socket)?;",
    );
    line(out, "        {");
    line(out, "            use std::os::unix::fs::PermissionsExt;");
    line(out, "            std::fs::set_permissions(__ls_socket, std::fs::Permissions::from_mode(__LS_FILE_MODE))?;");
    line(out, "        }");
    line(out, "        return Ok(__LsListener::Unix(__ls_listener));");
    line(out, "    }");
    line(out, "#[cfg(windows)]");
    line(out, "    {");
    line(
        out,
        "        let __ls_address = __ls_requested.unwrap_or_else(|| \"127.0.0.1:0\".to_string());",
    );
    line(
        out,
        "        let __ls_listener = std::net::TcpListener::bind(&__ls_address)?;",
    );
    line(out, "        let __ls_local = __ls_listener.local_addr()?;");
    line(
        out,
        "        __ls_cli_store_endpoint(__ls_socket, &__ls_local.to_string())?;",
    );
    line(out, "        Ok(__LsListener::Tcp(__ls_listener))");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "impl __LsListener {");
    line(
        out,
        "    fn set_nonblocking(&self, __ls_nonblocking: bool) -> io::Result<()> {",
    );
    line(out, "        match self {");
    line(out, "            #[cfg(unix)]");
    line(
        out,
        "            __LsListener::Unix(__ls_listener) => __ls_listener.set_nonblocking(__ls_nonblocking),",
    );
    line(
        out,
        "            __LsListener::Tcp(__ls_listener) => __ls_listener.set_nonblocking(__ls_nonblocking),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_accept(__ls_listener: &__LsListener) -> io::Result<__LsStream> {",
    );
    line(out, "    match __ls_listener {");
    line(out, "        #[cfg(unix)]");
    line(out, "        __LsListener::Unix(__ls_listener) => {");
    line(
        out,
        "            let (__ls_stream, _) = __ls_listener.accept()?;",
    );
    line(out, "            __ls_stream.set_nonblocking(true)?;");
    line(out, "            Ok(__LsStream::Unix(__ls_stream))");
    line(out, "        }");
    line(out, "        __LsListener::Tcp(__ls_listener) => {");
    line(
        out,
        "            let (__ls_stream, _) = __ls_listener.accept()?;",
    );
    line(out, "            __ls_stream.set_nonblocking(true)?;");
    line(out, "            Ok(__LsStream::Tcp(__ls_stream))");
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_endpoint(__ls_socket: &Path) -> String {");
    line(out, "    __ls_cli_read_endpoint(__ls_socket)");
    line(
        out,
        "        .map(|__ls_text| __ls_text.trim().to_string())",
    );
    line(
        out,
        "        .filter(|__ls_text| __ls_text.parse::<std::net::SocketAddr>().is_ok())",
    );
    line(
        out,
        "        .unwrap_or_else(|| __ls_socket.display().to_string())",
    );
    line(out, "}");
    line(out, "");
    let table_index = |name: &str| -> usize {
        workload
            .tables
            .iter()
            .position(|table| table.name == name)
            .expect("validated workload table")
    };
    let mut operation_reads = Vec::new();
    let mut operation_writes = Vec::new();
    for operation in &workload.operations {
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        match &operation.body {
            Body::Query { table, joins, .. } => {
                reads.push(table_index(table));
                for join in joins {
                    reads.push(table_index(&join.table));
                }
            }
            Body::Update { table, .. } | Body::Delete { table, .. } => {
                let index = table_index(table);
                reads.push(index);
                writes.push(index);
            }
            Body::Insert { table } => {
                let index = table_index(table);
                reads.push(index);
                writes.push(index);
            }
        }
        reads.sort_unstable();
        reads.dedup();
        writes.sort_unstable();
        writes.dedup();
        operation_reads.push(reads);
        operation_writes.push(writes);
    }

    line(
        out,
        "fn __ls_cli_operation_reads(__ls_index: u32) -> &'static [usize] {",
    );
    line(out, "    match __ls_index {");
    for (index, reads) in operation_reads.iter().enumerate() {
        line(
            out,
            &format!(
                "        {index} => &[{}],",
                reads
                    .iter()
                    .map(|table| table.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    line(out, "        _ => &[],");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_operation_writes(__ls_index: u32) -> &'static [usize] {",
    );
    line(out, "    match __ls_index {");
    for (index, writes) in operation_writes.iter().enumerate() {
        line(
            out,
            &format!(
                "        {index} => &[{}],",
                writes
                    .iter()
                    .map(|table| table.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    line(out, "        _ => &[],");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_table_name(__ls_index: usize) -> &'static str {",
    );
    line(out, "    match __ls_index {");
    for (index, table) in workload.tables.iter().enumerate() {
        line(out, &format!("        {index} => {:?},", table.name));
    }
    line(out, "        _ => \"unknown\",");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "struct __LsTransactionGuard {");
    line(out, "    server: std::sync::Arc<__LsServer>,");
    line(out, "}");
    line(out, "impl Drop for __LsTransactionGuard {");
    line(out, "    fn drop(&mut self) {");
    line(
        out,
        "        self.server.transactions.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "struct __LsTransaction {");
    line(out, "    db: Db,");
    line(out, "    versions: Vec<u64>,");
    line(out, "    reads: Vec<bool>,");
    line(out, "    writes: Vec<bool>,");
    line(out, "    base_rows: u64,");
    line(out, "    _quota: __LsTransactionGuard,");
    line(out, "}");
    line(out, "");
    line(out, "impl __LsTransaction {");
    line(
        out,
        "    fn new(db: Db, versions: Vec<u64>, base_rows: u64, quota: __LsTransactionGuard) -> Self { let __ls_tables = versions.len(); Self { db, versions, reads: vec![false; __ls_tables], writes: vec![false; __ls_tables], base_rows, _quota: quota } }",
    );
    line(
        out,
        "    fn mark(&mut self, __ls_index: u32, __ls_changed: bool) {",
    );
    line(
        out,
        "        for &__ls_table in __ls_cli_operation_reads(__ls_index) { self.reads[__ls_table] = true; }",
    );
    line(out, "        if __ls_changed {");
    line(
        out,
        "            for &__ls_table in __ls_cli_operation_writes(__ls_index) { self.writes[__ls_table] = true; }",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_conflict(__ls_transaction: &__LsTransaction, __ls_db: &Db) -> Option<String> {",
    );
    line(
        out,
        "    if !__ls_transaction.writes.iter().any(|__ls_wrote| *__ls_wrote) { return None; }",
    );
    line(
        out,
        "    for (__ls_index, (&__ls_snapshot, &__ls_current)) in __ls_transaction.versions.iter().zip(__ls_db.__ls_versions.iter()).enumerate() {",
    );
    line(
        out,
        "        if (__ls_transaction.reads[__ls_index] || __ls_transaction.writes[__ls_index]) && __ls_snapshot != __ls_current {",
    );
    line(
        out,
        "            return Some(format!(\"write conflict on table `{}`; transaction rolled back\", __ls_cli_table_name(__ls_index)));",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "    None");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_bump_tables(__ls_db: &mut Db, __ls_tables: &[usize]) {",
    );
    line(
        out,
        "    for &__ls_table in __ls_tables { if let Some(__ls_version) = __ls_db.__ls_versions.get_mut(__ls_table) { *__ls_version = __ls_version.wrapping_add(1); } }",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_bump_writes(__ls_db: &mut Db, __ls_writes: &[bool]) {",
    );
    line(
        out,
        "    for (__ls_table, __ls_wrote) in __ls_writes.iter().enumerate() { if *__ls_wrote { if let Some(__ls_version) = __ls_db.__ls_versions.get_mut(__ls_table) { *__ls_version = __ls_version.wrapping_add(1); } } }",
    );
    line(out, "}");
    line(out, "");
    line(out, "struct __LsDurable {");
    line(out, "    lsn: u64,");
    line(out, "    syncing: bool,");
    line(out, "}");
    line(out, "");
    line(out, "struct __LsServer {");
    line(out, "    db: std::sync::RwLock<Db>,");
    line(out, "    durable: std::sync::Mutex<__LsDurable>,");
    line(out, "    durability_wake: std::sync::Condvar,");
    line(out, "    group_commit: std::time::Duration,");
    line(out, "    connections: std::sync::atomic::AtomicUsize,");
    line(out, "    transactions: std::sync::atomic::AtomicUsize,");
    line(out, "    max_transactions: usize,");
    line(
        out,
        "    credentials: std::sync::Mutex<__LsCredentialStore>,",
    );
    line(out, "    audit: std::sync::Arc<__LsAudit>,");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_assert_db_send_sync() { fn __ls_assert<T: Send + Sync>() {} __ls_assert::<Db>(); }",
    );
    line(out, "");
    line(out, "fn __ls_cli_read_db<'a>(__ls_server: &'a __LsServer, __ls_deadline: std::time::Instant) -> io::Result<std::sync::RwLockReadGuard<'a, Db>> {");
    line(out, "    loop {");
    line(out, "        match __ls_server.db.try_read() {");
    line(out, "            Ok(__ls_db) => return Ok(__ls_db),");
    line(
        out,
        "            Err(std::sync::TryLockError::Poisoned(_)) => {",
    );
    line(
        out,
        "                eprintln!(\"server database state is poisoned; exiting\");",
    );
    line(out, "                std::process::exit(1);");
    line(out, "            }");
    line(
        out,
        "            Err(std::sync::TryLockError::WouldBlock) => {",
    );
    line(
        out,
        "                let __ls_now = std::time::Instant::now();",
    );
    line(out, "                if __ls_now >= __ls_deadline {");
    line(out, "                    return Err(io::Error::new(io::ErrorKind::TimedOut, \"timed out waiting for the database lock\"));");
    line(out, "                }");
    line(out, "                std::thread::yield_now();");
    line(out, "                std::thread::sleep(__ls_deadline.saturating_duration_since(__ls_now).min(std::time::Duration::from_micros(10)));");
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_write_db<'a>(__ls_server: &'a __LsServer, __ls_deadline: std::time::Instant) -> io::Result<std::sync::RwLockWriteGuard<'a, Db>> {");
    line(out, "    loop {");
    line(out, "        match __ls_server.db.try_write() {");
    line(out, "            Ok(__ls_db) => return Ok(__ls_db),");
    line(
        out,
        "            Err(std::sync::TryLockError::Poisoned(_)) => {",
    );
    line(
        out,
        "                eprintln!(\"server database state is poisoned; exiting\");",
    );
    line(out, "                std::process::exit(1);");
    line(out, "            }");
    line(
        out,
        "            Err(std::sync::TryLockError::WouldBlock) => {",
    );
    line(
        out,
        "                let __ls_now = std::time::Instant::now();",
    );
    line(out, "                if __ls_now >= __ls_deadline {");
    line(out, "                    return Err(io::Error::new(io::ErrorKind::TimedOut, \"timed out waiting for the database lock\"));");
    line(out, "                }");
    line(out, "                std::thread::yield_now();");
    line(out, "                std::thread::sleep(__ls_deadline.saturating_duration_since(__ls_now).min(std::time::Duration::from_micros(10)));");
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_wait_durable(__ls_server: &__LsServer, __ls_lsn: u64, __ls_deadline: std::time::Instant) -> io::Result<()> {",
    );
    line(out, "    loop {");
    line(
        out,
        "        let mut __ls_state = __ls_server.durable.lock().map_err(|_| io::Error::other(\"server durability state is poisoned\"))?;",
    );
    line(out, "        if __ls_state.lsn >= __ls_lsn {");
    line(out, "            return Ok(());");
    line(out, "        }");
    line(out, "        if !__ls_state.syncing {");
    line(out, "            __ls_state.syncing = true;");
    line(out, "            drop(__ls_state);");
    line(
        out,
        "            std::thread::sleep(__ls_server.group_commit);",
    );
    line(
        out,
        "            let __ls_outcome = __ls_cli_read_db(__ls_server, __ls_deadline).map(|__ls_db| {",
    );
    line(
        out,
        "                let __ls_result = __ls_db.__ls_flush_wal();",
    );
    line(out, "                (__ls_result, __ls_db.__ls_lsn)");
    line(out, "            });");
    line(
        out,
        "            let mut __ls_state = __ls_server.durable.lock().map_err(|_| io::Error::other(\"server durability state is poisoned\"))?;",
    );
    line(out, "            __ls_state.syncing = false;");
    line(out, "            match __ls_outcome {");
    line(
        out,
        "                Ok((Ok(__ls_flushed), __ls_written)) => {",
    );
    line(
        out,
        "                    __ls_state.lsn = __ls_state.lsn.max(__ls_written).max(__ls_flushed);",
    );
    line(out, "                }");
    line(out, "                Ok((Err(__ls_error), _)) => {");
    line(out, "                    drop(__ls_state);");
    line(
        out,
        "                    eprintln!(\"WAL sync failed: {__ls_error}; server exiting\");",
    );
    line(out, "                    std::process::exit(1);");
    line(out, "                }");
    line(out, "                Err(__ls_error) => {");
    line(out, "                    drop(__ls_state);");
    line(
        out,
        "                    __ls_server.durability_wake.notify_all();",
    );
    line(out, "                    return Err(__ls_error);");
    line(out, "                }");
    line(out, "            }");
    line(out, "            __ls_server.durability_wake.notify_all();");
    line(out, "            continue;");
    line(out, "        }");
    line(out, "        let __ls_now = std::time::Instant::now();");
    line(out, "        if __ls_now >= __ls_deadline {");
    line(
        out,
        "            return Err(io::Error::new(io::ErrorKind::TimedOut, \"timed out waiting for a durable write\"));",
    );
    line(out, "        }");
    line(
        out,
        "        let (__ls_next, _) = __ls_server.durability_wake.wait_timeout(__ls_state, __ls_deadline - __ls_now).map_err(|_| io::Error::other(\"server durability state is poisoned\"))?;",
    );
    line(out, "        drop(__ls_next);");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "struct __LsConnectionGuard {");
    line(out, "    server: std::sync::Arc<__LsServer>,");
    line(out, "}");
    line(out, "impl Drop for __LsConnectionGuard {");
    line(out, "    fn drop(&mut self) {");
    line(
        out,
        "        self.server.connections.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_idle_timeout() -> std::time::Duration {");
    line(
        out,
        "    let __ls_millis = std::env::var(\"LIGHTSHADE_IDLE_TIMEOUT_MS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<u64>().ok())",
    );
    line(out, "        .filter(|__ls_millis| *__ls_millis > 0)");
    line(out, "        .unwrap_or(30_000);");
    line(out, "    std::time::Duration::from_millis(__ls_millis)");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_handshake_timeout() -> std::time::Duration {",
    );
    line(
        out,
        "    let __ls_millis = std::env::var(\"LIGHTSHADE_HANDSHAKE_TIMEOUT_MS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<u64>().ok())",
    );
    line(out, "        .filter(|__ls_millis| *__ls_millis > 0)");
    line(out, "        .unwrap_or(10_000);");
    line(out, "    std::time::Duration::from_millis(__ls_millis)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_lock_timeout() -> std::time::Duration {");
    line(
        out,
        "    let __ls_millis = std::env::var(\"LIGHTSHADE_LOCK_TIMEOUT_MS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<u64>().ok())",
    );
    line(out, "        .unwrap_or(30_000);");
    line(out, "    std::time::Duration::from_millis(__ls_millis)");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_shutdown_timeout() -> std::time::Duration {",
    );
    line(
        out,
        "    let __ls_millis = std::env::var(\"LIGHTSHADE_SHUTDOWN_TIMEOUT_MS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<u64>().ok())",
    );
    line(out, "        .unwrap_or(10_000);");
    line(out, "    std::time::Duration::from_millis(__ls_millis)");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_group_commit_delay() -> std::time::Duration {",
    );
    line(
        out,
        "    let __ls_micros = std::env::var(\"LIGHTSHADE_GROUP_COMMIT_US\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<u64>().ok())",
    );
    line(out, "        .unwrap_or(1_000)");
    line(out, "        .min(1_000_000);");
    line(out, "    std::time::Duration::from_micros(__ls_micros)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_ok() -> Vec<u8> { vec![0u8] }");
    line(out, "");
    line(out, "fn __ls_cli_error(__ls_message: &str) -> Vec<u8> {");
    line(out, "    let mut __ls_out = vec![1u8];");
    line(out, "    __ls_put_text(&mut __ls_out, __ls_message);");
    line(out, "    __ls_out");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_write_all(__ls_stream: &mut __LsStream, mut __ls_buf: &[u8]) -> io::Result<()> {",
    );
    line(
        out,
        "    let __ls_deadline = std::time::Instant::now() + __ls_cli_idle_timeout();",
    );
    line(out, "    while !__ls_buf.is_empty() {");
    line(out, "        match __ls_stream.write(&__ls_buf) {");
    line(
        out,
        "            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, \"connection closed while writing\")),",
    );
    line(
        out,
        "            Ok(__ls_n) => __ls_buf = &__ls_buf[__ls_n..],",
    );
    line(
        out,
        "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::Interrupted => continue,",
    );
    line(
        out,
        "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::WouldBlock => {",
    );
    line(
        out,
        "                if std::time::Instant::now() >= __ls_deadline {",
    );
    line(
        out,
        "                    return Err(io::Error::new(io::ErrorKind::TimedOut, \"protocol write timed out\"));",
    );
    line(out, "                }");
    line(
        out,
        "                std::thread::sleep(std::time::Duration::from_millis(1));",
    );
    line(out, "            }");
    line(
        out,
        "            Err(__ls_error) => return Err(__ls_error),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_send(__ls_stream: &mut __LsStream, __ls_payload: &[u8]) -> io::Result<()> {",
    );
    line(
        out,
        "    if __ls_payload.len() > __LS_PROTOCOL_MAX || __ls_payload.len() > u32::MAX as usize {",
    );
    line(
        out,
        "        return __ls_cli_send(__ls_stream, &__ls_cli_error(\"response is too large\"));",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_header = (__ls_payload.len() as u32).to_le_bytes();",
    );
    line(out, "    __ls_cli_write_all(__ls_stream, &__ls_header)?;");
    line(out, "    __ls_cli_write_all(__ls_stream, __ls_payload)?;");
    line(out, "    __ls_stream.flush()");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_receive(__ls_stream: &mut __LsStream) -> io::Result<Option<Vec<u8>>> {",
    );
    line(
        out,
        "    __ls_cli_receive_limit(__ls_stream, __LS_PROTOCOL_MAX)",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_read_exact(__ls_stream: &mut __LsStream, __ls_buffer: &mut [u8], __ls_deadline: Option<std::time::Instant>) -> io::Result<()> {",
    );
    line(out, "    let Some(__ls_deadline) = __ls_deadline else { return __ls_stream.read_exact(__ls_buffer); };");
    line(out, "    let mut __ls_offset = 0usize;");
    line(out, "    while __ls_offset < __ls_buffer.len() {");
    line(out, "        let __ls_now = std::time::Instant::now();");
    line(out, "        if __ls_now >= __ls_deadline {");
    line(out, "            return Err(io::Error::new(io::ErrorKind::TimedOut, \"protocol read timed out\"));");
    line(out, "        }");
    line(out, "        let __ls_remaining = __ls_deadline.saturating_duration_since(__ls_now).max(std::time::Duration::from_millis(1));");
    line(
        out,
        "        let _ = __ls_stream.set_read_timeout(Some(__ls_remaining));",
    );
    line(
        out,
        "        match __ls_stream.read(&mut __ls_buffer[__ls_offset..]) {",
    );
    line(out, "            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, \"protocol frame ended early\")),");
    line(
        out,
        "            Ok(__ls_read) => __ls_offset += __ls_read,",
    );
    line(out, "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::Interrupted => continue,");
    line(
        out,
        "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::WouldBlock => {",
    );
    line(
        out,
        "                std::thread::sleep(std::time::Duration::from_millis(1));",
    );
    line(out, "                continue;");
    line(out, "            }");
    line(
        out,
        "            Err(__ls_error) => return Err(__ls_error),",
    );
    line(out, "        }");
    line(out, "    }");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_receive_limit(__ls_stream: &mut __LsStream, __ls_limit: usize) -> io::Result<Option<Vec<u8>>> {",
    );
    line(
        out,
        "    __ls_cli_receive_limit_deadline(__ls_stream, __ls_limit, None)",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_receive_limit_deadline(__ls_stream: &mut __LsStream, __ls_limit: usize, __ls_deadline: Option<std::time::Instant>) -> io::Result<Option<Vec<u8>>> {",
    );
    line(out, "    let mut __ls_header = [0u8; 4];");
    line(
        out,
        "    match __ls_cli_read_exact(__ls_stream, &mut __ls_header, __ls_deadline) {",
    );
    line(out, "        Ok(()) => {}");
    line(out, "        Err(__ls_error) if __ls_error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),");
    line(out, "        Err(__ls_error) => return Err(__ls_error),");
    line(out, "    }");
    line(
        out,
        "    let __ls_length = u32::from_le_bytes(__ls_header) as usize;",
    );
    line(out, "    if __ls_length > __ls_limit {");
    line(out, "        return Err(io::Error::new(io::ErrorKind::InvalidData, \"protocol frame is too large\"));");
    line(out, "    }");
    line(
        out,
        "    let mut __ls_payload = Vec::with_capacity(__ls_length.min(64 * 1024));",
    );
    line(out, "    let mut __ls_chunk = [0u8; 64 * 1024];");
    line(out, "    let mut __ls_remaining = __ls_length;");
    line(out, "    while __ls_remaining > 0 {");
    line(
        out,
        "        let __ls_take = __ls_remaining.min(__ls_chunk.len());",
    );
    line(
        out,
        "        __ls_cli_read_exact(__ls_stream, &mut __ls_chunk[..__ls_take], __ls_deadline)?;",
    );
    line(
        out,
        "        __ls_payload.extend_from_slice(&__ls_chunk[..__ls_take]);",
    );
    line(out, "        __ls_remaining -= __ls_take;");
    line(out, "    }");
    line(out, "    Ok(Some(__ls_payload))");
    line(out, "}");
    line(out, "");
    out.push_str(r#"
pub(crate) fn __ls_cli_serve_handshake(
    __ls_stream: &mut __LsStream,
    __ls_deadline: std::time::Instant,
    __ls_credentials: &std::sync::Mutex<__LsCredentialStore>,
    __ls_audit: &__LsAudit,
    __ls_connection: u64,
) -> io::Result<Option<__LsIdentity>> {
    let __ls_hello = match __ls_cli_receive_limit_deadline(__ls_stream, __LS_HANDSHAKE_MAX, Some(__ls_deadline))? {
        Some(__ls_payload) => __ls_payload,
        None => return Ok(None),
    };
    let mut __ls_reader = __LsReader::new(&__ls_hello);
    let mut __ls_id = String::new();
    let __ls_check = (|| -> Result<__LsIdentity, String> {
        if __ls_reader.take(4).map_err(|error| error.to_string())? != __LS_PROTOCOL_MAGIC {
            return Err("bad protocol magic".to_string());
        }
        if __ls_reader.u32().map_err(|error| error.to_string())? != __LS_PROTOCOL_VERSION {
            return Err("unsupported protocol version".to_string());
        }
        if __ls_reader.u64().map_err(|error| error.to_string())? != __LS_WORKLOAD {
            return Err("server is running a different workload".to_string());
        }
        __ls_id = __ls_reader.text().map_err(|error| error.to_string())?;
        let __ls_secret = __ls_reader.text().map_err(|error| error.to_string())?;
        let mut __ls_store = __ls_cli_credentials_lock(__ls_credentials);
        __ls_store.maybe_reload(__ls_audit);
        let __ls_identity = __ls_cli_authenticate(&__ls_store.credentials, &__ls_id, &__ls_secret)
            .ok_or_else(|| "authentication failed".to_string())?;
        if !__ls_reader.is_empty() {
            return Err("handshake has trailing bytes".to_string());
        }
        Ok(__ls_identity)
    })();
    match __ls_check {
        Ok(__ls_identity) => {
            __ls_cli_send(__ls_stream, &__ls_cli_ok())?;
            __ls_audit.event(
                "auth_success",
                __ls_connection,
                &__ls_identity.id,
                Some(__ls_identity.role),
                "handshake",
            );
            Ok(Some(__ls_identity))
        }
        Err(__ls_message) => {
            __ls_audit.event(
                "auth_failure",
                __ls_connection,
                &__ls_id,
                None,
                if __ls_message == "authentication failed" {
                    "bad_credentials"
                } else {
                    "invalid_handshake"
                },
            );
            __ls_cli_send(__ls_stream, &__ls_cli_error(&__ls_message))?;
            Ok(None)
        }
    }
}

"#);
    line(
        out,
        "fn __ls_cli_serve(__ls_path: &str) -> Result<(), String> {",
    );
    line(
        out,
        "    let __ls_credentials = __LsCredentialStore::new().map_err(|error| format!(\"cannot load credentials: {error}\"))?;",
    );
    line(
        out,
        "    let __ls_tenant = std::env::var(\"LIGHTSHADE_TENANT\").ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).unwrap_or_else(|| Path::new(__ls_path).file_stem().and_then(|name| name.to_str()).unwrap_or(\"default\").to_string());",
    );
    line(
        out,
        "    let __ls_audit = __ls_cli_audit(&__ls_tenant).map_err(|error| format!(\"cannot open audit log: {error}\"))?;",
    );
    line(
        out,
        "    let mut __ls_db = Db::open(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let __ls_initial_lsn = __ls_db.__ls_lsn;");
    line(
        out,
        "    if let Some(__ls_store) = __ls_db.__ls_store.as_mut() { __ls_store.sync_each = false; }",
    );
    line(
        out,
        "    let __ls_socket = __ls_sidecar(Path::new(__ls_path), \".sock\");",
    );
    line(out, "    let __ls_listener = __ls_cli_listen(&__ls_socket).map_err(|error| format!(\"cannot listen on `{}`: {error}\", __ls_socket.display()))?;");
    line(
        out,
        "    __ls_listener.set_nonblocking(true).map_err(|error| format!(\"cannot configure listener: {error}\"))?;",
    );
    line(out, "    __ls_install_shutdown_handler();");
    line(
        out,
        "    println!(\"serving tenant `{}` at {} on {}\", __ls_tenant, __ls_path, __ls_cli_endpoint(&__ls_socket));",
    );
    line(
        out,
        "    let __ls_max_connections = std::env::var(\"LIGHTSHADE_MAX_CONNECTIONS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<usize>().ok())",
    );
    line(out, "        .unwrap_or(128)");
    line(out, "        .max(1);");
    line(
        out,
        "    let __ls_max_transactions = std::env::var(\"LIGHTSHADE_MAX_TRANSACTIONS\")",
    );
    line(out, "        .ok()");
    line(
        out,
        "        .and_then(|__ls_value| __ls_value.parse::<usize>().ok())",
    );
    line(out, "        .unwrap_or(__ls_max_connections);");
    line(
        out,
        "    __ls_audit.event(\"server_start\", 0, \"-\", None, \"listening\");",
    );
    line(out, "    let __ls_server = std::sync::Arc::new(__LsServer { db: std::sync::RwLock::new(__ls_db), durable: std::sync::Mutex::new(__LsDurable { lsn: __ls_initial_lsn, syncing: false }), durability_wake: std::sync::Condvar::new(), group_commit: __ls_cli_group_commit_delay(), connections: std::sync::atomic::AtomicUsize::new(0), transactions: std::sync::atomic::AtomicUsize::new(0), max_transactions: __ls_max_transactions, credentials: std::sync::Mutex::new(__ls_credentials), audit: std::sync::Arc::clone(&__ls_audit) });");
    line(out, "    let mut __ls_next = 1u64;");
    line(out, "    loop {");
    line(out, "        match __ls_cli_accept(&__ls_listener) {");
    line(out, "            Ok(mut __ls_stream) => {");
    line(out, "                if let Err(__ls_error) = __ls_stream.set_timeouts(__ls_cli_idle_timeout()) {");
    line(
        out,
        "                    eprintln!(\"cannot set connection timeouts: {__ls_error}\");",
    );
    line(out, "                    continue;");
    line(out, "                }");
    line(out, "                let __ls_active = __ls_server.connections.fetch_add(1, std::sync::atomic::Ordering::AcqRel);");
    line(
        out,
        "                if __ls_active >= __ls_max_connections {",
    );
    line(out, "                    __ls_server.connections.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);");
    line(out, "                    let _ = __ls_cli_send(&mut __ls_stream, &__ls_cli_error(\"server is busy\"));");
    line(out, "                    continue;");
    line(out, "                }");
    line(
        out,
        "                let __ls_worker = std::sync::Arc::clone(&__ls_server);",
    );
    line(
        out,
        "                let __ls_guard = __LsConnectionGuard { server: std::sync::Arc::clone(&__ls_server) };",
    );
    line(out, "                let __ls_id = __ls_next;");
    line(out, "                __ls_next += 1;");
    line(out, "                let __ls_spawned = std::thread::Builder::new().name(format!(\"lightshade-{__ls_id}\")).spawn(move || {");
    line(out, "                    let _ls_guard = __ls_guard;");
    line(
        out,
        "                    if let Err(__ls_error) = __ls_cli_serve_connection(&mut __ls_stream, &__ls_worker, __ls_id) {",
    );
    line(
        out,
        "                        eprintln!(\"connection {} ended: {__ls_error}\", __ls_id);",
    );
    line(out, "                    }");
    line(out, "                });");
    line(
        out,
        "                if let Err(__ls_error) = __ls_spawned {",
    );
    line(
        out,
        "                    eprintln!(\"cannot start connection worker: {__ls_error}\");",
    );
    line(out, "                }");
    line(out, "            }");
    line(
        out,
        "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::Interrupted => {}",
    );
    line(
        out,
        "            Err(__ls_error) if __ls_error.kind() == io::ErrorKind::WouldBlock => {",
    );
    line(
        out,
        "                if __ls_shutdown_requested() { break; }",
    );
    line(
        out,
        "                std::thread::sleep(std::time::Duration::from_millis(25));",
    );
    line(out, "            }");
    line(out, "            Err(__ls_error) => {");
    line(
        out,
        "                eprintln!(\"accept failed: {__ls_error}\");",
    );
    line(
        out,
        "                std::thread::sleep(std::time::Duration::from_millis(50));",
    );
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(
        out,
        "    let __ls_deadline = std::time::Instant::now() + __ls_cli_shutdown_timeout();",
    );
    line(out, "    while __ls_server.connections.load(std::sync::atomic::Ordering::Acquire) > 0 && std::time::Instant::now() < __ls_deadline {");
    line(
        out,
        "        std::thread::sleep(std::time::Duration::from_millis(25));",
    );
    line(out, "    }");
    line(
        out,
        "    __ls_audit.event(\"server_stop\", 0, \"-\", None, \"shutdown\");",
    );
    line(out, "    let _ = __ls_cli_remove_endpoint(&__ls_socket);");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    out.push_str(
        r#"
pub(crate) fn __ls_cli_serve_connection(__ls_stream: &mut __LsStream, __ls_server: &std::sync::Arc<__LsServer>, __ls_id: u64) -> io::Result<()> {
    let __ls_handshake_deadline = std::time::Instant::now() + __ls_cli_handshake_timeout();
    let __ls_identity = match __ls_cli_serve_handshake(
        __ls_stream,
        __ls_handshake_deadline,
        &__ls_server.credentials,
        &__ls_server.audit,
        __ls_id,
    )? {
        Some(__ls_identity) => __ls_identity,
        None => return Ok(()),
    };
    let _ls_audit_connection = __LsAuditConnection {
        audit: std::sync::Arc::clone(&__ls_server.audit),
        connection: __ls_id,
        id: __ls_identity.id.clone(),
        role: __ls_identity.role,
    };
    __ls_stream.set_timeouts(__ls_cli_idle_timeout())?;
    let mut __ls_transaction: Option<__LsTransaction> = None;
    loop {
        let __ls_deadline =
            std::time::Instant::now() + __ls_cli_idle_timeout();
        let __ls_payload =
            match __ls_cli_receive_limit_deadline(__ls_stream, __LS_PROTOCOL_MAX, Some(__ls_deadline))? {
                Some(__ls_payload) => __ls_payload,
                None => break,
            };
        {
            let mut __ls_store = __ls_cli_credentials_lock(&__ls_server.credentials);
            __ls_store.maybe_reload(&__ls_server.audit);
            if !__ls_store.identity_is_current(&__ls_identity) {
                drop(__ls_store);
                if __ls_transaction.take().is_some() {
                    __ls_server.audit.event(
                        "transaction_rollback",
                        __ls_id,
                        &__ls_identity.id,
                        Some(__ls_identity.role),
                        "authentication_expired",
                    );
                }
                __ls_server.audit.event(
                    "authentication_expired",
                    __ls_id,
                    &__ls_identity.id,
                    Some(__ls_identity.role),
                    "credentials_changed",
                );
                __ls_cli_send(__ls_stream, &__ls_cli_error("authentication expired"))?;
                return Ok(());
            }
        }
        let __ls_kind = __ls_payload.first().copied().unwrap_or(255);
        let __ls_deadline = std::time::Instant::now() + __ls_cli_lock_timeout();
        let __ls_response = match __ls_kind {
            0 => {
                let __ls_read = __ls_cli_request_is_read(&__ls_payload);
                if let Err(__ls_message) = __ls_cli_authorize(&__ls_identity, __ls_read) {
                    __ls_server.audit.event(
                        "permission_denied",
                        __ls_id,
                        &__ls_identity.id,
                        Some(__ls_identity.role),
                        if __ls_read { "read" } else { "write" },
                    );
                    __ls_cli_error(&__ls_message)
                } else if let Some(__ls_transaction) = __ls_transaction.as_mut() {
                    let (__ls_response, __ls_changed) = if __ls_read {
                        (__ls_cli_serve_read(&__ls_transaction.db, &__ls_payload), false)
                    } else {
                        let (__ls_response, _ls_lsn, __ls_changed) = __ls_cli_serve_write(&mut __ls_transaction.db, &__ls_payload);
                        (__ls_response, __ls_changed)
                    };
                    if __ls_response.first() == Some(&0) {
                        let __ls_index = __ls_cli_request_index(&__ls_payload);
                        if let Some(__ls_index) = __ls_index {
                            __ls_transaction.mark(__ls_index, __ls_changed);
                        }
                    }
                    __ls_response
                } else if __ls_read {
                    let __ls_db = __ls_cli_read_db(__ls_server, __ls_deadline)?;
                    __ls_cli_serve_read(&__ls_db, &__ls_payload)
                } else {
                    let __ls_index = __ls_cli_request_index(&__ls_payload);
                    let mut __ls_db = __ls_cli_write_db(__ls_server, __ls_deadline)?;
                    let (__ls_response, __ls_lsn, __ls_changed) = __ls_cli_serve_write(&mut __ls_db, &__ls_payload);
                    if __ls_response.first() == Some(&0) && __ls_changed {
                        if let Some(__ls_index) = __ls_index {
                            __ls_cli_bump_tables(&mut __ls_db, __ls_cli_operation_writes(__ls_index));
                        }
                    }
                    drop(__ls_db);
                    match __ls_cli_wait_durable(__ls_server, __ls_lsn, __ls_deadline) {
                        Ok(()) => __ls_response,
                        Err(__ls_error) => __ls_cli_error(&__ls_error.to_string()),
                    }
                }
            }
            1 => {
                if __ls_transaction.is_some() {
                    __ls_cli_error("transaction already active on this connection")
                } else {
                    let __ls_active = __ls_server.transactions.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    if __ls_server.max_transactions != 0 && __ls_active >= __ls_server.max_transactions {
                        __ls_server.transactions.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                        __ls_cli_error(&format!("transaction quota exceeded: at most {} active transactions", __ls_server.max_transactions))
                    } else {
                        let __ls_quota = __LsTransactionGuard { server: std::sync::Arc::clone(__ls_server) };
                        let __ls_db = __ls_cli_read_db(__ls_server, __ls_deadline)?;
                        let __ls_base_rows = __ls_db.__ls_row_count();
                        let __ls_clone = __ls_db.__ls_clone_committed();
                        let __ls_versions = __ls_db.__ls_versions.clone();
                        drop(__ls_db);
                        match __ls_clone {
                            Ok(__ls_clone) => {
                                __ls_transaction = Some(__LsTransaction::new(__ls_clone, __ls_versions, __ls_base_rows, __ls_quota));
                                __ls_cli_ok()
                            }
                            Err(__ls_error) => __ls_cli_error(&__ls_error.to_string()),
                        }
                    }
                }
            }
            2 => {
                if let Some(mut __ls_transaction) = __ls_transaction.take() {
                    let __ls_effects = match __ls_transaction.db.__ls_take_effects() {
                        Ok(__ls_effects) => __ls_effects,
                        Err(__ls_error) => {
                            __ls_cli_send(__ls_stream, &__ls_cli_error(&__ls_error.to_string()))?;
                            continue;
                        }
                    };
                    if !__ls_transaction.writes.iter().any(|__ls_wrote| *__ls_wrote) {
                        __ls_cli_ok()
                    } else if let Err(__ls_message) = __ls_cli_authorize(&__ls_identity, false) {
                        __ls_server.audit.event(
                            "permission_denied",
                            __ls_id,
                            &__ls_identity.id,
                            Some(__ls_identity.role),
                            "commit",
                        );
                        __ls_cli_error(&__ls_message)
                    } else {
                        let mut __ls_db = __ls_cli_write_db(__ls_server, __ls_deadline)?;
                        if let Some(__ls_conflict) = __ls_cli_conflict(&__ls_transaction, &__ls_db) {
                            drop(__ls_db);
                            __ls_cli_error(&__ls_conflict)
                        } else {
                            let __ls_rows = __ls_db.__ls_row_count()
                                .saturating_add(__ls_transaction.db.__ls_row_count())
                                .saturating_sub(__ls_transaction.base_rows);
                            if let Err(__ls_error) = __ls_db.__ls_check_row_limit(__ls_rows) {
                                drop(__ls_db);
                                __ls_cli_error(&__ls_error.to_string())
                            } else {
                            let __ls_result = __ls_db.__ls_commit_effects(&__ls_effects);
                            if let Some(__ls_message) = __ls_db.__ls_broken_message() {
                                eprintln!("database entered a failed state: {__ls_message}; server exiting");
                                std::process::exit(1);
                            }
                            if __ls_result.is_ok() {
                                __ls_cli_bump_writes(&mut __ls_db, &__ls_transaction.writes);
                            }
                            let __ls_lsn = __ls_db.__ls_lsn;
                            drop(__ls_db);
                            match __ls_result {
                                Ok(()) => match __ls_cli_wait_durable(__ls_server, __ls_lsn, __ls_deadline) {
                                    Ok(()) => __ls_cli_ok(),
                                    Err(__ls_error) => __ls_cli_error(&__ls_error.to_string()),
                                },
                                Err(__ls_error) => __ls_cli_error(&__ls_error.to_string()),
                            }
                            }
                        }
                    }
                } else {
                    __ls_cli_error("no transaction is open on this connection")
                }
            }
            3 => {
                if __ls_transaction.take().is_some() {
                    __ls_cli_ok()
                } else {
                    __ls_cli_error("no transaction is open on this connection")
                }
            }
            _ => __ls_cli_error(&format!("unknown request kind {__ls_kind}")),
        };
        if __ls_response.first() == Some(&0) {
            let __ls_role = Some(__ls_identity.role);
            match __ls_kind {
                0 if !__ls_cli_request_is_read(&__ls_payload) => {
                    let __ls_detail = __ls_cli_request_index(&__ls_payload)
                        .map(__ls_cli_operation_name)
                        .unwrap_or("unknown");
                    __ls_server.audit.event(
                        "write",
                        __ls_id,
                        &__ls_identity.id,
                        __ls_role,
                        __ls_detail,
                    );
                }
                1 => __ls_server.audit.event(
                    "transaction_begin",
                    __ls_id,
                    &__ls_identity.id,
                    __ls_role,
                    "begin",
                ),
                2 => __ls_server.audit.event(
                    "transaction_commit",
                    __ls_id,
                    &__ls_identity.id,
                    __ls_role,
                    "commit",
                ),
                3 => __ls_server.audit.event(
                    "transaction_rollback",
                    __ls_id,
                    &__ls_identity.id,
                    __ls_role,
                    "rollback",
                ),
                _ => {}
            }
        }
        __ls_cli_send(__ls_stream, &__ls_response)?;
    }
    Ok(())
}

"#,
    );
    line(
        out,
        "fn __ls_cli_serve_read(__ls_db: &Db, __ls_payload: &[u8]) -> Vec<u8> {",
    );
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(__ls_payload);",
    );
    line(
        out,
        "    let __ls_result = (|| -> Result<Vec<u8>, String> {",
    );
    line(
        out,
        "        let __ls_kind = __ls_reader.u8().map_err(|error| error.to_string())?;",
    );
    line(out, "        match __ls_kind {");
    line(out, "            0 => {");
    line(
        out,
        "                let __ls_index = __ls_reader.u32().map_err(|error| error.to_string())?;",
    );
    line(out, "                match __ls_index {");

    for (index, operation) in workload.operations.iter().enumerate() {
        if !matches!(&operation.body, Body::Query { .. }) {
            continue;
        }
        line(out, &format!("                    {index} => {{"));
        for (param_index, param) in operation.params.iter().enumerate() {
            let method = match param.ty {
                Ty::I8 => "i8",
                Ty::I16 => "i16",
                Ty::I32 => "i32",
                Ty::I64 => "i64",
                Ty::U8 => "u8",
                Ty::U16 => "u16",
                Ty::U32 => "u32",
                Ty::U64 => "u64",
                Ty::F32 => "f32",
                Ty::F64 => "f64",
                Ty::Bool => "bool",
                Ty::Text => "text",
                Ty::Bytes => "bytes",
                Ty::Uuid => "uuid",
            };
            line(
                out,
                &format!(
                    "                        let __ls_arg_{param_index} = __ls_reader.{method}().map_err(|error| error.to_string())?;"
                ),
            );
        }
        line(
            out,
            "                        if !__ls_reader.is_empty() { return Err(\"request has trailing bytes\".to_string()); }",
        );
        let args = (0..operation.params.len())
            .map(|index| format!("__ls_arg_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        match &operation.body {
            Body::Query { count: true, .. } => {
                line(
                    out,
                    &format!(
                        "                        let __ls_value = __ls_db.{}({args});",
                        operation.name
                    ),
                );
                line(out, "                        let mut __ls_out = vec![0u8];");
                line(
                    out,
                    "                        __ls_put_i64(&mut __ls_out, __ls_value);",
                );
                line(out, "                        Ok(__ls_out)");
            }
            Body::Query { count: false, .. } => {
                let returned = match &operation.returns {
                    Returns::Many(returned) => returned,
                    _ => {
                        return Err(format!(
                            "operation `{}` must return `many <table>`",
                            operation.name
                        ))
                    }
                };
                let table = workload
                    .table(returned)
                    .ok_or_else(|| format!("unknown table `{returned}`"))?;
                line(
                    out,
                    &format!(
                        "                        let __ls_rows = __ls_db.{}({args});",
                        operation.name
                    ),
                );
                line(out, "                        let mut __ls_out = vec![0u8];");
                line(
                    out,
                    "                        __ls_put_u32(&mut __ls_out, __ls_rows.len() as u32);",
                );
                line(out, "                        for __ls_row in __ls_rows {");
                for field in &table.fields {
                    let put = match field.ty {
                        Ty::I8 => format!("__ls_put_i8(&mut __ls_out, __ls_row.{});", field.name),
                        Ty::I16 => {
                            format!("__ls_put_i16(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::I32 => {
                            format!("__ls_put_i32(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::I64 => format!("__ls_put_i64(&mut __ls_out, __ls_row.{});", field.name),
                        Ty::U8 => format!("__ls_put_u8(&mut __ls_out, __ls_row.{});", field.name),
                        Ty::U16 => {
                            format!("__ls_put_u16(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::U32 => {
                            format!("__ls_put_u32(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::U64 => {
                            format!("__ls_put_u64(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::F32 => {
                            format!("__ls_put_f32(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::F64 => {
                            format!("__ls_put_f64(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::Bool => {
                            format!("__ls_put_bool(&mut __ls_out, __ls_row.{});", field.name)
                        }
                        Ty::Text => {
                            format!("__ls_put_text(&mut __ls_out, &__ls_row.{});", field.name)
                        }
                        Ty::Bytes => {
                            format!("__ls_put_bytes(&mut __ls_out, &__ls_row.{});", field.name)
                        }
                        Ty::Uuid => {
                            format!("__ls_put_uuid(&mut __ls_out, &__ls_row.{});", field.name)
                        }
                    };
                    line(out, &format!("                            {put}"));
                }
                line(out, "                        }");
                line(out, "                        Ok(__ls_out)");
            }
            Body::Insert { .. } => {
                line(
                    out,
                    &format!(
                        "                        let __ls_outcome = __ls_db.{}({args}).map_err(|error| error.to_string())?;",
                        operation.name
                    ),
                );
                line(out, "                        let mut __ls_out = vec![0u8];");
                line(
                    out,
                    "                        __ls_put_bool(&mut __ls_out, __ls_outcome);",
                );
                line(out, "                        Ok(__ls_out)");
            }
            Body::Update { .. } | Body::Delete { .. } => {
                line(
                    out,
                    &format!(
                        "                        let __ls_outcome = __ls_db.{}({args}).map_err(|error| error.to_string())?;",
                        operation.name
                    ),
                );
                line(out, "                        let mut __ls_out = vec![0u8];");
                line(
                    out,
                    "                        __ls_put_u64(&mut __ls_out, __ls_outcome);",
                );
                line(out, "                        Ok(__ls_out)");
            }
        }
        line(out, "                    }");
    }

    line(
        out,
        "                    _ => Err(format!(\"unknown operation index {__ls_index}\")),",
    );
    line(out, "                }");
    line(out, "            }");
    line(
        out,
        "            _ => Err(format!(\"unknown request kind {__ls_kind}\")),",
    );
    line(out, "        }");
    line(out, "    })();");
    line(out, "    let __ls_response = match __ls_result {");
    line(out, "        Ok(__ls_payload) => __ls_payload,");
    line(
        out,
        "        Err(__ls_message) => __ls_cli_error(&__ls_message),",
    );
    line(out, "    };");
    line(
        out,
        "    if let Some(__ls_message) = __ls_db.__ls_broken_message() {",
    );
    line(
        out,
        "        eprintln!(\"database entered a failed state: {__ls_message}; server exiting\");",
    );
    line(out, "        std::process::exit(1);");
    line(out, "    }");
    line(out, "    __ls_response");
    line(out, "}");
    line(out, "");

    let read_indices = workload
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| matches!(&operation.body, Body::Query { .. }))
        .map(|(index, _)| index.to_string())
        .collect::<Vec<_>>();
    line(
        out,
        "fn __ls_cli_request_is_read(__ls_payload: &[u8]) -> bool {",
    );
    line(out, "    if __ls_payload.first().copied() != Some(0) {");
    line(out, "        return false;");
    line(out, "    }");
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(__ls_payload);",
    );
    line(out, "    if __ls_reader.u8().is_err() {");
    line(out, "        return false;");
    line(out, "    }");
    line(out, "    match __ls_reader.u32() {");
    if read_indices.is_empty() {
        line(out, "        Ok(_) => false,");
    } else {
        line(
            out,
            &format!(
                "        Ok(__ls_index) => matches!(__ls_index, {}),",
                read_indices.join(" | ")
            ),
        );
    }
    line(out, "        Err(_) => false,");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_request_index(__ls_payload: &[u8]) -> Option<u32> {",
    );
    line(
        out,
        "    if __ls_payload.first().copied() != Some(0) { return None; }",
    );
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(__ls_payload);",
    );
    line(out, "    __ls_reader.u8().ok()?;");
    line(out, "    __ls_reader.u32().ok()");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_operation_name(__ls_index: u32) -> &'static str {",
    );
    line(out, "    match __ls_index {");
    for (index, operation) in workload.operations.iter().enumerate() {
        out.push_str(&format!("        {index} => {:?},\n", operation.name));
    }
    line(out, "        _ => \"unknown\",");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_serve_write(__ls_db: &mut Db, __ls_payload: &[u8]) -> (Vec<u8>, u64, bool) {",
    );
    line(out, "    let mut __ls_changed = false;");
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(__ls_payload);",
    );
    line(
        out,
        "    let __ls_result = (|| -> Result<Vec<u8>, String> {",
    );
    line(
        out,
        "        let __ls_kind = __ls_reader.u8().map_err(|error| error.to_string())?;",
    );
    line(out, "        if __ls_kind != 0 {");
    line(
        out,
        "            return Err(format!(\"unknown request kind {__ls_kind}\"));",
    );
    line(out, "        }");
    line(
        out,
        "        let __ls_index = __ls_reader.u32().map_err(|error| error.to_string())?;",
    );
    line(out, "        match __ls_index {");

    for (index, operation) in workload.operations.iter().enumerate() {
        if !matches!(
            &operation.body,
            Body::Insert { .. } | Body::Update { .. } | Body::Delete { .. }
        ) {
            continue;
        }
        line(out, &format!("            {index} => {{"));
        for (param_index, param) in operation.params.iter().enumerate() {
            let method = match param.ty {
                Ty::I8 => "i8",
                Ty::I16 => "i16",
                Ty::I32 => "i32",
                Ty::I64 => "i64",
                Ty::U8 => "u8",
                Ty::U16 => "u16",
                Ty::U32 => "u32",
                Ty::U64 => "u64",
                Ty::F32 => "f32",
                Ty::F64 => "f64",
                Ty::Bool => "bool",
                Ty::Text => "text",
                Ty::Bytes => "bytes",
                Ty::Uuid => "uuid",
            };
            line(
                out,
                &format!(
                    "                let __ls_arg_{param_index} = __ls_reader.{method}().map_err(|error| error.to_string())?;"
                ),
            );
        }
        line(
            out,
            "                if !__ls_reader.is_empty() { return Err(\"request has trailing bytes\".to_string()); }",
        );
        let args = (0..operation.params.len())
            .map(|index| format!("__ls_arg_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        match &operation.body {
            Body::Insert { .. } => {
                line(
                    out,
                    &format!(
                        "                let __ls_outcome = __ls_db.{}({args}).map_err(|error| error.to_string())?;",
                        operation.name
                    ),
                );
                line(out, "                let mut __ls_out = vec![0u8];");
                line(out, "                __ls_changed = __ls_outcome;");
                line(
                    out,
                    "                __ls_put_bool(&mut __ls_out, __ls_outcome);",
                );
                line(out, "                Ok(__ls_out)");
            }
            Body::Update { .. } | Body::Delete { .. } => {
                line(
                    out,
                    &format!(
                        "                let __ls_outcome = __ls_db.{}({args}).map_err(|error| error.to_string())?;",
                        operation.name
                    ),
                );
                line(out, "                let mut __ls_out = vec![0u8];");
                line(out, "                __ls_changed = __ls_outcome > 0;");
                line(
                    out,
                    "                __ls_put_u64(&mut __ls_out, __ls_outcome);",
                );
                line(out, "                Ok(__ls_out)");
            }
            Body::Query { .. } => {}
        }
        line(out, "            }");
    }

    line(
        out,
        "            _ => Err(format!(\"unknown operation index {__ls_index}\")),",
    );
    line(out, "        }");
    line(out, "    })();");
    line(out, "    let __ls_response = match __ls_result {");
    line(out, "        Ok(__ls_payload) => __ls_payload,");
    line(
        out,
        "        Err(__ls_message) => __ls_cli_error(&__ls_message),",
    );
    line(out, "    };");
    line(
        out,
        "    if let Some(__ls_message) = __ls_db.__ls_broken_message() {",
    );
    line(
        out,
        "        eprintln!(\"database entered a failed state: {__ls_message}; server exiting\");",
    );
    line(out, "        std::process::exit(1);");
    line(out, "    }");
    line(out, "    (__ls_response, __ls_db.__ls_lsn, __ls_changed)");
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_connect(__ls_socket: &str) -> Result<__LsStream, String> {",
    );
    line(
        out,
        "    let __ls_address = __ls_cli_read_endpoint(Path::new(__ls_socket))",
    );
    line(
        out,
        "        .map(|__ls_text| __ls_text.trim().to_string())",
    );
    line(
        out,
        "        .filter(|__ls_text| __ls_text.parse::<std::net::SocketAddr>().is_ok());",
    );
    line(out, "    let mut __ls_stream = match __ls_address {");
    line(out, "        Some(__ls_address) => {");
    line(
        out,
        "            let __ls_address: std::net::SocketAddr = __ls_address.parse().map_err(|error| format!(\"invalid server address `{__ls_address}`: {error}\"))?;",
    );
    line(
        out,
        "            let __ls_stream = std::net::TcpStream::connect_timeout(&__ls_address, std::time::Duration::from_secs(5))",
    );
    line(out, "                .map_err(|error| format!(\"cannot connect to `{__ls_socket}` ({__ls_address}): {error}\"))?;");
    line(out, "            __LsStream::Tcp(__ls_stream)");
    line(out, "        }");
    line(out, "        None => {");
    line(out, "            #[cfg(unix)]");
    line(out, "            {");
    line(
        out,
        "                let __ls_stream = std::os::unix::net::UnixStream::connect(__ls_socket)",
    );
    line(out, "                    .map_err(|error| format!(\"cannot connect to `{__ls_socket}`: {error}\"))?;");
    line(out, "                __LsStream::Unix(__ls_stream)");
    line(out, "            }");
    line(out, "            #[cfg(windows)]");
    line(out, "            {");
    line(out, "                return Err(format!(\"cannot read a server address from `{__ls_socket}`\"));");
    line(out, "            }");
    line(out, "        }");
    line(out, "    };");
    line(
        out,
        "    __ls_stream.set_timeouts(__ls_cli_idle_timeout()).map_err(|error| format!(\"cannot set client timeouts: {error}\"))?;",
    );
    line(out, "    let mut __ls_hello = Vec::new();");
    line(
        out,
        "    __ls_hello.extend_from_slice(&__LS_PROTOCOL_MAGIC);",
    );
    line(
        out,
        "    __ls_put_u32(&mut __ls_hello, __LS_PROTOCOL_VERSION);",
    );
    line(out, "    __ls_put_u64(&mut __ls_hello, __LS_WORKLOAD);");
    line(
        out,
        "    __ls_put_text(&mut __ls_hello, &__ls_cli_credential_id());",
    );
    line(
        out,
        "    __ls_put_text(&mut __ls_hello, &__ls_cli_credential_secret());",
    );
    line(
        out,
        "    __ls_cli_send(&mut __ls_stream, &__ls_hello).map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    let __ls_response = __ls_cli_receive(&mut __ls_stream)",
    );
    line(out, "        .map_err(|error| error.to_string())?");
    line(
        out,
        "        .ok_or(\"server closed the connection during the handshake\")?;",
    );
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(&__ls_response);",
    );
    line(
        out,
        "    let __ls_status = __ls_reader.u8().map_err(|error| error.to_string())?;",
    );
    line(out, "    if __ls_status != 0 {");
    line(
        out,
        "        return Err(__ls_reader.text().map_err(|error| error.to_string())?);",
    );
    line(out, "    }");
    line(out, "    if !__ls_reader.is_empty() { return Err(\"handshake response has trailing bytes\".to_string()); }");
    line(out, "    Ok(__ls_stream)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_client_call(__ls_socket: &str, __ls_request: &[u8]) -> Result<Vec<u8>, String> {");
    line(
        out,
        "    let mut __ls_stream = __ls_cli_connect(__ls_socket)?;",
    );
    line(
        out,
        "    __ls_cli_send(&mut __ls_stream, __ls_request).map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    let __ls_response = __ls_cli_receive(&mut __ls_stream)",
    );
    line(out, "        .map_err(|error| error.to_string())?");
    line(out, "        .ok_or(\"server closed the connection\")?;");
    line(
        out,
        "    let mut __ls_reader = __LsReader::new(&__ls_response);",
    );
    line(
        out,
        "    let __ls_status = __ls_reader.u8().map_err(|error| error.to_string())?;",
    );
    line(out, "    if __ls_status != 0 {");
    line(
        out,
        "        return Err(__ls_reader.text().map_err(|error| error.to_string())?);",
    );
    line(out, "    }");
    line(out, "    Ok(__ls_response[1..].to_vec())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_client_run(__ls_socket: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(
        out,
        "    let (__ls_operation, __ls_rest) = match __ls_args.split_first() {",
    );
    line(
        out,
        "        Some((__ls_operation, __ls_rest)) => (__ls_operation.as_str(), __ls_rest),",
    );
    line(
        out,
        "        None => return Err(\"run expects an operation name\".to_string()),",
    );
    line(out, "    };");
    line(out, "    match __ls_operation {");

    for (index, operation) in workload.operations.iter().enumerate() {
        let known = operation
            .params
            .iter()
            .map(|param| format!("{:?}", param.name))
            .collect::<Vec<_>>()
            .join(", ");
        line(out, &format!("        {:?} => {{", operation.name));
        line(
            out,
            &format!(
                "            let __ls_pairs = __ls_cli_pairs(__ls_rest, &[{known}], {:?})?;",
                operation.name
            ),
        );
        for (param_index, param) in operation.params.iter().enumerate() {
            let parser = argument_expr(param);
            line(
                out,
                &format!("            let __ls_arg_{param_index} = {parser};"),
            );
        }
        line(out, "            let mut __ls_request = vec![0u8];");
        line(
            out,
            &format!("            __ls_put_u32(&mut __ls_request, {index}u32);"),
        );
        for (param_index, param) in operation.params.iter().enumerate() {
            let put = match param.ty {
                Ty::I8 => {
                    format!("            __ls_put_i8(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::I16 => {
                    format!("            __ls_put_i16(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::I32 => {
                    format!("            __ls_put_i32(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::I64 => {
                    format!("            __ls_put_i64(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::U8 => {
                    format!("            __ls_put_u8(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::U16 => {
                    format!("            __ls_put_u16(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::U32 => {
                    format!("            __ls_put_u32(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::U64 => {
                    format!("            __ls_put_u64(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::F32 => {
                    format!("            __ls_put_f32(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::F64 => {
                    format!("            __ls_put_f64(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::Bool => {
                    format!("            __ls_put_bool(&mut __ls_request, __ls_arg_{param_index});")
                }
                Ty::Text => format!(
                    "            __ls_put_text(&mut __ls_request, &__ls_arg_{param_index});"
                ),
                Ty::Bytes => format!(
                    "            __ls_put_bytes(&mut __ls_request, &__ls_arg_{param_index});"
                ),
                Ty::Uuid => format!(
                    "            __ls_put_uuid(&mut __ls_request, &__ls_arg_{param_index});"
                ),
            };
            line(out, &put);
        }
        line(
            out,
            "            let __ls_body = __ls_cli_client_call(__ls_socket, &__ls_request)?;",
        );
        line(
            out,
            "            let mut __ls_reader = __LsReader::new(&__ls_body);",
        );
        match &operation.body {
            Body::Query { count: true, .. } => {
                line(
                    out,
                    "            let __ls_value = __ls_reader.i64().map_err(|error| error.to_string())?;",
                );
                line(out, "            println!(\"{__ls_value}\");");
            }
            Body::Query { count: false, .. } => {
                let returned = match &operation.returns {
                    Returns::Many(returned) => returned,
                    _ => {
                        return Err(format!(
                            "operation `{}` must return `many <table>`",
                            operation.name
                        ))
                    }
                };
                let table = workload
                    .table(returned)
                    .ok_or_else(|| format!("unknown table `{returned}`"))?;
                line(
                    out,
                    "            let __ls_count = __ls_reader.u32().map_err(|error| error.to_string())?;",
                );
                line(out, "            for _ in 0..__ls_count {");
                line(out, "                let mut __ls_cells = Vec::new();");
                for field in &table.fields {
                    let expression = match field.ty {
                        Ty::I8 => "__ls_reader.i8()".to_string(),
                        Ty::I16 => "__ls_reader.i16()".to_string(),
                        Ty::I32 => "__ls_reader.i32()".to_string(),
                        Ty::I64 => "__ls_reader.i64()".to_string(),
                        Ty::U8 => "__ls_reader.u8()".to_string(),
                        Ty::U16 => "__ls_reader.u16()".to_string(),
                        Ty::U32 => "__ls_reader.u32()".to_string(),
                        Ty::U64 => "__ls_reader.u64()".to_string(),
                        Ty::F32 => "__ls_reader.f32()".to_string(),
                        Ty::F64 => "__ls_reader.f64()".to_string(),
                        Ty::Bool => "__ls_reader.bool()".to_string(),
                        Ty::Text => "__ls_cli_escape(&__ls_reader.text()".to_string(),
                        Ty::Bytes => "__ls_cli_hex(&__ls_reader.bytes()".to_string(),
                        Ty::Uuid => "__ls_cli_uuid_text(&__ls_reader.uuid()".to_string(),
                    };
                    let suffix =
                        if field.ty == Ty::Text || field.ty == Ty::Bytes || field.ty == Ty::Uuid {
                            ".map_err(|error| error.to_string())?)"
                        } else {
                            ".map_err(|error| error.to_string())?.to_string()"
                        };
                    line(
                        out,
                        &format!("                __ls_cells.push({expression}{suffix});"),
                    );
                }
                line(
                    out,
                    "                println!(\"{}\", __ls_cells.join(\"\\t\"));",
                );
                line(out, "            }");
            }
            Body::Insert { .. } => {
                line(
                    out,
                    "            let __ls_outcome = __ls_reader.bool().map_err(|error| error.to_string())?;",
                );
                line(out, "            println!(\"ok {__ls_outcome}\");");
            }
            Body::Update { .. } | Body::Delete { .. } => {
                line(
                    out,
                    "            let __ls_outcome = __ls_reader.u64().map_err(|error| error.to_string())?;",
                );
                line(out, "            println!(\"ok {__ls_outcome}\");");
            }
        }
        line(
            out,
            "            if !__ls_reader.is_empty() { return Err(\"response has trailing bytes\".to_string()); }",
        );
        line(out, "            Ok(())");
        line(out, "        }");
    }

    line(
        out,
        "        __ls_other => Err(format!(\"unknown operation `{__ls_other}` (available: {})\", __LS_CLI_OPERATIONS)),",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    Ok(())
}
