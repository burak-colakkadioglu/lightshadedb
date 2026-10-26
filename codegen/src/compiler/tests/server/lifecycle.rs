use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[test]
fn concurrent_readers_share_a_db() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut db = Db::open_relaxed(&path).unwrap();
    for id in 0..200 {
        db.add_note(id, format!("note-{id}"), id % 10, id % 2 == 0).unwrap();
    }
    db.flush().unwrap();
    let db = std::sync::Arc::new(db);
    let mut workers = Vec::new();
    for _ in 0..8 {
        let db = std::sync::Arc::clone(&db);
        workers.push(std::thread::spawn(move || {
            for _ in 0..200 {
                assert_eq!(db.finished(), 100);
                assert_eq!(db.by_title("note-17".to_string()).len(), 1);
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!("{}\n{HARNESS}", generate_library(&workload).unwrap());
    let (directory, binary) = compile_generated_test(&source, "concurrent-readers");
    let database = directory.join("notes.lsdb");
    let output = Command::new(&binary)
        .arg(&database)
        .output()
        .expect("failed to run generated binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn server_group_commit_preserves_acknowledged_writes() {
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "group-commit");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let credentials = write_credentials(&directory);

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_CHECKPOINT_BYTES", "0")
        .env("LIGHTSHADE_GROUP_COMMIT_US", "20000")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start group-commit server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "group-commit server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut writers = Vec::new();
    for id in 0..16 {
        let binary = binary.clone();
        let socket = socket.clone();
        let credentials = credentials.clone();
        writers.push(std::thread::spawn(move || {
            let output = Command::new(&binary)
                .arg(&socket)
                .args([
                    "run",
                    "add_note",
                    &format!("id={id}"),
                    &format!("title=group-{id}"),
                    &format!("score={id}"),
                    "done=true",
                ])
                .env("LIGHTSHADE_CREDENTIALS", &credentials)
                .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
                .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
                .output()
                .expect("failed to run group-commit writer");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok true");
        }));
    }
    for writer in writers {
        writer.join().unwrap();
    }

    let wal = directory.join("notes.lsdb.wal");
    assert!(
        std::fs::metadata(&wal).unwrap().len() > 0,
        "acknowledged writes were still buffered"
    );

    server.kill().unwrap();
    let _ = server.wait();

    let output = Command::new(&binary)
        .arg(&database)
        .args(["run", "finished"])
        .output()
        .expect("failed to reopen group-commit database");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "16");
    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn sigterm_stops_the_server_cleanly_and_preserves_writes() {
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "shutdown");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let credentials = write_credentials(&directory);
    let audit = directory.join("audit.jsonl");

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_AUDIT_LOG", &audit)
        .env("LIGHTSHADE_SHUTDOWN_TIMEOUT_MS", "2000")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start shutdown server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "server never opened its socket");
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = Command::new(&binary)
        .arg(&socket)
        .args([
            "run",
            "add_note",
            "id=1",
            "title=shutdown",
            "score=1",
            "done=true",
        ])
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .output()
        .expect("failed to write through the server");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(server.id().to_string())
        .status()
        .expect("failed to send SIGTERM");
    assert!(killed.success(), "kill -TERM failed");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "server ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(status.success(), "server exited with {status}");
    assert!(!socket.exists(), "server left its endpoint behind");
    let events = std::fs::read_to_string(&audit).unwrap();
    assert!(events.contains("\"event\":\"auth_success\""));
    assert!(events.contains("\"event\":\"server_stop\""));

    let output = Command::new(&binary)
        .arg(&database)
        .args(["run", "finished"])
        .output()
        .expect("failed to reopen the database");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1");

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn sigterm_drain_timeout_bounds_idle_connections() {
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "shutdown-timeout");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let credentials = write_credentials(&directory);

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_IDLE_TIMEOUT_MS", "30000")
        .env("LIGHTSHADE_SHUTDOWN_TIMEOUT_MS", "200")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start shutdown-timeout server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "server never opened its socket");
        std::thread::sleep(Duration::from_millis(50));
    }

    let _idle = UnixStream::connect(&socket).expect("failed to open idle connection");
    let started = Instant::now();
    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(server.id().to_string())
        .status()
        .expect("failed to send SIGTERM");
    assert!(killed.success(), "kill -TERM failed");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "shutdown drain ignored its timeout"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(status.success(), "server exited with {status}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "shutdown drain exceeded its timeout"
    );
    assert!(!socket.exists(), "server left its endpoint behind");

    let _ = std::fs::remove_dir_all(&directory);
}
