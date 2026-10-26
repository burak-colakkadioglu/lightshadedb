use super::super::*;
use super::helpers::*;
use crate::ir::parse::parse_workload;

#[test]
fn generates_typed_rows_and_maintained_structures() {
    let source = generate(&workload()).unwrap();
    assert!(source.contains("struct Row_messages"));
    assert!(source.contains("idx0"));
    assert!(source.contains("cnt0"));
    assert!(source.contains("fn recent_messages(&self, inbox: i64, limit: i64)"));
    assert!(source.contains("fn unread_count(&self, inbox: i64) -> i64"));
    assert!(source.contains("fn mark_read(&mut self, message: i64) -> io::Result<u64>"));
    assert!(!source.contains("fn main()"));
}

#[test]
fn keeps_generated_code_deterministic() {
    let first = generate(&workload()).unwrap();
    let second = generate(&workload()).unwrap();
    assert_eq!(first, second);
}

#[test]
fn events_use_range_seek_and_pk_delete() {
    let workload = parse_workload(include_str!("../../../../examples/events.lightshade")).unwrap();
    validate(&workload).unwrap();
    let source = generate_library(&workload).unwrap();
    assert!(source.contains("values_from(&__ls_start)"));
    assert!(source.contains("pub fn purge_event(&mut self, event: i64) -> io::Result<u64>"));
    assert!(!source.contains("fn main()"));
}

#[test]
fn advanced_queries_run_through_generated_interfaces() {
    use std::process::Command;

    let workload = parse_workload(INBOX_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "inbox-digest");
    let database = directory.join("inbox.lightshade");

    let run = |args: &[&str]| {
        let output = Command::new(&binary)
            .arg(&database)
            .args(args)
            .output()
            .expect("failed to run generated interface");
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

    let (success, output) = run(&["run", "add_inbox", "id=99", "owner=seed"]);
    assert!(success, "{output}");
    assert_eq!(output, "ok true");

    let (success, output) = run(&["run", "overall"]);
    assert!(success, "{output}");
    assert_eq!(output, "0\t0\t0\t0\t0");

    let inserts: &[&[&str]] = &[
        &["run", "add_inbox", "id=1", "owner=alice"],
        &["run", "add_inbox", "id=2", "owner=bob"],
        &[
            "run",
            "add_message",
            "id=1",
            "inbox_id=1",
            "sent_at=100",
            "is_read=false",
            "body=first",
        ],
        &[
            "run",
            "add_message",
            "id=2",
            "inbox_id=1",
            "sent_at=200",
            "is_read=true",
            "body=second",
        ],
        &[
            "run",
            "add_message",
            "id=3",
            "inbox_id=1",
            "sent_at=300",
            "is_read=false",
            "body=third",
        ],
        &[
            "run",
            "add_message",
            "id=4",
            "inbox_id=2",
            "sent_at=400",
            "is_read=false",
            "body=other",
        ],
    ];
    for args in inserts {
        let (success, output) = run(args);
        assert!(success, "{output}");
        assert_eq!(output, "ok true");
    }

    let (success, output) = run(&["run", "unread_digest"]);
    assert!(success, "{output}");
    assert_eq!(output, "alice\t2\t300\t100\t400\nbob\t1\t400\t400\t400");

    let (success, output) = run(&["run", "overall"]);
    assert!(success, "{output}");
    assert_eq!(output, "4\t100\t400\t250\t1000");

    let (success, output) = run(&["run", "excerpts", "minimum=150"]);
    assert!(success, "{output}");
    assert_eq!(output, "3\talice\tthird\n2\talice\tsecond");

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn non_demo_workloads_compile_and_run() {
    use std::process::Command;

    const HARNESS: &str = r#"
fn main() {
    let mut db = Db::__ls_new().unwrap();
    assert!(db.add_note(1, "alpha".to_string(), 10, false).unwrap());
    assert!(db.add_note(2, "beta".to_string(), 20, true).unwrap());
    assert!(db.add_note(3, "gamma".to_string(), 15, false).unwrap());
    assert_eq!(db.finished(), 1);

    let searched = db.search_notes(10, "nope".to_string());
    assert_eq!(
        searched.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 3, 2]
    );

    let alpha = db.by_title("alpha".to_string());
    assert_eq!(alpha.len(), 1);
    assert_eq!(alpha[0].id, 1);

    db.finish(1).unwrap();
    db.finish(1).unwrap();
    assert_eq!(db.finished(), 2);

    db.mark_all_done().unwrap();
    assert_eq!(db.finished(), 3);

    db.retitle("zeta".to_string()).unwrap();
    assert_eq!(db.by_title("zeta".to_string()).len(), 3);
    assert!(db.by_title("alpha".to_string()).is_empty());
    assert!(db.either(5, 100).is_empty());
    assert_eq!(db.either(11, 100).len(), 1);

    assert_eq!(db.delete_note(1).unwrap(), 1);
    assert_eq!(db.delete_note(1).unwrap(), 0);
    assert_eq!(db.finished(), 2);
    assert_eq!(db.purge_low(100).unwrap(), 2);
    assert_eq!(db.finished(), 0);
    assert!(db.by_title("zeta".to_string()).is_empty());
    assert_eq!(db.clear_notes().unwrap(), 0);
    println!("ok");
}
"#;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();

    let source = format!("{}\n{HARNESS}", generate_library(&workload).unwrap());
    let dir = std::env::temp_dir().join(format!(
        "lightshadedb-codegen-{}-{}",
        std::process::id(),
        "non_demo"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("notes.rs");
    let binary_path = dir.join("notes");
    std::fs::write(&source_path, source).unwrap();

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(&rustc)
        .arg("--edition=2021")
        .arg("-O")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("failed to run rustc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "rustc failed:\n{stderr}");

    let output = Command::new(&binary_path)
        .output()
        .expect("failed to run generated binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "generated binary failed:\n{stderr}"
    );
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn interface_runs_operations_and_reports() {
    use std::process::Command;

    let workload = parse_workload(ALT_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "interface");
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

    let (success, output) = run(&["run", "clear_notes"]);
    assert!(success, "{output}");
    assert_eq!(output.trim(), "ok 0");

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn expanded_types_round_trip_through_generated_interfaces() {
    use std::process::Command;

    let workload = parse_workload(EXPANDED_SOURCE).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "expanded-types");
    let database = directory.join("types.lsdb");

    let run = |args: &[&str]| {
        let output = Command::new(&binary)
            .arg(&database)
            .args(args)
            .output()
            .expect("failed to run generated interface");
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

    let (success, output) = run(&[
        "run",
        "add_all",
        "id=123e4567-e89b-12d3-a456-426614174000",
        "small=-5",
        "tiny=250",
        "big=18446744073709551615",
        "half=1.5",
        "real=-2.25",
        "flag=true",
        "label=hello",
        "blob=deadbeef",
    ]);
    assert!(success, "{output}");
    let (success, output) = run(&[
        "run",
        "add_all",
        "id=223e4567-e89b-12d3-a456-426614174001",
        "small=2",
        "tiny=3",
        "big=8",
        "half=3.25",
        "real=4.5",
        "flag=false",
        "label=second",
        "blob=00ff",
    ]);
    assert!(success, "{output}");

    let (success, output) = run(&["run", "scan"]);
    assert!(success, "{output}");
    assert!(
        output.contains("-5\t250\t18446744073709551615\t1.5\t-2.25\ttrue\thello\tdeadbeef"),
        "{output}"
    );
    assert!(
        output.contains("2\t3\t8\t3.25\t4.5\tfalse\tsecond\t00ff"),
        "{output}"
    );

    let (success, output) = run(&["run", "floats", "min=1.5"]);
    assert!(success, "{output}");
    assert!(
        output.contains("second") && !output.contains("hello"),
        "{output}"
    );

    let (success, output) = run(&["run", "huge"]);
    assert!(success, "{output}");
    assert_eq!(output, "1");

    let (success, output) = run(&["run", "retag", "real=9.5", "label=tagged"]);
    assert!(success, "{output}");
    assert_eq!(output, "ok 1");
    let (success, output) = run(&["run", "scan"]);
    assert!(success, "{output}");
    assert!(output.contains("9.5\ttrue\ttagged"), "{output}");

    let (success, output) = run(&["inspect"]);
    assert!(success, "{output}");
    assert!(output.contains("version: 8"), "{output}");
    assert!(output.contains("half: f32"), "{output}");
    assert!(output.contains("real: f64"), "{output}");
    assert!(output.contains("blob: bytes"), "{output}");
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn float_primary_keys_round_trip_and_order_totally() {
    use std::process::Command;

    let source = r#"
table points {
  id: f64 primary_key
  label: text
}

operation add(id: f64, label: text) {
  insert points
}

operation all() -> many points {
  order by id asc
}

operation fetch(wanted: f64) -> many points {
  filter id == wanted
}

operation remove(wanted: f64) {
  delete points where id == wanted
}
"#;
    let workload = parse_workload(source).unwrap();
    validate(&workload).unwrap();
    let source = generate_interface(&workload).unwrap();
    let (directory, binary) = compile_generated_test(&source, "float-primary-key");
    let database = directory.join("points.lsdb");

    let run = |args: &[&str]| {
        let output = Command::new(&binary)
            .arg(&database)
            .args(args)
            .output()
            .expect("failed to run generated interface");
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

    for (id, label) in [
        ("1.5", "positive"),
        ("-2.25", "negative"),
        ("0", "zero"),
        ("-0", "negative-zero"),
    ] {
        let (success, output) =
            run(&["run", "add", &format!("id={id}"), &format!("label={label}")]);
        assert!(success, "{output}");
    }

    let (success, output) = run(&["run", "all"]);
    assert!(success, "{output}");
    let labels: Vec<&str> = output
        .lines()
        .map(|line| line.split('\t').nth(1).unwrap())
        .collect();
    assert_eq!(labels, ["negative", "negative-zero", "zero", "positive"]);

    let (success, output) = run(&["run", "fetch", "wanted=-0"]);
    assert!(success, "{output}");
    assert!(output.contains("negative-zero"), "{output}");
    assert!(!output.contains("\tzero\t"), "{output}");

    let (success, output) = run(&["run", "remove", "wanted=1.5"]);
    assert!(success, "{output}");
    assert_eq!(output, "ok 1");
    let (success, output) = run(&["run", "fetch", "wanted=1.5"]);
    assert!(success, "{output}");
    assert!(output.is_empty(), "{output}");

    let (success, output) = run(&["run", "remove", "wanted=-0"]);
    assert!(success, "{output}");
    assert_eq!(output, "ok 1");
    let (success, output) = run(&["run", "fetch", "wanted=-0"]);
    assert!(success, "{output}");
    assert!(output.is_empty(), "{output}");
    let (success, output) = run(&["run", "fetch", "wanted=0"]);
    assert!(success, "{output}");
    assert!(output.contains("zero"), "{output}");

    let (success, output) = run(&["verify"]);
    assert!(success, "{output}");
    let (success, output) = run(&["run", "all"]);
    assert!(success, "{output}");
    assert!(
        output.contains("negative") && output.contains("zero"),
        "{output}"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn format_version_tracks_the_expanded_types() {
    let workload = parse_workload(ALT_SOURCE).unwrap();
    let source = generate_interface(&workload).unwrap();
    assert!(source.contains("const __LS_VERSION: u32 = 8;"));
    assert!(source.contains("const __LS_PREVIOUS_VERSION: u32 = 7;"));
    assert!(source.contains("const __LS_OLD_VERSION: u32 = 6;"));
    assert!(source.contains("const __LS_WAL_VERSION: u32 = 3;"));
}
