use super::super::*;
use super::helpers::*;
use crate::ir::parse::parse_workload;

#[test]
fn open_migrated_maps_old_snapshots_by_name() {
    use std::process::Command;

    let compile = |source: &str, harness: &str, name: &str| {
        let workload = parse_workload(source).unwrap();
        validate(&workload).unwrap();
        let combined = format!("{}\n{harness}", generate_library(&workload).unwrap());
        compile_generated_test(&combined, name).1
    };

    let old_binary = compile(MIGRATION_OLD, SEED_HARNESS, "migration-old");
    let new_binary = compile(MIGRATION_NEW, MIGRATION_HARNESS, "migration-new");
    let bad_binary = compile(MIGRATION_BAD, MIGRATION_HARNESS, "migration-bad");
    let type_binary = compile(TYPE_OLD, TYPE_HARNESS, "migration-type");

    let directory = std::env::temp_dir().join(format!(
        "lightshadedb-codegen-{}-{}",
        std::process::id(),
        "migration"
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("notes.lightshade");
    let mut wal_name = database.as_os_str().to_os_string();
    wal_name.push(".wal");
    let wal = std::path::PathBuf::from(wal_name);

    let run = |binary: &std::path::Path, mode: &str, path: &std::path::Path| {
        let output = Command::new(binary)
            .arg(mode)
            .arg(path)
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

    assert!(run(&old_binary, "seed", &database).0);
    let original = std::fs::read(&database).unwrap();

    let (success, output) = run(&new_binary, "strict", &database);
    assert!(success, "{output}");
    assert!(
        output.contains("snapshot schema mismatch; reopen with open_migrated to upgrade"),
        "unexpected strict output: {output}"
    );

    let migrated_rows = "row 1 alpha 10 false misc 7 true\nrow 2 beta 20 true misc 7 true\nrows 2";
    let (success, output) = run(&new_binary, "reader", &database);
    assert!(success, "reader migration failed: {output}");
    assert_eq!(output.trim(), migrated_rows, "reader output: {output}");
    assert_eq!(
        std::fs::read(&database).unwrap(),
        original,
        "read-only migration must not rewrite the snapshot"
    );

    let (success, output) = run(&new_binary, "migrate", &database);
    assert!(success, "migrate failed: {output}");
    assert_eq!(output.trim(), migrated_rows, "migrate output: {output}");
    let (success, output) = run(&new_binary, "verify", &database);
    assert!(success, "verify after migrate failed: {output}");
    assert_eq!(output.trim(), migrated_rows, "verify output: {output}");

    let type_database = directory.join("types.lightshade");
    assert!(run(&type_binary, "seed", &type_database).0);
    let (success, output) = run(&new_binary, "migrate", &type_database);
    assert!(success, "{output}");
    assert!(
        output.contains("cannot migrate column `notes.score`: stored type does not match"),
        "unexpected type migration output: {output}"
    );

    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(&database);
    assert!(run(&old_binary, "seed", &database).0);
    let seeded = std::fs::read(&database).unwrap();
    let (success, output) = run(&bad_binary, "migrate", &database);
    assert!(success, "{output}");
    assert!(
        output.contains("cannot migrate table `notes`: column `tag` is new and has no default"),
        "unexpected bad migration output: {output}"
    );
    assert_eq!(std::fs::read(&database).unwrap(), seeded);

    assert!(run(&old_binary, "seed_dirty", &database).0);
    assert!(std::fs::metadata(&wal).unwrap().len() > 0);
    let (success, output) = run(&new_binary, "migrate", &database);
    assert!(success, "migrate with a pending WAL failed: {output}");
    assert_eq!(
            output.trim(),
            "row 1 alpha 10 false misc 7 true\nrow 2 beta 20 true misc 7 true\nrow 3 gamma 30 false misc 7 true\nrows 3",
            "migrate with a pending WAL output: {output}"
        );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn reads_version_six_snapshots() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let old_source = source
        .replace(
            "const __LS_VERSION: u32 = 9;",
            "const __LS_VERSION: u32 = 6;",
        )
        .replacen(
            "__ls_put_u64(&mut header, __LS_SCHEMA);",
            "__ls_put_u64(&mut header, __LS_WORKLOAD);",
            1,
        );
    assert_ne!(old_source, source);
    let (old_directory, old_binary) = compile_generated_test(&old_source, "version-six");
    let (_, new_binary) = compile_generated_test(&source, "version-six");
    let database = old_directory.join("notes.lsdb");

    let output = Command::new(&old_binary)
        .arg(&database)
        .args([
            "run",
            "add_note",
            "id=1",
            "title=old",
            "score=5",
            "done=false",
        ])
        .output()
        .expect("failed to run old interface");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(&new_binary)
        .arg(&database)
        .args(["run", "by_title", "wanted=old"])
        .output()
        .expect("failed to run new interface");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert!(text.contains("old"), "{text}");

    let output = Command::new(&new_binary)
        .arg(&database)
        .args(["run", "finish", "note=1"])
        .output()
        .expect("failed to run new interface");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");

    let output = Command::new(&new_binary)
        .arg(&database)
        .arg("inspect")
        .output()
        .expect("failed to inspect");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("version: 9"), "{text}");
    let _ = std::fs::remove_dir_all(&old_directory);
}

#[test]
fn replays_a_legacy_operation_wal() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let (lsn, wal) = {
        let mut db = Db::open(&path).unwrap();
        db.add_note(1, "alpha".to_string(), 10, false).unwrap();
        let lsn = db.__ls_lsn;
        let mut wal = std::path::PathBuf::from(&path);
        let mut name = wal.as_os_str().to_os_string();
        name.push(".wal");
        wal = std::path::PathBuf::from(name);
        db.close().unwrap();
        (lsn, wal)
    };

    let mut payload = vec![0u8];
    __ls_put_u32(&mut payload, 0);
    __ls_put_u32(&mut payload, 4);
    __ls_put_value(&mut payload, __LsValue::I64(2));
    __ls_put_value(&mut payload, __LsValue::Text("beta"));
    __ls_put_value(&mut payload, __LsValue::I64(20));
    __ls_put_value(&mut payload, __LsValue::Bool(true));

    let mut record = Vec::new();
    record.extend_from_slice(&__LS_MAGIC);
    __ls_put_u32(&mut record, __LS_LEGACY_WAL_VERSION);
    __ls_put_u64(&mut record, __LS_WORKLOAD);
    __ls_put_u32(&mut record, payload.len() as u32);
    __ls_put_u64(&mut record, lsn + 1);
    __ls_put_u64(&mut record, __ls_hash(&payload));
    record.extend_from_slice(&payload);

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&wal)
        .unwrap();
    file.set_len(0).unwrap();
    file.write_all(&record).unwrap();
    file.sync_data().unwrap();

    let db = Db::open(&path).unwrap();
    println!(
        "rows {} title {} done {}",
        db.t_notes.len(),
        db.t_notes.get(&2).unwrap().title,
        db.t_notes.get(&2).unwrap().done
    );
    db.close().unwrap();
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = format!(
        "{}
{HARNESS}",
        generate_library(&workload).unwrap()
    );
    let (directory, binary) = compile_generated_test(&source, "legacy-wal");
    let database = directory.join("legacy.lightshade");
    let output = Command::new(&binary)
        .arg(&database)
        .output()
        .expect("failed to run generated binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "rows 2 title beta done true"
    );
    let _ = std::fs::remove_dir_all(&directory);
}
