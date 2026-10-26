use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[test]
fn persists_reopens_and_recovers_from_crash() {
    use std::process::Command;

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        PERSIST_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "persistence");
    let database = directory.join("notes.lightshade");
    let mut wal_name = database.as_os_str().to_os_string();
    wal_name.push(".wal");
    let wal = std::path::PathBuf::from(wal_name);

    let run = |mode: &str| {
        let output = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary");
        (output.status.success(), output)
    };

    assert!(run("write").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "row 1 alpha 10 false\nrow 2 beta 20 true\nrow 3 gamma 15 false\nfinished 1"
    );

    assert!(run("mutate").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "row 1 zeta 10 true\nrow 3 zeta 15 false\nfinished 1"
    );

    assert!(run("crash").0);
    let snapshot_backup = directory.join("notes.snapshot.bak");
    let wal_backup = directory.join("notes.wal.bak");
    std::fs::copy(&database, &snapshot_backup).unwrap();
    std::fs::copy(&wal, &wal_backup).unwrap();
    assert!(std::fs::metadata(&wal).unwrap().len() > 0);

    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "finished 0");

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::copy(&wal_backup, &wal).unwrap();
    let wal_len = std::fs::metadata(&wal).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wal)
        .unwrap()
        .set_len(wal_len - 10)
        .unwrap();
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "row 1 omega 10 true\nrow 3 omega 15 true\nfinished 2"
    );

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::copy(&wal_backup, &wal).unwrap();
    let mut wal_bytes = std::fs::read(&wal).unwrap();
    let position = wal_bytes.len() - 5;
    wal_bytes[position] ^= 0xff;
    std::fs::write(&wal, &wal_bytes).unwrap();
    let (success, output) = run("verify");
    assert!(!success);
    assert!(String::from_utf8_lossy(&output.stderr).contains("WAL checksum mismatch"));

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::copy(&wal_backup, &wal).unwrap();
    let mut snapshot_bytes = std::fs::read(&database).unwrap();
    snapshot_bytes[20] ^= 0xff;
    std::fs::write(&database, &snapshot_bytes).unwrap();
    let (success, output) = run("verify");
    assert!(!success);
    assert!(String::from_utf8_lossy(&output.stderr).contains("snapshot checksum mismatch"));

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::remove_file(&wal).unwrap();
    assert!(run("relaxed_write").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "row 1 zeta 10 true\nrow 3 zeta 15 false\nrow 4 delta 30 false\nfinished 1"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let snapshot_inode = std::fs::metadata(&database).unwrap().ino();
        assert!(run("open_only").0);
        assert_eq!(std::fs::metadata(&database).unwrap().ino(), snapshot_inode);
        assert!(run("open_close").0);
        assert_eq!(std::fs::metadata(&database).unwrap().ino(), snapshot_inode);
    }
    #[cfg(not(unix))]
    {
        assert!(run("open_only").0);
        assert!(run("open_close").0);
    }

    let (success, output) = run("lock");
    assert!(success);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("database is already open by another process"),
        "unexpected lock output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let mismatch_workload = parse_workload(SOURCE).unwrap();
    validate(&mismatch_workload).unwrap();
    let mismatch_source = format!(
        "{}\n{}",
        generate_library(&mismatch_workload).unwrap(),
        OPEN_HARNESS
    );
    let (mismatch_directory, mismatch_binary) =
        compile_generated_test(&mismatch_source, "persistence-mismatch");
    let output = std::process::Command::new(&mismatch_binary)
        .arg(&database)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("snapshot schema mismatch"),
        "unexpected snapshot mismatch output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let wal_only = mismatch_directory.join("wal-only.lightshade");
    let mut wal_only_path = wal_only.as_os_str().to_os_string();
    wal_only_path.push(".wal");
    std::fs::copy(&wal_backup, std::path::Path::new(&wal_only_path)).unwrap();
    let output = std::process::Command::new(&mismatch_binary)
        .arg(&wal_only)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("WAL schema mismatch"),
        "unexpected WAL mismatch output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = std::fs::remove_dir_all(&directory);
    let _ = std::fs::remove_dir_all(&mismatch_directory);
}

#[test]
fn segmented_snapshots_handle_large_tables() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "seed" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            for id in 0..24 {
                db.add_note(id, "x".repeat(64 * 1024), id, false).unwrap();
            }
            db.commit().unwrap();
            db.add_note(1000, "y".repeat(2 * 1024 * 1024), 1000, true).unwrap();
            println!("rows {}", db.t_notes.len());
            db.close().unwrap();
        }
        "verify" => {
            let db = Db::open_read_only(path).unwrap();
            println!("rows {}", db.t_notes.len());
            println!("big {}", db.t_notes.get(&1000).unwrap().title.len());
            println!("finished {}", db.finished());
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let mut source = generate(&workload).unwrap();
    source.push('\n');
    source.push_str(HARNESS);
    let (directory, binary) = compile_generated_test(&source, "segment");
    let database = directory.join("large.lightshade");

    let run = |mode: &str| -> std::process::Output {
        Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run segment binary")
    };

    let seeded = run("seed");
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&seeded.stdout).trim(), "rows 25");

    let snapshot = std::fs::read(&database).unwrap();
    assert!(
        snapshot.len() > 3 * 1024 * 1024,
        "snapshot is {}",
        snapshot.len()
    );

    let verified = run("verify");
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let output = String::from_utf8_lossy(&verified.stdout);
    assert_eq!(output.trim(), "rows 25\nbig 2097152\nfinished 1");

    let mut damaged = snapshot.clone();
    let middle = damaged.len() / 2;
    damaged[middle] ^= 0x40;
    std::fs::write(&database, &damaged).unwrap();
    let failed = run("verify");
    assert!(!failed.status.success());
    let message = String::from_utf8_lossy(&failed.stderr).to_string();
    assert!(
        message.contains("snapshot chunk checksum mismatch")
            || message.contains("snapshot checksum mismatch")
            || message.contains("invalid UTF-8")
            || message.contains("invalid boolean"),
        "{message}"
    );

    std::fs::write(&database, &snapshot[..snapshot.len() / 2]).unwrap();
    let truncated = run("verify");
    assert!(!truncated.status.success());
    let message = String::from_utf8_lossy(&truncated.stderr).to_string();
    assert!(
        message.contains("snapshot is truncated") || message.contains("unexpected end of data"),
        "{message}"
    );

    std::fs::write(&database, &snapshot).unwrap();
    let restored = run("verify");
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&restored.stdout).trim(),
        "rows 25\nbig 2097152\nfinished 1"
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn bounded_row_cache_spills_and_reopens() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    std::env::set_var("LIGHTSHADE_ROW_CACHE_BYTES", "4096");
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "seed" => {
            let mut db = Db::open(path).unwrap();
            db.begin().unwrap();
            for id in 0..1500 {
                db.add_note(id, "x".repeat(512), id, false).unwrap();
            }
            db.commit().unwrap();
            assert!(db.t_notes.__ls_cached_bytes() <= 4096);
            assert!(db.t_notes.__ls_spill_bytes() > 512 * 1024);
            println!(
                "seeded {} cache {} spill {}",
                db.t_notes.len(),
                db.t_notes.__ls_cached_bytes(),
                db.t_notes.__ls_spill_bytes()
            );
            db.close().unwrap();
        }
        "read" => {
            let db = Db::open_read_only(path).unwrap();
            let mut scanned = 0usize;
            for row in db.t_notes.values() {
                assert_eq!(row.title.len(), 512);
                scanned += 1;
            }
            assert!(db.t_notes.__ls_cached_bytes() <= 4096);
            assert!(db.t_notes.__ls_spill_bytes() > 512 * 1024);
            println!(
                "read {} scanned {} cache {} spill {}",
                db.t_notes.len(),
                scanned,
                db.t_notes.__ls_cached_bytes(),
                db.t_notes.__ls_spill_bytes()
            );
        }
        "mutate" => {
            let mut db = Db::open(path).unwrap();
            db.mark_all_done().unwrap();
            db.checkpoint().unwrap();
            assert_eq!(db.finished(), 1500);
            assert!(db.t_notes.__ls_cached_bytes() <= 4096);
            println!("mutated {} {}", db.finished(), db.t_notes.len());
            db.close().unwrap();
        }
        "verify" => {
            let mut db = Db::open(path).unwrap();
            assert_eq!(db.t_notes.len(), 1500);
            assert_eq!(db.finished(), 1500);
            assert_eq!(db.t_notes.get(&17).unwrap().title.len(), 512);
            assert!(db.t_notes.__ls_cached_bytes() <= 4096);
            println!(
                "verified {} cache {}",
                db.t_notes.len(),
                db.t_notes.__ls_cached_bytes()
            );
            db.close().unwrap();
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let mut source = generate(&workload).unwrap();
    source.push('\n');
    source.push_str(HARNESS);
    let (directory, binary) = compile_generated_test(&source, "row-cache");
    let database = directory.join("bounded.lightshade");

    let run = |mode: &str| -> std::process::Output {
        Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run row-cache binary")
    };

    let seeded = run("seed");
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let output = String::from_utf8_lossy(&seeded.stdout);
    assert!(output.starts_with("seeded 1500 cache "), "{output}");
    assert!(output.contains("spill "), "{output}");

    let read = run("read");
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let output = String::from_utf8_lossy(&read.stdout);
    assert!(
        output.starts_with("read 1500 scanned 1500 cache "),
        "{output}"
    );

    let mutated = run("mutate");
    assert!(
        mutated.status.success(),
        "{}",
        String::from_utf8_lossy(&mutated.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&mutated.stdout).trim(),
        "mutated 1500 1500"
    );

    let verified = run("verify");
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let output = String::from_utf8_lossy(&verified.stdout);
    assert!(output.starts_with("verified 1500 cache "), "{output}");

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn spill_dir_is_configurable_and_validated() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args[1].as_str();
    let mut db = Db::open(path).unwrap();
    for id in 0..10 {
        db.add_note(id, "spill probe".to_string(), id, false).unwrap();
    }
    let dir = std::env::var("LIGHTSHADE_SPILL_DIR").unwrap();
    let spills = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "spill"))
        .count();
    println!("seeded {} spills {}", db.t_notes.len(), spills);
    db.close().unwrap();
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let mut source = generate(&workload).unwrap();
    source.push('\n');
    source.push_str(HARNESS);
    let (directory, binary) = compile_generated_test(&source, "spill-dir");
    let spill = directory.join("spill-files");
    std::fs::create_dir_all(&spill).unwrap();

    let database = directory.join("spill.lightshade");
    let seeded = Command::new(&binary)
        .arg(&database)
        .env("LIGHTSHADE_SPILL_DIR", &spill)
        .output()
        .expect("failed to run spill-dir binary");
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&seeded.stdout).trim(),
        "seeded 10 spills 1"
    );

    let missing = directory.join("spill-missing.lightshade");
    let failed = Command::new(&binary)
        .arg(&missing)
        .env("LIGHTSHADE_SPILL_DIR", directory.join("no-such-dir"))
        .output()
        .expect("failed to run spill-dir binary");
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("LIGHTSHADE_SPILL_DIR"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn automatic_checkpoints_preserve_acknowledged_writes() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "checkpoint-order");
    let database = directory.join("notes.lsdb");

    let run = |args: &[&str], checkpoint: bool| {
        let mut command = Command::new(&binary);
        command.arg(&database).args(args);
        if checkpoint {
            command.env("LIGHTSHADE_CHECKPOINT_BYTES", "1");
        }
        let output = command.output().expect("failed to run generated interface");
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

    let (success, output) = run(
        &[
            "run",
            "add_note",
            "id=1",
            "title=alpha",
            "score=10",
            "done=false",
        ],
        true,
    );
    assert!(success, "{output}");
    assert_eq!(output, "ok true");

    let (success, output) = run(&["run", "by_title", "wanted=alpha"], false);
    assert!(success, "{output}");
    assert!(output.contains("alpha"), "{output}");

    let (success, output) = run(&["run", "delete_note", "note=1"], true);
    assert!(success, "{output}");
    assert_eq!(output, "ok 1");

    let (success, output) = run(&["run", "by_title", "wanted=alpha"], false);
    assert!(success, "{output}");
    assert!(output.is_empty(), "{output}");
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn ops_backup_compact_and_recovery_preserve_rows() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "ops");
    let database = directory.join("notes.lightshade");

    let run = |args: &[&str]| -> (bool, String) {
        let output = Command::new(&binary)
            .arg(&database)
            .args(args)
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
    assert!(
        run(&[
            "run",
            "add_note",
            "id=1",
            "title=one",
            "score=1",
            "done=false"
        ])
        .0
    );
    assert!(
        run(&[
            "run",
            "add_note",
            "id=2",
            "title=two",
            "score=2",
            "done=true"
        ])
        .0
    );

    let (success, output) = run(&["verify"]);
    assert!(success, "{output}");
    assert_eq!(
        output.trim(),
        "ok: lsn 2, 1 tables, 2 rows\n  notes: 2 rows"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            &database,
            &directory.join("notes.lightshade.wal"),
            &directory.join("notes.lightshade.lock"),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600,
                "{} is not private",
                path.display()
            );
        }
    }

    let (success, output) = run(&["inspect"]);
    assert!(success, "{output}");
    assert!(output.contains("version: 8"), "{output}");
    assert!(output.contains("lsn: 2"), "{output}");
    assert!(output.contains("rows: 2"), "{output}");
    assert!(output.contains("table notes: 2 rows"), "{output}");
    assert!(output.contains("  id: i64 primary_key"), "{output}");

    let backup = directory.join("backup.lightshade");
    let backup_arg = backup.to_str().unwrap().to_string();
    let (success, output) = run(&["backup", backup_arg.as_str()]);
    assert!(success, "{output}");
    assert_eq!(
        output.trim(),
        format!("backed up: lsn 2, 2 rows, {} bytes -> {backup_arg}", {
            std::fs::metadata(&database).unwrap().len()
        })
    );
    assert!(!directory.join("backup.lightshade.wal").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let temporary_alias = directory.join("notes.lightshade.tmp");
    let temporary_alias_arg = temporary_alias.to_str().unwrap().to_string();
    let (success, output) = run(&["backup", temporary_alias_arg.as_str()]);
    assert!(!success);
    assert!(output.contains("aliases the database"), "{output}");
    assert!(!temporary_alias.exists());
    let verify_backup = Command::new(&binary)
        .arg(&backup)
        .arg("verify")
        .output()
        .expect("failed to verify backup");
    assert!(verify_backup.status.success());
    assert_eq!(
        String::from_utf8_lossy(&verify_backup.stdout).trim(),
        "ok: lsn 2, 1 tables, 2 rows\n  notes: 2 rows"
    );
    let backup_query = Command::new(&binary)
        .arg(&backup)
        .arg("run")
        .arg("finished")
        .output()
        .expect("failed to query backup");
    assert_eq!(String::from_utf8_lossy(&backup_query.stdout).trim(), "1");

    let (success, output) = run(&["backup", backup_arg.as_str()]);
    assert!(!success);
    assert!(output.contains("already exists"), "{output}");

    let wal = directory.join("notes.lightshade.wal");
    let mut bytes = std::fs::read(&wal).unwrap();
    bytes.extend_from_slice(b"torn!!!");
    std::fs::write(&wal, &bytes).unwrap();
    let (success, output) = run(&["verify"]);
    assert!(success, "{output}");

    let (success, output) = run(&["compact"]);
    assert!(success, "{output}");
    assert!(output.contains("compacted: lsn 2, 2 rows"), "{output}");
    assert!(output.contains("wal 7 -> 0 bytes"), "{output}");
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
    let verify = Command::new(&binary)
        .arg(&database)
        .arg("verify")
        .output()
        .expect("failed to verify compacted database");
    assert!(verify.status.success());

    std::fs::write(&wal, b"torn!!!").unwrap();
    let (success, output) = run(&["recover"]);
    assert!(success, "{output}");
    assert!(output.contains("recovered: lsn 2, 2 rows"), "{output}");
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);

    std::fs::write(&wal, b"torn!!!").unwrap();
    let (success, output) = run(&["recover", "--discard-wal"]);
    assert!(success, "{output}");
    assert!(output.contains("discarded WAL"), "{output}");
    assert!(output.contains("recovered: lsn 2, 2 rows"), "{output}");
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);

    std::fs::write(&database, b"not a snapshot").unwrap();
    std::fs::write(&wal, b"do-not-delete").unwrap();
    let (success, output) = run(&["recover", "--discard-wal"]);
    assert!(!success, "{output}");
    assert_eq!(std::fs::read(&wal).unwrap(), b"do-not-delete");

    let relative = directory.join("relative");
    std::fs::create_dir_all(&relative).unwrap();
    let relative_run = Command::new(&binary)
        .current_dir(&relative)
        .arg("notes.lightshade")
        .args([
            "run",
            "add_note",
            "id=3",
            "title=relative",
            "score=3",
            "done=false",
        ])
        .output()
        .expect("failed to run interface with a relative database path");
    assert!(
        relative_run.status.success(),
        "{}",
        String::from_utf8_lossy(&relative_run.stderr)
    );
    assert!(relative.join("notes.lightshade").exists());

    let missing = directory.join("missing.lightshade");
    let (success, output) = Command::new(&binary)
        .arg(&missing)
        .arg("verify")
        .output()
        .map(|output| {
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        })
        .unwrap();
    assert!(!success);
    assert!(output.contains("database does not exist"), "{output}");

    let (success, output) = Command::new(&binary)
        .arg(&missing)
        .args(["run", "finished"])
        .output()
        .map(|output| {
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        })
        .unwrap();
    assert!(!success);
    assert!(output.contains("database does not exist"), "{output}");

    let (success, output) = Command::new(&binary)
        .arg(&missing)
        .args(["verify"])
        .output()
        .map(|output| {
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        })
        .unwrap();
    assert!(!success);
    assert!(output.contains("database does not exist"), "{output}");

    let (success, output) = Command::new(&binary)
        .arg(&missing)
        .arg("migrate")
        .output()
        .map(|output| {
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        })
        .unwrap();
    assert!(!success);
    assert!(output.contains("database does not exist"), "{output}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let victim = relative.join("victim.txt");
        let safe = relative.join("notes.lightshade");
        let lock = relative.join("notes.lightshade.lock");
        let wal = relative.join("notes.lightshade.wal");
        let temporary = relative.join("notes.lightshade.tmp");
        std::fs::write(&victim, b"keep").unwrap();

        std::fs::remove_file(&lock).unwrap();
        symlink(&victim, &lock).unwrap();
        let output = Command::new(&binary)
            .arg(&safe)
            .args([
                "run",
                "add_note",
                "id=9",
                "title=safe",
                "score=9",
                "done=false",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        std::fs::remove_file(&lock).unwrap();

        let _ = std::fs::remove_file(&wal);
        symlink(&victim, &wal).unwrap();
        let output = Command::new(&binary)
            .arg(&safe)
            .args([
                "run",
                "add_note",
                "id=10",
                "title=safe",
                "score=10",
                "done=false",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        std::fs::remove_file(&wal).unwrap();

        symlink(&victim, &temporary).unwrap();
        let output = Command::new(&binary)
            .arg(&safe)
            .arg("compact")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert!(!temporary.exists());
        for path in [&safe, &lock, &wal] {
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{} is not private", path.display());
        }
    }

    let (success, output) = run(&["recover", "--unknown"]);
    assert!(!success);
    assert!(
        output.contains("recover accepts only --discard-wal"),
        "{output}"
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn concurrent_readers_see_a_consistent_prefix() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        READER_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "readers");
    let database = directory.join("notes.lightshade");

    let run = |mode: &str| -> (bool, String) {
        let output = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };

    assert!(run("base").0);

    for writer in ["writer_append", "writer_checkpoint"] {
        let mut child = Command::new(&binary)
            .arg(writer)
            .arg(&database)
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn writer");
        let mut lines = BufReader::new(child.stdout.take().unwrap());
        let guard = KillOnDrop(child);

        let acked = |line: &str| -> Option<i64> {
            line.strip_prefix("wrote ")
                .or_else(|| line.strip_prefix("committed "))
                .and_then(|value| value.trim().parse::<i64>().ok())
        };

        let wanted_checkpoints = if writer == "writer_checkpoint" { 3 } else { 0 };
        let mut last_wrote = 0i64;
        let mut checkpoints = 0usize;
        let mut observed = 0usize;
        let mut line = String::new();
        while observed < 6 || checkpoints < wanted_checkpoints {
            line.clear();
            let read = lines.read_line(&mut line).unwrap();
            assert!(read > 0, "{writer} exited before acknowledging work");
            observed += 1;
            if let Some(value) = acked(&line) {
                last_wrote = value;
            }
            if line.starts_with("checkpointed ") {
                checkpoints += 1;
            }
        }

        let mut previous = 0i64;
        for index in 0..25 {
            let (success, output) = run("reader");
            assert!(success, "reader {index} against {writer} failed: {output}");
            assert!(
                output.contains("dense true"),
                "reader {index} against {writer} was not a dense prefix: {output}"
            );
            let rows = output
                .lines()
                .find_map(|line| line.strip_prefix("rows "))
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or_else(|| panic!("reader {index}: missing row count: {output}"));
            assert!(rows >= 2, "reader {index} lost base rows: {output}");
            assert!(
                rows >= previous,
                "reader {index} against {writer} regressed from {previous} to {rows}"
            );
            previous = rows;
        }

        drop(guard);

        let (success, output) = run("reader");
        assert!(success, "final reader against {writer} failed: {output}");
        let rows = output
            .lines()
            .find_map(|line| line.strip_prefix("rows "))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("final reader: missing row count: {output}"));
        assert!(
            rows >= last_wrote,
            "final reader lost acknowledged rows: {rows} < {last_wrote}"
        );

        let (success, output) = run("reader_write_rejected");
        assert!(success, "read-only rejection run failed: {output}");
        assert!(
            output.contains("write rejected database handle is read-only"),
            "read-only insert was not rejected: {output}"
        );
        assert!(
            output.contains("begin rejected database handle is read-only"),
            "read-only begin was not rejected: {output}"
        );
    }

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn transactions_commit_abort_and_rollback_atomically() {
    use std::process::Command;

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        TXN_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "transactions");
    let database = directory.join("notes.lightshade");
    let mut wal_name = database.as_os_str().to_os_string();
    wal_name.push(".wal");
    let wal = std::path::PathBuf::from(wal_name);

    let run = |mode: &str| {
        let output = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary");
        (output.status.success(), output)
    };

    assert!(run("base").0);
    assert!(run("commit").0);
    let snapshot_backup = directory.join("notes.snapshot.bak");
    let wal_backup = directory.join("notes.wal.bak");
    std::fs::copy(&database, &snapshot_backup).unwrap();
    std::fs::copy(&wal, &wal_backup).unwrap();
    let wal_bytes = std::fs::read(&wal).unwrap();
    let payload_len = u32::from_le_bytes(wal_bytes[20..24].try_into().unwrap()) as usize;
    assert_eq!(wal_bytes.len(), 40 + payload_len);
    assert_eq!(wal_bytes[40], 1);
    assert!(payload_len > 1);
    let committed = "row 1 alpha 10 false\nrow 2 beta 20 true\nrow 3 gamma 15 true\nrow 4 delta 30 false\nfinished 2";
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), committed);

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::copy(&wal_backup, &wal).unwrap();
    let wal_len = std::fs::metadata(&wal).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wal)
        .unwrap()
        .set_len(wal_len - 10)
        .unwrap();
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "row 1 alpha 10 false\nrow 2 beta 20 true\nfinished 1"
    );

    std::fs::copy(&snapshot_backup, &database).unwrap();
    std::fs::copy(&wal_backup, &wal).unwrap();
    assert!(run("abort").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), committed);

    assert!(run("rollback").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), committed);

    let (success, output) = run("errors");
    assert!(success);
    let errors = String::from_utf8_lossy(&output.stdout);
    assert!(
        errors.contains("commit no transaction is active"),
        "{errors}"
    );
    assert!(
        errors.contains("rollback no transaction is active"),
        "{errors}"
    );
    assert!(
        errors.contains("begin transaction already active"),
        "{errors}"
    );
    assert!(
        errors.contains("checkpoint cannot checkpoint during a transaction"),
        "{errors}"
    );
    assert!(
        errors.contains("flush cannot flush during a transaction"),
        "{errors}"
    );
    assert!(
        errors.contains("close cannot close during a transaction"),
        "{errors}"
    );
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), committed);

    assert!(run("rollback_after_autocommit").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "row 1 alpha 10 false\nrow 2 beta 20 true\nrow 3 gamma 15 true\nrow 4 delta 30 false\nrow 8 theta 70 false\nfinished 2"
        );

    let _ = std::fs::remove_dir_all(&directory);
}
