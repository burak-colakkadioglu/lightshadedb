use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[cfg(unix)]
#[test]
fn server_serves_typed_operations_over_a_socket() {
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

    fn wire_connect_as(
        socket: &std::path::Path,
        fingerprint: u64,
        id: &str,
        secret: &str,
    ) -> Result<UnixStream, String> {
        let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
        let mut hello = b"LSRP".to_vec();
        hello.extend_from_slice(&3u32.to_le_bytes());
        hello.extend_from_slice(&fingerprint.to_le_bytes());
        push_credential(&mut hello, id, secret);
        wire_send(&mut stream, &hello);
        let response = wire_receive(&mut stream);
        if response[0] != 0 {
            return Err(String::from_utf8_lossy(&response[5..]).to_string());
        }
        Ok(stream)
    }

    fn wire_connect(socket: &std::path::Path, fingerprint: u64) -> Result<UnixStream, String> {
        wire_connect_as(socket, fingerprint, TEST_CREDENTIAL_ID, TEST_SECRET)
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

    fn finished() -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&3u32.to_le_bytes());
        request
    }

    fn finished_value(response: &[u8]) -> i64 {
        let body = ok_body(response);
        i64::from_le_bytes(body.try_into().unwrap())
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "server");
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
        .env("LIGHTSHADE_LOCK_TIMEOUT_MS", "1500")
        .env("LIGHTSHADE_CHECKPOINT_BYTES", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "server never opened its socket");
        std::thread::sleep(Duration::from_millis(50));
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "unix socket is not private");
    }

    let run = |target: &std::path::Path, args: &[&str]| -> (bool, String) {
        let output = Command::new(&binary)
            .arg(target)
            .args(args)
            .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
            .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
            .output()
            .expect("failed to run interface binary");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };

    let (success, output) = run(
        &socket,
        &[
            "run",
            "add_note",
            "id=1",
            "title=alpha",
            "score=10",
            "done=false",
        ],
    );
    assert!(success, "{output}");
    assert_eq!(output.trim(), "ok true");
    let (success, output) = run(
        &socket,
        &[
            "run",
            "add_note",
            "id=2",
            "title=beta",
            "score=20",
            "done=true",
        ],
    );
    assert!(success, "{output}");

    let live_backup = directory.join("live-backup.lsdb");
    let live_backup_arg = live_backup.to_str().unwrap().to_string();
    let (success, output) = run(&database, &["backup", live_backup_arg.as_str()]);
    assert!(success, "{output}");
    let (success, output) = run(&live_backup, &["run", "by_title", "wanted=beta"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "2\tbeta\t20\ttrue");

    let (success, output) = run(&socket, &["run", "finished"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "1");
    let (success, output) = run(&socket, &["run", "by_title", "wanted=alpha"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "1\talpha\t10\tfalse");

    let mut raw = UnixStream::connect(&socket).unwrap();
    let mut hello = b"LSRP".to_vec();
    hello.extend_from_slice(&3u32.to_le_bytes());
    hello.extend_from_slice(&(fingerprint ^ 1).to_le_bytes());
    push_credential(&mut hello, TEST_CREDENTIAL_ID, TEST_SECRET);
    wire_send(&mut raw, &hello);
    let response = wire_receive(&mut raw);
    assert_eq!(response[0], 1);
    assert!(
        error_body(&response).contains("different workload"),
        "{}",
        error_body(&response)
    );
    drop(raw);

    let mut raw = UnixStream::connect(&socket).unwrap();
    let mut hello = b"LSRP".to_vec();
    hello.extend_from_slice(&3u32.to_le_bytes());
    hello.extend_from_slice(&fingerprint.to_le_bytes());
    push_credential(&mut hello, TEST_CREDENTIAL_ID, "wrong-secret");
    wire_send(&mut raw, &hello);
    let response = wire_receive(&mut raw);
    assert_eq!(response[0], 1);
    assert!(
        error_body(&response).contains("authentication failed"),
        "{}",
        error_body(&response)
    );
    drop(raw);

    let mut reader = wire_connect_as(&socket, fingerprint, "reader", TEST_SECRET).unwrap();
    wire_send(&mut reader, &finished());
    assert_eq!(finished_value(&wire_receive(&mut reader)), 1);
    wire_send(&mut reader, &add_note(99, "denied", 99, false));
    assert!(
        error_body(&wire_receive(&mut reader)).contains("permission denied"),
        "a reader was allowed to mutate"
    );
    wire_send(&mut reader, &[1u8]);
    assert!(ok_body(&wire_receive(&mut reader)).is_empty());
    wire_send(&mut reader, &add_note(100, "denied", 100, false));
    assert!(error_body(&wire_receive(&mut reader)).contains("permission denied"));
    wire_send(&mut reader, &[2u8]);
    assert!(ok_body(&wire_receive(&mut reader)).is_empty());
    drop(reader);

    let (success, output) = run(&socket, &["run", "by_title", "wanted=denied"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");

    let mut first = wire_connect(&socket, fingerprint).unwrap();
    wire_send(&mut first, &[1u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    wire_send(&mut first, &[1u8]);
    assert!(error_body(&wire_receive(&mut first)).contains("already active"));

    let mut second = wire_connect(&socket, fingerprint).unwrap();
    wire_send(&mut second, &finished());
    assert_eq!(
        finished_value(&wire_receive(&mut second)),
        1,
        "a reader waited for an unrelated open transaction"
    );

    wire_send(&mut first, &add_note(3, "gamma", 30, false));
    assert_eq!(ok_body(&wire_receive(&mut first)), &[1u8]);
    wire_send(&mut first, &finished());
    assert_eq!(finished_value(&wire_receive(&mut first)), 1);

    wire_send(&mut second, &finished());
    assert_eq!(
        finished_value(&wire_receive(&mut second)),
        1,
        "an uncommitted transaction became visible to a reader"
    );

    wire_send(&mut first, &[2u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    wire_send(&mut second, &finished());
    assert_eq!(finished_value(&wire_receive(&mut second)), 1);
    assert_eq!(
        std::fs::metadata(directory.join("notes.lsdb.wal"))
            .unwrap()
            .len(),
        0,
        "the WAL was not checkpointed after a commit"
    );

    wire_send(&mut first, &[3u8]);
    assert!(error_body(&wire_receive(&mut first)).contains("no transaction"));

    wire_send(&mut first, &[1u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());
    wire_send(&mut second, &finished());
    assert_eq!(
        finished_value(&wire_receive(&mut second)),
        1,
        "a read blocked behind another connection's transaction"
    );
    wire_send(&mut first, &[3u8]);
    assert!(ok_body(&wire_receive(&mut first)).is_empty());

    let mut dropped = wire_connect(&socket, fingerprint).unwrap();
    wire_send(&mut dropped, &[1u8]);
    assert!(ok_body(&wire_receive(&mut dropped)).is_empty());
    wire_send(&mut dropped, &add_note(4, "delta", 40, false));
    assert_eq!(ok_body(&wire_receive(&mut dropped)), &[1u8]);
    wire_send(&mut dropped, &finished());
    assert_eq!(finished_value(&wire_receive(&mut dropped)), 1);
    drop(dropped);

    let mut check = wire_connect(&socket, fingerprint).unwrap();
    wire_send(&mut check, &finished());
    assert_eq!(
        finished_value(&wire_receive(&mut check)),
        1,
        "disconnecting committed an uncommitted transaction"
    );
    drop(check);

    let (success, output) = run(&socket, &["run", "by_title", "wanted=gamma"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "3\tgamma\t30\tfalse");
    let (success, output) = run(&socket, &["run", "by_title", "wanted=delta"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "");

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    let mut workers = Vec::new();
    for id in 10..18 {
        let socket = socket.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let mut stream = wire_connect(&socket, fingerprint).unwrap();
            wire_send(
                &mut stream,
                &add_note(id, &format!("concurrent-{id}"), id, true),
            );
            assert_eq!(ok_body(&wire_receive(&mut stream)), &[1u8]);
        }));
    }
    for _ in 0..8 {
        let socket = socket.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let mut stream = wire_connect(&socket, fingerprint).unwrap();
            for _ in 0..25 {
                wire_send(&mut stream, &finished());
                let count = finished_value(&wire_receive(&mut stream));
                assert!((1..=9).contains(&count), "unexpected count {count}");
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    let mut check = wire_connect(&socket, fingerprint).unwrap();
    wire_send(&mut check, &finished());
    assert_eq!(finished_value(&wire_receive(&mut check)), 9);
    let (success, output) = run(&socket, &["run", "by_title", "wanted=concurrent-17"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "17\tconcurrent-17\t17\ttrue");

    server.kill().unwrap();
    let _ = server.wait();

    let (success, output) = run(&database, &["run", "finished"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "9", "acknowledged writes were lost");
    let (success, output) = run(&database, &["run", "by_title", "wanted=concurrent-17"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "17\tconcurrent-17\t17\ttrue");

    let limited_database = directory.join("limited.lsdb");
    let limited_socket = directory.join("limited.lsdb.sock");
    let mut limited_server = Command::new(&binary)
        .arg(&limited_database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_IDLE_TIMEOUT_MS", "250")
        .env("LIGHTSHADE_MAX_CONNECTIONS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start limited server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !limited_socket.exists() {
        assert!(
            Instant::now() < deadline,
            "limited server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut idle = wire_connect(&limited_socket, fingerprint).unwrap();
    wire_send(&mut idle, &[1u8]);
    assert!(ok_body(&wire_receive(&mut idle)).is_empty());
    let busy = wire_connect(&limited_socket, fingerprint).unwrap_err();
    assert!(busy.contains("server is busy"), "{busy}");
    std::thread::sleep(Duration::from_millis(750));
    idle.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut byte = [0u8; 1];
        match idle.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => panic!("unexpected data on idle connection"),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "the idle connection was not closed"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("{error}"),
        }
    }
    drop(idle);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(mut stream) = wire_connect(&limited_socket, fingerprint) {
            wire_send(&mut stream, &[1u8]);
            let response = wire_receive(&mut stream);
            if response[0] == 0 {
                wire_send(&mut stream, &[3u8]);
                assert!(ok_body(&wire_receive(&mut stream)).is_empty());
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the closed connection did not release its server slot"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    limited_server.kill().unwrap();
    let _ = limited_server.wait();

    let slow_database = directory.join("slow.lsdb");
    let slow_socket = directory.join("slow.lsdb.sock");
    let mut slow_server = Command::new(&binary)
        .arg(&slow_database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .env("LIGHTSHADE_HANDSHAKE_TIMEOUT_MS", "250")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start handshake server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !slow_socket.exists() {
        assert!(
            Instant::now() < deadline,
            "handshake server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut slow = UnixStream::connect(&slow_socket).unwrap();
    slow.write_all(&[0u8]).unwrap();
    std::thread::sleep(Duration::from_millis(750));
    slow.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut byte = [0u8; 1];
        match slow.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => panic!("unexpected data from half-open handshake"),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "a half-open handshake was not closed after the handshake deadline"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("{error}"),
        }
    }

    slow_server.kill().unwrap();
    let _ = slow_server.wait();

    let tcp_database = directory.join("tcp.lsdb");
    let tcp_socket = directory.join("tcp.lsdb.sock");
    let mut tcp_server = Command::new(&binary)
        .arg(&tcp_database)
        .arg("serve")
        .env("LIGHTSHADE_LISTEN", "127.0.0.1:0")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start tcp server");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let address = std::fs::read_to_string(&tcp_socket).unwrap_or_default();
        if address.trim().parse::<std::net::SocketAddr>().is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tcp server never wrote its address"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&tcp_socket).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "tcp endpoint is not private");
    }
    let (success, output) = run(
        &tcp_socket,
        &[
            "run",
            "add_note",
            "id=7",
            "title=tcp",
            "score=70",
            "done=true",
        ],
    );
    assert!(success, "{output}");
    let (success, output) = run(&tcp_socket, &["run", "by_title", "wanted=tcp"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "7\ttcp\t70\ttrue");
    tcp_server.kill().unwrap();
    let _ = tcp_server.wait();

    let unsafe_output = Command::new(&binary)
        .arg(directory.join("unsafe.lsdb"))
        .arg("serve")
        .env("LIGHTSHADE_LISTEN", "0.0.0.0:0")
        .env_remove("LIGHTSHADE_CREDENTIALS")
        .output()
        .expect("failed to run unsafe server");
    assert!(!unsafe_output.status.success());
    assert!(
        String::from_utf8_lossy(&unsafe_output.stderr)
            .contains("cannot load credentials: LIGHTSHADE_CREDENTIALS is required"),
        "{}",
        String::from_utf8_lossy(&unsafe_output.stderr)
    );

    let loopback_output = Command::new(&binary)
        .arg(directory.join("loopback.lsdb"))
        .arg("serve")
        .env("LIGHTSHADE_LISTEN", "127.0.0.1:0")
        .env_remove("LIGHTSHADE_CREDENTIALS")
        .output()
        .expect("failed to run loopback server");
    assert!(!loopback_output.status.success());
    assert!(
        String::from_utf8_lossy(&loopback_output.stderr)
            .contains("cannot load credentials: LIGHTSHADE_CREDENTIALS is required"),
        "{}",
        String::from_utf8_lossy(&loopback_output.stderr)
    );

    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let public_output = Command::new(&binary)
        .arg(directory.join("public.lsdb"))
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .output()
        .expect("failed to run public-credentials server");
    assert!(!public_output.status.success());
    assert!(
        String::from_utf8_lossy(&public_output.stderr)
            .contains("must not be readable by group or others"),
        "{}",
        String::from_utf8_lossy(&public_output.stderr)
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn lsrp_framing_rejects_malformed_frames_without_killing_the_server() {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn send_frame(stream: &mut UnixStream, payload: &[u8]) {
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(payload).unwrap();
    }
    fn receive_frame(stream: &mut UnixStream) -> Option<Vec<u8>> {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut header = [0u8; 4];
        if stream.read_exact(&mut header).is_err() {
            return None;
        }
        let mut payload = vec![0u8; u32::from_le_bytes(header) as usize];
        if stream.read_exact(&mut payload).is_err() {
            return None;
        }
        Some(payload)
    }
    fn connect(socket: &std::path::Path) -> UnixStream {
        UnixStream::connect(socket).unwrap()
    }
    fn handshake(fingerprint: u64) -> Vec<u8> {
        let mut hello = b"LSRP".to_vec();
        hello.extend_from_slice(&3u32.to_le_bytes());
        hello.extend_from_slice(&fingerprint.to_le_bytes());
        push_credential(&mut hello, "reader", TEST_SECRET);
        hello
    }
    fn finished() -> Vec<u8> {
        let mut request = vec![0u8];
        request.extend_from_slice(&3u32.to_le_bytes());
        request
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "lsrp-framing");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let credentials = write_credentials(&directory);
    let fingerprint = workload_fingerprint(&workload);
    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start framing server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "framing server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut stream = connect(&socket);
    stream.write_all(&(64u32 * 1024 + 1).to_le_bytes()).unwrap();
    stream.write_all(b"LSRP").unwrap();
    assert!(
        receive_frame(&mut stream).is_none(),
        "an oversized handshake received a response"
    );

    let mut stream = connect(&socket);
    send_frame(&mut stream, &[]);
    let response = receive_frame(&mut stream).expect("zero-length handshake got no response");
    assert_eq!(response.first(), Some(&1));
    assert!(
        String::from_utf8_lossy(&response[5..]).contains("unexpected end of data"),
        "{}",
        String::from_utf8_lossy(&response[5..])
    );

    let mut stream = connect(&socket);
    send_frame(&mut stream, b"NOPE");
    let response = receive_frame(&mut stream).expect("bad magic got no response");
    assert_eq!(response.first(), Some(&1));
    assert!(
        String::from_utf8_lossy(&response[5..]).contains("bad protocol magic"),
        "{}",
        String::from_utf8_lossy(&response[5..])
    );

    let mut stream = connect(&socket);
    stream.write_all(&[0u8, 0u8]).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    assert!(
        receive_frame(&mut stream).is_none(),
        "a truncated header received a response"
    );

    let mut stream = connect(&socket);
    stream.write_all(&10u32.to_le_bytes()).unwrap();
    stream.write_all(b"LSR").unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    assert!(
        receive_frame(&mut stream).is_none(),
        "a truncated payload received a response"
    );

    let mut stream = connect(&socket);
    let mut trailing = handshake(fingerprint);
    trailing.push(0xff);
    send_frame(&mut stream, &trailing);
    let response = receive_frame(&mut stream).expect("trailing handshake got no response");
    assert_eq!(response.first(), Some(&1));
    assert!(
        String::from_utf8_lossy(&response[5..]).contains("trailing bytes"),
        "{}",
        String::from_utf8_lossy(&response[5..])
    );

    let mut stream = connect(&socket);
    let mut hello = handshake(fingerprint);
    hello[4..8].copy_from_slice(&4u32.to_le_bytes());
    send_frame(&mut stream, &hello);
    let response = receive_frame(&mut stream).expect("bad version got no response");
    assert_eq!(response.first(), Some(&1));
    assert!(
        String::from_utf8_lossy(&response[5..]).contains("unsupported protocol version"),
        "{}",
        String::from_utf8_lossy(&response[5..])
    );

    let mut stream = connect(&socket);
    send_frame(&mut stream, &handshake(fingerprint));
    let response = receive_frame(&mut stream).expect("valid handshake got no response");
    assert_eq!(response.first(), Some(&0));
    send_frame(&mut stream, &[255u8]);
    let response = receive_frame(&mut stream).expect("unknown request got no response");
    assert_eq!(response.first(), Some(&1));
    assert!(
        String::from_utf8_lossy(&response[5..]).contains("unknown request kind"),
        "{}",
        String::from_utf8_lossy(&response[5..])
    );
    send_frame(&mut stream, &finished());
    let response = receive_frame(&mut stream).expect("known request got no response");
    assert_eq!(response.first(), Some(&0));
    drop(stream);

    let mut stream = connect(&socket);
    send_frame(&mut stream, &handshake(fingerprint));
    let response = receive_frame(&mut stream).expect("valid handshake got no response");
    assert_eq!(response.first(), Some(&0));
    stream
        .write_all(&(256u32 * 1024 * 1024 + 1).to_le_bytes())
        .unwrap();
    assert!(
        receive_frame(&mut stream).is_none(),
        "an oversized request received a response"
    );

    let mut stream = connect(&socket);
    send_frame(&mut stream, &handshake(fingerprint));
    let response = receive_frame(&mut stream).expect("valid handshake got no response");
    assert_eq!(response.first(), Some(&0));
    let mut state = 0x51a7_9d13_c4e2_8b60u64;
    for _ in 0..64 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let length = (state >> 33) as usize % 32;
        let mut payload = vec![0u8];
        payload.extend_from_slice(&((state >> 21) as u32 % 12).to_le_bytes());
        for _ in 0..length {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            payload.push((state >> 33) as u8);
        }
        send_frame(&mut stream, &payload);
        let response = receive_frame(&mut stream).expect("fuzz frame got no response");
        assert!(!response.is_empty());
    }
    send_frame(&mut stream, &finished());
    let response = receive_frame(&mut stream).expect("server died during frame fuzzing");
    assert_eq!(response.first(), Some(&0));
    drop(stream);

    let mut check = connect(&socket);
    send_frame(&mut check, &handshake(fingerprint));
    let response = receive_frame(&mut check).expect("server died after malformed frames");
    assert_eq!(response.first(), Some(&0));

    server.kill().unwrap();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&directory);
}
