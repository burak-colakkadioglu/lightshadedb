use super::super::*;
use crate::ir::parse::parse_workload;

pub(crate) const TEST_CREDENTIAL_ID: &str = "writer";

pub(crate) const TEST_SECRET: &str =
    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

pub(crate) const TEST_SECRET_HASH: &str =
    "6c86c6aac5fb24bcf5d9939cb7d7d5645ce39418f449e03b262dd4fa14b4b92b";

pub(crate) fn write_credentials(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("credentials");
    std::fs::write(
            &path,
            format!(
                "# test credentials\nreader\treader\t{TEST_SECRET_HASH}\nwriter\twriter\t{TEST_SECRET_HASH}\nadmin\tadmin\t{TEST_SECRET_HASH}\n"
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

pub(crate) fn push_credential(hello: &mut Vec<u8>, id: &str, secret: &str) {
    hello.extend_from_slice(&(id.len() as u32).to_le_bytes());
    hello.extend_from_slice(id.as_bytes());
    hello.extend_from_slice(&(secret.len() as u32).to_le_bytes());
    hello.extend_from_slice(secret.as_bytes());
}

pub(crate) fn command_available(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub(crate) fn build_fault_shim(directory: &std::path::Path) -> Option<std::path::PathBuf> {
    if !command_available("cc") {
        return None;
    }
    let source = directory.join("fault_inject.c");
    let library = directory.join("liblightshade_fault.so");
    std::fs::write(
        &source,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int loaded;
static int write_after = -2;
static int write_calls;
static int tmp_write_after = -2;
static int tmp_write_calls;
static int sync_after = -2;
static int sync_calls;
static int rename_after = -2;
static int rename_calls;
static int partial_write;

static void load_faults(void) {
    if (loaded) return;
    loaded = 1;
    const char *value = getenv("LS_FAULT_WRITE_AFTER");
    if (value && *value) write_after = atoi(value);
    value = getenv("LS_FAULT_PARTIAL_WRITE");
    if (value && *value) partial_write = atoi(value);
    value = getenv("LS_FAULT_TMP_WRITE_AFTER");
    if (value && *value) tmp_write_after = atoi(value);
    value = getenv("LS_FAULT_SYNC_AFTER");
    if (value && *value) sync_after = atoi(value);
    value = getenv("LS_FAULT_RENAME_AFTER");
    if (value && *value) rename_after = atoi(value);
}

static int suffix_is(int fd, const char *suffix) {
    char link[64];
    char target[4096];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t length = readlink(link, target, sizeof(target) - 1);
    if (length <= 0) return 0;
    target[length] = '\0';
    size_t size = strlen(target);
    size_t suffix_size = strlen(suffix);
    return size >= suffix_size && strcmp(target + size - suffix_size, suffix) == 0;
}

static int is_wal(int fd) {
    return suffix_is(fd, ".wal");
}

ssize_t write(int fd, const void *buffer, size_t count) {
    static ssize_t (*real_write)(int, const void *, size_t);
    if (!real_write) real_write = dlsym(RTLD_NEXT, "write");
    load_faults();
    if (write_after >= 0 && is_wal(fd) && write_calls++ >= write_after) {
        if (partial_write && count > 1) {
            real_write(fd, buffer, count / 2);
        }
        errno = EIO;
        return -1;
    }
    if (tmp_write_after >= 0 && suffix_is(fd, ".tmp") && tmp_write_calls++ >= tmp_write_after) {
        if (partial_write && count > 1) {
            real_write(fd, buffer, count / 2);
        }
        errno = EIO;
        return -1;
    }
    return real_write(fd, buffer, count);
}

static int fail_sync(void) {
    load_faults();
    return sync_after >= 0 && sync_calls++ >= sync_after;
}

int fsync(int fd) {
    static int (*real_fsync)(int);
    if (!real_fsync) real_fsync = dlsym(RTLD_NEXT, "fsync");
    if (fail_sync()) {
        errno = EIO;
        return -1;
    }
    return real_fsync(fd);
}

int fdatasync(int fd) {
    static int (*real_fdatasync)(int);
    if (!real_fdatasync) real_fdatasync = dlsym(RTLD_NEXT, "fdatasync");
    if (fail_sync()) {
        errno = EIO;
        return -1;
    }
    return real_fdatasync(fd);
}

int rename(const char *from, const char *to) {
    static int (*real_rename)(const char *, const char *);
    if (!real_rename) real_rename = dlsym(RTLD_NEXT, "rename");
    load_faults();
    size_t length = strlen(from);
    if (rename_after >= 0 && length >= 4 && strcmp(from + length - 4, ".tmp") == 0
        && rename_calls++ >= rename_after) {
        errno = EIO;
        return -1;
    }
    return real_rename(from, to);
}
"#,
    )
    .unwrap();
    let output = std::process::Command::new("cc")
        .arg("-shared")
        .arg("-fPIC")
        .arg("-O2")
        .arg("-o")
        .arg(&library)
        .arg(&source)
        .arg("-ldl")
        .output()
        .expect("failed to run cc");
    assert!(
        output.status.success(),
        "cc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(library)
}

pub(crate) const SOURCE: &str = include_str!("../../../../examples/messages.lightshade");

pub(crate) const INBOX_SOURCE: &str = include_str!("../../../../examples/inbox_digest.lightshade");

pub(crate) const BENCH_SOURCE: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
}

operation add_note(id: i64, title: text, score: i64, done: bool) {
  insert notes
}

operation finish(note: i64) {
  update notes set done = true where id == note
}

operation by_title(wanted: text) -> many notes {
  filter title == wanted
  order by id asc
}

operation finished() -> one i64 {
  count notes where done == true
}

table logs {
  id: i64 primary_key
  message: text
}

operation add_log(id: i64, message: text) {
  insert logs
}
"#;

pub(crate) const ALT_SOURCE: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
}

operation add_note(id: i64, title: text, score: i64, done: bool) {
  insert notes
}

operation search_notes(min: i64, q: text) -> many notes {
  filter score >= min and title != q
  order by score asc, id asc
  limit 10
}

operation by_title(wanted: text) -> many notes {
  filter title == wanted
  order by id asc
}

operation finished() -> one i64 {
  count notes where done == true
}

operation either(lo: i64, hi: i64) -> many notes {
  filter score < lo or score > hi
}

operation finish(note: i64) {
  update notes set done = true where id == note
}

operation mark_all_done() {
  update notes set done = true where score >= 0
}

operation retitle(title: text) {
  update notes set title = title where score >= 0
}

operation delete_note(note: i64) {
  delete notes where id == note
}

operation purge_low(threshold: i64) {
  delete notes where score < threshold
}

operation clear_notes() {
  delete notes
}
"#;

pub(crate) fn workload() -> Workload {
    let workload = parse_workload(SOURCE).unwrap();
    validate(&workload).unwrap();
    workload
}

pub(crate) fn compile_generated_test(
    source: &str,
    name: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::process::Command;

    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (std::process::id(), name).hash(&mut hasher);
    let directory = std::env::temp_dir().join(format!("ls-{:08x}", hasher.finish() as u32));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("test.rs");
    let binary_path = directory.join("test");
    std::fs::write(&source_path, source).unwrap();

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(rustc)
        .arg("--edition=2021")
        .arg("-O")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("failed to run rustc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "rustc failed:\n{stderr}");
    (directory, binary_path)
}

#[cfg(unix)]
pub(crate) fn run_limited(
    binary: &std::path::Path,
    mode: &str,
    database: &std::path::Path,
) -> std::process::Output {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("trap '' XFSZ; ulimit -f 8; exec \"$0\" \"$@\"")
        .arg(binary)
        .arg(mode)
        .arg(database)
        .output()
        .expect("failed to run limited generated binary")
}

#[cfg(unix)]
pub(crate) fn bench_send(stream: &mut std::os::unix::net::UnixStream, payload: &[u8]) {
    use std::io::Write;
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .unwrap();
    stream.write_all(payload).unwrap();
}

#[cfg(unix)]
pub(crate) fn bench_receive(stream: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    use std::io::Read;
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).unwrap();
    let mut payload = vec![0u8; u32::from_le_bytes(header) as usize];
    stream.read_exact(&mut payload).unwrap();
    payload
}

#[cfg(unix)]
pub(crate) fn bench_call(stream: &mut std::os::unix::net::UnixStream, request: &[u8]) -> Vec<u8> {
    bench_send(stream, request);
    bench_receive(stream)
}

#[cfg(unix)]
pub(crate) fn bench_status(response: &[u8]) -> Result<&[u8], String> {
    if response[0] == 0 {
        Ok(&response[1..])
    } else {
        Err(String::from_utf8_lossy(&response[5..]).to_string())
    }
}

#[cfg(unix)]
pub(crate) fn bench_connect(
    socket: &std::path::Path,
    fingerprint: u64,
) -> std::os::unix::net::UnixStream {
    let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(120)));
    let mut stream = stream;
    let mut hello = b"LSRP".to_vec();
    hello.extend_from_slice(&3u32.to_le_bytes());
    hello.extend_from_slice(&fingerprint.to_le_bytes());
    push_credential(&mut hello, TEST_CREDENTIAL_ID, TEST_SECRET);
    let response = bench_call(&mut stream, &hello);
    if let Err(error) = bench_status(&response) {
        panic!("benchmark handshake failed: {error}");
    }
    stream
}

#[cfg(unix)]
pub(crate) fn bench_text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

#[cfg(unix)]
pub(crate) fn bench_begin() -> Vec<u8> {
    vec![1u8]
}

#[cfg(unix)]
pub(crate) fn bench_commit() -> Vec<u8> {
    vec![2u8]
}

#[cfg(unix)]
pub(crate) fn bench_add_note(index: u32, id: i64, title: &str, score: i64, done: bool) -> Vec<u8> {
    let mut request = vec![0u8];
    request.extend_from_slice(&index.to_le_bytes());
    request.extend_from_slice(&id.to_le_bytes());
    bench_text(&mut request, title);
    request.extend_from_slice(&score.to_le_bytes());
    request.push(u8::from(done));
    request
}

#[cfg(unix)]
pub(crate) fn bench_add_log(index: u32, id: i64, message: &str) -> Vec<u8> {
    let mut request = vec![0u8];
    request.extend_from_slice(&index.to_le_bytes());
    request.extend_from_slice(&id.to_le_bytes());
    bench_text(&mut request, message);
    request
}

#[cfg(unix)]
pub(crate) fn bench_by_title(index: u32, title: &str) -> Vec<u8> {
    let mut request = vec![0u8];
    request.extend_from_slice(&index.to_le_bytes());
    bench_text(&mut request, title);
    request
}

#[cfg(unix)]
pub(crate) fn bench_empty_operation(index: u32) -> Vec<u8> {
    let mut request = vec![0u8];
    request.extend_from_slice(&index.to_le_bytes());
    request
}

#[cfg(unix)]
pub(crate) fn bench_index(workload: &Workload, name: &str) -> u32 {
    workload
        .operations
        .iter()
        .position(|operation| operation.name == name)
        .unwrap_or_else(|| panic!("benchmark workload has no operation `{name}`")) as u32
}

#[cfg(unix)]
pub(crate) fn bench_percentile(values: &mut [u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = (((values.len() - 1) as f64) * percentile).round() as usize;
    values[index]
}

#[cfg(unix)]
pub(crate) fn bench_micros(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1_000.0
}

#[cfg(unix)]
pub(crate) fn bench_seed(socket: &std::path::Path, fingerprint: u64, add_note: u32, rows: usize) {
    let mut stream = bench_connect(socket, fingerprint);
    let response = bench_call(&mut stream, &bench_begin());
    bench_status(&response).unwrap();
    for row in 0..rows {
        let response = bench_call(
            &mut stream,
            &bench_add_note(
                add_note,
                row as i64 + 1,
                &format!("seed-{row}"),
                row as i64,
                false,
            ),
        );
        let body = bench_status(&response).unwrap();
        assert_eq!(body, &[1u8]);
    }
    let response = bench_call(&mut stream, &bench_commit());
    bench_status(&response).unwrap();
}

#[cfg(unix)]
pub(crate) fn bench_client_scaling(
    socket: &std::path::Path,
    fingerprint: u64,
    add_note: u32,
    finished: u32,
    by_title: u32,
) {
    use std::sync::{Arc, Barrier};

    const OPERATIONS_PER_CLIENT: usize = 200;
    const WARMUP: usize = 5;

    println!();
    println!("C4 autocommit client scaling (5,000 seeded rows, 1 ms group-commit window)");
    println!("| clients | read % | ops/s | p50 us | p95 us | p99 us | errors |");
    println!("|---:|---:|---:|---:|---:|---:|---:|");

    for clients in [1usize, 4, 16, 64] {
        for read_percent in [0usize, 50, 100] {
            let base = 1_000_000 + (clients * 10 + read_percent) * 100_000;
            let barrier = Arc::new(Barrier::new(clients + 1));
            let mut workers = Vec::with_capacity(clients);
            for client in 0..clients {
                let socket = socket.to_path_buf();
                let barrier = Arc::clone(&barrier);
                workers.push(std::thread::spawn(move || {
                    let mut stream = bench_connect(&socket, fingerprint);
                    for warmup in 0..WARMUP {
                        let request = if warmup.is_multiple_of(2) {
                            bench_empty_operation(finished)
                        } else {
                            bench_add_note(
                                add_note,
                                base as i64 + 90_000 + (client * WARMUP + warmup) as i64,
                                &format!("warm-{client}-{warmup}"),
                                0,
                                false,
                            )
                        };
                        let response = bench_call(&mut stream, &request);
                        bench_status(&response).unwrap();
                    }
                    barrier.wait();
                    let mut latencies = Vec::with_capacity(OPERATIONS_PER_CLIENT);
                    let mut errors = 0usize;
                    for operation in 0..OPERATIONS_PER_CLIENT {
                        let write = (operation % 100) >= read_percent;
                        let read_counter = operation.is_multiple_of(2);
                        let request = if write {
                            bench_add_note(
                                add_note,
                                base as i64 + (client * 1_000 + operation) as i64,
                                &format!("w-{client}-{operation}"),
                                0,
                                false,
                            )
                        } else if read_counter {
                            bench_empty_operation(finished)
                        } else {
                            bench_by_title(by_title, &format!("seed-{client}"))
                        };
                        let start = std::time::Instant::now();
                        let response = bench_call(&mut stream, &request);
                        latencies.push(start.elapsed().as_nanos() as u64);
                        match bench_status(&response) {
                            Ok(body) => {
                                if write {
                                    assert_eq!(body, &[1u8]);
                                } else if read_counter {
                                    assert_eq!(body.len(), 8);
                                }
                            }
                            Err(_) => errors += 1,
                        }
                    }
                    (latencies, errors)
                }));
            }
            barrier.wait();
            let start = std::time::Instant::now();
            let mut latencies = Vec::with_capacity(clients * OPERATIONS_PER_CLIENT);
            let mut errors = 0usize;
            for worker in workers {
                let (mut client_latencies, client_errors) = worker.join().unwrap();
                latencies.append(&mut client_latencies);
                errors += client_errors;
            }
            let elapsed = start.elapsed();
            let throughput = (clients * OPERATIONS_PER_CLIENT) as f64 / elapsed.as_secs_f64();
            let p50 = bench_percentile(&mut latencies, 0.50);
            let p95 = bench_percentile(&mut latencies, 0.95);
            let p99 = bench_percentile(&mut latencies, 0.99);
            println!(
                    "| {clients} | {read_percent} | {throughput:.0} | {:.1} | {:.1} | {:.1} | {errors} |",
                    bench_micros(p50),
                    bench_micros(p95),
                    bench_micros(p99)
                );
            assert_eq!(errors, 0, "autocommit benchmark saw errors");
        }
    }
}

#[cfg(unix)]
pub(crate) fn bench_long_read_transactions(
    socket: &std::path::Path,
    fingerprint: u64,
    finished: u32,
    clients: usize,
) {
    use std::sync::{Arc, Barrier};

    const TRANSACTIONS: usize = 10;
    const READS: usize = 50;

    let barrier = Arc::new(Barrier::new(clients + 1));
    let mut workers = Vec::with_capacity(clients);
    for _ in 0..clients {
        let socket = socket.to_path_buf();
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            let mut stream = bench_connect(&socket, fingerprint);
            let mut begins = Vec::with_capacity(TRANSACTIONS);
            let mut commits = Vec::with_capacity(TRANSACTIONS);
            barrier.wait();
            for _ in 0..TRANSACTIONS {
                let start = std::time::Instant::now();
                let response = bench_call(&mut stream, &bench_begin());
                bench_status(&response).unwrap();
                begins.push(start.elapsed().as_nanos() as u64);
                for _ in 0..READS {
                    let response = bench_call(&mut stream, &bench_empty_operation(finished));
                    assert_eq!(bench_status(&response).unwrap().len(), 8);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                let start = std::time::Instant::now();
                let response = bench_call(&mut stream, &bench_commit());
                bench_status(&response).unwrap();
                commits.push(start.elapsed().as_nanos() as u64);
            }
            (begins, commits)
        }));
    }
    barrier.wait();
    let start = std::time::Instant::now();
    let mut begins = Vec::new();
    let mut commits = Vec::new();
    for worker in workers {
        let (mut worker_begins, mut worker_commits) = worker.join().unwrap();
        begins.append(&mut worker_begins);
        commits.append(&mut worker_commits);
    }
    let elapsed = start.elapsed();
    let begin_p50 = bench_percentile(&mut begins, 0.50);
    let begin_p95 = bench_percentile(&mut begins, 0.95);
    let commit_p50 = bench_percentile(&mut commits, 0.50);
    let commit_p95 = bench_percentile(&mut commits, 0.95);
    println!(
        "| {clients} | {TRANSACTIONS} | {READS} | {:.1} | {:.1} | {:.1} | {:.1} | {:.0} |",
        bench_micros(begin_p50),
        bench_micros(begin_p95),
        bench_micros(commit_p50),
        bench_micros(commit_p95),
        (clients * TRANSACTIONS) as f64 / elapsed.as_secs_f64()
    );
}

#[cfg(unix)]
pub(crate) fn bench_write_transactions(
    socket: &std::path::Path,
    fingerprint: u64,
    add_note: u32,
    add_log: u32,
    mode: &str,
    clients: usize,
    rounds: usize,
) -> (usize, usize, usize) {
    use std::sync::{Arc, Barrier};

    const WRITES_PER_TRANSACTION: usize = 4;

    let barrier = Arc::new(Barrier::new(clients));
    let start_barrier = Arc::new(Barrier::new(clients + 1));
    let split = mode == "disjoint-tables";
    let mut workers = Vec::with_capacity(clients);
    for client in 0..clients {
        let socket = socket.to_path_buf();
        let barrier = Arc::clone(&barrier);
        let start_barrier = Arc::clone(&start_barrier);
        workers.push(std::thread::spawn(move || {
            let mut stream = bench_connect(&socket, fingerprint);
            let mut successes = 0usize;
            let mut conflicts = 0usize;
            let mut failures = 0usize;
            let mut commits = Vec::new();
            start_barrier.wait();
            for round in 0..rounds {
                let response = bench_call(&mut stream, &bench_begin());
                let began = bench_status(&response).is_ok();
                if began {
                    for write in 0..WRITES_PER_TRANSACTION {
                        let id = if split {
                            if client == 1 {
                                103_000_000 + (round * 10 + write) as i64
                            } else {
                                102_000_000 + (round * 10 + write) as i64
                            }
                        } else {
                            100_000_000 + (client * 1_000 + round * 10 + write) as i64
                        };
                        let request = if split && client == 1 {
                            bench_add_log(add_log, id, &format!("l-{round}-{write}"))
                        } else {
                            bench_add_note(
                                add_note,
                                id,
                                &format!("t-{client}-{round}-{write}"),
                                0,
                                false,
                            )
                        };
                        let response = bench_call(&mut stream, &request);
                        match bench_status(&response) {
                            Ok(body) if body == [1u8] => {}
                            Ok(_) => failures += 1,
                            Err(_) => failures += 1,
                        }
                    }
                } else {
                    failures += 1;
                }
                barrier.wait();
                if began {
                    let start = std::time::Instant::now();
                    let response = bench_call(&mut stream, &bench_commit());
                    let commit_ns = start.elapsed().as_nanos() as u64;
                    match bench_status(&response) {
                        Ok(_) => {
                            successes += 1;
                            commits.push(commit_ns);
                        }
                        Err(error) if error.contains("write conflict") => conflicts += 1,
                        Err(_) => failures += 1,
                    }
                }
                barrier.wait();
            }
            (successes, conflicts, failures, commits)
        }));
    }
    start_barrier.wait();
    let start = std::time::Instant::now();
    let mut successes = 0usize;
    let mut conflicts = 0usize;
    let mut failures = 0usize;
    let mut commits = Vec::new();
    for worker in workers {
        let (worker_successes, worker_conflicts, worker_failures, mut worker_commits) =
            worker.join().unwrap();
        successes += worker_successes;
        conflicts += worker_conflicts;
        failures += worker_failures;
        commits.append(&mut worker_commits);
    }
    let elapsed = start.elapsed();
    let p50 = bench_percentile(&mut commits, 0.50);
    let p95 = bench_percentile(&mut commits, 0.95);
    println!(
            "| {mode} | {clients} | {rounds} | {successes} | {conflicts} | {failures} | {:.1} | {:.1} | {:.0} |",
            bench_micros(p50),
            bench_micros(p95),
            successes as f64 / elapsed.as_secs_f64()
        );
    (successes, conflicts, failures)
}

pub(crate) const EXPANDED_SOURCE: &str = r#"
table all_types {
  id: uuid primary_key
  small: i8
  tiny: u16
  big: u64
  half: f32
  real: f64
  flag: bool
  label: text
  blob: bytes
}

operation add_all(id: uuid, small: i8, tiny: u16, big: u64, half: f32, real: f64, flag: bool, label: text, blob: bytes) {
  insert all_types
}

operation scan() -> many all_types {
  order by small asc, id asc
}

operation floats(min: f64) -> many all_types {
  filter real >= min and half < 10.0
  order by real desc, id asc
}

operation retag(real: f64, label: text) {
  update all_types set real = real, label = label where small < 0
}

operation huge() -> one i64 {
  count all_types where big == 18446744073709551615
}
"#;

pub(crate) const MIGRATION_OLD: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
  origin: text
}

operation add_note(id: i64, title: text, score: i64, done: bool, origin: text) {
  insert notes
}
"#;

pub(crate) const SEED_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "seed" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false, "legacy".to_string()).unwrap();
            db.add_note(2, "beta".to_string(), 20, true, "legacy".to_string()).unwrap();
            db.close().unwrap();
        }
        "seed_dirty" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(3, "gamma".to_string(), 30, false, "legacy".to_string()).unwrap();
            std::process::exit(0);
        }
        "recover" => {
            let mut db = Db::open(path).unwrap();
            db.checkpoint().unwrap();
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const CRASH_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.close().unwrap();
        }
        "spin_autocommit" => {
            let mut db = Db::open(path).unwrap();
            let mut next = db.t_notes.keys().max().unwrap_or(0) + 1;
            loop {
                db.add_note(next, "x".repeat(512), next, false).unwrap();
                println!("ack {next}");
                next += 1;
            }
        }
        "spin_transaction" => {
            let mut db = Db::open(path).unwrap();
            let mut next = db.t_notes.keys().max().unwrap_or(0) + 1;
            loop {
                db.begin().unwrap();
                for _ in 0..8 {
                    db.add_note(next, "x".repeat(512), next, false).unwrap();
                    next += 1;
                }
                db.commit().unwrap();
                println!("committed {}", next - 1);
            }
        }
        "spin_checkpoint" => {
            let mut db = Db::open(path).unwrap();
            let mut next = db.t_notes.keys().max().unwrap_or(0) + 1;
            loop {
                for _ in 0..4 {
                    db.add_note(next, "x".repeat(512), next, false).unwrap();
                    next += 1;
                }
                db.checkpoint().unwrap();
                println!("snapshot {}", next - 1);
            }
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            let mut ids: Vec<i64> = db.t_notes.keys().collect();
            ids.sort_unstable();
            let dense =
                !ids.is_empty() && ids[0] == 1 && ids.len() as i64 == *ids.last().unwrap();
            println!("rows {}", ids.len());
            println!("dense {dense}");
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const DAMAGE_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "a".to_string(), 1, false).unwrap();
            db.add_note(2, "b".to_string(), 2, true).unwrap();
            db.close().unwrap();
        }
        "make" => {
            let mut db = Db::open_relaxed(path).unwrap();
            for round in 0..2i64 {
                db.begin().unwrap();
                for step in 0..2i64 {
                    let id = 3 + round * 2 + step;
                    db.add_note(id, "c".to_string(), id, false).unwrap();
                }
                db.commit().unwrap();
            }
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            let mut ids: Vec<i64> = db.t_notes.keys().collect();
            ids.sort_unstable();
            let dense =
                !ids.is_empty() && ids[0] == 1 && ids.len() as i64 == *ids.last().unwrap();
            println!("rows {}", ids.len());
            println!("dense {dense}");
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const ERROR_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.close().unwrap();
        }
        "autocommit" => {
            let mut db = Db::open(path).unwrap();
            let mut inserted = 0i64;
            loop {
                match db.add_note(3 + inserted, "x".repeat(24), inserted, false) {
                    Ok(true) => inserted += 1,
                    Ok(false) => panic!("duplicate primary key"),
                    Err(error) => {
                        println!("write err {error}");
                        break;
                    }
                }
            }
            println!("inserted {inserted}");
            match db.add_note(50_000, "after".to_string(), 0, false) {
                Ok(_) => println!("after ok"),
                Err(error) => println!("after err {error}"),
            }
            println!("rows {}", db.t_notes.len());
            println!("finished {}", db.finished());
        }
        "transaction" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            for index in 0..400i64 {
                db.add_note(100_000 + index, "z".repeat(24), index, false).unwrap();
            }
            match db.commit() {
                Ok(()) => panic!("commit unexpectedly succeeded"),
                Err(error) => println!("commit err {error}"),
            }
            match db.commit() {
                Ok(()) => panic!("commit without a transaction succeeded"),
                Err(error) => println!("again {error}"),
            }
            match db.flush() {
                Ok(()) => println!("flush ok"),
                Err(error) => println!("flush err {error}"),
            }
        }
        "recover" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            db.add_note(500_000, "small".to_string(), 0, true).unwrap();
            db.commit().unwrap();
            db.close().unwrap();
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            println!("rows {}", db.t_notes.len());
            println!("finished {}", db.finished());
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const FAULT_HARNESS: &str = r#"
fn report(db: &Db) {
    let mut ids: Vec<i64> = db.t_notes.keys().collect();
    ids.sort_unstable();
    let dense = !ids.is_empty() && ids[0] == 1 && ids.len() as i64 == *ids.last().unwrap();
    println!("rows {}", ids.len());
    println!("dense {dense}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.close().unwrap();
        }
        "append" => {
            let mut db = Db::open(path).unwrap();
            let next = db.t_notes.keys().max().unwrap_or(0) + 1;
            match db.add_note(next, "x".repeat(128), next, false) {
                Ok(true) => println!("ack {next}"),
                Ok(false) => println!("duplicate {next}"),
                Err(error) => println!("write err {error}"),
            }
            std::process::exit(0);
        }
        "checkpoint" => {
            let mut db = Db::open(path).unwrap();
            let next = db.t_notes.keys().max().unwrap_or(0) + 1;
            db.add_note(next, "x".repeat(128), next, false).unwrap();
            match db.checkpoint() {
                Ok(()) => println!("checkpoint ok"),
                Err(error) => println!("checkpoint err {error}"),
            }
            std::process::exit(0);
        }
        "fill" => {
            let mut db = Db::open(path).unwrap();
            for id in 0..200i64 {
                db.add_note(id + 1, "x".repeat(512), id, false).unwrap();
            }
            db.close().unwrap();
        }
        "verify" => {
            let db = Db::open(path).unwrap();
            report(&db);
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const MIGRATION_BAD: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
  tag: text
  weight: i64 default 7
  flair: bool default true
}

operation add_note(id: i64, title: text, score: i64, done: bool, tag: text, weight: i64, flair: bool) {
  insert notes
}
"#;

pub(crate) const MIGRATION_HARNESS: &str = r#"
fn dump(db: &Db) {
    for (id, row) in &db.t_notes {
        println!(
            "row {} {} {} {} {} {} {}",
            id, row.title, row.score, row.done, row.tag, row.weight, row.flair
        );
    }
    println!("rows {}", db.t_notes.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "strict" => match Db::open(path) {
            Ok(db) => {
                let _ = db.close();
                println!("opened");
            }
            Err(error) => println!("{error}"),
        },
        "migrate" => match Db::open_migrated(path) {
            Ok(db) => {
                dump(&db);
                let _ = db.close();
            }
            Err(error) => println!("error {error}"),
        },
        "reader" => match Db::open_read_only(path) {
            Ok(db) => dump(&db),
            Err(error) => println!("error {error}"),
        },
        "verify" => {
            let db = Db::open(path).unwrap();
            dump(&db);
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const MIGRATION_NEW: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
  tag: text default "misc"
  weight: i64 default 7
  flair: bool default true
}

operation add_note(id: i64, title: text, score: i64, done: bool, tag: text, weight: i64, flair: bool) {
  insert notes
}
"#;

pub(crate) const OPEN_HARNESS: &str = r#"
fn main() {
    let path = std::env::args().nth(1).unwrap();
    match Db::open(path) {
        Ok(db) => {
            let _ = db.close();
            println!("opened");
        }
        Err(error) => println!("{error}"),
    }
}
"#;

pub(crate) const PERSIST_HARNESS: &str = r#"
fn dump(db: &Db) {
    for (id, row) in &db.t_notes {
        println!("row {} {} {} {}", id, row.title, row.score, row.done);
    }
    println!("finished {}", db.finished());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "write" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.add_note(3, "gamma".to_string(), 15, false).unwrap();
            db.close().unwrap();
        }
        "mutate" => {
            let mut db = Db::open(path).unwrap();
            db.finish(1).unwrap();
            db.retitle("zeta".to_string()).unwrap();
            db.delete_note(2).unwrap();
            db.close().unwrap();
        }
        "crash" => {
            let mut db = Db::open(path).unwrap();
            db.mark_all_done().unwrap();
            db.retitle("omega".to_string()).unwrap();
            db.purge_low(16).unwrap();
            std::process::exit(0);
        }
        "relaxed_write" => {
            let mut db = Db::open_relaxed(path).unwrap();
            db.add_note(4, "delta".to_string(), 30, false).unwrap();
            db.flush().unwrap();
            std::process::exit(0);
        }
        "open_only" => {
            let db = Db::open(path).unwrap();
            drop(db);
        }
        "open_close" => {
            let db = Db::open(path).unwrap();
            db.close().unwrap();
        }
        "lock" => {
            let db = Db::open(path).unwrap();
            match Db::open(path) {
                Ok(other) => {
                    drop(other);
                    println!("opened unexpectedly");
                }
                Err(error) => println!("{error}"),
            }
            db.close().unwrap();
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            dump(&db);
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const READER_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "a".to_string(), 1, false).unwrap();
            db.add_note(2, "b".to_string(), 2, true).unwrap();
            db.close().unwrap();
        }
        "writer_append" => {
            let mut db = Db::open(path).unwrap();
            let mut next = db.t_notes.keys().max().unwrap_or(0) + 1;
            loop {
                db.begin().unwrap();
                for _ in 0..8 {
                    db.add_note(next, "x".repeat(128), next, false).unwrap();
                    next += 1;
                }
                db.commit().unwrap();
                println!("committed {}", next - 1);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        "writer_checkpoint" => {
            let mut db = Db::open(path).unwrap();
            let mut next = db.t_notes.keys().max().unwrap_or(0) + 1;
            let mut round = 0u64;
            loop {
                for _ in 0..4 {
                    db.add_note(next, "x".repeat(128), next, false).unwrap();
                    next += 1;
                }
                println!("wrote {}", next - 1);
                round += 1;
                if round % 2 == 0 {
                    db.checkpoint().unwrap();
                    println!("checkpointed {}", next - 1);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        "reader" => {
            let db = Db::open_read_only(path).unwrap();
            let mut ids: Vec<i64> = db.t_notes.keys().collect();
            ids.sort_unstable();
            let dense =
                !ids.is_empty() && ids[0] == 1 && ids.len() as i64 == *ids.last().unwrap();
            println!("rows {}", ids.len());
            println!("dense {dense}");
        }
        "reader_write_rejected" => {
            let mut db = Db::open_read_only(path).unwrap();
            match db.add_note(999_999, "nope".to_string(), 0, false) {
                Ok(_) => println!("write ok"),
                Err(error) => println!("write rejected {error}"),
            }
            match db.begin() {
                Ok(()) => println!("begin ok"),
                Err(error) => println!("begin rejected {error}"),
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const TXN_HARNESS: &str = r#"
fn dump(db: &Db) {
    for (id, row) in &db.t_notes {
        println!("row {} {} {} {}", id, row.title, row.score, row.done);
    }
    println!("finished {}", db.finished());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "base" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.close().unwrap();
        }
        "commit" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            db.add_note(3, "gamma".to_string(), 15, false).unwrap();
            db.add_note(4, "delta".to_string(), 30, false).unwrap();
            db.finish(3).unwrap();
            db.commit().unwrap();
            std::process::exit(0);
        }
        "abort" => {
            let mut db = Db::open_relaxed(path).unwrap();
            db.begin().unwrap();
            db.add_note(5, "epsilon".to_string(), 40, false).unwrap();
            db.finish(1).unwrap();
            std::process::exit(0);
        }
        "rollback" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            db.add_note(6, "zeta".to_string(), 50, false).unwrap();
            db.add_note(7, "eta".to_string(), 60, true).unwrap();
            db.rollback().unwrap();
            db.close().unwrap();
        }
        "rollback_after_autocommit" => {
            let mut db = Db::open_relaxed(path).unwrap();
            db.add_note(8, "theta".to_string(), 70, false).unwrap();
            db.begin().unwrap();
            db.add_note(9, "iota".to_string(), 80, false).unwrap();
            db.rollback().unwrap();
            db.close().unwrap();
        }
        "errors" => {
            let mut db = Db::open_relaxed(path).unwrap();
            println!("commit {}", db.commit().unwrap_err());
            println!("rollback {}", db.rollback().unwrap_err());
            db.begin().unwrap();
            println!("begin {}", db.begin().unwrap_err());
            println!("checkpoint {}", db.checkpoint().unwrap_err());
            println!("flush {}", db.flush().unwrap_err());
            db.commit().unwrap();
            db.begin().unwrap();
            println!("close {}", db.close().unwrap_err());
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            dump(&db);
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

pub(crate) const TYPE_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args[2].as_str();
    let mut db = Db::open(path).unwrap();
    db.add_note(1, "alpha".to_string(), "ten".to_string(), false).unwrap();
    db.close().unwrap();
}
"#;

pub(crate) const TYPE_OLD: &str = r#"
table notes {
  id: i64 primary_key
  title: text
  score: text
  done: bool
}

operation add_note(id: i64, title: text, score: text, done: bool) {
  insert notes
}
"#;

#[cfg(unix)]
pub(crate) struct BenchServer(pub(crate) std::process::Child);

#[cfg(unix)]
impl Drop for BenchServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
