use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[cfg(unix)]
#[test]
#[ignore = "C4 performance gate; run in release with --ignored --nocapture"]
fn server_concurrency_benchmark() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let workload = parse_workload(BENCH_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "concurrency-benchmark");
    let database = directory.join("bench.lsdb");
    let socket = directory.join("bench.lsdb.sock");
    let fingerprint = workload_fingerprint(&workload);
    let credentials = write_credentials(&directory);
    let add_note = bench_index(&workload, "add_note");
    let add_log = bench_index(&workload, "add_log");
    let finished = bench_index(&workload, "finished");
    let by_title = bench_index(&workload, "by_title");

    let mut command = Command::new(&binary);
    command
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_CHECKPOINT_BYTES", "0")
        .env("LIGHTSHADE_GROUP_COMMIT_US", "1000")
        .env("LIGHTSHADE_LOCK_TIMEOUT_MS", "10000")
        .env("LIGHTSHADE_MAX_CONNECTIONS", "128")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let server = BenchServer(command.spawn().expect("failed to start benchmark server"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "benchmark server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }

    bench_seed(&socket, fingerprint, add_note, 5_000);
    println!();
    println!(
            "C4 benchmark: {} logical CPUs, 5,000 seeded rows, group commit 1 ms, checkpointing disabled",
            std::thread::available_parallelism().map_or(1, |count| count.get())
        );

    bench_client_scaling(&socket, fingerprint, add_note, finished, by_title);

    println!();
    println!(
        "C4 long read-only transactions (5,000 seeded rows plus autocommit writes, 10 ms hold)"
    );
    println!("| clients | tx/client | reads/tx | begin p50 us | begin p95 us | commit p50 us | commit p95 us | commit/s |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|");
    bench_long_read_transactions(&socket, fingerprint, finished, 1);
    bench_long_read_transactions(&socket, fingerprint, finished, 16);

    println!();
    println!("C4 write transactions (4 writes/tx, 20 synchronized rounds)");
    println!("| mode | clients | rounds | successes | conflicts | failures | commit p50 us | commit p95 us | commits/s |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let (successes, conflicts, failures) =
        bench_write_transactions(&socket, fingerprint, add_note, add_log, "same-table", 8, 20);
    assert_eq!(successes, 20);
    assert_eq!(conflicts, 140);
    assert_eq!(failures, 0);

    let (successes, conflicts, failures) = bench_write_transactions(
        &socket,
        fingerprint,
        add_note,
        add_log,
        "disjoint-tables",
        2,
        20,
    );
    assert_eq!(successes, 40);
    assert_eq!(conflicts, 0);
    assert_eq!(failures, 0);

    drop(server);
    let _ = std::fs::remove_dir_all(&directory);
}
