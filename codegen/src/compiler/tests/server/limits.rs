use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[cfg(unix)]
#[test]
fn server_enforces_transaction_row_and_wal_quotas() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn wire_send(stream: &mut UnixStream, payload: &[u8]) {
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(payload).unwrap();
    }

    fn wire_receive(stream: &mut UnixStream) -> Vec<u8> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let mut payload = vec![0u8; u32::from_le_bytes(header) as usize];
        stream.read_exact(&mut payload).unwrap();
        payload
    }

    fn wire_connect(socket: &std::path::Path, fingerprint: u64) -> UnixStream {
        let mut stream = UnixStream::connect(socket).unwrap();
        let mut hello = b"LSRP".to_vec();
        hello.extend_from_slice(&3u32.to_le_bytes());
        hello.extend_from_slice(&fingerprint.to_le_bytes());
        push_credential(&mut hello, TEST_CREDENTIAL_ID, TEST_SECRET);
        wire_send(&mut stream, &hello);
        let response = wire_receive(&mut stream);
        assert_eq!(
            response[0],
            0,
            "{}",
            String::from_utf8_lossy(&response[5..])
        );
        stream
    }

    fn ok_body(response: &[u8]) -> &[u8] {
        assert_eq!(
            response[0],
            0,
            "{}",
            String::from_utf8_lossy(&response[5..])
        );
        &response[1..]
    }

    fn error_body(response: &[u8]) -> String {
        assert_eq!(response[0], 1);
        String::from_utf8(response[5..].to_vec()).unwrap()
    }

    fn add_note(id: i64, title: &str, score: i64, done: bool) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&0u32.to_le_bytes());
        request.extend_from_slice(&id.to_le_bytes());
        request.extend_from_slice(&(title.len() as u32).to_le_bytes());
        request.extend_from_slice(title.as_bytes());
        request.extend_from_slice(&score.to_le_bytes());
        request.push(u8::from(done));
        request
    }

    fn add_log(id: i64, message: &str) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&1u32.to_le_bytes());
        request.extend_from_slice(&id.to_le_bytes());
        request.extend_from_slice(&(message.len() as u32).to_le_bytes());
        request.extend_from_slice(message.as_bytes());
        request
    }

    fn delete_note(note: i64) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&8u32.to_le_bytes());
        request.extend_from_slice(&note.to_le_bytes());
        request
    }

    fn finished() -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&3u32.to_le_bytes());
        request
    }

    fn start_server(
        binary: &std::path::Path,
        credentials: &std::path::Path,
        directory: &std::path::Path,
        name: &str,
        envs: &[(&str, &str)],
    ) -> (std::path::PathBuf, std::path::PathBuf, std::process::Child) {
        let database = directory.join(format!("{name}.lsdb"));
        let socket = directory.join(format!("{name}.lsdb.sock"));
        let mut command = Command::new(binary);
        command
            .arg(&database)
            .arg("serve")
            .env("LIGHTSHADE_CREDENTIALS", credentials)
            .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
            .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        for (key, value) in envs {
            command.env(key, value);
        }
        let child = command.spawn().expect("failed to start quota server");
        let deadline = Instant::now() + Duration::from_secs(30);
        while UnixStream::connect(&socket).is_err() {
            assert!(
                Instant::now() < deadline,
                "quota server never opened its socket"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        (database, socket, child)
    }

    fn run(binary: &std::path::Path, target: &std::path::Path, args: &[&str]) -> (bool, String) {
        let output = Command::new(binary)
            .arg(target)
            .args(args)
            .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
            .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
            .output()
            .expect("failed to run quota interface");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "quotas");
    let fingerprint = workload_fingerprint(&workload);
    let credentials = write_credentials(&directory);

    let (_, transaction_socket, mut transaction_server) = start_server(
        &binary,
        &credentials,
        &directory,
        "transactions",
        &[
            ("LIGHTSHADE_MAX_TRANSACTIONS", "1"),
            ("LIGHTSHADE_MAX_CONNECTIONS", "4"),
        ],
    );
    let mut first = wire_connect(&transaction_socket, fingerprint);
    wire_send(&mut first, &[1u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    let mut second = wire_connect(&transaction_socket, fingerprint);
    wire_send(&mut second, &[1u8]);
    assert!(
        error_body(&wire_receive(&mut second)).contains("transaction quota exceeded"),
        "a second transaction was admitted past the quota"
    );
    wire_send(&mut first, &[3u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    wire_send(&mut second, &[1u8]);
    assert!(ok_body(&wire_receive(&mut second)).is_empty());
    drop(second);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut probe = wire_connect(&transaction_socket, fingerprint);
        wire_send(&mut probe, &[1u8]);
        let response = wire_receive(&mut probe);
        if response[0] == 0 {
            wire_send(&mut probe, &[3u8]);
            assert!(ok_body(&wire_receive(&mut probe)).is_empty());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a dropped transaction did not release its quota slot: {}",
            error_body(&response)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    transaction_server.kill().unwrap();
    let _ = transaction_server.wait();

    let (_, row_socket, mut row_server) = start_server(
        &binary,
        &credentials,
        &directory,
        "rows",
        &[("LIGHTSHADE_MAX_ROWS", "2")],
    );
    let mut rows = wire_connect(&row_socket, fingerprint);
    wire_send(&mut rows, &add_note(1, "one", 1, false));
    assert!(ok_body(&wire_receive(&mut rows)).len() == 1);
    wire_send(&mut rows, &add_note(2, "two", 2, false));
    assert!(ok_body(&wire_receive(&mut rows)).len() == 1);
    wire_send(&mut rows, &add_note(3, "three", 3, false));
    assert!(error_body(&wire_receive(&mut rows)).contains("row quota exceeded"));
    wire_send(&mut rows, &finished());
    assert_eq!(ok_body(&wire_receive(&mut rows)), &0i64.to_le_bytes());

    wire_send(&mut rows, &[1u8]);
    assert!(ok_body(&wire_receive(&mut rows)).is_empty());
    wire_send(&mut rows, &add_note(3, "three", 3, false));
    assert!(error_body(&wire_receive(&mut rows)).contains("row quota exceeded"));
    wire_send(&mut rows, &[3u8]);
    assert!(ok_body(&wire_receive(&mut rows)).is_empty());

    wire_send(&mut rows, &delete_note(1));
    assert_eq!(ok_body(&wire_receive(&mut rows)), &1u64.to_le_bytes());
    wire_send(&mut rows, &add_note(3, "three", 3, false));
    assert_eq!(ok_body(&wire_receive(&mut rows)), &[1u8]);
    let (success, output) = run(&binary, &row_socket, &["run", "by_title", "wanted=one"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");
    let (success, output) = run(&binary, &row_socket, &["run", "by_title", "wanted=three"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "3\tthree\t3\tfalse");
    row_server.kill().unwrap();
    let _ = row_server.wait();

    let (wal_database, wal_socket, mut wal_server) = start_server(
        &binary,
        &credentials,
        &directory,
        "wal",
        &[
            ("LIGHTSHADE_MAX_WAL_BYTES", "1"),
            ("LIGHTSHADE_CHECKPOINT_BYTES", "0"),
            ("LIGHTSHADE_WAL_PREALLOC_BYTES", "0"),
        ],
    );
    let mut wal = wire_connect(&wal_socket, fingerprint);
    wire_send(&mut wal, &add_note(1, "wal", 1, false));
    assert!(error_body(&wire_receive(&mut wal)).contains("WAL quota exceeded"));
    let (success, output) = run(&binary, &wal_socket, &["run", "by_title", "wanted=wal"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "", "a rejected write reached the database");

    wire_send(&mut wal, &[1u8]);
    assert!(ok_body(&wire_receive(&mut wal)).is_empty());
    wire_send(&mut wal, &add_note(1, "wal", 1, false));
    assert!(ok_body(&wire_receive(&mut wal)).len() == 1);
    wire_send(&mut wal, &[2u8]);
    assert!(error_body(&wire_receive(&mut wal)).contains("WAL quota exceeded"));
    let (success, output) = run(&binary, &wal_socket, &["run", "by_title", "wanted=wal"]);
    assert!(success, "{output}");
    assert_eq!(
        output.trim(),
        "",
        "a rejected transaction commit was applied"
    );
    wire_send(&mut wal, &[1u8]);
    assert!(ok_body(&wire_receive(&mut wal)).is_empty());
    wire_send(&mut wal, &[3u8]);
    assert!(ok_body(&wire_receive(&mut wal)).is_empty());
    assert_eq!(
        std::fs::metadata(wal_database.with_extension("lsdb.wal"))
            .map(|meta| meta.len())
            .unwrap_or(0),
        0
    );
    wal_server.kill().unwrap();
    let _ = wal_server.wait();

    let two_table_source = r#"
table notes {
  id: i64 primary_key
  title: text
  score: i64
  done: bool
}

table logs {
  id: i64 primary_key
  message: text
}

operation add_note(id: i64, title: text, score: i64, done: bool) {
  insert notes
}

operation add_log(id: i64, message: text) {
  insert logs
}

operation note_count() -> one i64 {
  count notes where id > 0
}

operation log_count() -> one i64 {
  count logs where id > 0
}
"#;
    let two_table_workload = parse_workload(two_table_source).unwrap();
    validate(&two_table_workload).unwrap();
    let two_table_fingerprint = workload_fingerprint(&two_table_workload);
    let two_table_source = generate_interface(&two_table_workload).unwrap();
    let (two_table_directory, two_table_binary) =
        compile_generated_test(&two_table_source, "quota-commit");
    let two_table_credentials = write_credentials(&two_table_directory);
    let (_, two_table_socket, mut two_table_server) = start_server(
        &two_table_binary,
        &two_table_credentials,
        &two_table_directory,
        "commit",
        &[("LIGHTSHADE_MAX_ROWS", "1")],
    );
    let mut notes_tx = wire_connect(&two_table_socket, two_table_fingerprint);
    wire_send(&mut notes_tx, &[1u8]);
    assert!(ok_body(&wire_receive(&mut notes_tx)).is_empty());
    wire_send(&mut notes_tx, &add_note(1, "note", 1, false));
    assert_eq!(ok_body(&wire_receive(&mut notes_tx)), &[1u8]);
    let mut logs_tx = wire_connect(&two_table_socket, two_table_fingerprint);
    wire_send(&mut logs_tx, &[1u8]);
    assert!(ok_body(&wire_receive(&mut logs_tx)).is_empty());
    wire_send(&mut logs_tx, &add_log(1, "log"));
    assert_eq!(ok_body(&wire_receive(&mut logs_tx)), &[1u8]);
    wire_send(&mut notes_tx, &[2u8]);
    assert!(ok_body(&wire_receive(&mut notes_tx)).is_empty());
    wire_send(&mut logs_tx, &[2u8]);
    assert!(error_body(&wire_receive(&mut logs_tx)).contains("row quota exceeded"));
    let mut check = wire_connect(&two_table_socket, two_table_fingerprint);
    let mut request = vec![0u8];
    request.extend_from_slice(&2u32.to_le_bytes());
    wire_send(&mut check, &request);
    assert_eq!(ok_body(&wire_receive(&mut check)), &1i64.to_le_bytes());
    let mut request = vec![0u8];
    request.extend_from_slice(&3u32.to_le_bytes());
    wire_send(&mut check, &request);
    assert_eq!(ok_body(&wire_receive(&mut check)), &0i64.to_le_bytes());
    two_table_server.kill().unwrap();
    let _ = two_table_server.wait();

    let _ = std::fs::remove_dir_all(&directory);
    let _ = std::fs::remove_dir_all(&two_table_directory);
}

#[cfg(unix)]
#[test]
fn tenants_are_isolated_across_servers_and_artifacts() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SECOND_SECRET: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const SECOND_SECRET_HASH: &str =
        "4f2e8d65483c641648cdb374ae9d8abd368d269e4ddffe092a8237b8162cddd6";

    fn wire_send(stream: &mut UnixStream, payload: &[u8]) {
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(payload).unwrap();
    }

    fn wire_receive(stream: &mut UnixStream) -> Vec<u8> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let mut payload = vec![0u8; u32::from_le_bytes(header) as usize];
        stream.read_exact(&mut payload).unwrap();
        payload
    }

    fn wire_connect_as(
        socket: &std::path::Path,
        fingerprint: u64,
        secret: &str,
    ) -> Result<UnixStream, String> {
        let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
        let mut hello = b"LSRP".to_vec();
        hello.extend_from_slice(&3u32.to_le_bytes());
        hello.extend_from_slice(&fingerprint.to_le_bytes());
        push_credential(&mut hello, TEST_CREDENTIAL_ID, secret);
        wire_send(&mut stream, &hello);
        let response = wire_receive(&mut stream);
        if response[0] != 0 {
            return Err(String::from_utf8_lossy(&response[5..]).to_string());
        }
        Ok(stream)
    }

    fn ok_body(response: &[u8]) -> &[u8] {
        assert_eq!(
            response[0],
            0,
            "{}",
            String::from_utf8_lossy(&response[5..])
        );
        &response[1..]
    }

    fn add_note(id: i64, title: &str, score: i64, done: bool) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&0u32.to_le_bytes());
        request.extend_from_slice(&id.to_le_bytes());
        request.extend_from_slice(&(title.len() as u32).to_le_bytes());
        request.extend_from_slice(title.as_bytes());
        request.extend_from_slice(&score.to_le_bytes());
        request.push(u8::from(done));
        request
    }

    fn write_second_credentials(directory: &std::path::Path) -> std::path::PathBuf {
        let path = directory.join("credentials-b");
        std::fs::write(
                &path,
                format!(
                    "reader\treader\t{SECOND_SECRET_HASH}\nwriter\twriter\t{SECOND_SECRET_HASH}\nadmin\tadmin\t{SECOND_SECRET_HASH}\n"
                ),
            )
            .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn start_server(
        binary: &std::path::Path,
        credentials: &std::path::Path,
        secret: &str,
        directory: &std::path::Path,
        name: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::process::Child) {
        let database = directory.join(format!("{name}.lsdb"));
        let socket = directory.join(format!("{name}.lsdb.sock"));
        let child = Command::new(binary)
            .arg(&database)
            .arg("serve")
            .env("LIGHTSHADE_CREDENTIALS", credentials)
            .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
            .env("LIGHTSHADE_CREDENTIAL_SECRET", secret)
            .env("LIGHTSHADE_TENANT", name)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start tenant server");
        let deadline = Instant::now() + Duration::from_secs(30);
        while UnixStream::connect(&socket).is_err() {
            assert!(
                Instant::now() < deadline,
                "tenant server never opened its socket"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        (database, socket, child)
    }

    fn run(
        binary: &std::path::Path,
        target: &std::path::Path,
        args: &[&str],
        secret: &str,
    ) -> (bool, String) {
        let output = Command::new(binary)
            .arg(target)
            .args(args)
            .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
            .env("LIGHTSHADE_CREDENTIAL_SECRET", secret)
            .output()
            .expect("failed to run tenant interface");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "tenants");
    let fingerprint = workload_fingerprint(&workload);
    let credentials_a = write_credentials(&directory);
    let credentials_b = write_second_credentials(&directory);

    let (database_a, socket_a, mut server_a) =
        start_server(&binary, &credentials_a, TEST_SECRET, &directory, "alpha");
    let (database_b, socket_b, mut server_b) =
        start_server(&binary, &credentials_b, SECOND_SECRET, &directory, "beta");

    let mut tenant_a = wire_connect_as(&socket_a, fingerprint, TEST_SECRET).unwrap();
    wire_send(&mut tenant_a, &add_note(1, "alpha-only", 1, false));
    assert_eq!(ok_body(&wire_receive(&mut tenant_a)), &[1u8]);
    let mut tenant_b = wire_connect_as(&socket_b, fingerprint, SECOND_SECRET).unwrap();
    wire_send(&mut tenant_b, &add_note(2, "beta-only", 2, false));
    assert_eq!(ok_body(&wire_receive(&mut tenant_b)), &[1u8]);

    let cross = wire_connect_as(&socket_b, fingerprint, TEST_SECRET).unwrap_err();
    assert!(cross.contains("authentication failed"), "{cross}");
    let cross = wire_connect_as(&socket_a, fingerprint, SECOND_SECRET).unwrap_err();
    assert!(cross.contains("authentication failed"), "{cross}");

    let (success, output) = run(
        &binary,
        &socket_a,
        &["run", "by_title", "wanted=beta-only"],
        TEST_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");
    let (success, output) = run(
        &binary,
        &socket_b,
        &["run", "by_title", "wanted=alpha-only"],
        SECOND_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");

    let backup_a = directory.join("alpha-backup.lsdb");
    let backup_b = directory.join("beta-backup.lsdb");
    let (success, output) = run(
        &binary,
        &database_a,
        &["backup", backup_a.to_str().unwrap()],
        TEST_SECRET,
    );
    assert!(success, "{output}");
    let (success, output) = run(
        &binary,
        &database_b,
        &["backup", backup_b.to_str().unwrap()],
        SECOND_SECRET,
    );
    assert!(success, "{output}");

    server_a.kill().unwrap();
    let _ = server_a.wait();
    server_b.kill().unwrap();
    let _ = server_b.wait();

    let (success, output) = run(
        &binary,
        &database_a,
        &["run", "by_title", "wanted=alpha-only"],
        TEST_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "1\talpha-only\t1\tfalse");
    let (success, output) = run(
        &binary,
        &database_b,
        &["run", "by_title", "wanted=beta-only"],
        SECOND_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "2\tbeta-only\t2\tfalse");
    let (success, output) = run(
        &binary,
        &backup_a,
        &["run", "by_title", "wanted=beta-only"],
        TEST_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");
    let (success, output) = run(
        &binary,
        &backup_b,
        &["run", "by_title", "wanted=alpha-only"],
        SECOND_SECRET,
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn server_transactions_detect_conflicts_and_survive_crash() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn wire_send(stream: &mut UnixStream, payload: &[u8]) {
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(payload).unwrap();
    }

    fn wire_receive(stream: &mut UnixStream) -> Vec<u8> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let mut payload = vec![0u8; u32::from_le_bytes(header) as usize];
        stream.read_exact(&mut payload).unwrap();
        payload
    }

    fn wire_connect(socket: &std::path::Path, fingerprint: u64) -> UnixStream {
        let mut stream = UnixStream::connect(socket).unwrap();
        let mut hello = b"LSRP".to_vec();
        hello.extend_from_slice(&3u32.to_le_bytes());
        hello.extend_from_slice(&fingerprint.to_le_bytes());
        push_credential(&mut hello, TEST_CREDENTIAL_ID, TEST_SECRET);
        wire_send(&mut stream, &hello);
        let response = wire_receive(&mut stream);
        assert_eq!(
            response[0],
            0,
            "{}",
            String::from_utf8_lossy(&response[5..])
        );
        stream
    }

    fn ok_body(response: &[u8]) -> &[u8] {
        assert_eq!(
            response[0],
            0,
            "{}",
            String::from_utf8_lossy(&response[5..])
        );
        &response[1..]
    }

    fn error_body(response: &[u8]) -> String {
        assert_eq!(response[0], 1);
        String::from_utf8(response[5..].to_vec()).unwrap()
    }

    fn add_note(id: i64, title: &str, score: i64, done: bool) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&0u32.to_le_bytes());
        request.extend_from_slice(&id.to_le_bytes());
        request.extend_from_slice(&(title.len() as u32).to_le_bytes());
        request.extend_from_slice(title.as_bytes());
        request.extend_from_slice(&score.to_le_bytes());
        request.push(u8::from(done));
        request
    }

    fn finish(note: i64) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&5u32.to_le_bytes());
        request.extend_from_slice(&note.to_le_bytes());
        request
    }

    fn finished() -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&3u32.to_le_bytes());
        request
    }

    fn delete_note(note: i64) -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&8u32.to_le_bytes());
        request.extend_from_slice(&note.to_le_bytes());
        request
    }

    fn finished_value(response: &[u8]) -> i64 {
        let body = ok_body(response);
        i64::from_le_bytes(body.try_into().unwrap())
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "transaction-conflicts");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let fingerprint = workload_fingerprint(&workload);
    let credentials = write_credentials(&directory);

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_CHECKPOINT_BYTES", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start transaction server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "transaction server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut setup = wire_connect(&socket, fingerprint);
    wire_send(&mut setup, &add_note(1, "alpha", 10, false));
    assert_eq!(ok_body(&wire_receive(&mut setup)), &[1u8]);

    let mut first = wire_connect(&socket, fingerprint);
    let mut second = wire_connect(&socket, fingerprint);
    wire_send(&mut first, &[1u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    wire_send(&mut second, &[1u8]);
    assert!(ok_body(&wire_receive(&mut second)).is_empty());

    wire_send(&mut first, &finished());
    assert_eq!(finished_value(&wire_receive(&mut first)), 0);

    wire_send(&mut second, &finish(1));
    assert_eq!(ok_body(&wire_receive(&mut second)), &1u64.to_le_bytes());
    wire_send(&mut second, &[2u8]);
    assert!(ok_body(&wire_receive(&mut second)).is_empty());

    wire_send(&mut first, &finished());
    assert_eq!(
        finished_value(&wire_receive(&mut first)),
        0,
        "a transaction observed a concurrent commit"
    );

    wire_send(&mut first, &add_note(2, "beta", 20, false));
    assert_eq!(ok_body(&wire_receive(&mut first)), &[1u8]);
    wire_send(&mut first, &[2u8]);
    let conflict = error_body(&wire_receive(&mut first));
    assert!(conflict.contains("write conflict"), "{conflict}");
    wire_send(&mut first, &[3u8]);
    assert!(error_body(&wire_receive(&mut first)).contains("no transaction"));

    let mut check = wire_connect(&socket, fingerprint);
    wire_send(&mut check, &finished());
    assert_eq!(finished_value(&wire_receive(&mut check)), 1);

    wire_send(&mut setup, &add_note(6, "zeta", 60, false));
    assert_eq!(ok_body(&wire_receive(&mut setup)), &[1u8]);
    let mut sixth = wire_connect(&socket, fingerprint);
    let mut seventh = wire_connect(&socket, fingerprint);
    wire_send(&mut sixth, &[1u8]);
    assert!(ok_body(&wire_receive(&mut sixth)).is_empty());
    wire_send(&mut seventh, &[1u8]);
    assert!(ok_body(&wire_receive(&mut seventh)).is_empty());
    wire_send(&mut sixth, &delete_note(6));
    assert_eq!(ok_body(&wire_receive(&mut sixth)), &1u64.to_le_bytes());
    wire_send(&mut seventh, &delete_note(6));
    assert_eq!(ok_body(&wire_receive(&mut seventh)), &1u64.to_le_bytes());
    wire_send(&mut sixth, &[2u8]);
    assert!(ok_body(&wire_receive(&mut sixth)).is_empty());
    wire_send(&mut seventh, &[2u8]);
    let lost_update = error_body(&wire_receive(&mut seventh));
    assert!(lost_update.contains("write conflict"), "{lost_update}");

    let mut eighth = wire_connect(&socket, fingerprint);
    wire_send(&mut eighth, &[1u8]);
    assert!(ok_body(&wire_receive(&mut eighth)).is_empty());
    wire_send(&mut eighth, &finished());
    assert_eq!(finished_value(&wire_receive(&mut eighth)), 1);
    wire_send(&mut setup, &add_note(7, "eta", 70, false));
    assert_eq!(ok_body(&wire_receive(&mut setup)), &[1u8]);
    wire_send(&mut eighth, &[2u8]);
    assert!(
        ok_body(&wire_receive(&mut eighth)).is_empty(),
        "a read-only transaction failed to commit"
    );

    let mut third = wire_connect(&socket, fingerprint);
    let mut fourth = wire_connect(&socket, fingerprint);
    wire_send(&mut third, &[1u8]);
    assert!(ok_body(&wire_receive(&mut third)).is_empty());
    wire_send(&mut fourth, &[1u8]);
    assert!(ok_body(&wire_receive(&mut fourth)).is_empty());
    wire_send(&mut third, &add_note(3, "gamma", 30, false));
    assert_eq!(ok_body(&wire_receive(&mut third)), &[1u8]);
    wire_send(&mut fourth, &add_note(4, "delta", 40, false));
    assert_eq!(ok_body(&wire_receive(&mut fourth)), &[1u8]);
    wire_send(&mut third, &[2u8]);
    assert!(ok_body(&wire_receive(&mut third)).is_empty());
    wire_send(&mut fourth, &[2u8]);
    let conflict = error_body(&wire_receive(&mut fourth));
    assert!(conflict.contains("write conflict"), "{conflict}");

    let mut fifth = wire_connect(&socket, fingerprint);
    wire_send(&mut fifth, &[1u8]);
    assert!(ok_body(&wire_receive(&mut fifth)).is_empty());
    wire_send(&mut fifth, &add_note(5, "epsilon", 50, true));
    assert_eq!(ok_body(&wire_receive(&mut fifth)), &[1u8]);
    wire_send(&mut fifth, &[2u8]);
    assert!(ok_body(&wire_receive(&mut fifth)).is_empty());

    assert!(
        std::fs::metadata(directory.join("notes.lsdb.wal"))
            .unwrap()
            .len()
            > 0
    );
    server.kill().unwrap();
    let _ = server.wait();

    let run = |args: &[&str]| -> String {
        let output = Command::new(&binary)
            .arg(&database)
            .args(args)
            .output()
            .expect("failed to inspect recovered transactions");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    assert_eq!(run(&["run", "finished"]), "2");
    assert_eq!(
        run(&["run", "by_title", "wanted=alpha"]),
        "1\talpha\t10\ttrue"
    );
    assert_eq!(run(&["run", "by_title", "wanted=beta"]), "");
    assert_eq!(
        run(&["run", "by_title", "wanted=gamma"]),
        "3\tgamma\t30\tfalse"
    );
    assert_eq!(run(&["run", "by_title", "wanted=delta"]), "");
    assert_eq!(
        run(&["run", "by_title", "wanted=epsilon"]),
        "5\tepsilon\t50\ttrue"
    );
    assert_eq!(run(&["run", "by_title", "wanted=zeta"]), "");
    assert_eq!(run(&["run", "by_title", "wanted=eta"]), "7\teta\t70\tfalse");

    let _ = std::fs::remove_dir_all(&directory);
}
