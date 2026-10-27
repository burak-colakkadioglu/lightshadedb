#![cfg(all(feature = "host", feature = "client"))]

use lightshadedb::prelude::*;
use lightshadedb::secret::{Credential, Role};
use lightshadedb::{ClientOptions, Credentials, CredentialsSource, Listen, ServeOptions};
use rust_messages::store::{self, Db, Inbox, Message};

fn credentials() -> Credentials {
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "tester".into(),
        secret: Secret::new("correct horse battery staple"),
        role: Role::Writer,
    });
    credentials
}

fn options() -> ClientOptions {
    ClientOptions::new(lightshadedb::ClientCredentials::new(
        "tester",
        "correct horse battery staple",
    ))
}

#[test]
fn follower_tails_and_promotes() {
    let directory = tempfile::tempdir().unwrap();
    let primary_path = directory.path().join("primary.lsdb");
    let mut db = Db::open(&primary_path).unwrap();
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
    db.insert(Message {
        id: 2,
        inbox_id: 7,
        sent_at: 200,
        is_read: false,
        body: "again".into(),
    })
    .unwrap();
    let primary_lsn = db.lsn();

    let mut serve_options = ServeOptions::new(CredentialsSource::Static(credentials()));
    serve_options.listen = Listen::tcp_local(0);
    let server = store::Server::start(db, serve_options).unwrap();
    let endpoint = server.endpoint_value().clone();

    let follower_path = directory.path().join("follower.lsdb");
    let mut follower = store::Follower::open(&follower_path, endpoint, options()).unwrap();
    let follower_lsn = follower.sync_once().unwrap();
    assert_eq!(follower_lsn, primary_lsn);
    assert_eq!(follower.handle().count::<Message>().unwrap(), 2);
    assert_eq!(
        follower.handle().get::<Message>(1).unwrap().unwrap().body,
        "hello"
    );

    let nothing_new = follower.sync_once().unwrap();
    assert_eq!(nothing_new, follower_lsn);

    let mut promoted = follower.promote().unwrap();
    promoted
        .insert(Message {
            id: 3,
            inbox_id: 7,
            sent_at: 300,
            is_read: true,
            body: "promoted".into(),
        })
        .unwrap();
    assert_eq!(promoted.count::<Message>().unwrap(), 3);
    promoted.close().unwrap();
    server.join().unwrap();
}
