use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[cfg(unix)]
#[test]
fn tls_termination_enforces_certificate_validity() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn available(program: &str) -> bool {
        Command::new(program)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    if !available("openssl") || !available("python3") {
        eprintln!("skipping TLS proxy test: openssl or python3 is unavailable");
        return;
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "tls-proxy");
    let credentials = write_credentials(&directory);
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let fingerprint = workload_fingerprint(&workload);

    let certificate = |name: &str| -> (std::path::PathBuf, std::path::PathBuf) {
        let cert = directory.join(format!("{name}.crt"));
        let key = directory.join(format!("{name}.key"));
        let status = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-subj",
                "/CN=127.0.0.1",
                "-addext",
                "subjectAltName=IP:127.0.0.1",
                "-days",
                "1",
            ])
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("failed to run openssl");
        assert!(
            status.success(),
            "openssl could not create {name} certificate"
        );
        (cert, key)
    };
    let (valid_cert, valid_key) = certificate("valid");

    let expired_cert = directory.join("expired.crt");
    let expired_key = directory.join("expired.key");
    let expired_csr = directory.join("expired.csr");
    let config = directory.join("openssl.cnf");
    let index = directory.join("index.txt");
    let serial = directory.join("serial");
    std::fs::write(&index, "").unwrap();
    std::fs::write(&serial, "1000\n").unwrap();
    std::fs::write(
        &config,
        format!(
            "[ ca ]\ndefault_ca = CA_default\n\
                 [ CA_default ]\ndir = {}\ndatabase = $dir/index.txt\n\
                 new_certs_dir = $dir\nserial = $dir/serial\ndefault_md = sha256\n\
                 policy = policy_anything\ncopy_extensions = copy\nunique_subject = no\n\
                 [ policy_anything ]\ncommonName = supplied\n\
                 [ v3_req ]\nsubjectAltName = IP:127.0.0.1\n\
                 basicConstraints = critical,CA:TRUE\n\
                 keyUsage = critical,digitalSignature,keyEncipherment,keyCertSign\n",
            directory.display()
        ),
    )
    .unwrap();
    let status = Command::new("openssl")
        .args([
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=127.0.0.1",
        ])
        .arg("-keyout")
        .arg(&expired_key)
        .arg("-out")
        .arg(&expired_csr)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to run openssl");
    assert!(status.success(), "openssl could not create the expired CSR");
    let status = Command::new("openssl")
        .args(["ca", "-batch", "-selfsign", "-config"])
        .arg(&config)
        .arg("-in")
        .arg(&expired_csr)
        .arg("-out")
        .arg(&expired_cert)
        .arg("-keyfile")
        .arg(&expired_key)
        .args([
            "-startdate",
            "20200101000000Z",
            "-enddate",
            "20200102000000Z",
            "-extensions",
            "v3_req",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to run openssl");
    assert!(
        status.success(),
        "openssl could not create the expired certificate"
    );

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_LISTEN", "127.0.0.1:0")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIAL_ID", TEST_CREDENTIAL_ID)
        .env("LIGHTSHADE_CREDENTIAL_SECRET", TEST_SECRET)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start TLS backend");
    let deadline = Instant::now() + Duration::from_secs(30);
    let backend = loop {
        let text = std::fs::read_to_string(&socket).unwrap_or_default();
        if let Ok(address) = text.trim().parse::<std::net::SocketAddr>() {
            break address;
        }
        assert!(
            Instant::now() < deadline,
            "TLS backend never wrote its endpoint"
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    let probe = directory.join("tls_probe.py");
    std::fs::write(
        &probe,
        r#"
import select
import socket
import ssl
import struct
import sys
import threading

cert, key, backend, fingerprint, mode, credential_id, secret = sys.argv[1:]
host, port = backend.rsplit(":", 1)
port = int(port)
listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", 0))
listener.listen(1)
proxy_port = listener.getsockname()[1]
errors = []

def relay():
    connection, _ = listener.accept()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    try:
        downstream = context.wrap_socket(connection, server_side=True)
    except Exception as error:
        errors.append(error)
        connection.close()
        return
    upstream = None
    try:
        upstream = socket.create_connection((host, port), timeout=5)
        downstream.settimeout(5)
        upstream.settimeout(5)
        while True:
            readable, _, _ = select.select([downstream, upstream], [], [], 5)
            if not readable:
                break
            for source in readable:
                data = source.recv(65536)
                if not data:
                    return
                (upstream if source is downstream else downstream).sendall(data)
    except Exception as error:
        errors.append(error)
    finally:
        downstream.close()
        if upstream is not None:
            upstream.close()

threading.Thread(target=relay, daemon=True).start()

def receive_exact(stream, length):
    data = b""
    while len(data) < length:
        chunk = stream.recv(length - len(data))
        if not chunk:
            raise RuntimeError("server closed during the handshake")
        data += chunk
    return data

hello = b"LSRP" + struct.pack("<IQ", 3, int(fingerprint))
hello += struct.pack("<I", len(credential_id)) + credential_id.encode()
hello += struct.pack("<I", len(secret)) + secret.encode()
context = ssl.create_default_context(cafile=cert)
try:
    raw = socket.create_connection(("127.0.0.1", proxy_port), timeout=5)
    with context.wrap_socket(raw, server_hostname="127.0.0.1") as stream:
        stream.sendall(struct.pack("<I", len(hello)) + hello)
        length = struct.unpack("<I", receive_exact(stream, 4))[0]
        response = receive_exact(stream, length)
        if mode != "valid":
            print("unexpected-success")
            sys.exit(1)
        if response[0] != 0:
            print("backend-error:" + response[5:].decode())
            sys.exit(1)
        print("ok")
except ssl.SSLCertVerificationError as error:
    if mode == "expired" and "expired" in str(error).lower():
        print("expired")
    else:
        print("certificate-error:" + str(error))
        sys.exit(1)
except Exception as error:
    print("proxy-error:" + str(error))
    sys.exit(1)
"#,
    )
    .unwrap();

    let run_probe = |cert: &std::path::Path, key: &std::path::Path, mode: &str| -> String {
        let output = Command::new("python3")
            .arg(&probe)
            .arg(cert)
            .arg(key)
            .arg(backend.to_string())
            .arg(fingerprint.to_string())
            .arg(mode)
            .arg(TEST_CREDENTIAL_ID)
            .arg(TEST_SECRET)
            .output()
            .expect("failed to run TLS probe");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    assert_eq!(run_probe(&valid_cert, &valid_key, "valid"), "ok");
    assert_eq!(run_probe(&expired_cert, &expired_key, "expired"), "expired");

    server.kill().unwrap();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn server_reloads_credentials_revokes_sessions_and_audits() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SECOND_SECRET: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const SECOND_SECRET_HASH: &str =
        "4f2e8d65483c641648cdb374ae9d8abd368d269e4ddffe092a8237b8162cddd6";

    fn write_credential_file(path: &std::path::Path, text: &str) {
        std::fs::write(path, text).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

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

    fn audit_text(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn wait_for_audit(path: &std::path::Path, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = audit_text(path);
            if text.contains(needle) {
                return text;
            }
            assert!(
                Instant::now() < deadline,
                "audit log never contained {needle}:\n{text}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "credential-rotation");
    let database = directory.join("notes.lsdb");
    let socket = directory.join("notes.lsdb.sock");
    let credentials = write_credentials(&directory);
    let audit = directory.join("audit.jsonl");
    let fingerprint = workload_fingerprint(&workload);

    let mut server = Command::new(&binary)
        .arg(&database)
        .arg("serve")
        .env("LIGHTSHADE_CREDENTIALS", &credentials)
        .env("LIGHTSHADE_CREDENTIALS_RELOAD_MS", "20")
        .env("LIGHTSHADE_AUDIT_LOG", &audit)
        .env("LIGHTSHADE_TENANT", "rotation")
        .env("LIGHTSHADE_IDLE_TIMEOUT_MS", "5000")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start credential server");
    let deadline = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "credential server never opened its socket"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut writer = wire_connect_as(&socket, fingerprint, TEST_CREDENTIAL_ID, TEST_SECRET)
        .expect("the initial writer could not authenticate");
    wire_send(&mut writer, &[1u8]);
    assert!(ok_body(&wire_receive(&mut writer)).is_empty());
    wire_send(&mut writer, &add_note(1, "secret-title-hunter2", 10, true));
    assert_eq!(ok_body(&wire_receive(&mut writer)), &[1u8]);
    wire_send(&mut writer, &[2u8]);
    assert!(ok_body(&wire_receive(&mut writer)).is_empty());
    wire_send(&mut writer, &[1u8]);
    assert!(ok_body(&wire_receive(&mut writer)).is_empty());
    wire_send(&mut writer, &[3u8]);
    assert!(ok_body(&wire_receive(&mut writer)).is_empty());
    wire_send(&mut writer, &finished());
    assert_eq!(
        i64::from_le_bytes(ok_body(&wire_receive(&mut writer)).try_into().unwrap()),
        1
    );

    let text = wait_for_audit(&audit, "server_start");
    assert!(text.contains("\"auth_success\""), "{text}");
    assert!(text.contains("\"transaction_begin\""), "{text}");
    assert!(text.contains("\"transaction_commit\""), "{text}");
    assert!(text.contains("\"transaction_rollback\""), "{text}");
    assert!(text.contains("\"event\":\"write\""), "{text}");
    assert!(text.contains("\"detail\":\"add_note\""), "{text}");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&audit).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "audit log is not private");
    }

    std::thread::sleep(Duration::from_millis(20));
    write_credential_file(&credentials, "writer\twriter\tnot-a-verifier\n");
    wire_send(&mut writer, &finished());
    assert_eq!(
        i64::from_le_bytes(ok_body(&wire_receive(&mut writer)).try_into().unwrap()),
        1,
        "an invalid credential reload replaced the last known good credentials"
    );
    let text = wait_for_audit(&audit, "credentials_reload_failed");
    assert!(
        text.contains("\"detail\":\"invalid_credentials\""),
        "{text}"
    );

    std::thread::sleep(Duration::from_millis(20));
    write_credential_file(
            &credentials,
            &format!(
                "reader\treader\t{TEST_SECRET_HASH}\nwriter\twriter\t{SECOND_SECRET_HASH}\nadmin\tadmin\t{SECOND_SECRET_HASH}\n"
            ),
        );
    wire_send(&mut writer, &finished());
    assert!(
        error_body(&wire_receive(&mut writer)).contains("authentication expired"),
        "a rotated credential kept its old connection alive"
    );
    let text = wait_for_audit(&audit, "credential_rotated");
    assert!(
        text.contains("\"event\":\"credentials_reloaded\""),
        "{text}"
    );
    let old_writer =
        wire_connect_as(&socket, fingerprint, TEST_CREDENTIAL_ID, TEST_SECRET).unwrap_err();
    assert!(old_writer.contains("authentication failed"), "{old_writer}");
    let mut rotated = wire_connect_as(&socket, fingerprint, TEST_CREDENTIAL_ID, SECOND_SECRET)
        .expect("the rotated writer could not authenticate");
    wire_send(&mut rotated, &finished());
    assert_eq!(
        i64::from_le_bytes(ok_body(&wire_receive(&mut rotated)).try_into().unwrap()),
        1
    );

    std::thread::sleep(Duration::from_millis(20));
    write_credential_file(
            &credentials,
            &format!(
                "reader\treader\t{TEST_SECRET_HASH}\nwriter\treader\t{SECOND_SECRET_HASH}\nadmin\tadmin\t{SECOND_SECRET_HASH}\n"
            ),
        );
    wire_send(&mut rotated, &finished());
    assert!(
        error_body(&wire_receive(&mut rotated)).contains("authentication expired"),
        "a role change kept the old privilege level alive"
    );
    let text = wait_for_audit(&audit, "credential_role_changed");
    assert!(
        text.contains("\"event\":\"credential_role_changed\""),
        "{text}"
    );
    let mut demoted = wire_connect_as(&socket, fingerprint, TEST_CREDENTIAL_ID, SECOND_SECRET)
        .expect("the demoted credential could not authenticate");
    wire_send(&mut demoted, &finished());
    assert_eq!(
        i64::from_le_bytes(ok_body(&wire_receive(&mut demoted)).try_into().unwrap()),
        1
    );
    wire_send(&mut demoted, &add_note(2, "denied", 20, false));
    assert!(
        error_body(&wire_receive(&mut demoted)).contains("permission denied"),
        "the demoted credential retained write permission"
    );
    let text = wait_for_audit(&audit, "permission_denied");
    assert!(text.contains("\"event\":\"permission_denied\""), "{text}");

    std::thread::sleep(Duration::from_millis(20));
    write_credential_file(
        &credentials,
        &format!("reader\treader\t{TEST_SECRET_HASH}\nadmin\tadmin\t{SECOND_SECRET_HASH}\n"),
    );
    wire_send(&mut demoted, &finished());
    assert!(
        error_body(&wire_receive(&mut demoted)).contains("authentication expired"),
        "a revoked credential kept its session alive"
    );
    let text = wait_for_audit(&audit, "credential_removed");
    assert!(text.contains("\"event\":\"credential_removed\""), "{text}");
    let revoked =
        wire_connect_as(&socket, fingerprint, TEST_CREDENTIAL_ID, SECOND_SECRET).unwrap_err();
    assert!(revoked.contains("authentication failed"), "{revoked}");

    let mut reader = wire_connect_as(&socket, fingerprint, "reader", TEST_SECRET)
        .expect("the reader lost access after an unrelated revocation");
    wire_send(&mut reader, &finished());
    assert_eq!(
        i64::from_le_bytes(ok_body(&wire_receive(&mut reader)).try_into().unwrap()),
        1
    );
    drop(reader);
    let text = wait_for_audit(&audit, "authentication_expired");
    wait_for_audit(&audit, "connection_closed");

    assert!(!text.contains(TEST_SECRET), "audit log leaked a secret");
    assert!(!text.contains(SECOND_SECRET), "audit log leaked a secret");
    assert!(
        !text.contains("secret-title-hunter2"),
        "audit log leaked a request payload"
    );
    for line in text.lines() {
        assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        assert!(line.contains("\"ts_ms\":"), "{line}");
        assert!(line.contains("\"tenant\":\"rotation\""), "{line}");
    }

    server.kill().unwrap();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&directory);
}
