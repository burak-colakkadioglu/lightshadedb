use super::super::super::*;
use super::super::helpers::*;
use crate::ir::parse::parse_workload;

const STORAGE_HARNESS: &str = r#"
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let path = args[2].as_str();
    match mode {
        "seed" => {
            let mut db = Db::open(path).unwrap();
            for id in 0..40i64 {
                db.add_note(id, "compressible-".to_string() + &"a".repeat(1024), id, id % 2 == 0).unwrap();
            }
            db.close().unwrap();
        }
        "seed_small" => {
            let mut db = Db::open(path).unwrap();
            db.add_note(1, "alpha".to_string(), 10, false).unwrap();
            db.add_note(2, "beta".to_string(), 20, true).unwrap();
            db.close().unwrap();
        }
        "append_exit" => {
            let mut db = Db::open(path).unwrap();
            let next = db.t_notes.keys().max().unwrap_or(0) + 1;
            db.add_note(next, "unclosed-write".to_string(), next, false).unwrap();
            std::process::exit(0);
        }
        "verify" => {
            match Db::open(path) {
                Ok(db) => {
                    let mut ids: Vec<i64> = db.t_notes.keys().collect();
                    ids.sort_unstable();
                    println!("rows {}", ids.len());
                    if let Some(first) = db.t_notes.get(&ids[0]) {
                        println!("title_len {}", first.title.len());
                    }
                    println!("finished {}", db.finished());
                    let _ = db.close();
                }
                Err(error) => {
                    eprintln!("open failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        "verify_key" => {
            match Db::open(path) {
                Ok(db) => {
                    println!("rows {}", db.t_notes.len());
                    let _ = db.close();
                }
                Err(error) => {
                    eprintln!("open failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
"#;

fn build_storage_binary(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!(
        "{}\n{}",
        generate_library(&workload).unwrap(),
        STORAGE_HARNESS
    );
    compile_generated_test(&source, name)
}

fn write_key_file(directory: &std::path::Path, name: &str, byte: u8) -> std::path::PathBuf {
    let path = directory.join(name);
    let hex: String = std::iter::repeat_n(byte, 32)
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(&path, hex.as_bytes()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

#[test]
fn preallocated_wal_recovers_through_zero_tail() {
    use std::process::Command;
    let (directory, binary) = build_storage_binary("storage-prealloc");
    let database = directory.join("prealloc.lightshade");
    let mut wal_name = database.as_os_str().to_os_string();
    wal_name.push(".wal");
    let wal = std::path::PathBuf::from(wal_name);
    let run = |mode: &str, vars: &[(&str, &str)]| -> std::process::Output {
        let mut command = Command::new(&binary);
        command.arg(mode).arg(&database);
        for (name, value) in vars {
            command.env(name, value);
        }
        command.output().expect("failed to run storage binary")
    };
    let prealloc: &[(&str, &str)] = &[("LIGHTSHADE_WAL_PREALLOC_BYTES", "65536")];
    let seeded = run("seed_small", prealloc);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let wal_len = std::fs::metadata(&wal).unwrap().len();
    assert!(wal_len >= 65536, "wal is {wal_len}");
    let verified = run("verify", prealloc);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(String::from_utf8_lossy(&verified.stdout).contains("rows 2"));
    let appended = run("append_exit", prealloc);
    assert!(appended.status.success());
    let wal_len = std::fs::metadata(&wal).unwrap().len();
    assert!(wal_len >= 65536, "wal is {wal_len}");
    let raw = std::fs::read(&wal).unwrap();
    assert!(raw.len() > 40);
    let mut extended = raw.clone();
    extended.extend_from_slice(&vec![0u8; 4096]);
    std::fs::write(&wal, &extended).unwrap();
    let verified = run("verify", prealloc);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let output = String::from_utf8_lossy(&verified.stdout).to_string();
    assert!(output.contains("rows 3"), "{output}");
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn compression_round_trip_and_mixed_replay() {
    use std::process::Command;
    let (directory, binary) = build_storage_binary("storage-compress");
    let database = directory.join("compressed.lightshade");
    let plain_db = directory.join("plain.lightshade");
    let run_on =
        |db: &std::path::Path, mode: &str, vars: &[(&str, &str)]| -> std::process::Output {
            let mut command = Command::new(&binary);
            command.arg(mode).arg(db);
            for (name, value) in vars {
                command.env(name, value);
            }
            command.output().expect("failed to run storage binary")
        };
    let on: &[(&str, &str)] = &[("LIGHTSHADE_COMPRESSION", "1")];
    let off: &[(&str, &str)] = &[];
    let seeded = run_on(&database, "seed", on);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let verified = run_on(&database, "verify", on);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let output = String::from_utf8_lossy(&verified.stdout).to_string();
    assert!(output.contains("rows 40"), "{output}");
    assert!(output.contains("title_len 1037"), "{output}");
    let reread = run_on(&database, "verify", off);
    assert!(
        reread.status.success(),
        "{}",
        String::from_utf8_lossy(&reread.stderr)
    );
    assert!(String::from_utf8_lossy(&reread.stdout).contains("rows 40"));
    let seeded = run_on(&plain_db, "seed", off);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let plain_size = std::fs::metadata(&plain_db).unwrap().len();
    let coded_size = std::fs::metadata(&database).unwrap().len();
    assert!(
        coded_size < plain_size,
        "coded {coded_size} plain {plain_size}"
    );
    let mixed = run_on(&plain_db, "verify", on);
    assert!(
        mixed.status.success(),
        "{}",
        String::from_utf8_lossy(&mixed.stderr)
    );
    assert!(String::from_utf8_lossy(&mixed.stdout).contains("rows 40"));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn encryption_round_trip_and_wrong_key_fails() {
    use std::process::Command;
    let (directory, binary) = build_storage_binary("storage-encrypt");
    let database = directory.join("encrypted.lightshade");
    let key_a = write_key_file(&directory, "key-a", 0x11);
    let key_b = write_key_file(&directory, "key-b", 0x22);
    let run = |mode: &str, key: Option<&std::path::Path>| -> std::process::Output {
        let mut command = Command::new(&binary);
        command.arg(mode).arg(&database);
        match key {
            Some(path) => {
                command.env("LIGHTSHADE_ENCRYPTION_KEY_FILE", path);
            }
            None => {
                command.env_remove("LIGHTSHADE_ENCRYPTION_KEY_FILE");
            }
        }
        command.output().expect("failed to run storage binary")
    };
    let seeded = run("seed_small", Some(&key_a));
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let verified = run("verify_key", Some(&key_a));
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(String::from_utf8_lossy(&verified.stdout).contains("rows 2"));
    let wrong = run("verify_key", Some(&key_b));
    assert!(!wrong.status.success());
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&wrong.stdout),
        String::from_utf8_lossy(&wrong.stderr)
    );
    assert!(
        message.contains("authentication")
            || message.contains("checksum")
            || message.contains("open failed"),
        "{message}"
    );
    assert!(!message.contains("panicked"));
    let missing = run("verify_key", None);
    assert!(!missing.status.success());
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&missing.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(
        message.contains("encrypted")
            || message.contains("checksum")
            || message.contains("open failed"),
        "{message}"
    );
    assert!(!message.contains("panicked"));
    let snapshot_before = std::fs::read(&database).unwrap();
    let _ = run("verify_key", Some(&key_b));
    let snapshot_after = std::fs::read(&database).unwrap();
    assert_eq!(snapshot_before, snapshot_after);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn mixed_codec_replay_reads_plain_and_coded_records() {
    use std::process::Command;
    let (directory, binary) = build_storage_binary("storage-mixed");
    let database = directory.join("mixed.lightshade");
    let key = write_key_file(&directory, "mix-key", 0x33);
    let run = |mode: &str, vars: &[(&str, &str)]| -> std::process::Output {
        let mut command = Command::new(&binary);
        command.arg(mode).arg(&database);
        for (name, value) in vars {
            command.env(name, value);
        }
        command.output().expect("failed to run storage binary")
    };
    let plain: &[(&str, &str)] = &[];
    let seeded = run("seed_small", plain);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let key_str = key.to_str().unwrap().to_string();
    let appended = {
        let mut command = Command::new(&binary);
        command.arg("append_exit").arg(&database);
        command.env("LIGHTSHADE_COMPRESSION", "1");
        command.env("LIGHTSHADE_ENCRYPTION_KEY_FILE", &key_str);
        command.output().expect("failed to run storage binary")
    };
    assert!(appended.status.success());
    let verify_both = {
        let mut command = Command::new(&binary);
        command.arg("verify").arg(&database);
        command.env("LIGHTSHADE_COMPRESSION", "1");
        command.env("LIGHTSHADE_ENCRYPTION_KEY_FILE", &key_str);
        command.output().expect("failed to run storage binary")
    };
    assert!(
        verify_both.status.success(),
        "{}",
        String::from_utf8_lossy(&verify_both.stderr)
    );
    assert!(String::from_utf8_lossy(&verify_both.stdout).contains("rows 3"));
    let _ = std::fs::remove_dir_all(&directory);
}
