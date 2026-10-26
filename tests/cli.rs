use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temporary(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("lightshade-build-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn reports_the_package_version() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let output = Command::new(binary).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("lightshade {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn exit_codes_distinguish_usage_from_runtime_errors() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let output = Command::new(binary).arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
    let output = Command::new(binary).arg("run").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    let output = Command::new(binary)
        .args(["check", "/nonexistent.lightshade"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
}

#[test]
fn build_produces_a_prebuilt_interface_that_skips_rustc() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("interface");
    let interface = directory.join("messages-interface");

    let built = Command::new(binary)
        .args(["build", workload, "--out"])
        .arg(&interface)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(interface.is_file());
    assert!(
        String::from_utf8_lossy(&built.stdout).contains("wrote "),
        "{}",
        String::from_utf8_lossy(&built.stdout)
    );

    let database = directory.join("inbox.lsdb");
    let run = Command::new(binary)
        .args(["run", workload])
        .arg(&database)
        .args([
            "insert_message",
            "id=1",
            "inbox_id=7",
            "sent_at=100",
            "is_read=false",
            "body=hello",
        ])
        .env("LIGHTSHADE_INTERFACE", &interface)
        .env("LIGHTSHADE_CACHE", directory.join("cache"))
        .env("RUSTC", directory.join("missing-rustc"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok true");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn build_rejects_a_missing_interface_override() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("override");
    let database = directory.join("inbox.lsdb");
    let run = Command::new(binary)
        .args(["run", workload])
        .arg(&database)
        .args([
            "insert_message",
            "id=1",
            "inbox_id=7",
            "sent_at=100",
            "is_read=false",
            "body=hello",
        ])
        .env("LIGHTSHADE_INTERFACE", directory.join("absent"))
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("LIGHTSHADE_INTERFACE"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn run_prints_the_projected_return_table_header() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/inbox_digest.lightshade"
    );
    let directory = temporary("projection");
    let interface = directory.join("inbox-interface");
    let built = Command::new(binary)
        .args(["build", workload, "--out"])
        .arg(&interface)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let database = directory.join("inbox.lsdb");
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .args(["run", workload])
            .arg(&database)
            .args(args)
            .env("LIGHTSHADE_INTERFACE", &interface)
            .output()
            .unwrap();
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string(),
        )
    };

    for args in [
        &["add_inbox", "id=1", "owner=alice"][..],
        &[
            "add_message",
            "id=1",
            "inbox_id=1",
            "sent_at=100",
            "is_read=false",
            "body=hello",
        ][..],
    ] {
        let (success, output) = run(args);
        assert!(success, "{output}");
        assert_eq!(output, "ok true");
    }

    let (success, output) = run(&["unread_digest"]);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "# owner\ttotal\tnewest\toldest\tspan\nalice\t1\t100\t100\t100"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn cache_is_private_and_refuses_symlinked_interfaces() {
    use std::os::unix::fs::PermissionsExt;

    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("cache-security");
    let cache = directory.join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).unwrap();
    let database = directory.join("inbox.lsdb");
    let run = || {
        Command::new(binary)
            .args(["run", workload])
            .arg(&database)
            .args([
                "insert_message",
                "id=1",
                "inbox_id=7",
                "sent_at=100",
                "is_read=false",
                "body=hello",
            ])
            .env("LIGHTSHADE_CACHE", &cache)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let cached = fs::read_dir(&cache)
        .unwrap()
        .filter_map(Result::ok)
        .find_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let stem = name.strip_suffix(".rs")?;
            (stem.starts_with("interface-")).then(|| cache.join(stem))
        })
        .expect("cache did not contain an interface binary");
    fs::remove_file(&cached).unwrap();
    std::os::unix::fs::symlink("/bin/false", &cached).unwrap();

    let second = run();
    assert!(!second.status.success());
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("refusing unsafe cache entry"),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn init_scaffolds_a_rust_native_project_and_refuses_overwrite() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let directory = temporary("init");
    let project = directory.join("demo");
    let output = Command::new(binary)
        .args([
            "init",
            project.to_str().unwrap(),
            "--name",
            "demo",
            "--features",
            "host",
            "--path",
            env!("CARGO_MANIFEST_DIR"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"demo\""));
    assert!(manifest.contains("default = [\"host\", \"embedded\"]"));
    assert!(manifest.contains("host = [\"embedded\", \"lightshadedb/host\"]"));
    assert!(manifest.contains("client = [\"lightshadedb/client\"]"));
    assert!(manifest.contains("default-features = false"));
    let source = fs::read_to_string(project.join("src/main.rs")).unwrap();
    assert!(source.contains("#[lightshadedb::schema]"));
    assert!(source.contains("#[lightshadedb::operations]"));
    assert!(source.contains("AppConfig::embedded"));
    assert!(!project.join("README.md").exists());

    let output = Command::new(binary)
        .args(["init", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));

    let output = Command::new(binary)
        .args(["init", project.to_str().unwrap(), "--force"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn migrate_dry_run_reports_a_plan_without_writing() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let directory = temporary("migrate-dry-run");
    let old = directory.join("old.lightshade");
    let new = directory.join("new.lightshade");
    fs::write(
        &old,
        "table notes {\n    id: i64 primary_key\n    title: text\n    score: i64\n}\noperation add(id: i64, title: text, score: i64) {\n    insert notes\n}\n",
    )
    .unwrap();
    fs::write(
        &new,
        "table notes {\n    id: i64 primary_key\n    title: text\n    rank: i64 default 7\n}\noperation add(id: i64, title: text, rank: i64) {\n    insert notes\n}\n",
    )
    .unwrap();
    let database = directory.join("notes.lsdb");
    let seed = Command::new(binary)
        .args(["run", old.to_str().unwrap()])
        .arg(&database)
        .args(["add", "id=1", "title=hello", "score=5"])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );

    let before = fs::read(&database).unwrap();
    let plan = Command::new(binary)
        .args(["migrate", new.to_str().unwrap()])
        .arg(&database)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout).into_owned();
    assert!(stdout.contains("plan: 2 step(s)"), "{stdout}");
    assert!(stdout.contains("add field `notes.rank`"), "{stdout}");
    assert!(
        stdout.contains("remove field `notes.score` (destructive)"),
        "{stdout}"
    );
    assert!(stdout.contains("destructive: yes"), "{stdout}");
    assert_eq!(fs::read(&database).unwrap(), before);
    assert!(
        fs::read_dir(&directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .all(|entry| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".pre-migrate-")
            }),
        "dry run must not write a backup",
    );

    let same = Command::new(binary)
        .args(["migrate", old.to_str().unwrap()])
        .arg(&database)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(
        same.status.success(),
        "{}",
        String::from_utf8_lossy(&same.stderr)
    );
    assert!(
        String::from_utf8_lossy(&same.stdout).contains("no migration needed"),
        "{}",
        String::from_utf8_lossy(&same.stdout)
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn migrate_apply_gates_destructive_changes_and_reports_the_backup() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let directory = temporary("migrate-apply");
    let old = directory.join("old.lightshade");
    let new = directory.join("new.lightshade");
    fs::write(
        &old,
        "table notes {\n    id: i64 primary_key\n    title: text\n    score: i64\n}\noperation add(id: i64, title: text, score: i64) {\n    insert notes\n}\n",
    )
    .unwrap();
    fs::write(
        &new,
        "table notes {\n    id: i64 primary_key\n    title: text\n    rank: i64 default 7\n}\noperation add(id: i64, title: text, rank: i64) {\n    insert notes\n}\n",
    )
    .unwrap();
    let database = directory.join("notes.lsdb");
    let seed = Command::new(binary)
        .args(["run", old.to_str().unwrap()])
        .arg(&database)
        .args(["add", "id=1", "title=hello", "score=5"])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );

    let refused = Command::new(binary)
        .args(["migrate", new.to_str().unwrap()])
        .arg(&database)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--allow-destructive"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let applied = Command::new(binary)
        .args(["migrate", new.to_str().unwrap()])
        .arg(&database)
        .args(["--apply", "--allow-destructive"])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let stdout = String::from_utf8_lossy(&applied.stdout).into_owned();
    assert!(stdout.contains("migrated"), "{stdout}");
    assert!(stdout.contains("backup: "), "{stdout}");

    let backup = fs::read_dir(&directory)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("lsdb")
                && path != &database
        })
        .expect("expected a pre-migrate backup");
    let verify = Command::new(binary)
        .args(["verify", old.to_str().unwrap()])
        .arg(&backup)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stderr)
    );
    assert!(
        String::from_utf8_lossy(&verify.stdout).contains("1 rows"),
        "{}",
        String::from_utf8_lossy(&verify.stdout)
    );
    let inspect = Command::new(binary)
        .args(["inspect", new.to_str().unwrap()])
        .arg(&database)
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(
        String::from_utf8_lossy(&inspect.stdout).contains("rank: i64"),
        "{}",
        String::from_utf8_lossy(&inspect.stdout)
    );

    let second = directory.join("second.lsdb");
    let seed = Command::new(binary)
        .args(["run", old.to_str().unwrap()])
        .arg(&second)
        .args(["add", "id=2", "title=bye", "score=6"])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let applied = Command::new(binary)
        .args(["migrate", new.to_str().unwrap()])
        .arg(&second)
        .args(["--apply", "--allow-destructive", "--no-backup"])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(
        String::from_utf8_lossy(&applied.stdout).contains("(no backup)"),
        "{}",
        String::from_utf8_lossy(&applied.stdout)
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn migrate_apply_refuses_blocked_steps_before_writing() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let directory = temporary("migrate-blocked");
    let old = directory.join("old.lightshade");
    let typed = directory.join("typed.lightshade");
    fs::write(
        &old,
        "table notes {\n    id: i64 primary_key\n    score: i64\n}\noperation add(id: i64, score: i64) {\n    insert notes\n}\n",
    )
    .unwrap();
    fs::write(
        &typed,
        "table notes {\n    id: i64 primary_key\n    score: text\n}\noperation add(id: i64, score: text) {\n    insert notes\n}\n",
    )
    .unwrap();
    let database = directory.join("notes.lsdb");
    let seed = Command::new(binary)
        .args(["run", old.to_str().unwrap()])
        .arg(&database)
        .args(["add", "id=1", "score=5"])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );

    let plan = Command::new(binary)
        .args(["migrate", typed.to_str().unwrap()])
        .arg(&database)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(
        String::from_utf8_lossy(&plan.stdout).contains("(blocked: type mismatch)"),
        "{}",
        String::from_utf8_lossy(&plan.stdout)
    );

    let before = fs::read(&database).unwrap();
    let applied = Command::new(binary)
        .args(["migrate", typed.to_str().unwrap()])
        .arg(&database)
        .args(["--apply", "--allow-destructive"])
        .output()
        .unwrap();
    assert!(!applied.status.success());
    assert_eq!(fs::read(&database).unwrap(), before);

    let _ = fs::remove_dir_all(&directory);
}

fn build_interface(binary: &str, workload: &str, out: &std::path::Path) {
    let built = Command::new(binary)
        .args(["build", workload, "--out"])
        .arg(out)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
}

fn seed_messages(binary: &str, workload: &str, db: &std::path::Path, interface: &std::path::Path) {
    for args in [
        &[
            "insert_message",
            "id=1",
            "inbox_id=7",
            "sent_at=100",
            "is_read=false",
            "body=hello",
        ][..],
        &[
            "insert_message",
            "id=2",
            "inbox_id=7",
            "sent_at=200",
            "is_read=false",
            "body=again, \"quoted\"",
        ][..],
        &[
            "insert_message",
            "id=3",
            "inbox_id=7",
            "sent_at=300",
            "is_read=true",
            "body=older",
        ][..],
    ] {
        let run = Command::new(binary)
            .args(["run", workload])
            .arg(db)
            .args(args)
            .env("LIGHTSHADE_INTERFACE", interface)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

fn run_op(
    binary: &str,
    workload: &str,
    db: &std::path::Path,
    interface: &std::path::Path,
    args: &[&str],
) -> (bool, String) {
    let output = Command::new(binary)
        .args(["run", workload])
        .arg(db)
        .args(args)
        .env("LIGHTSHADE_INTERFACE", interface)
        .output()
        .unwrap();
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string(),
    )
}

#[test]
fn export_writes_csv_and_import_reads_it_back() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("data-plane");
    let interface = directory.join("messages-interface");
    build_interface(binary, workload, &interface);

    let database = directory.join("messages.lsdb");
    seed_messages(binary, workload, &database, &interface);

    let csv = directory.join("messages.csv");
    let output = Command::new(binary)
        .args(["export", workload])
        .arg(&database)
        .args(["messages"])
        .arg(&csv)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(&csv).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.remove(0), "id,inbox_id,sent_at,is_read,body");
    assert_eq!(lines.len(), 3);
    assert!(lines.contains(&"1,7,100,false,hello"));
    assert!(lines.contains(&"3,7,300,true,older"));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("2,7,200,false,\"again, ")),
        "{lines:?}"
    );

    let database2 = directory.join("messages2.lsdb");
    let output = Command::new(binary)
        .args(["import", workload])
        .arg(&database2)
        .args(["messages"])
        .arg(&csv)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (ok, text) = run_op(
        binary,
        workload,
        &database2,
        &interface,
        &["unread_count", "inbox=7"],
    );
    assert!(ok, "{text}");
    assert!(text.contains('2'), "{text}");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn dump_and_restore_round_trip() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("dump-restore");
    let interface = directory.join("messages-interface");
    build_interface(binary, workload, &interface);

    let database = directory.join("messages.lsdb");
    seed_messages(binary, workload, &database, &interface);

    let dump = directory.join("messages.dump");
    let output = Command::new(binary)
        .args(["dump", workload])
        .arg(&database)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = fs::read(&dump).unwrap();
    assert!(bytes.starts_with(b"LIGHTDMP"));

    let output = Command::new(binary)
        .args(["dump", workload])
        .arg(&database)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());

    let database2 = directory.join("restored.lsdb");
    let output = Command::new(binary)
        .args(["restore", workload])
        .arg(&database2)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (ok, before) = run_op(
        binary,
        workload,
        &database,
        &interface,
        &["recent_messages", "inbox=7", "limit=10"],
    );
    assert!(ok, "{before}");
    let (ok, after) = run_op(
        binary,
        workload,
        &database2,
        &interface,
        &["recent_messages", "inbox=7", "limit=10"],
    );
    assert!(ok, "{after}");
    assert_eq!(before, after);

    let output = Command::new(binary)
        .args(["restore", workload])
        .arg(&database2)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn data_plane_rejects_bad_inputs() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("data-plane-errors");
    let interface = directory.join("messages-interface");
    build_interface(binary, workload, &interface);

    let database = directory.join("messages.lsdb");
    seed_messages(binary, workload, &database, &interface);

    let missing = directory.join("missing.csv");
    let output = Command::new(binary)
        .args(["export", workload])
        .arg(&database)
        .args(["nope", &missing.to_string_lossy()])
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown table"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bad_value = directory.join("bad-value.csv");
    fs::write(
        &bad_value,
        "id,inbox_id,sent_at,is_read,body\n1,7,NaN,oops,hello\n",
    )
    .unwrap();
    let output = Command::new(binary)
        .args(["import", workload])
        .arg(&database)
        .args(["messages"])
        .arg(&bad_value)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("row 2"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bad_column = directory.join("bad-column.csv");
    fs::write(&bad_column, "id,nope\n1,2\n").unwrap();
    let output = Command::new(binary)
        .args(["import", workload])
        .arg(&database)
        .args(["messages"])
        .arg(&bad_column)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());

    let missing_column = directory.join("missing-column.csv");
    fs::write(&missing_column, "id,inbox_id\n1,7\n").unwrap();
    let output = Command::new(binary)
        .args(["import", workload])
        .arg(&database)
        .args(["messages"])
        .arg(&missing_column)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("sent_at"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let kv_workload = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/kv.lightshade");
    let kv_interface = directory.join("kv-interface");
    build_interface(binary, kv_workload, &kv_interface);
    let dump = directory.join("messages.dump");
    let output = Command::new(binary)
        .args(["dump", workload])
        .arg(&database)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(output.status.success());
    let database2 = directory.join("kv.lsdb");
    let output = Command::new(binary)
        .args(["restore", kv_workload])
        .arg(&database2)
        .arg(&dump)
        .env("LIGHTSHADE_INTERFACE", &kv_interface)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("messages"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(&directory);
}

fn explain(
    binary: &str,
    workload: &str,
    interface: &std::path::Path,
    operation: Option<&str>,
) -> (bool, String) {
    let mut command = Command::new(binary);
    command
        .args(["explain", workload])
        .env("LIGHTSHADE_INTERFACE", interface);
    if let Some(operation) = operation {
        command.arg(operation);
    }
    let output = command.output().unwrap();
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

#[test]
fn explain_reports_strategies_without_a_database() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let messages = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/messages.lightshade");
    let directory = temporary("explain");
    let interface = directory.join("messages-interface");
    build_interface(binary, messages, &interface);

    let (ok, text) = explain(binary, messages, &interface, None);
    assert!(ok, "{text}");
    assert!(
        text.contains("operation `insert_message` (insert into messages):"),
        "{text}"
    );
    assert!(
        text.contains("strategy: point put into messages (duplicate key returns false)"),
        "{text}"
    );
    assert!(
        text.contains("maintains index Idx0 on (inbox_id)"),
        "{text}"
    );
    assert!(
        text.contains("strategy: index scan on messages via Idx0 (1 equality, 2 order)"),
        "{text}"
    );
    assert!(text.contains("equality: inbox_id = $inbox"), "{text}");
    assert!(
        text.contains("strategy: maintained counter Cnt0 on messages (inbox_id, is_read)"),
        "{text}"
    );
    assert!(
        text.contains("strategy: point update of messages by primary key (id = $message)"),
        "{text}"
    );

    let (ok, text) = explain(binary, messages, &interface, Some("unread_count"));
    assert!(ok, "{text}");
    assert!(text.contains("maintained counter"), "{text}");
    assert!(!text.contains("recent_messages"), "{text}");

    let (ok, text) = explain(binary, messages, &interface, Some("nope"));
    assert!(!ok, "{text}");
    assert!(text.contains("unknown operation `nope`"), "{text}");
    assert!(text.contains("available: insert_message"), "{text}");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn explain_covers_scans_ranges_and_advanced_queries() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let directory = temporary("explain-shapes");
    let kv = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/kv.lightshade");
    let kv_interface = directory.join("kv-interface");
    build_interface(binary, kv, &kv_interface);

    let (ok, text) = explain(binary, kv, &kv_interface, None);
    assert!(ok, "{text}");
    assert!(
        text.contains("strategy: primary-key point lookup on kv (id = $wanted)"),
        "{text}"
    );
    assert!(
        text.contains("strategy: ordered primary-key range scan on kv from lower bound $start"),
        "{text}"
    );
    assert!(text.contains("strategy: full scan of kv"), "{text}");
    assert!(
        text.contains("no counter: filter has no usable equality prefix"),
        "{text}"
    );

    let digest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/inbox_digest.lightshade"
    );
    let digest_interface = directory.join("digest-interface");
    build_interface(binary, digest, &digest_interface);
    let (ok, text) = explain(binary, digest, &digest_interface, Some("unread_digest"));
    assert!(ok, "{text}");
    assert!(text.contains("advanced query over 2 source(s)"), "{text}");
    assert!(
        text.contains("join i: primary-key point lookup (m.inbox_id = i.id)"),
        "{text}"
    );
    assert!(text.contains("in-memory group by (i.owner)"), "{text}");
    assert!(text.contains("no index: is_read bool"), "{text}");
    assert!(
        text.contains("in-memory sort by total desc, owner"),
        "{text}"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn slow_log_records_operations_as_jsonl() {
    let binary = env!("CARGO_BIN_EXE_lightshade");
    let kv = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/kv.lightshade");
    let directory = temporary("slow-log");
    let interface = directory.join("kv-interface");
    build_interface(binary, kv, &interface);
    let database = directory.join("kv.lsdb");
    seed_kv(binary, kv, &database, &interface);

    let log = directory.join("slow.jsonl");
    let output = Command::new(binary)
        .args(["run", kv])
        .arg(&database)
        .args(["get_kv", "wanted=1"])
        .env("LIGHTSHADE_INTERFACE", &interface)
        .env("LIGHTSHADE_SLOW_LOG", &log)
        .env("LIGHTSHADE_SLOW_MS", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("\"op\":\"get_kv\""), "{text}");
    assert!(
        lines[0].contains("\"strategy\":\"primary-key point lookup on kv (id = $wanted)\""),
        "{text}"
    );
    assert!(lines[0].contains("\"rows\":1"), "{text}");
    assert!(lines[0].contains("\"elapsed_ms\":"), "{text}");
    assert!(lines[0].contains("\"ts_ms\":"), "{text}");

    let quiet = directory.join("quiet.jsonl");
    let output = Command::new(binary)
        .args(["run", kv])
        .arg(&database)
        .args(["kv_count"])
        .env("LIGHTSHADE_INTERFACE", &interface)
        .env("LIGHTSHADE_SLOW_LOG", &quiet)
        .env("LIGHTSHADE_SLOW_MS", "3600000")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!quiet.exists());

    let output = Command::new(binary)
        .args(["run", kv])
        .arg(&database)
        .args(["kv_count"])
        .env("LIGHTSHADE_INTERFACE", &interface)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!quiet.exists());

    let _ = fs::remove_dir_all(&directory);
}

fn seed_kv(binary: &str, workload: &str, db: &std::path::Path, interface: &std::path::Path) {
    for args in [
        &["put_kv", "id=1", "value=hello"][..],
        &["put_kv", "id=2", "value=world"][..],
    ] {
        let run = Command::new(binary)
            .args(["run", workload])
            .arg(db)
            .args(args)
            .env("LIGHTSHADE_INTERFACE", interface)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
