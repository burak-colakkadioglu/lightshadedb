#![cfg(feature = "embedded")]

use self::ttl_store::{Db, Session};

#[lightshadedb::schema]
pub mod ttl_store {

    #[lightshadedb(ttl_secs = 3600)]
    pub struct Session {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub expires_at: i64,
        pub owner: String,
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn inserts_stamp_missing_expiry() {
    let mut db = Db::open_in_memory().unwrap();
    let before = now_millis();
    db.insert(Session {
        id: 1,
        expires_at: 0,
        owner: "ada".into(),
    })
    .unwrap();
    let row = db.get::<Session>(1).unwrap().unwrap();
    assert!(row.expires_at >= before + 3600 * 1000);
    assert!(row.expires_at <= now_millis() + 3600 * 1000);
}

#[test]
fn expired_rows_are_invisible_until_purged() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(Session {
        id: 1,
        expires_at: 1,
        owner: "ada".into(),
    })
    .unwrap();
    db.insert(Session {
        id: 2,
        expires_at: 0,
        owner: "ada".into(),
    })
    .unwrap();

    assert!(db.get::<Session>(1).unwrap().is_none());
    assert_eq!(db.count::<Session>().unwrap(), 1);

    let removed = db.purge_expired().unwrap();
    assert_eq!(removed, 1);
    assert_eq!(db.count::<Session>().unwrap(), 1);
    assert!(db.get::<Session>(2).unwrap().is_some());
}

#[test]
fn expiry_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(Session {
            id: 1,
            expires_at: 1,
            owner: "ada".into(),
        })
        .unwrap();
        db.insert(Session {
            id: 2,
            expires_at: 0,
            owner: "ada".into(),
        })
        .unwrap();
        db.close().unwrap();
    }
    {
        let mut db = Db::open(&path).unwrap();
        assert!(db.get::<Session>(1).unwrap().is_none());
        assert_eq!(db.count::<Session>().unwrap(), 1);
        db.insert(Session {
            id: 3,
            expires_at: 1,
            owner: "ada".into(),
        })
        .unwrap();
        assert_eq!(db.purge_expired().unwrap(), 1);
        assert_eq!(db.count::<Session>().unwrap(), 1);
        db.close().unwrap();
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.count::<Session>().unwrap(), 1);
        db.close().unwrap();
    }
}
