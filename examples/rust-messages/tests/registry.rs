#![cfg(all(feature = "host", feature = "client"))]

use rust_messages::store::{self, Inbox, Message};

lightshadedb::registry!("lightshade.toml");

fn set_var(name: &str, value: &std::path::Path) {
    unsafe {
        std::env::set_var(name, value);
    }
}

fn remove_vars() {
    for var in [
        "LIGHTSHADE_A1_PATH",
        "LIGHTSHADE_A2_PATH",
        "LIGHTSHADE_B1_PATH",
        "LIGHTSHADE_B1_CREDENTIALS",
        "LIGHTSHADE_B1_ALLOW",
        "LIGHTSHADE_C1_SECRET",
    ] {
        unsafe {
            std::env::remove_var(var);
        }
    }
}

#[test]
fn registry_opens_hosts_and_connects_every_database() {
    let directory = tempfile::tempdir().unwrap();
    let a1 = directory.path().join("a1.lsdb");
    let a2 = directory.path().join("a2.lsdb");
    let b1 = directory.path().join("b1.lsdb");
    let creds = directory.path().join("b1.creds");
    std::fs::write(&creds, "tester writer correct-horse-battery-staple\n").unwrap();
    set_var("LIGHTSHADE_A1_PATH", &a1);
    set_var("LIGHTSHADE_A2_PATH", &a2);
    set_var("LIGHTSHADE_B1_PATH", &b1);
    set_var("LIGHTSHADE_B1_CREDENTIALS", &creds);
    set_var(
        "LIGHTSHADE_C1_SECRET",
        std::path::Path::new("correct-horse-battery-staple"),
    );

    assert_eq!(IDS, &["a1", "a2", "b1", "c1"]);

    {
        let mut db = store::Db::open(&b1).unwrap();
        db.insert(Inbox {
            id: 7,
            owner: "ada".into(),
        })
        .unwrap();
        db.insert(Message {
            id: 1,
            inbox_id: 7,
            sent_at: 100,
            is_read: false,
            body: "hello".into(),
        })
        .unwrap();
        db.close().unwrap();
    }

    let mut dbs = Databases::from_env_all().unwrap();

    dbs.a1
        .insert(Inbox {
            id: 1,
            owner: "ada".into(),
        })
        .unwrap();
    dbs.a2
        .insert(Inbox {
            id: 2,
            owner: "grace".into(),
        })
        .unwrap();
    dbs.a1
        .insert(Message {
            id: 10,
            inbox_id: 1,
            sent_at: 100,
            is_read: false,
            body: "a1-only".into(),
        })
        .unwrap();
    dbs.a2
        .insert(Message {
            id: 20,
            inbox_id: 2,
            sent_at: 200,
            is_read: false,
            body: "a2-only".into(),
        })
        .unwrap();
    assert_eq!(dbs.a1.unread_count(1).unwrap(), 1);
    assert_eq!(dbs.a2.unread_count(2).unwrap(), 1);
    assert_eq!(dbs.a1.count::<Message>().unwrap(), 1);
    assert_eq!(dbs.a2.count::<Message>().unwrap(), 1);

    assert_eq!(dbs.c1.unread_count(7).unwrap(), 1);
    assert_eq!(dbs.c1.mark_read(1).unwrap(), 1);
    assert_eq!(dbs.c1.unread_count(7).unwrap(), 0);
    dbs.c1
        .insert(Message {
            id: 2,
            inbox_id: 7,
            sent_at: 200,
            is_read: false,
            body: "via-client".into(),
        })
        .unwrap();
    assert_eq!(dbs.c1.count::<Message>().unwrap(), 2);

    dbs.close_all().unwrap();
    remove_vars();
}
