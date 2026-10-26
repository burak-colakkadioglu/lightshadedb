use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

#[cfg(unix)]
#[test]
fn crash_recovery_keeps_committed_prefix() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        CRASH_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "crash");
    let database = directory.join("notes.lightshade");

    let run = |mode: &str| -> (bool, String) {
        let output = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary");
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    };

    let verify = || -> (i64, bool) {
        let (success, output) = run("verify");
        assert!(success, "verify failed: {output}");
        let rows = output
            .lines()
            .find_map(|line| line.strip_prefix("rows "))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("missing row count: {output}"));
        let dense = output
            .lines()
            .find_map(|line| line.strip_prefix("dense "))
            .map(|value| value == "true")
            .unwrap_or_else(|| panic!("missing density flag: {output}"));
        (rows, dense)
    };

    let kill_after_ack = |mode: &str| -> i64 {
        let mut child = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn generated binary");
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let ack = loop {
            line.clear();
            let read = reader.read_line(&mut line).unwrap();
            assert!(read > 0, "{mode} exited before acknowledging work");
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let [kind, value] = parts.as_slice() {
                if matches!(*kind, "ack" | "committed" | "snapshot") {
                    break value.parse::<i64>().unwrap();
                }
            }
        };
        child.kill().unwrap();
        child.wait().unwrap();
        ack
    };

    assert!(run("base").0);

    let (mut rows, dense) = verify();
    assert_eq!(rows, 2);
    assert!(dense);

    for round in 0..4 {
        let ack = kill_after_ack("spin_autocommit");
        let (now, dense) = verify();
        assert!(dense, "autocommit round {round} left gaps");
        assert!(
            now >= ack,
            "autocommit round {round} lost acknowledged row {ack}: {now} rows"
        );
        rows = now;
    }

    let before_transactions = rows;
    for round in 0..4 {
        let ack = kill_after_ack("spin_transaction");
        let (now, dense) = verify();
        assert!(dense, "transaction round {round} left gaps");
        assert!(
            now >= ack,
            "transaction round {round} lost acknowledged row {ack}: {now} rows"
        );
        assert_eq!(
            (now - before_transactions) % 8,
            0,
            "transaction round {round} persisted a partial batch: {now} rows"
        );
        assert!(
            now >= before_transactions + 8,
            "transaction round {round} lost an acknowledged commit"
        );
        rows = now;
    }

    for round in 0..4 {
        let ack = kill_after_ack("spin_checkpoint");
        let (now, dense) = verify();
        assert!(dense, "checkpoint round {round} left gaps");
        assert!(
            now >= ack,
            "checkpoint round {round} lost acknowledged row {ack}: {now} rows"
        );
        rows = now;
    }

    let (final_rows, final_dense) = verify();
    assert!(final_dense);
    assert_eq!(final_rows, rows, "clean reopen changed the recovered state");

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn damaged_wal_is_truncated_to_a_committed_prefix() {
    use std::process::Command;

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        DAMAGE_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "damage");
    let database = directory.join("notes.lightshade");
    let wal = directory.join("notes.lightshade.wal");

    let run = |mode: &str| {
        Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary")
    };

    assert!(run("base").status.success());
    assert!(run("make").status.success());
    let snapshot = std::fs::read(&database).unwrap();
    let original = std::fs::read(&wal).unwrap();
    assert!(
        original.len() > 80,
        "WAL too small to damage: {}",
        original.len()
    );

    let verify = || -> (bool, String) {
        let output = run("verify");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    };

    let check_prefix = |output: &str, context: &str| {
        let rows = output
            .lines()
            .find_map(|line| line.strip_prefix("rows "))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("{context}: missing row count: {output}"));
        assert!(
            (rows - 2) % 2 == 0 && (2..=6).contains(&rows),
            "{context}: recovered {rows} rows, expected 2, 4, or 6: {output}"
        );
        assert!(
            output.contains("dense true"),
            "{context}: recovered state is not a dense prefix: {output}"
        );
    };

    for length in 0..=original.len() {
        std::fs::write(&database, &snapshot).unwrap();
        std::fs::write(&wal, &original[..length]).unwrap();
        let (success, output) = verify();
        assert!(success, "truncation to {length} bytes failed: {output}");
        check_prefix(&output, &format!("truncation to {length} bytes"));
    }

    let known = [
        "bad WAL magic",
        "unsupported WAL version",
        "WAL workload mismatch",
        "WAL schema mismatch",
        "WAL record is too large",
        "WAL checksum mismatch",
        "WAL LSN gap",
        "WAL record has trailing bytes",
        "unknown WAL record kind",
        "WAL was truncated during read",
    ];
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state >> 33
    };
    for flip in 0..64 {
        let position = (next() as usize) % original.len();
        let bit = (next() % 8) as u32;
        let mut damaged = original.clone();
        damaged[position] ^= 1 << bit;
        std::fs::write(&database, &snapshot).unwrap();
        std::fs::write(&wal, &damaged).unwrap();
        let (success, output) = verify();
        let context = format!("flip {flip} at byte {position} bit {bit}");
        if success {
            check_prefix(&output, &context);
        } else {
            assert!(
                known.iter().any(|message| output.contains(message)),
                "{context}: unexpected failure: {output}"
            );
        }
    }

    for mutation in 0..128 {
        let mut damaged = original.clone();
        let position = (next() as usize) % damaged.len();
        match next() % 4 {
            0 => damaged[position] ^= 1 << (next() % 8),
            1 => damaged[position] = (next() % 256) as u8,
            2 => damaged.insert(position, (next() % 256) as u8),
            _ => {
                damaged.remove(position);
            }
        }
        std::fs::write(&database, &snapshot).unwrap();
        std::fs::write(&wal, &damaged).unwrap();
        let (success, output) = verify();
        let context = format!("mutation {mutation} at byte {position}");
        if success {
            check_prefix(&output, &context);
        } else {
            assert!(
                known.iter().any(|message| output.contains(message)),
                "{context}: unexpected failure: {output}"
            );
        }
    }

    let mut future = snapshot.clone();
    future[8..12].copy_from_slice(&9u32.to_le_bytes());
    std::fs::write(&database, &future).unwrap();
    std::fs::write(&wal, []).unwrap();
    let (success, output) = verify();
    assert!(!success);
    assert!(output.contains("unsupported snapshot version"), "{output}");

    let mut future = original.clone();
    future[8..12].copy_from_slice(&6u32.to_le_bytes());
    std::fs::write(&database, &snapshot).unwrap();
    std::fs::write(&wal, &future).unwrap();
    let (success, output) = verify();
    assert!(!success);
    assert!(output.contains("unsupported WAL version"), "{output}");

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(unix)]
#[test]
fn write_failures_surface_as_errors_and_recover() {
    use std::process::Command;

    let notes_workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&notes_workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&notes_workload).unwrap(),
        ERROR_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "write-errors");
    let database = directory.join("notes.lightshade");

    let run = |mode: &str| {
        let output = Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary");
        (output.status.success(), output)
    };

    assert!(run("base").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "rows 2\nfinished 1"
    );

    let limited = run_limited(&binary, "autocommit", &database);
    assert!(
        limited.status.success(),
        "limited autocommit failed: {}",
        String::from_utf8_lossy(&limited.stderr)
    );
    let stdout = String::from_utf8_lossy(&limited.stdout);
    assert!(stdout.contains("write err "), "unexpected output: {stdout}");
    assert!(
        stdout.contains("failed state"),
        "handle was not poisoned: {stdout}"
    );
    let inserted = stdout
        .lines()
        .find_map(|line| line.strip_prefix("inserted "))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("missing inserted count: {stdout}"));
    assert!(
        inserted >= 1,
        "no writes succeeded before the failure: {stdout}"
    );
    assert!(
        stdout.contains(&format!("rows {}", 2 + inserted)),
        "failed write was applied: {stdout}"
    );

    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("rows {}\nfinished 1", 2 + inserted)
    );

    let before = 2 + inserted;
    let limited = run_limited(&binary, "transaction", &database);
    assert!(
        limited.status.success(),
        "limited transaction failed: {}",
        String::from_utf8_lossy(&limited.stderr)
    );
    let stdout = String::from_utf8_lossy(&limited.stdout);
    assert!(
        stdout.contains("commit err "),
        "unexpected output: {stdout}"
    );
    assert!(
        stdout.contains("again database is in a failed state"),
        "failed commit did not poison the handle: {stdout}"
    );
    assert!(
        stdout.contains("flush err database is in a failed state"),
        "failed flush did not poison the handle: {stdout}"
    );

    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("rows {before}\nfinished 1")
    );

    assert!(run("recover").0);
    let (success, output) = run("verify");
    assert!(success);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("rows {}\nfinished 2", before + 1)
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(target_os = "linux")]
#[test]
fn fault_injection_preserves_committed_prefix() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&workload).unwrap(),
        FAULT_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "fault-injection");
    let database = directory.join("fault.lsdb");
    let shim = match build_fault_shim(&directory) {
        Some(shim) => shim,
        None => {
            eprintln!("skipping fault injection test: cc is unavailable");
            return;
        }
    };

    let run = |mode: &str, variables: &[(&str, &str)]| -> std::process::Output {
        let mut command = Command::new(&binary);
        command.arg(mode).arg(&database);
        for (name, value) in variables {
            command.env(name, value);
        }
        command.output().expect("failed to run generated binary")
    };
    let run_fault = |mode: &str, variables: &[(&str, &str)]| -> std::process::Output {
        let mut command = Command::new(&binary);
        command.arg(mode).arg(&database).env("LD_PRELOAD", &shim);
        for (name, value) in variables {
            command.env(name, value);
        }
        command.output().expect("failed to run generated binary")
    };
    let verify = |expected: &[i64]| -> i64 {
        let output = run("verify", &[]);
        assert!(
            output.status.success(),
            "verify failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let rows = stdout
            .lines()
            .find_map(|line| line.strip_prefix("rows "))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("missing row count: {stdout}"));
        assert!(
            expected.contains(&rows),
            "recovered {rows} rows, expected one of {expected:?}: {stdout}"
        );
        assert!(
            stdout.contains("dense true"),
            "state is not dense: {stdout}"
        );
        rows
    };

    assert!(run("base", &[]).status.success());
    let mut rows = verify(&[2]);

    let output = run_fault("append", &[("LS_FAULT_WRITE_AFTER", "0")]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("write err "),
        "write failure was not reported: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    rows = verify(&[rows]);

    let output = run_fault(
        "append",
        &[
            ("LS_FAULT_WRITE_AFTER", "0"),
            ("LS_FAULT_PARTIAL_WRITE", "1"),
        ],
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("write err "),
        "partial write failure was not reported: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    rows = verify(&[rows, rows + 1]);

    let output = run_fault("append", &[("LS_FAULT_SYNC_AFTER", "0")]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("write err "),
        "sync failure was not reported: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    rows = verify(&[rows, rows + 1]);

    let output = run_fault("checkpoint", &[("LS_FAULT_RENAME_AFTER", "0")]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("checkpoint err "),
        "checkpoint failure was not reported: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    rows = verify(&[rows + 1]);

    let output = run("append", &[]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ack "),
        "normal append failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    verify(&[rows + 1]);

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(target_os = "linux")]
#[test]
fn checkpoint_write_failure_preserves_the_previous_snapshot() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&workload).unwrap(),
        FAULT_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "disk-full");
    let database = directory.join("notes.lsdb");
    let run = |mode: &str| -> std::process::Output {
        Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary")
    };

    let shim = match build_fault_shim(&directory) {
        Some(shim) => shim,
        None => {
            eprintln!("skipping checkpoint failure test: cc is unavailable");
            return;
        }
    };
    let output = run("fill");
    assert!(
        output.status.success(),
        "fill failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let failed = Command::new(&binary)
        .arg("checkpoint")
        .arg(&database)
        .env("LD_PRELOAD", &shim)
        .env("LS_FAULT_TMP_WRITE_AFTER", "0")
        .output()
        .expect("failed to run checkpoint");
    assert!(
        failed.status.success(),
        "checkpoint process crashed: {}",
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&failed.stdout).contains("checkpoint err "),
        "checkpoint unexpectedly succeeded: {}",
        String::from_utf8_lossy(&failed.stdout)
    );

    let output = run("verify");
    assert!(
        output.status.success(),
        "the previous snapshot was not recoverable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("rows 201"));

    let _ = std::fs::remove_dir_all(&directory);
}

#[cfg(target_os = "linux")]
#[test]
fn permission_loss_is_reported_and_recoverable() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::process::Command;

    if matches!(
        std::fs::metadata("/proc/self").map(|meta| meta.uid()),
        Ok(0)
    ) {
        eprintln!("skipping permission loss test: running as root");
        return;
    }
    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&workload).unwrap(),
        FAULT_HARNESS
    );
    let (directory, binary) = compile_generated_test(&source, "permission-loss");
    let data = directory.join("data");
    std::fs::create_dir(&data).unwrap();
    let database = data.join("notes.lsdb");
    let wal = data.join("notes.lsdb.wal");
    let run = |mode: &str| -> std::process::Output {
        Command::new(&binary)
            .arg(mode)
            .arg(&database)
            .output()
            .expect("failed to run generated binary")
    };
    let verify = || -> std::process::Output { run("verify") };

    assert!(run("base").status.success());
    assert!(verify().status.success());

    std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o000)).unwrap();
    let output = verify();
    assert!(!output.status.success(), "an unreadable WAL was accepted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Permission denied"),
        "unexpected permission error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output = verify();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("rows 2"));

    std::fs::remove_file(&wal).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
    let output = verify();
    assert!(
        !output.status.success(),
        "a database in a read-only directory was writable"
    );
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = verify();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("rows 2"));

    let _ = std::fs::remove_dir_all(&directory);
}
