#![cfg(feature = "embedded")]

use lightshade::{Error, ErrorKind};
use rust_messages::store::{self, Db, Inbox, Message, MessagePatch};

fn inbox(id: i64) -> Inbox {
    Inbox {
        id,
        owner: format!("owner-{id}"),
    }
}

fn message(id: i64, inbox_id: i64, sent_at: i64, is_read: bool) -> Message {
    Message {
        id,
        inbox_id,
        sent_at,
        is_read,
        body: format!("body-{id}"),
    }
}

#[test]
fn crud_round_trips() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(inbox(1)).unwrap();
    db.insert(message(1, 1, 100, false)).unwrap();
    db.insert(message(2, 1, 200, true)).unwrap();

    assert!(db.contains::<Inbox>(&1).unwrap());
    assert_eq!(db.get::<Inbox>(1).unwrap().unwrap().owner, "owner-1");
    assert_eq!(db.count::<Message>().unwrap(), 2);

    let mut rows = db.scan::<Message>().unwrap();
    rows.sort_by_key(|row| row.id);
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 2]
    );

    let replaced = Message {
        body: "replaced".into(),
        ..message(1, 1, 100, false)
    };
    assert!(db.update(replaced).unwrap());
    assert_eq!(db.get::<Message>(1).unwrap().unwrap().body, "replaced");

    assert!(db
        .patch::<Message>(
            1,
            MessagePatch {
                is_read: Some(true),
                ..Default::default()
            }
        )
        .unwrap());
    assert!(db.get::<Message>(1).unwrap().unwrap().is_read);

    assert!(db.delete::<Message>(2).unwrap());
    assert!(!db.delete::<Message>(2).unwrap());
    assert_eq!(db.count::<Message>().unwrap(), 1);
}

#[test]
fn duplicate_insert_and_batch_operations_are_atomic() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(inbox(1)).unwrap();
    let error = db.insert(inbox(1)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DuplicateKey);
    assert!(matches!(error, Error::DuplicateKey { table } if table == "inbox"));

    db.insert_many([inbox(2), inbox(3)]).unwrap();
    assert_eq!(db.count::<Inbox>().unwrap(), 3);
    assert!(!db.upsert(inbox(2)).unwrap());
    assert!(db.upsert(inbox(4)).unwrap());
    assert_eq!(db.upsert_many([inbox(5), inbox(6)]).unwrap(), 2);
    assert_eq!(db.count::<Inbox>().unwrap(), 6);
}

#[test]
fn transactions_commit_and_roll_back() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(inbox(1)).unwrap();
    db.begin().unwrap();
    db.insert(inbox(2)).unwrap();
    assert_eq!(db.count::<Inbox>().unwrap(), 2);
    db.rollback().unwrap();
    assert_eq!(db.count::<Inbox>().unwrap(), 1);

    db.transaction(|db| {
        db.insert(inbox(3))?;
        db.insert(inbox(4))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(db.count::<Inbox>().unwrap(), 3);

    let outcome: lightshade::Result<()> = db.transaction(|db| {
        db.insert(inbox(5))?;
        Err(Error::Busy)
    });
    assert!(outcome.is_err());
    assert_eq!(db.count::<Inbox>().unwrap(), 3);
}

#[test]
fn typed_operations_count_update_delete_and_order() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(inbox(7)).unwrap();
    db.insert(message(1, 7, 100, false)).unwrap();
    db.insert(message(2, 7, 200, false)).unwrap();
    assert_eq!(db.unread_count(7).unwrap(), 2);
    assert_eq!(db.mark_read(1).unwrap(), 1);
    assert_eq!(db.unread_count(7).unwrap(), 1);
    let rows = db.recent_unread(7, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 2);
    let summary = db.summary(7).unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total, 2);
    assert_eq!(summary[0].newest, 200);
    let digest = db.unread_digest(7).unwrap();
    assert_eq!(digest.len(), 1);
    assert_eq!(digest[0].owner, "owner-7");
    assert_eq!(digest[0].total, 1);
    assert_eq!(digest[0].newest, 200);
    assert_eq!(db.purge_before(150).unwrap(), 1);
    assert_eq!(db.count::<Message>().unwrap(), 1);
}

#[test]
fn databases_survive_close_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(inbox(1)).unwrap();
        db.insert(message(1, 1, 100, false)).unwrap();
        db.flush().unwrap();
        db.close().unwrap();
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.count::<Message>().unwrap(), 1);
        assert_eq!(db.get::<Message>(1).unwrap().unwrap().body, "body-1");
    }
}

#[test]
fn app_generic_backend_runs_the_same_logic() {
    fn run(app: &mut impl store::Backend) -> lightshade::Result<()> {
        app.insert(inbox(1))?;
        app.insert(message(1, 1, 100, false))?;
        assert_eq!(app.unread_count(1)?, 1);
        app.mark_read(1)?;
        assert_eq!(app.unread_count(1)?, 0);
        Ok(())
    }

    let mut db = Db::open_in_memory().unwrap();
    run(&mut db).unwrap();
    assert_eq!(db.count::<Message>().unwrap(), 1);
}

#[test]
fn read_only_and_missing_paths_are_typed() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.lsdb");
    let error = match Db::open_read_only(&missing) {
        Ok(_) => panic!("a missing database opened read-only"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Io { .. } | Error::NotFound { .. }));

    let path = directory.path().join("database.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(inbox(1)).unwrap();
        db.close().unwrap();
    }
    let mut db = Db::open_read_only(&path).unwrap();
    assert!(db.is_read_only());
    assert!(matches!(db.insert(inbox(2)).unwrap_err(), Error::ReadOnly));
}

#[test]
fn admin_reports_and_backups_are_typed() {
    use lightshade::admin::{BackupOptions, VerifyOptions};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(inbox(1)).unwrap();
        db.insert(message(1, 1, 100, false)).unwrap();
        db.close().unwrap();
    }

    let verify = Db::verify(&path, VerifyOptions::default()).unwrap();
    assert!(verify.ok);
    assert_eq!(verify.rows, 2);
    assert_eq!(verify.tables.len(), 2);
    assert_eq!(verify.tables[0].name, "inbox");
    assert_eq!(verify.tables[0].rows, 1);
    assert!(verify.duration.as_nanos() > 0);

    let inspect = Db::inspect(&path).unwrap();
    assert_eq!(inspect.lsn, verify.lsn);
    assert_eq!(inspect.rows, 2);
    assert_eq!(inspect.tables.len(), 2);
    assert_eq!(inspect.tables[0].fields[0].name, "id");
    assert!(inspect.tables[0].fields[0].primary_key);
    assert_eq!(inspect.tables[0].fields[1].name, "owner");
    assert!(!inspect.tables[0].fields[1].primary_key);

    let backup = directory.path().join("backup.lsdb");
    let report = Db::backup(&path, &backup, BackupOptions::default()).unwrap();
    assert_eq!(report.rows, 2);
    assert_eq!(report.destination, backup);
    assert!(report.bytes > 0);
    assert!(backup.exists());
    assert!(matches!(
        Db::backup(&path, &backup, BackupOptions::default()).unwrap_err(),
        Error::Io { .. }
    ));
    let verify_backup = Db::verify(&backup, VerifyOptions::default()).unwrap();
    assert_eq!(verify_backup.rows, 2);

    let compact = Db::compact(&path).unwrap();
    assert_eq!(compact.rows, 2);
    assert!(compact.snapshot_after > 0);
    assert_eq!(compact.wal_after, 0);

    let missing = directory.path().join("missing.lsdb");
    assert!(matches!(
        Db::inspect(&missing).unwrap_err(),
        Error::NotFound { .. }
    ));
}

#[test]
fn offset_skips_before_limit_applies() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(inbox(1)).unwrap();
    for (id, sent_at) in [(1, 100), (2, 200), (3, 300), (4, 400)] {
        db.insert(message(id, 1, sent_at, false)).unwrap();
    }

    let ids = |rows: Vec<Message>| rows.into_iter().map(|row| row.id).collect::<Vec<_>>();
    assert_eq!(ids(db.inbox_page(1, 10, 0).unwrap()), vec![1, 2, 3, 4]);
    assert_eq!(ids(db.inbox_page(1, 2, 0).unwrap()), vec![1, 2]);
    assert_eq!(ids(db.inbox_page(1, 2, 1).unwrap()), vec![2, 3]);
    assert_eq!(ids(db.inbox_page(1, 10, 3).unwrap()), vec![4]);
    assert!(db.inbox_page(1, 10, 9).unwrap().is_empty());
    assert!(db.inbox_page(1, 0, 0).unwrap().is_empty());
    assert!(db.inbox_page(2, 10, 0).unwrap().is_empty());
}
