use compat_store::{Db, Widget};
use std::path::Path;
use std::process::Command;

#[lightshade::schema]
pub mod compat_store {
    use lightshade::prelude::*;

    pub struct Widget {
        #[lightshade(primary_key)]
        pub id: i64,
        pub name: String,
        pub score: i64,
    }

    #[lightshade::operations]
    pub trait WidgetOps {
        #[lightshade::query(filter = Widget::id.eq(id))]
        fn by_id(&self, id: i64) -> Result<Vec<Widget>>;
    }
}

#[lightshade::schema]
pub mod nullable_store {
    use lightshade::prelude::*;

    pub struct Widget {
        #[lightshade(primary_key)]
        pub id: i64,
        pub name: Option<String>,
        pub score: i64,
    }

    #[lightshade::operations]
    pub trait NullableWidgetOps {
        #[lightshade::query(filter = Widget::id.eq(id))]
        fn by_id(&self, id: i64) -> Result<Vec<Widget>>;
    }
}

const COMPAT_WORKLOAD: &str = "table widget {\n    id: i64 primary_key\n    name: text\n    score: i64\n}\noperation get_widget(wanted: i64) -> many widget {\n    filter id == wanted\n    limit 1\n}\n";

fn interface_binary(workload: &str, out: &Path) {
    let parsed = lightshade_codegen::ir::parse_workload(workload).unwrap();
    lightshade_codegen::ir::validate(&parsed).unwrap();
    let source = lightshade_codegen::compiler::generate_interface(&parsed).unwrap();
    let rs = out.with_extension("rs");
    std::fs::write(&rs, source).unwrap();
    let status = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(out)
        .arg(&rs)
        .status()
        .unwrap();
    assert!(status.success());
}

fn run(interface: &Path, db: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(interface).arg(db).args(args).output().unwrap();
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

fn seed_widgets() -> Db {
    let mut db = Db::open_in_memory().unwrap();
    for (id, name, score) in [(1, "alpha", 10), (2, "beta, \"quoted\"", 20)] {
        db.insert(Widget {
            id,
            name: name.to_string(),
            score,
        })
        .unwrap();
    }
    db
}

#[test]
fn rust_dump_restores_through_the_legacy_interface() {
    let directory = tempfile::tempdir().unwrap();
    let interface = directory.path().join("compat-interface");
    interface_binary(COMPAT_WORKLOAD, &interface);

    let dump = directory.path().join("widgets.dump");
    seed_widgets().dump(&dump).unwrap();

    let database = directory.path().join("cli.lsdb");
    let (ok, text) = run(&interface, &database, &["restore", &dump.to_string_lossy()]);
    assert!(ok, "{text}");

    let (ok, text) = run(&interface, &database, &["run", "get_widget", "wanted=2"]);
    assert!(ok, "{text}");
    assert!(text.contains("2\tbeta, \"quoted\"\t20"), "{text}");
}

#[test]
fn legacy_dump_restores_through_rust() {
    let directory = tempfile::tempdir().unwrap();
    let interface = directory.path().join("compat-interface");
    interface_binary(COMPAT_WORKLOAD, &interface);

    let seed = directory.path().join("seed.lsdb");
    let mut db = Db::open(&seed).unwrap();
    db.insert(Widget {
        id: 1,
        name: "alpha".into(),
        score: 10,
    })
    .unwrap();
    db.insert(Widget {
        id: 2,
        name: "beta".into(),
        score: 20,
    })
    .unwrap();
    db.close().unwrap();

    let dump = directory.path().join("cli.dump");
    let (ok, text) = run(&interface, &seed, &["dump", &dump.to_string_lossy()]);
    assert!(ok, "{text}");

    let restored = directory.path().join("rust.lsdb");
    let report = Db::restore(&restored, &dump).unwrap();
    assert_eq!(report.tables, 1);
    assert_eq!(report.rows, 2);
    let reopened = Db::open(&restored).unwrap();
    let mut rows = reopened.scan::<Widget>().unwrap();
    rows.sort_by_key(|row| row.id);
    assert_eq!(rows[0].name, "alpha");
    assert_eq!(rows[1].score, 20);
}

#[test]
fn legacy_restore_rejects_a_nullable_mismatch() {
    let directory = tempfile::tempdir().unwrap();
    let interface = directory.path().join("compat-interface");
    interface_binary(COMPAT_WORKLOAD, &interface);

    let mut db = nullable_store::Db::open_in_memory().unwrap();
    db.insert(nullable_store::Widget {
        id: 1,
        name: None,
        score: 5,
    })
    .unwrap();
    let dump = directory.path().join("notes.dump");
    db.dump(&dump).unwrap();

    let database = directory.path().join("cli.lsdb");
    let (ok, text) = run(&interface, &database, &["restore", &dump.to_string_lossy()]);
    assert!(!ok, "{text}");
    assert!(text.contains("mismatch"), "{text}");
}
