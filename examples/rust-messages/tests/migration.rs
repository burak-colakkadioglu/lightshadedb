#![cfg(feature = "embedded")]

use lightshade::prelude::*;

mod old {

    #[lightshade::schema]
    pub mod store {

        #[lightshade(table = "old_note")]
        pub struct OldNote {
            #[lightshade(primary_key)]
            pub id: i64,
            pub body: String,
            pub score: i32,
            pub obsolete: bool,
        }
    }
}

mod added {

    #[lightshade::schema]
    pub mod store {

        #[lightshade(table = "note", rename_from = "old_note")]
        pub struct Note {
            #[lightshade(primary_key)]
            pub id: i64,
            #[lightshade(rename_from = body)]
            pub text: String,
            #[lightshade(default = 5)]
            pub priority: i64,
            #[lightshade(convert_from = i32, with = widen)]
            pub score: i64,
        }

        pub fn widen(value: i32) -> lightshade::Result<i64> {
            Ok(i64::from(value) * 10)
        }
    }
}

mod reordered {

    #[lightshade::schema]
    pub mod store {
        use lightshade::prelude::*;

        #[lightshade(table = "old_note")]
        pub struct Note {
            #[lightshade(primary_key)]
            pub id: i64,
            pub obsolete: bool,
            pub score: i32,
            pub body: String,
        }

        #[lightshade::operations]
        pub trait Notes {
            #[lightshade::query(filter = Note::id.ge(1))]
            fn all_notes(&self) -> Result<Vec<Note>>;
        }
    }
}

mod no_default {

    #[lightshade::schema]
    pub mod store {

        #[lightshade(table = "old_note")]
        pub struct Note {
            #[lightshade(primary_key)]
            pub id: i64,
            pub body: String,
            pub score: i32,
            pub obsolete: bool,
            pub required: String,
        }
    }
}

mod no_conversion {

    #[lightshade::schema]
    pub mod store {

        #[lightshade(table = "old_note")]
        pub struct Note {
            #[lightshade(primary_key)]
            pub id: i64,
            pub body: String,
            pub score: i64,
            pub obsolete: bool,
        }
    }
}

fn seed(path: &std::path::Path) {
    use old::store::{Db, OldNote};
    let mut db = Db::open(path).unwrap();
    db.insert(OldNote {
        id: 1,
        body: "first".into(),
        score: 2,
        obsolete: true,
    })
    .unwrap();
    db.insert(OldNote {
        id: 2,
        body: "second".into(),
        score: 3,
        obsolete: false,
    })
    .unwrap();
    db.close().unwrap();
}

#[test]
fn migration_plan_renames_adds_converts_and_removes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    seed(&path);

    let plan = added::store::migration_plan(&path).unwrap();
    assert!(plan.steps.contains(&MigrationStep::RenameTable {
        from: "old_note".into(),
        to: "note".into()
    }));
    assert!(plan.steps.contains(&MigrationStep::RenameField {
        table: "note".into(),
        from: "body".into(),
        to: "text".into()
    }));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        MigrationStep::AddField { table, field, ty: FieldType::I64, .. }
            if table == "note" && field == "priority"
    )));
    assert!(plan.steps.contains(&MigrationStep::ConvertField {
        table: "note".into(),
        field: "score".into(),
        from: FieldType::I32,
        to: FieldType::I64
    }));
    assert!(plan.steps.contains(&MigrationStep::RemoveField {
        table: "note".into(),
        field: "obsolete".into()
    }));
    assert!(plan.additions >= 1);
    assert_eq!(plan.removals, 1);
    assert!(plan.destructive);

    let blocked = added::store::migrate(&path, MigrationOptions::default()).unwrap_err();
    assert!(matches!(
        blocked,
        Error::Migration(MigrationError::DestructiveNotAllowed)
    ));

    let options = MigrationOptions {
        allow_destructive: true,
        ..MigrationOptions::default()
    };
    let report = added::store::migrate(&path, options).unwrap();
    assert_eq!(report.rows, 2);
    assert!(report.from_lsn <= report.to_lsn);
    let backup = report.backup.expect("default backup path");
    assert!(backup.exists());
    assert!(backup.to_string_lossy().contains(".pre-migrate-"));

    let db = added::store::Db::open(&path).unwrap();
    let mut rows = db.scan::<added::store::Note>().unwrap();
    rows.sort_by_key(|row| row.id);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].text, "first");
    assert_eq!(rows[0].priority, 5);
    assert_eq!(rows[0].score, 20);
    assert_eq!(rows[1].score, 30);
    drop(db);

    let old_db = old::store::Db::open(&backup).unwrap();
    assert_eq!(old_db.scan::<old::store::OldNote>().unwrap().len(), 2);
}

#[test]
fn strict_open_reports_a_typed_migration_requirement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    seed(&path);
    let error = match added::store::Db::open(&path) {
        Ok(_) => panic!("strict open accepted a schema mismatch"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        Error::Migration(MigrationError::Required { .. })
    ));
    assert!(old::store::Db::open(&path).is_ok());
}

#[test]
fn new_fields_need_defaults_and_type_changes_need_conversions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    seed(&path);
    assert!(matches!(
        no_default::store::migration_plan(&path).unwrap_err(),
        Error::Migration(MigrationError::MissingDefault { .. })
    ));
    assert!(matches!(
        no_conversion::store::migration_plan(&path).unwrap_err(),
        Error::Migration(MigrationError::ConversionRequired { .. })
    ));
    assert_eq!(
        old::store::Db::open(&path)
            .unwrap()
            .scan::<old::store::OldNote>()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn open_migrated_with_writes_a_backup_and_preserves_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    seed(&path);
    let db = added::store::Db::open_migrated_with(
        &path,
        MigrationOptions {
            allow_destructive: true,
            ..MigrationOptions::default()
        },
    )
    .unwrap();
    assert_eq!(db.count::<added::store::Note>().unwrap(), 2);
    assert!(std::fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .contains(".pre-migrate-")));
}

#[test]
fn reordered_fields_and_new_operations_open_without_a_plan() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    seed(&path);
    assert!(reordered::store::migration_plan(&path).unwrap().is_empty());
    let db = reordered::store::Db::open(&path).unwrap();
    let mut rows = reordered::store::Notes::all_notes(&db).unwrap();
    rows.sort_by_key(|row| row.id);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].body, "first");
    assert_eq!(rows[0].score, 2);
    assert!(rows[0].obsolete);
}
