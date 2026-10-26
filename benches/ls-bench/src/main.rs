use lightshade::prelude::*;
use redb::{ReadableDatabase, ReadableTable};
use std::path::PathBuf;
use std::time::Instant;

#[lightshade::schema]
pub mod store {
    use lightshade::prelude::*;

    pub struct Message {
        #[lightshade(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshade::operations]
    pub trait Messages {
        #[lightshade::query(
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            order = (Message::sent_at.desc(), Message::id.desc()),
            limit = limit,
        )]
        fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;

        #[lightshade::query(
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

fn run_lightshade(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> lightshade::Result<Vec<Phase>> {
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
    db.close()?;
    Ok(phases)
}

fn run_lightshade_client(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> lightshade::Result<Vec<Phase>> {
    use lightshade::secret::{Credential, Role};
    use lightshade::{ClientOptions, Credentials, CredentialsSource, Secret};
    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "bench".to_string(),
        secret: Secret::new("bench-secret"),
        role: Role::Writer,
    });
    let mut options = lightshade::ServeOptions::new(CredentialsSource::Static(credentials));
    options.listen = lightshade::Listen::tcp_local(0);
    let server = store::Server::start(db, options)?;
    let addr = server.local_addr().expect("bound TCP address");
    let client_options =
        ClientOptions::new(lightshade::ClientCredentials::new("bench", "bench-secret"));
    let mut client = store::Client::connect_tcp(addr, client_options)?;
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
    client.close()?;
    server.shutdown();
    server.join()?;
    Ok(phases)
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
) -> lightshade::Result<Vec<Phase>> {
    use lightshade::secret::{Credential, Role};
    use lightshade::{ClientOptions, Credentials, CredentialsSource, Secret};
    let db = store::Db::open(dir.join("bench.lsdb"))?;
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "bench".to_string(),
        secret: Secret::new("bench-secret"),
        role: Role::Writer,
    });
    let mut options = lightshade::ServeOptions::new(CredentialsSource::Static(credentials));
    options.listen = lightshade::Listen::tcp_local(0);
    let server = store::Server::start(db, options)?;
    let addr = server.local_addr().expect("bound TCP address");
    let threads: usize = std::env::var("LS_BENCH_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let chunk = rows.div_ceil(threads);
    let insert_handles: Vec<std::thread::JoinHandle<lightshade::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let start = t * chunk;
            let end = ((t + 1) * chunk).min(rows);
            let options =
                ClientOptions::new(lightshade::ClientCredentials::new("bench", "bench-secret"));
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
    let get_handles: Vec<std::thread::JoinHandle<lightshade::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let start = t * chunk;
            let end = ((t + 1) * chunk).min(rows);
            let options =
                ClientOptions::new(lightshade::ClientCredentials::new("bench", "bench-secret"));
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
    type ScanPair = lightshade::Result<(Vec<u64>, Vec<u64>)>;
    let scan_handles: Vec<std::thread::JoinHandle<ScanPair>> = (0..threads)
        .map(|t| {
            let options =
                ClientOptions::new(lightshade::ClientCredentials::new("bench", "bench-secret"));
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
    let batch_handles: Vec<std::thread::JoinHandle<lightshade::Result<Vec<u64>>>> = (0..threads)
        .map(|t| {
            let options =
                ClientOptions::new(lightshade::ClientCredentials::new("bench", "bench-secret"));
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
    Ok(phases)
}

fn run_postgres(
    _dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<Vec<Phase>, Box<dyn std::error::Error>> {
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
    drop(client);
    pg.stop()?;
    Ok(phases)
}

fn run_redb(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<Vec<Phase>, Box<dyn std::error::Error>> {
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
    Ok(phases)
}

fn run_sqlite(
    dir: &std::path::Path,
    rows: usize,
    sample: &mut Sample,
) -> Result<Vec<Phase>, Box<dyn std::error::Error>> {
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
    Ok(phases)
}

fn usage() -> ! {
    eprintln!("usage: ls-bench --engine <lightshade|lightshade-client|lightshade-client-x16|redb|sqlite|postgres> --rows <N> --out <results.json>");
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
    let phases = match engine {
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
        _ => usage(),
    };
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
    json.push_str(&format!(
        "}},\"peak_rss_mb\":{:.1},\"cpu_pct\":{cpu:.1}}}",
        sample.peak_rss as f64 / 1024.0 / 1024.0
    ));
    Ok(json)
}
