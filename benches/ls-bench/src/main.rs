use lightshadedb::prelude::*;
use redb::{ReadableDatabase, ReadableTable};
use std::path::PathBuf;
use std::time::Instant;

#[lightshadedb::schema]
pub mod store {
    use lightshadedb::prelude::*;

    pub struct Message {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshadedb::operations]
    pub trait Messages {
        #[lightshadedb::query(
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            order = (Message::sent_at.desc(), Message::id.desc()),
            limit = limit,
        )]
        fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;

        #[lightshadedb::query(
            count,
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
        )]
        fn unread_count(&self, inbox: i64) -> Result<i64>;
    }
}

struct Sample {
    sys: sysinfo::System,
    pid: sysinfo::Pid,
    peak_rss: u64,
    peak_cpu: f32,
}

impl Sample {
    fn refresh(&mut self) {
        self.sys
            .refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), true);
    }

    fn new() -> Self {
        let sys = sysinfo::System::new_all();
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut sample = Self {
            sys,
            pid,
            peak_rss: 0,
            peak_cpu: 0.0,
        };
        sample.refresh();
        sample
    }

    fn observe(&mut self) {
        self.refresh();
        if let Some(p) = self.sys.process(self.pid) {
            self.peak_rss = self.peak_rss.max(p.memory());
            self.peak_cpu = self.peak_cpu.max(p.cpu_usage());
        }
    }
}

struct Phase {
    name: &'static str,
    ops: usize,
    nanos: Vec<u64>,
}

impl Phase {
    fn report(&self) -> String {
        let total_ms = self.nanos.iter().sum::<u64>() as f64 / 1e6;
        let ops_s = self.ops as f64 / (total_ms / 1000.0);
        let mut sorted = self.nanos.clone();
        sorted.sort_unstable();
        let at = |q: f64| {
            sorted[((q * sorted.len() as f64) as usize).min(sorted.len() - 1)] as f64 / 1000.0
        };
        format!(
            "{{\"ops\":{},\"ms\":{:.1},\"ops_s\":{:.1},\"p50_us\":{:.1},\"p99_us\":{:.1}}}",
            self.ops,
            total_ms,
            ops_s,
            at(0.5),
            at(0.99)
        )
    }
}

fn row_body(id: usize) -> String {
    format!("bench-message-{id:08}")
}

const BATCH_ROWS: usize = 100;

struct StorageReport {
    rows: usize,
    logical_bytes: u64,
    compacted_bytes: u64,
    disk_bytes: u64,
}

struct ReopenReport {
    open_ms: f64,
    cold_get_us: f64,
}

struct EngineOutput {
    phases: Vec<Phase>,
    storage: Option<StorageReport>,
    reopen: Option<ReopenReport>,
}

fn sidecar_size(path: &std::path::Path, suffix: &str) -> u64 {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    let sidecar = std::path::PathBuf::from(name);
    let mut total = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    total += std::fs::metadata(&sidecar)
        .map(|meta| meta.len())
        .unwrap_or(0);
    total
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

fn logical_row_bytes(body_len: usize) -> u64 {
    25 + body_len as u64
}

fn storage_body(id: usize, rows: usize) -> String {
    let third = rows.div_ceil(3).max(1);
    let (tag, filler_len) = if id <= third {
        ("s", 30)
    } else if id <= 2 * third {
        ("w", 1016)
    } else {
        ("b", 65512)
    };
    // Deterministic incompressible filler so storage footprints measure real
    // bytes rather than codec-friendly repeats.
    let mut state = (id as u64)
        .wrapping_mul(0x9e3779b97f4a7c15)
        .wrapping_add(0xbf58476d1ce4e5b9);
    let mut body = String::with_capacity(tag.len() + 1 + filler_len);
    body.push_str(tag);
    body.push('-');
    for _ in 0..filler_len {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        body.push((b'a' + ((state >> 33) % 26) as u8) as char);
    }
    body
}

fn run_lightshade(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> lightshadedb::Result<EngineOutput> {
    let mut db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let t = Instant::now();
        db.insert(store::Message {
            id,
            inbox_id: id % 16,
            sent_at: id,
            is_read: id % 3 == 0,
            body: row_body(id as usize),
        })?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let row: Option<store::Message> = db.get(id as i64)?;
        assert!(row.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let _ = db.recent_unread(inbox, 10)?;
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let _ = db.unread_count(inbox)?;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let batch_rows: Vec<store::Message> = (start..=end)
            .map(|id| {
                let id = id as i64;
                store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: id % 3 == 0,
                    body: row_body(id as usize),
                }
            })
            .collect();
        let t = Instant::now();
        db.insert_many(batch_rows)?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                db.update(store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: false,
                    body: format!("churn-{i:08}"),
                })?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                db.insert(store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: false,
                    body: row_body(id as usize),
                })?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                db.delete::<store::Message>(id)?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);
    db.close()?;

    let storage_dir = dir.join("storage.lsdb");
    let storage_spill = dir.join("storage-spill");
    std::fs::create_dir_all(&storage_spill)
        .map_err(|e| lightshadedb::Error::io("creating the spill dir", e))?;
    let prev_spill = std::env::var("LIGHTSHADE_SPILL_DIR").ok();
    std::env::set_var("LIGHTSHADE_SPILL_DIR", &storage_spill);
    let mut storage_db = store::Db::open(&storage_dir)?;
    let mut logical = 0u64;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        storage_db.insert(store::Message {
            id,
            inbox_id: id % 16,
            sent_at: id,
            is_read: id % 3 == 0,
            body,
        })?;
    }
    storage_db.checkpoint()?;
    let storage_path = dir.join("storage.lsdb");
    let disk = sidecar_size(&storage_path, ".wal") + dir_size(&dir.join("storage-spill"));
    for id in (2..=rows).step_by(2) {
        storage_db.delete::<store::Message>(id as i64)?;
    }
    storage_db.checkpoint()?;
    let compacted = sidecar_size(&storage_path, ".wal") + dir_size(&dir.join("storage-spill"));
    drop(storage_db);
    match prev_spill {
        Some(previous) => std::env::set_var("LIGHTSHADE_SPILL_DIR", previous),
        None => std::env::remove_var("LIGHTSHADE_SPILL_DIR"),
    }
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    let t = Instant::now();
    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    assert!(db.get::<store::Message>(1)?.is_some());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    db.close()?;
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_lightshade_client(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> lightshadedb::Result<EngineOutput> {
    use lightshadedb::secret::{Credential, Role};
    use lightshadedb::{ClientOptions, Credentials, CredentialsSource, Secret};
    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "bench".to_string(),
        secret: Secret::new("bench-secret"),
        role: Role::Writer,
    });
    let mut options =
        lightshadedb::ServeOptions::new(CredentialsSource::Static(credentials.clone()));
    options.listen = lightshadedb::Listen::tcp_local(0);
    let server = store::Server::start(db, options)?;
    let addr = server.local_addr().expect("bound TCP address");
    let client_options = ClientOptions::new(lightshadedb::ClientCredentials::new(
        "bench",
        "bench-secret",
    ));
    let mut client = store::Client::connect_tcp(addr, client_options.clone())?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let t = Instant::now();
        client.insert(store::Message {
            id,
            inbox_id: id % 16,
            sent_at: id,
            is_read: id % 3 == 0,
            body: row_body(id as usize),
        })?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let row: Option<store::Message> = client.get(id as i64)?;
        assert!(row.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let _ = client.recent_unread(inbox, 10)?;
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let _ = client.unread_count(inbox)?;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let batch_rows: Vec<store::Message> = (start..=end)
            .map(|id| {
                let id = id as i64;
                store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: id % 3 == 0,
                    body: row_body(id as usize),
                }
            })
            .collect();
        let t = Instant::now();
        client.insert_many(batch_rows)?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                client.update(store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: false,
                    body: format!("churn-{i:08}"),
                })?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                client.insert(store::Message {
                    id,
                    inbox_id: id % 16,
                    sent_at: id,
                    is_read: false,
                    body: row_body(id as usize),
                })?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                client.delete::<store::Message>(id)?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);
    client.close()?;
    server.shutdown();
    server.join()?;

    let storage_spill = dir.join("storage-spill");
    std::fs::create_dir_all(&storage_spill)
        .map_err(|e| lightshadedb::Error::io("creating the spill dir", e))?;
    let prev_spill = std::env::var("LIGHTSHADE_SPILL_DIR").ok();
    std::env::set_var("LIGHTSHADE_SPILL_DIR", &storage_spill);
    let storage_db = store::Db::open(dir.join("storage.lsdb"))?;
    let mut storage_options =
        lightshadedb::ServeOptions::new(CredentialsSource::Static(credentials.clone()));
    storage_options.listen = lightshadedb::Listen::tcp_local(0);
    let storage_server = store::Server::start(storage_db, storage_options)?;
    let storage_addr = storage_server.local_addr().expect("bound TCP address");
    let mut storage_client = store::Client::connect_tcp(storage_addr, client_options.clone())?;
    let mut logical = 0u64;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        storage_client.insert(store::Message {
            id,
            inbox_id: id % 16,
            sent_at: id,
            is_read: id % 3 == 0,
            body,
        })?;
    }
    let storage_path = dir.join("storage.lsdb");
    storage_client.checkpoint()?;
    let disk = sidecar_size(&storage_path, ".wal") + dir_size(&dir.join("storage-spill"));
    for id in (2..=rows).step_by(2) {
        storage_client.delete::<store::Message>(id as i64)?;
    }
    storage_client.checkpoint()?;
    let compacted = sidecar_size(&storage_path, ".wal") + dir_size(&dir.join("storage-spill"));
    match prev_spill {
        Some(previous) => std::env::set_var("LIGHTSHADE_SPILL_DIR", previous),
        None => std::env::remove_var("LIGHTSHADE_SPILL_DIR"),
    }
    storage_client.close()?;
    storage_server.shutdown();
    storage_server.join()?;
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut reopen_options =
        lightshadedb::ServeOptions::new(CredentialsSource::Static(credentials));
    reopen_options.listen = lightshadedb::Listen::tcp_local(0);
    let reopen_server = store::Server::start(db, reopen_options)?;
    let reopen_addr = reopen_server.local_addr().expect("bound TCP address");
    let t = Instant::now();
    let reopen_client = store::Client::connect_tcp(reopen_addr, client_options)?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    assert!(reopen_client.get::<store::Message>(1)?.is_some());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    reopen_client.close()?;
    reopen_server.shutdown();
    reopen_server.join()?;
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn encode_row(inbox: i64, sent: i64, is_read: bool, body: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(17 + body.len());
    v.extend_from_slice(&(inbox as u64).to_le_bytes());
    v.extend_from_slice(&(sent as u64).to_le_bytes());
    v.push(is_read as u8);
    v.extend_from_slice(body.as_bytes());
    v
}

fn decode_row(bytes: &[u8]) -> (i64, i64, bool, String) {
    let inbox = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as i64;
    let sent = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as i64;
    let is_read = bytes[16] != 0;
    let body = String::from_utf8(bytes[17..].to_vec()).unwrap();
    (inbox, sent, is_read, body)
}

fn run_lightshade_client_x16(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> lightshadedb::Result<EngineOutput> {
    use lightshadedb::secret::{Credential, Role};
    use lightshadedb::{ClientOptions, Credentials, CredentialsSource, Secret};
    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "bench".to_string(),
        secret: Secret::new("bench-secret"),
        role: Role::Writer,
    });
    let mut options = lightshadedb::ServeOptions::new(CredentialsSource::Static(credentials));
    options.listen = lightshadedb::Listen::tcp_local(0);
    let server = store::Server::start(db, options)?;
    let addr = server.local_addr().expect("bound TCP address");
    let threads: usize = std::env::var("LS_BENCH_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let chunk = rows.div_ceil(threads);
    let insert_handles: Vec<std::thread::JoinHandle<lightshadedb::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let start = t * chunk;
            let end = ((t + 1) * chunk).min(rows);
            let options = ClientOptions::new(lightshadedb::ClientCredentials::new(
                "bench",
                "bench-secret",
            ));
            std::thread::spawn(move || {
                let mut client = store::Client::connect_tcp(addr, options)?;
                let mut inserted = Vec::new();
                for id in start..end {
                    let id = (id + 1) as i64;
                    let begin = Instant::now();
                    client.insert(store::Message {
                        id,
                        inbox_id: id % 16,
                        sent_at: id,
                        is_read: id % 3 == 0,
                        body: row_body(id as usize),
                    })?;
                    inserted.push(begin.elapsed().as_nanos() as u64);
                }
                client.close()?;
                Ok(inserted)
            })
        })
        .collect();
    let mut inserted = Vec::with_capacity(rows);
    for handle in insert_handles {
        let mut part = handle.join().expect("bench thread panicked")?;
        inserted.append(&mut part);
    }
    sample.observe();
    let get_handles: Vec<std::thread::JoinHandle<lightshadedb::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let start = t * chunk;
            let end = ((t + 1) * chunk).min(rows);
            let options = ClientOptions::new(lightshadedb::ClientCredentials::new(
                "bench",
                "bench-secret",
            ));
            std::thread::spawn(move || {
                let client = store::Client::connect_tcp(addr, options)?;
                let mut fetched = Vec::new();
                for id in start..end {
                    let id = (id + 1) as i64;
                    let begin = Instant::now();
                    let row: Option<store::Message> = client.get(id)?;
                    assert!(row.is_some());
                    fetched.push(begin.elapsed().as_nanos() as u64);
                }
                client.close()?;
                Ok(fetched)
            })
        })
        .collect();
    let mut fetched = Vec::with_capacity(rows);
    for handle in get_handles {
        let mut part = handle.join().expect("bench thread panicked")?;
        fetched.append(&mut part);
    }
    sample.observe();
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    type ScanPair = lightshadedb::Result<(Vec<u64>, Vec<u64>)>;
    let scan_handles: Vec<std::thread::JoinHandle<ScanPair>> = (0..threads)
        .map(|t| {
            let options = ClientOptions::new(lightshadedb::ClientCredentials::new(
                "bench",
                "bench-secret",
            ));
            std::thread::spawn(move || {
                let client = store::Client::connect_tcp(addr, options)?;
                let mut scan_ns = Vec::new();
                let mut count_ns = Vec::new();
                let mut inbox = t as i64;
                while inbox < 16 {
                    let begin = Instant::now();
                    let _ = client.recent_unread(inbox, 10)?;
                    scan_ns.push(begin.elapsed().as_nanos() as u64);
                    let begin = Instant::now();
                    let _ = client.unread_count(inbox)?;
                    count_ns.push(begin.elapsed().as_nanos() as u64);
                    inbox += threads as i64;
                }
                client.close()?;
                Ok((scan_ns, count_ns))
            })
        })
        .collect();
    for handle in scan_handles {
        let (mut scan_part, mut count_part) = handle.join().expect("bench thread panicked")?;
        scan.nanos.append(&mut scan_part);
        count.nanos.append(&mut count_part);
    }
    sample.observe();
    let mut phases = vec![
        Phase {
            name: "insert",
            ops: inserted.len(),
            nanos: inserted,
        },
        Phase {
            name: "get",
            ops: fetched.len(),
            nanos: fetched,
        },
    ];
    phases.push(scan);
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let batch_handles: Vec<std::thread::JoinHandle<lightshadedb::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let options = ClientOptions::new(lightshadedb::ClientCredentials::new(
                "bench",
                "bench-secret",
            ));
            std::thread::spawn(move || {
                let mut client = store::Client::connect_tcp(addr, options)?;
                let span = rows.div_ceil(threads);
                let lo = rows + 1 + t * span;
                let hi = (rows + t * span + span).min(2 * rows);
                let mut lat = Vec::new();
                let mut s = lo;
                while s <= hi {
                    let e = (s + BATCH_ROWS - 1).min(hi);
                    let batch_rows: Vec<store::Message> = (s..=e)
                        .map(|id| {
                            let id = id as i64;
                            store::Message {
                                id,
                                inbox_id: id % 16,
                                sent_at: id,
                                is_read: id % 3 == 0,
                                body: row_body(id as usize),
                            }
                        })
                        .collect();
                    let begin = Instant::now();
                    client.insert_many(batch_rows)?;
                    lat.push(begin.elapsed().as_nanos() as u64);
                    s = e + 1;
                }
                client.close()?;
                Ok(lat)
            })
        })
        .collect();
    for handle in batch_handles {
        let mut part = handle.join().expect("bench thread panicked")?;
        batch.ops += part.len();
        batch.nanos.append(&mut part);
    }
    sample.observe();
    phases.push(batch);
    server.shutdown();
    server.join()?;
    Ok(EngineOutput {
        phases,
        storage: None,
        reopen: None,
    })
}

fn run_postgres(
    _dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    use postgresql_embedded::blocking::PostgreSQL;
    let settings = postgresql_embedded::SettingsBuilder::new()
        .host("127.0.0.1")
        .port(55433)
        .username("postgres")
        .password("bench")
        .temporary(true)
        .config("fsync", "on")
        .config("synchronous_commit", "on")
        .config("full_page_writes", "on")
        .build();
    let mut pg = PostgreSQL::new(settings);
    pg.setup()?;
    pg.start()?;
    pg.create_database("benchdb")?;
    let url = "postgres://postgres:bench@127.0.0.1:55433/benchdb".to_string();
    let mut client = postgres::Client::connect(&url, postgres::NoTls)?;
    client.batch_execute(
        "CREATE TABLE messages (id BIGINT PRIMARY KEY, inbox_id BIGINT NOT NULL, sent_at BIGINT NOT NULL, is_read BOOLEAN NOT NULL, body TEXT NOT NULL);
         CREATE INDEX idx_messages_inbox ON messages (inbox_id, is_read, sent_at DESC);",
    )?;
    let output = run_pg_phases(&mut client, rows, &url, sample, true)?;
    drop(client);
    pg.stop()?;
    Ok(output)
}

fn run_pg_phases(
    client: &mut postgres::Client,
    rows: usize,
    url: &str,
    sample: &mut Sample,
    vacuum_full: bool,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let body = row_body(id as usize);
        let t = Instant::now();
        client.execute(
            "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES ($1, $2, $3, $4, $5)",
            &[&id, &(id % 16), &id, &(id % 3 == 0), &body],
        )?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let t = Instant::now();
        let body: String = client
            .query_one("SELECT body FROM messages WHERE id = $1", &[&id])?
            .get(0);
        assert!(!body.is_empty());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16i64 {
        let t = Instant::now();
        let ids: Vec<i64> = client
            .query(
                "SELECT id FROM messages WHERE inbox_id = $1 AND is_read = false ORDER BY sent_at DESC, id DESC LIMIT 10",
                &[&inbox],
            )?
            .iter()
            .map(|row| row.get(0))
            .collect();
        let _ = ids;
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16i64 {
        let t = Instant::now();
        let n: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM messages WHERE inbox_id = $1 AND is_read = false",
                &[&inbox],
            )?
            .get(0);
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    {
        use postgres::types::ToSql;
        let mut start = rows + 1;
        while start <= 2 * rows {
            let end = (start + BATCH_ROWS - 1).min(2 * rows);
            let mut placeholders = Vec::new();
            let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
            for (k, id) in (start..=end).enumerate() {
                let id = id as i64;
                placeholders.push(format!(
                    "(${},${},${},${},${})",
                    k * 5 + 1,
                    k * 5 + 2,
                    k * 5 + 3,
                    k * 5 + 4,
                    k * 5 + 5
                ));
                params.push(Box::new(id));
                params.push(Box::new(id % 16));
                params.push(Box::new(id));
                params.push(Box::new(id % 3 == 0));
                params.push(Box::new(row_body(id as usize)));
            }
            let query = format!(
                "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES {}",
                placeholders.join(",")
            );
            let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
            let t = Instant::now();
            client.execute(&query[..], &refs)?;
            batch.nanos.push(t.elapsed().as_nanos() as u64);
            batch.ops += 1;
            start = end + 1;
        }
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                client.execute(
                    "UPDATE messages SET body = $1, is_read = false WHERE id = $2",
                    &[&format!("churn-{i:08}"), &id],
                )?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                let body = row_body(id as usize);
                client.execute(
                    "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES ($1, $2, $3, $4, $5)",
                    &[&id, &(id % 16), &id, &false, &body],
                )?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                client.execute("DELETE FROM messages WHERE id = $1", &[&id])?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let mut logical = 0u64;
    client.batch_execute(
        "DROP TABLE IF EXISTS messages_storage;
         CREATE TABLE messages_storage (id BIGINT PRIMARY KEY, inbox_id BIGINT NOT NULL, sent_at BIGINT NOT NULL, is_read BOOLEAN NOT NULL, body TEXT NOT NULL);
         CREATE INDEX idx_messages_storage_inbox ON messages_storage (inbox_id, is_read, sent_at DESC);",
    )?;
    // CockroachDB exposes no pg_total_relation_size equivalent, so when the
    // workflow points COCKROACH_STORE at the server's store directory we
    // measure the attributable bytes as a delta over a pre-load baseline.
    let store_dir = std::env::var("COCKROACH_STORE")
        .ok()
        .filter(|path| !path.is_empty());
    let store_base = store_dir
        .as_deref()
        .map(|path| dir_size(std::path::Path::new(path)));
    let measure_bytes = |client: &mut postgres::Client| -> Result<u64, Box<dyn std::error::Error>> {
        match store_base {
            Some(base) => {
                let path = std::path::Path::new(store_dir.as_deref().unwrap_or(""));
                Ok(dir_size(path).saturating_sub(base))
            }
            None => {
                let bytes: i64 = client
                    .query_one("SELECT pg_total_relation_size('messages_storage')", &[])?
                    .get(0);
                Ok(bytes.max(0) as u64)
            }
        }
    };
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        client.execute(
            "INSERT INTO messages_storage (id, inbox_id, sent_at, is_read, body) VALUES ($1, $2, $3, $4, $5)",
            &[&id, &(id % 16), &id, &(id % 3 == 0), &body],
        )?;
    }
    let disk = measure_bytes(&mut *client)?;
    client.execute("DELETE FROM messages_storage WHERE id % 2 = 0", &[])?;
    if vacuum_full {
        client.batch_execute("VACUUM FULL messages_storage")?;
    }
    let compacted = measure_bytes(&mut *client)?;
    client.batch_execute("DROP TABLE messages_storage")?;
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    let t = Instant::now();
    let mut client = postgres::Client::connect(url, postgres::NoTls)?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let body: String = client
        .query_one("SELECT body FROM messages WHERE id = 1", &[])?
        .get(0);
    assert!(!body.is_empty());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_redb(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    const TABLE: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("messages");
    let db = redb::Database::create(dir.join("bench.redb"))?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
        let t = Instant::now();
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(TABLE)?;
            table.insert(&(id as u64), value.as_slice())?;
        }
        txn.commit()?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        assert!(table.get(&(id as u64))?.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        let mut hits: Vec<(i64, i64, String)> = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let (row_inbox, sent, is_read, body) = decode_row(value.value());
            if row_inbox == inbox && !is_read {
                hits.push((sent, key.value() as i64, body));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        hits.truncate(10);
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let txn = db.begin_read()?;
        let table = txn.open_table(TABLE)?;
        let mut n = 0u64;
        for entry in table.iter()? {
            let value = entry?.1;
            let (row_inbox, _, is_read, _) = decode_row(value.value());
            if row_inbox == inbox && !is_read {
                n += 1;
            }
        }
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let t = Instant::now();
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(TABLE)?;
            for id in start..=end {
                let id = id as i64;
                let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
                table.insert(&(id as u64), value.as_slice())?;
            }
        }
        txn.commit()?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(TABLE)?;
            match i % 10 {
                0..=4 => {
                    let id = (i % rows + 1) as i64;
                    let value = encode_row(id % 16, id, false, &format!("churn-{i:08}"));
                    table.insert(&(id as u64), value.as_slice())?;
                }
                5..=7 => {
                    let id = (2 * rows + i + 1) as i64;
                    let value = encode_row(id % 16, id, false, &row_body(id as usize));
                    table.insert(&(id as u64), value.as_slice())?;
                }
                _ => {
                    let id = (i % rows + 1) as i64;
                    table.remove(&(id as u64))?;
                }
            }
        }
        txn.commit()?;
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let mut storage_db = redb::Database::create(dir.join("bench-storage.redb"))?;
    let mut logical = 0u64;
    {
        let txn = storage_db.begin_write()?;
        {
            let mut table = txn.open_table(TABLE)?;
            for id in 1..=rows {
                let body = storage_body(id, rows);
                logical += logical_row_bytes(body.len());
                let id = id as i64;
                let value = encode_row(id % 16, id, id % 3 == 0, &body);
                table.insert(&(id as u64), value.as_slice())?;
            }
            for id in (2..=rows).step_by(2) {
                table.remove(&((id as i64) as u64))?;
            }
        }
        txn.commit()?;
    }
    let disk = dir_size(dir);
    let _ = storage_db.compact()?;
    let compacted = dir_size(dir);
    drop(storage_db);
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(db);
    let t = Instant::now();
    let db = redb::Database::create(dir.join("bench.redb"))?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let txn = db.begin_read()?;
    let table = txn.open_table(TABLE)?;
    assert!(table.get(&(1u64))?.is_some());
    drop(table);
    drop(txn);
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_sqlite(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let mut conn = rusqlite::Connection::open(dir.join("bench.sqlite"))?;
    conn.execute_batch(
        "CREATE TABLE messages (id INTEGER PRIMARY KEY, inbox_id INTEGER NOT NULL, sent_at INTEGER NOT NULL, is_read INTEGER NOT NULL, body TEXT NOT NULL);
         CREATE INDEX idx_messages_inbox ON messages (inbox_id, is_read, sent_at DESC);",
    )?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let body = row_body(id as usize);
        let t = Instant::now();
        conn.execute(
            "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, id % 16, id, (id % 3 == 0) as i32, body],
        )?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let body: String = conn.query_row(
            "SELECT body FROM messages WHERE id = ?1",
            rusqlite::params![id as i64],
            |row| row.get(0),
        )?;
        assert!(!body.is_empty());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let mut stmt = conn.prepare("SELECT id FROM messages WHERE inbox_id = ?1 AND is_read = 0 ORDER BY sent_at DESC, id DESC LIMIT 10")?;
        let ids: Vec<i64> = stmt
            .query_map(rusqlite::params![inbox], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let _ = ids;
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE inbox_id = ?1 AND is_read = 0",
            rusqlite::params![inbox],
            |row| row.get(0),
        )?;
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let t = Instant::now();
        let txn = conn.transaction()?;
        {
            let mut stmt = txn.prepare(
                "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for id in start..=end {
                let id = id as i64;
                let body = row_body(id as usize);
                stmt.execute(rusqlite::params![
                    id,
                    id % 16,
                    id,
                    (id % 3 == 0) as i32,
                    body
                ])?;
            }
        }
        txn.commit()?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                conn.execute(
                    "UPDATE messages SET body = ?1, is_read = 0 WHERE id = ?2",
                    rusqlite::params![format!("churn-{i:08}"), id],
                )?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                let body = row_body(id as usize);
                conn.execute(
                    "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id, id % 16, id, 0, body],
                )?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                conn.execute("DELETE FROM messages WHERE id = ?1", rusqlite::params![id])?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let storage_path = dir.join("bench-storage.sqlite");
    let storage_conn = rusqlite::Connection::open(&storage_path)?;
    storage_conn.execute_batch(
        "CREATE TABLE messages (id INTEGER PRIMARY KEY, inbox_id INTEGER NOT NULL, sent_at INTEGER NOT NULL, is_read INTEGER NOT NULL, body TEXT NOT NULL);",
    )?;
    let mut logical = 0u64;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        storage_conn.execute(
            "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, id % 16, id, (id % 3 == 0) as i32, body],
        )?;
    }
    let disk = dir_size(dir);
    storage_conn.execute("DELETE FROM messages WHERE id % 2 = 0", [])?;
    storage_conn.execute_batch("VACUUM;")?;
    let compacted = dir_size(dir);
    drop(storage_conn);
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(conn);
    let t = Instant::now();
    let conn = rusqlite::Connection::open(dir.join("bench.sqlite"))?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let body: String = conn.query_row(
        "SELECT body FROM messages WHERE id = ?1",
        rusqlite::params![1i64],
        |row| row.get(0),
    )?;
    assert!(!body.is_empty());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn fjall_key(id: i64) -> Vec<u8> {
    (id as u64).to_be_bytes().to_vec()
}

fn run_fjall(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let db = fjall::Database::builder(dir.join("bench.fjall"))
        .cache_size(64 * 1024 * 1024)
        .open()?;
    let part = db.keyspace("messages", fjall::KeyspaceCreateOptions::default)?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
        let t = Instant::now();
        part.insert(fjall_key(id), value)?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        assert!(part.get(&fjall_key(id as i64)[..])?.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let mut hits: Vec<(i64, i64, String)> = Vec::new();
        for guard in part.iter() {
            let (key, value) = guard.into_inner()?;
            let (row_inbox, sent, is_read, body) = decode_row(&value);
            if row_inbox == inbox && !is_read {
                let id = i64::from_be_bytes(key[..].try_into().unwrap());
                hits.push((sent, id, body));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        hits.truncate(10);
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let mut n = 0u64;
        for guard in part.iter() {
            let (_, value) = guard.into_inner()?;
            let (row_inbox, _, is_read, _) = decode_row(&value);
            if row_inbox == inbox && !is_read {
                n += 1;
            }
        }
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let t = Instant::now();
        for id in start..=end {
            let id = id as i64;
            let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
            part.insert(fjall_key(id), value)?;
        }
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                let value = encode_row(id % 16, id, false, &format!("churn-{i:08}"));
                part.insert(fjall_key(id), value)?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                let value = encode_row(id % 16, id, false, &row_body(id as usize));
                part.insert(fjall_key(id), value)?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                part.remove(&fjall_key(id)[..])?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let storage_db = fjall::Database::builder(dir.join("bench-storage.fjall"))
        .cache_size(64 * 1024 * 1024)
        .open()?;
    let storage_part = storage_db.keyspace("messages", fjall::KeyspaceCreateOptions::default)?;
    let mut logical = 0u64;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        let value = encode_row(id % 16, id, id % 3 == 0, &body);
        storage_part.insert(fjall_key(id), value)?;
    }
    storage_db.persist(fjall::PersistMode::SyncAll)?;
    let disk = dir_size(&dir.join("bench-storage.fjall"));
    // Delete half the rows, then re-measure. fjall compacts in the
    // background; there is no manual compact to invoke.
    for id in (2..=rows).step_by(2) {
        storage_part.remove(&fjall_key(id as i64)[..])?;
    }
    storage_db.persist(fjall::PersistMode::SyncAll)?;
    let compacted = dir_size(&dir.join("bench-storage.fjall"));
    drop(storage_part);
    drop(storage_db);
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(part);
    drop(db);
    let path = dir.join("bench.fjall");
    let t = Instant::now();
    let db = fjall::Database::builder(&path)
        .cache_size(64 * 1024 * 1024)
        .open()?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let part = db.keyspace("messages", fjall::KeyspaceCreateOptions::default)?;
    let t = Instant::now();
    assert!(part.get(&fjall_key(1)[..])?.is_some());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_heed(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let path = dir.join("bench.heed");
    std::fs::create_dir_all(&path)?;
    let env = unsafe {
        heed::EnvOpenOptions::new()
            .max_dbs(2)
            .map_size(2usize * 1024 * 1024 * 1024)
            .open(&path)?
    };
    let wtxn = env.write_txn()?;
    let db: heed::Database<heed::types::Bytes, heed::types::Bytes> = env
        .create_databases_and_commit(wtxn, |maker, txn| {
            maker
                .create_database::<heed::types::Bytes, heed::types::Bytes>(txn, Some("messages"))
                .map(heed::CommitOrAbort::<_, ()>::Commit)
        })?
        .unwrap_commit();
    let put =
        |id: i64, inbox: i64, sent: i64, is_read: bool, body: &str| -> Result<(), heed::Error> {
            let key = (id as u64).to_be_bytes().to_vec();
            let value = encode_row(inbox, sent, is_read, body);
            let mut wtxn = env.write_txn()?;
            db.put(&mut wtxn, &key[..], &value[..])?;
            wtxn.commit()?;
            Ok(())
        };
    let get_row = |id: i64| -> Result<Option<(i64, i64, bool, String)>, heed::Error> {
        let key = (id as u64).to_be_bytes().to_vec();
        let rtxn = env.read_txn()?;
        Ok(db.get(&rtxn, &key[..])?.map(decode_row))
    };
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let t = Instant::now();
        put(id, id % 16, id, id % 3 == 0, &row_body(id as usize))?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        assert!(get_row(id as i64)?.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let rtxn = env.read_txn()?;
        let mut hits: Vec<(i64, i64, String)> = Vec::new();
        for entry in db.iter(&rtxn)? {
            let (key, value) = entry?;
            let (row_inbox, sent, is_read, body) = decode_row(value);
            if row_inbox == inbox && !is_read {
                let id = i64::from_be_bytes(key.try_into().unwrap());
                hits.push((sent, id, body));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        hits.truncate(10);
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let rtxn = env.read_txn()?;
        let mut n = 0u64;
        for entry in db.iter(&rtxn)? {
            let (_, value) = entry?;
            let (row_inbox, _, is_read, _) = decode_row(value);
            if row_inbox == inbox && !is_read {
                n += 1;
            }
        }
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let t = Instant::now();
        let mut wtxn = env.write_txn()?;
        for id in start..=end {
            let id = id as i64;
            let key = (id as u64).to_be_bytes().to_vec();
            let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
            db.put(&mut wtxn, &key[..], &value[..])?;
        }
        wtxn.commit()?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                put(id, id % 16, id, false, &format!("churn-{i:08}"))?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                put(id, id % 16, id, false, &row_body(id as usize))?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                let key = (id as u64).to_be_bytes().to_vec();
                let mut wtxn = env.write_txn()?;
                db.delete(&mut wtxn, &key[..])?;
                wtxn.commit()?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let storage_path = dir.join("bench-storage.heed");
    std::fs::create_dir_all(&storage_path)?;
    let storage_env = unsafe {
        heed::EnvOpenOptions::new()
            .max_dbs(2)
            .map_size(2usize * 1024 * 1024 * 1024)
            .open(&storage_path)?
    };
    let wtxn = storage_env.write_txn()?;
    let storage_db: heed::Database<heed::types::Bytes, heed::types::Bytes> = storage_env
        .create_databases_and_commit(wtxn, |maker, txn| {
            maker
                .create_database::<heed::types::Bytes, heed::types::Bytes>(txn, Some("messages"))
                .map(heed::CommitOrAbort::<_, ()>::Commit)
        })?
        .unwrap_commit();
    let mut logical = 0u64;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        let key = (id as u64).to_be_bytes().to_vec();
        let value = encode_row(id % 16, id, id % 3 == 0, &body);
        let mut wtxn = storage_env.write_txn()?;
        storage_db.put(&mut wtxn, &key[..], &value[..])?;
        wtxn.commit()?;
    }
    storage_env.force_sync()?;
    let disk = dir_size(&storage_path);
    // Delete half the rows. LMDB has no online compaction; freed pages are
    // reused by later writes but the file does not shrink.
    for id in (2..=rows).step_by(2) {
        let key = (id as u64).to_be_bytes().to_vec();
        let mut wtxn = storage_env.write_txn()?;
        storage_db.delete(&mut wtxn, &key[..])?;
        wtxn.commit()?;
    }
    let compacted = dir_size(&storage_path);
    drop(storage_env);
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(env);
    let path = dir.join("bench.heed");
    let t = Instant::now();
    let env = unsafe {
        heed::EnvOpenOptions::new()
            .max_dbs(2)
            .map_size(2usize * 1024 * 1024 * 1024)
            .open(&path)?
    };
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let rtxn = env.read_txn()?;
    let db: Option<heed::Database<heed::types::Bytes, heed::types::Bytes>> = env
        .open_databases_and_commit(rtxn, |maker, txn| {
            maker
                .open_database::<heed::types::Bytes, heed::types::Bytes>(txn, Some("messages"))
                .map(heed::CommitOrAbort::<_, ()>::Commit)
        })?
        .unwrap_commit();
    let db = db.ok_or("heed database is missing after reopen")?;
    let rtxn = env.read_txn()?;
    let t = Instant::now();
    let key = 1u64.to_be_bytes().to_vec();
    assert!(db.get(&rtxn, &key[..])?.is_some());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    drop(rtxn);
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_libsql(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let path = dir.join("bench.db");
        let path_text = path.display().to_string();
        let db = libsql::Builder::new_local(&path_text).build().await?;
        let conn = db.connect()?;
        conn.execute(
            "CREATE TABLE messages (id INTEGER PRIMARY KEY, inbox_id INTEGER NOT NULL, sent_at INTEGER NOT NULL, is_read INTEGER NOT NULL, body TEXT NOT NULL)",
            (),
        )
        .await?;
        conn.execute(
            "CREATE INDEX idx_messages_inbox ON messages (inbox_id, is_read, sent_at DESC)",
            (),
        )
        .await?;
        let mut phases = Vec::new();
        let mut insert = Phase {
            name: "insert",
            ops: rows,
            nanos: Vec::with_capacity(rows),
        };
        for id in 1..=rows {
            let id = id as i64;
            let body = row_body(id as usize);
            let t = Instant::now();
            conn.execute(
                "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
                libsql::params![id, id % 16, id, (id % 3 == 0) as i64, body],
            )
            .await?;
            insert.nanos.push(t.elapsed().as_nanos() as u64);
        }
        sample.observe();
        phases.push(insert);
        let mut get = Phase {
            name: "get",
            ops: rows,
            nanos: Vec::with_capacity(rows),
        };
        for id in 1..=rows {
            let t = Instant::now();
            let mut fetched = conn
                .query("SELECT body FROM messages WHERE id = ?1", [id as i64])
                .await?;
            let row = fetched.next().await?.ok_or("libsql row is missing")?;
            let body: String = row.get(0)?;
            assert!(!body.is_empty());
            get.nanos.push(t.elapsed().as_nanos() as u64);
        }
        sample.observe();
        phases.push(get);
        let mut scan = Phase {
            name: "scan",
            ops: 16,
            nanos: Vec::with_capacity(16),
        };
        for inbox in 0..16i64 {
            let t = Instant::now();
            let mut fetched = conn
                .query(
                    "SELECT id FROM messages WHERE inbox_id = ?1 AND is_read = 0 ORDER BY sent_at DESC, id DESC LIMIT 10",
                    [inbox],
                )
                .await?;
            while fetched.next().await?.is_some() {}
            scan.nanos.push(t.elapsed().as_nanos() as u64);
        }
        sample.observe();
        phases.push(scan);
        let mut count = Phase {
            name: "count",
            ops: 16,
            nanos: Vec::with_capacity(16),
        };
        for inbox in 0..16i64 {
            let t = Instant::now();
            let mut fetched = conn
                .query(
                    "SELECT COUNT(*) FROM messages WHERE inbox_id = ?1 AND is_read = 0",
                    [inbox],
                )
                .await?;
            let row = fetched.next().await?.ok_or("libsql count is missing")?;
            let _: i64 = row.get(0)?;
            count.nanos.push(t.elapsed().as_nanos() as u64);
        }
        sample.observe();
        phases.push(count);
        let mut batch = Phase {
            name: "batch",
            ops: 0,
            nanos: Vec::new(),
        };
        let mut start = rows + 1;
        while start <= 2 * rows {
            let end = (start + BATCH_ROWS - 1).min(2 * rows);
            let t = Instant::now();
            let txn = conn.transaction().await?;
            for id in start..=end {
                let id = id as i64;
                let body = row_body(id as usize);
                txn.execute(
                    "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
                    libsql::params![id, id % 16, id, (id % 3 == 0) as i64, body],
                )
                .await?;
            }
            txn.commit().await?;
            batch.nanos.push(t.elapsed().as_nanos() as u64);
            batch.ops += 1;
            start = end + 1;
        }
        sample.observe();
        phases.push(batch);

        let mut churn = Phase {
            name: "churn",
            ops: rows,
            nanos: Vec::with_capacity(rows),
        };
        for i in 0..rows {
            let t = Instant::now();
            match i % 10 {
                0..=4 => {
                    let id = (i % rows + 1) as i64;
                    conn.execute(
                        "UPDATE messages SET body = ?1, is_read = 0 WHERE id = ?2",
                        libsql::params![format!("churn-{i:08}"), id],
                    )
                    .await?;
                }
                5..=7 => {
                    let id = (2 * rows + i + 1) as i64;
                    let body = row_body(id as usize);
                    conn.execute(
                        "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
                        libsql::params![id, id % 16, id, 0i64, body],
                    )
                    .await?;
                }
                _ => {
                    let id = (i % rows + 1) as i64;
                    conn.execute("DELETE FROM messages WHERE id = ?1", [id])
                        .await?;
                }
            }
            churn.nanos.push(t.elapsed().as_nanos() as u64);
        }
        sample.observe();
        phases.push(churn);

        let mut logical = 0u64;
        conn.execute(
            "CREATE TABLE storage_messages (id INTEGER PRIMARY KEY, inbox_id INTEGER NOT NULL, sent_at INTEGER NOT NULL, is_read INTEGER NOT NULL, body TEXT NOT NULL)",
            (),
        )
        .await?;
        for id in 1..=rows {
            let body = storage_body(id, rows);
            logical += logical_row_bytes(body.len());
            let id = id as i64;
            conn.execute(
                "INSERT INTO storage_messages (id, inbox_id, sent_at, is_read, body) VALUES (?1, ?2, ?3, ?4, ?5)",
                libsql::params![id, id % 16, id, (id % 3 == 0) as i64, body],
            )
            .await?;
        }
        let disk = dir_size(dir);
        conn.execute("DELETE FROM storage_messages WHERE id % 2 = 0", ())
            .await?;
        conn.execute("VACUUM", ()).await?;
        let compacted = dir_size(dir);
        conn.execute("DROP TABLE storage_messages", ()).await?;
        sample.observe();
        let storage = StorageReport {
            rows,
            logical_bytes: logical,
            disk_bytes: disk,
            compacted_bytes: compacted,
        };

        drop(conn);
        drop(db);
        let t = Instant::now();
        let db = libsql::Builder::new_local(&path_text).build().await?;
        let open_ms = t.elapsed().as_secs_f64() * 1000.0;
        let conn = db.connect()?;
        let t = Instant::now();
        let mut fetched = conn
            .query("SELECT body FROM messages WHERE id = 1", ())
            .await?;
        assert!(fetched.next().await?.is_some());
        let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
        sample.observe();
        let reopen = ReopenReport {
            open_ms,
            cold_get_us,
        };
        Ok(EngineOutput {
            phases,
            storage: Some(storage),
            reopen: Some(reopen),
        })
    })
}

fn valkey_key(id: i64) -> String {
    format!("msg:{id}")
}

fn valkey_dataset_bytes(con: &mut redis::Connection) -> Result<u64, Box<dyn std::error::Error>> {
    let info: String = redis::cmd("INFO").arg("memory").query(con)?;
    for line in info.lines() {
        if let Some(value) = line
            .strip_prefix("used_memory_dataset_bytes:")
            .or_else(|| line.strip_prefix("used_memory_dataset:"))
        {
            return Ok(value.trim().parse().unwrap_or(0));
        }
    }
    Ok(0)
}

fn keys_with_prefix(
    con: &mut redis::Connection,
    prefix: &str,
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    use redis::Commands;
    Ok(con.keys(format!("{prefix}*"))?)
}

fn run_valkey(
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    use redis::Commands;
    let url = std::env::var("VALKEY_URL").unwrap_or_else(|_| "redis://127.0.0.1:7779/".to_string());
    let client = redis::Client::open(url)?;
    let mut con = client.get_connection()?;
    let old_msg: Vec<Vec<u8>> = con.keys("msg:*")?;
    let old_smsg: Vec<Vec<u8>> = con.keys("smsg:*")?;
    let mut old = old_msg;
    old.extend(old_smsg);
    if !old.is_empty() {
        let _: () = con.del(old)?;
    }
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
        let t = Instant::now();
        let _: () = con.set(valkey_key(id), value)?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let got: Option<Vec<u8>> = con.get(valkey_key(id as i64))?;
        assert!(got.is_some());
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let keys: Vec<Vec<u8>> = con.keys("msg:*")?;
        let values: Vec<Option<Vec<u8>>> = con.mget(&keys)?;
        let mut hits: Vec<(i64, i64, String)> = Vec::new();
        for (key, value) in keys.iter().zip(values.iter()) {
            let Some(value) = value else { continue };
            let (row_inbox, sent, is_read, body) = decode_row(value);
            if row_inbox == inbox && !is_read {
                let id: i64 = String::from_utf8_lossy(key)
                    .trim_start_matches("msg:")
                    .parse()
                    .unwrap_or(0);
                hits.push((sent, id, body));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        hits.truncate(10);
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16 {
        let t = Instant::now();
        let keys: Vec<Vec<u8>> = con.keys("msg:*")?;
        let values: Vec<Option<Vec<u8>>> = con.mget(&keys)?;
        let mut n = 0u64;
        for value in values.iter().flatten() {
            let (row_inbox, _, is_read, _) = decode_row(value);
            if row_inbox == inbox && !is_read {
                n += 1;
            }
        }
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let mut rows_batch: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for id in start..=end {
            let id = id as i64;
            let value = encode_row(id % 16, id, id % 3 == 0, &row_body(id as usize));
            rows_batch.push((valkey_key(id).into_bytes(), value));
        }
        let t = Instant::now();
        let mut pipe = redis::pipe();
        for (key, value) in &rows_batch {
            pipe.set(&key[..], &value[..]).ignore();
        }
        let _: () = pipe.query(&mut con)?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                let value = encode_row(id % 16, id, false, &format!("churn-{i:08}"));
                let _: () = con.set(valkey_key(id), value)?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                let value = encode_row(id % 16, id, false, &row_body(id as usize));
                let _: () = con.set(valkey_key(id), value)?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                let _: i32 = con.del(valkey_key(id))?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let mut logical = 0u64;
    let baseline = valkey_dataset_bytes(&mut con)?;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        let value = encode_row(id % 16, id, id % 3 == 0, &body);
        let _: () = con.set(format!("smsg:{id}"), value)?;
    }
    // The dataset bytes attributable to the storage load, measured as a
    // delta so the core-phase keys do not pollute the number.
    let disk = valkey_dataset_bytes(&mut con)?.saturating_sub(baseline);
    for id in (2..=rows).step_by(2) {
        let _: i32 = con.del(format!("smsg:{}", id as i64))?;
    }
    // Valkey has no compaction step for expired or deleted keys beyond
    // active expiry; freed memory is reused by later writes.
    let compacted = valkey_dataset_bytes(&mut con)?.saturating_sub(baseline);
    let leftover = keys_with_prefix(&mut con, "smsg:")?;
    if !leftover.is_empty() {
        let _: () = con.del(leftover)?;
    }
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(con);
    let t = Instant::now();
    let mut con = client.get_connection()?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let got: Option<Vec<u8>> = con.get(valkey_key(1))?;
    assert!(got.is_some());
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_mysql(rows: usize, sample: &mut Sample) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    use mysql::prelude::*;
    let url = std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:bench@127.0.0.1:3306/benchdb".to_string());
    let pool = mysql::Pool::new(url.as_str())?;
    let mut conn = pool.get_conn()?;
    conn.query_drop("DROP TABLE IF EXISTS messages")?;
    conn.query_drop(
        "CREATE TABLE messages (id BIGINT PRIMARY KEY, inbox_id BIGINT NOT NULL, sent_at BIGINT NOT NULL, is_read BOOLEAN NOT NULL, body TEXT NOT NULL)",
    )?;
    conn.query_drop(
        "CREATE INDEX idx_messages_inbox ON messages (inbox_id, is_read, sent_at DESC)",
    )?;
    let mut phases = Vec::new();
    let mut insert = Phase {
        name: "insert",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let id = id as i64;
        let body = row_body(id as usize);
        let t = Instant::now();
        conn.exec_drop(
            "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?,?,?,?,?)",
            (id, id % 16, id, id % 3 == 0, body),
        )?;
        insert.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(insert);
    let mut get = Phase {
        name: "get",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for id in 1..=rows {
        let t = Instant::now();
        let row: Option<(String,)> =
            conn.exec_first("SELECT body FROM messages WHERE id = ?", (id as i64,))?;
        assert!(row.map(|row| !row.0.is_empty()).unwrap_or(false));
        get.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(get);
    let mut scan = Phase {
        name: "scan",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16i64 {
        let t = Instant::now();
        let ids: Vec<i64> = conn.exec_map(
            "SELECT id FROM messages WHERE inbox_id = ? AND is_read = false ORDER BY sent_at DESC, id DESC LIMIT 10",
            (inbox,),
            |id| id,
        )?;
        let _ = ids;
        scan.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(scan);
    let mut count = Phase {
        name: "count",
        ops: 16,
        nanos: Vec::with_capacity(16),
    };
    for inbox in 0..16i64 {
        let t = Instant::now();
        let n: Option<(i64,)> = conn.exec_first(
            "SELECT COUNT(*) FROM messages WHERE inbox_id = ? AND is_read = false",
            (inbox,),
        )?;
        let _ = n;
        count.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(count);
    let mut batch = Phase {
        name: "batch",
        ops: 0,
        nanos: Vec::new(),
    };
    let mut start = rows + 1;
    while start <= 2 * rows {
        let end = (start + BATCH_ROWS - 1).min(2 * rows);
        let mut placeholders = Vec::new();
        let mut params: Vec<mysql::Value> = Vec::new();
        for id in start..=end {
            let id = id as i64;
            placeholders.push("(?,?,?,?,?)");
            params.push(id.into());
            params.push((id % 16).into());
            params.push(id.into());
            params.push((id % 3 == 0).into());
            params.push(row_body(id as usize).into());
        }
        let query = format!(
            "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES {}",
            placeholders.join(",")
        );
        let t = Instant::now();
        conn.exec_drop(&query[..], mysql::Params::Positional(params))?;
        batch.nanos.push(t.elapsed().as_nanos() as u64);
        batch.ops += 1;
        start = end + 1;
    }
    sample.observe();
    phases.push(batch);

    let mut churn = Phase {
        name: "churn",
        ops: rows,
        nanos: Vec::with_capacity(rows),
    };
    for i in 0..rows {
        let t = Instant::now();
        match i % 10 {
            0..=4 => {
                let id = (i % rows + 1) as i64;
                conn.exec_drop(
                    "UPDATE messages SET body = ?, is_read = false WHERE id = ?",
                    (format!("churn-{i:08}"), id),
                )?;
            }
            5..=7 => {
                let id = (2 * rows + i + 1) as i64;
                let body = row_body(id as usize);
                conn.exec_drop(
                    "INSERT INTO messages (id, inbox_id, sent_at, is_read, body) VALUES (?,?,?,?,?)",
                    (id, id % 16, id, false, body),
                )?;
            }
            _ => {
                let id = (i % rows + 1) as i64;
                conn.exec_drop("DELETE FROM messages WHERE id = ?", (id,))?;
            }
        }
        churn.nanos.push(t.elapsed().as_nanos() as u64);
    }
    sample.observe();
    phases.push(churn);

    let mut logical = 0u64;
    conn.query_drop("DROP TABLE IF EXISTS messages_storage")?;
    conn.query_drop(
        "CREATE TABLE messages_storage (id BIGINT PRIMARY KEY, inbox_id BIGINT NOT NULL, sent_at BIGINT NOT NULL, is_read BOOLEAN NOT NULL, body TEXT NOT NULL)",
    )?;
    for id in 1..=rows {
        let body = storage_body(id, rows);
        logical += logical_row_bytes(body.len());
        let id = id as i64;
        conn.exec_drop(
            "INSERT INTO messages_storage (id, inbox_id, sent_at, is_read, body) VALUES (?,?,?,?,?)",
            (id, id % 16, id, id % 3 == 0, body),
        )?;
    }
    // information_schema table statistics are cached and can lag a bulk
    // load, so force a recalculation before each measurement.
    conn.query_drop("ANALYZE TABLE messages_storage")?;
    let disk: Option<i64> = conn.query_first(
        "SELECT SUM(data_length + index_length) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'messages_storage'",
    )?;
    let disk = disk.unwrap_or(0).max(0) as u64;
    conn.exec_drop("DELETE FROM messages_storage WHERE id % 2 = 0", ())?;
    let _: () = conn
        .query_drop("OPTIMIZE TABLE messages_storage")
        .unwrap_or(());
    conn.query_drop("ANALYZE TABLE messages_storage")?;
    let compacted: Option<i64> = conn.query_first(
        "SELECT SUM(data_length + index_length) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'messages_storage'",
    )?;
    let compacted = compacted.unwrap_or(0).max(0) as u64;
    conn.query_drop("DROP TABLE messages_storage")?;
    sample.observe();
    let storage = StorageReport {
        rows,
        logical_bytes: logical,
        disk_bytes: disk,
        compacted_bytes: compacted,
    };

    drop(conn);
    let t = Instant::now();
    let mut conn = pool.get_conn()?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let row: Option<(String,)> = conn.exec_first("SELECT body FROM messages WHERE id = 1", ())?;
    assert!(row.map(|row| !row.0.is_empty()).unwrap_or(false));
    let cold_get_us = t.elapsed().as_nanos() as f64 / 1000.0;
    sample.observe();
    let reopen = ReopenReport {
        open_ms,
        cold_get_us,
    };
    Ok(EngineOutput {
        phases,
        storage: Some(storage),
        reopen: Some(reopen),
    })
}

fn run_cockroachdb(
    rows: usize,
    sample: &mut Sample,
) -> Result<EngineOutput, Box<dyn std::error::Error>> {
    let host = std::env::var("COCKROACH_ADDR").unwrap_or_else(|_| "127.0.0.1:26257".to_string());
    let admin_url = format!("postgres://root@{host}/defaultdb?sslmode=disable");
    let mut admin = postgres::Client::connect(&admin_url, postgres::NoTls)?;
    admin.batch_execute("CREATE DATABASE IF NOT EXISTS benchdb")?;
    drop(admin);
    let url = format!("postgres://root@{host}/benchdb?sslmode=disable");
    let mut client = postgres::Client::connect(&url, postgres::NoTls)?;
    client.batch_execute("DROP TABLE IF EXISTS messages")?;
    client.batch_execute(
        "CREATE TABLE messages (id BIGINT PRIMARY KEY, inbox_id BIGINT NOT NULL, sent_at BIGINT NOT NULL, is_read BOOLEAN NOT NULL, body TEXT NOT NULL);
         CREATE INDEX idx_messages_inbox ON messages (inbox_id, is_read, sent_at DESC);",
    )?;
    run_pg_phases(&mut client, rows, &url, sample, false)
}

fn usage() -> ! {
    eprintln!("usage: ls-bench --engine <lightshade|lightshade-client|lightshade-client-x16|redb|sqlite|postgres|fjall|heed|libsql|valkey|mysql|cockroachdb> --rows <N> --out <results.json>");
    std::process::exit(2);
}

fn main() {
    let mut engine = String::new();
    let mut rows = 0usize;
    let mut out = PathBuf::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--engine" => engine = args.next().unwrap_or_default(),
            "--rows" => rows = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--out" => out = PathBuf::from(args.next().unwrap_or_default()),
            _ => usage(),
        }
    }
    if rows == 0 || out.as_os_str().is_empty() {
        usage();
    }
    let result = run(&engine, rows);
    match result {
        Ok(json) => {
            std::fs::write(&out, &json).unwrap_or_else(|e| {
                eprintln!("write {}: {e}", out.display());
                std::process::exit(1);
            });
            println!("wrote {}", out.display());
        }
        Err(e) => {
            eprintln!("bench {engine} failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run(engine: &str, rows: usize) -> Result<String, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut sample = Sample::new();
    sample.observe();
    let output = match engine {
        "lightshade" => run_lightshade(dir.path(), rows, &mut sample).map_err(|e| e.to_string())?,
        "lightshade-client" => {
            run_lightshade_client(dir.path(), rows, &mut sample).map_err(|e| e.to_string())?
        }
        "lightshade-client-x16" => {
            run_lightshade_client_x16(dir.path(), rows, &mut sample).map_err(|e| e.to_string())?
        }
        "redb" => run_redb(dir.path(), rows, &mut sample)?,
        "sqlite" => run_sqlite(dir.path(), rows, &mut sample)?,
        "postgres" => run_postgres(dir.path(), rows, &mut sample)?,
        "fjall" => run_fjall(dir.path(), rows, &mut sample)?,
        "heed" => run_heed(dir.path(), rows, &mut sample)?,
        "libsql" => run_libsql(dir.path(), rows, &mut sample)?,
        "valkey" => run_valkey(rows, &mut sample)?,
        "mysql" => run_mysql(rows, &mut sample)?,
        "cockroachdb" => run_cockroachdb(rows, &mut sample)?,
        _ => usage(),
    };
    let phases = output.phases;
    let cpu = sample.peak_cpu;
    let mut sys = sample.sys;
    sys.refresh_cpu_all();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let mut json = format!(
        "{{\"engine\":\"{engine}\",\"rows\":{rows},\"os\":\"{}\",\"arch\":\"{}\",\"cpu\":{:?},\"cpus\":{},\"mem_mb\":{},",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cpu_brand,
        sys.cpus().len(),
        sys.total_memory() / 1024 / 1024,
    );
    json.push_str("\"phases\":{");
    for (i, phase) in phases.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("\"{}\":{}", phase.name, phase.report()));
    }
    json.push_str("},");
    match &output.storage {
        Some(storage) => json.push_str(&format!(
            "\"storage\":{{\"rows\":{},\"logical_bytes\":{},\"disk_bytes\":{},\"compacted_bytes\":{}}},",
            storage.rows, storage.logical_bytes, storage.disk_bytes, storage.compacted_bytes
        )),
        None => json.push_str("\"storage\":null,"),
    }
    match &output.reopen {
        Some(reopen) => json.push_str(&format!(
            "\"reopen\":{{\"open_ms\":{:.3},\"cold_get_us\":{:.1}}},",
            reopen.open_ms, reopen.cold_get_us
        )),
        None => json.push_str("\"reopen\":null,"),
    }
    json.push_str(&format!(
        "\"peak_rss_mb\":{:.1},\"cpu_pct\":{cpu:.1}}}",
        sample.peak_rss as f64 / 1024.0 / 1024.0
    ));
    Ok(json)
}
