use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static GROUP_COMMIT_BATCHES: AtomicU64 = AtomicU64::new(0);
static WAL_BYTES_WRITTEN: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static SLOW_QUERIES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub connections: u64,
    pub group_commit_batches: u64,
    pub wal_bytes_written: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub slow_queries: u64,
}

impl Snapshot {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "lightshade_connections_total {}\n",
            self.connections
        ));
        out.push_str(&format!(
            "lightshade_group_commit_batches_total {}\n",
            self.group_commit_batches
        ));
        out.push_str(&format!(
            "lightshade_wal_bytes_written_total {}\n",
            self.wal_bytes_written
        ));
        out.push_str(&format!(
            "lightshade_cache_hits_total {}\n",
            self.cache_hits
        ));
        out.push_str(&format!(
            "lightshade_cache_misses_total {}\n",
            self.cache_misses
        ));
        out.push_str(&format!(
            "lightshade_slow_queries_total {}\n",
            self.slow_queries
        ));
        out
    }
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        connections: CONNECTIONS.load(Ordering::Relaxed),
        group_commit_batches: GROUP_COMMIT_BATCHES.load(Ordering::Relaxed),
        wal_bytes_written: WAL_BYTES_WRITTEN.load(Ordering::Relaxed),
        cache_hits: CACHE_HITS.load(Ordering::Relaxed),
        cache_misses: CACHE_MISSES.load(Ordering::Relaxed),
        slow_queries: SLOW_QUERIES.load(Ordering::Relaxed),
    }
}

pub fn exposition() -> String {
    snapshot().render()
}

pub fn reset() {
    CONNECTIONS.store(0, Ordering::Relaxed);
    GROUP_COMMIT_BATCHES.store(0, Ordering::Relaxed);
    WAL_BYTES_WRITTEN.store(0, Ordering::Relaxed);
    CACHE_HITS.store(0, Ordering::Relaxed);
    CACHE_MISSES.store(0, Ordering::Relaxed);
    SLOW_QUERIES.store(0, Ordering::Relaxed);
}

pub fn record_connection() {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_group_commit_batch() {
    GROUP_COMMIT_BATCHES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_wal_bytes(bytes: u64) {
    WAL_BYTES_WRITTEN.fetch_add(bytes, Ordering::Relaxed);
}

pub fn record_cache_hit() {
    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cache_miss() {
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_slow_query() {
    SLOW_QUERIES.fetch_add(1, Ordering::Relaxed);
}
