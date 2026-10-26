#![cfg(all(feature = "host", feature = "client"))]

use lightshadedb::prelude::*;
use lightshadedb::secret::{Credential, Role};
use lightshadedb::{ClientOptions, Credentials, CredentialsSource, ServeOptions};
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

fn seed(db: &mut Db) {
    db.insert(Inbox {
        id: 7,
        owner: "ada".into(),
    })
    .unwrap();
    for (id, sent_at, is_read, body) in [
        (1, 100, false, "hello"),
        (2, 200, false, "again"),
        (3, 300, true, "older"),
    ] {
        db.insert(Message {
            id,
            inbox_id: 7,
            sent_at,
            is_read,
            body: body.into(),
        })
        .unwrap();
    }
}

fn exercise(client: &mut store::Client) {
    assert_eq!(client.unread_count(7).unwrap(), 2);
    assert_eq!(client.mark_read(1).unwrap(), 1);
    assert_eq!(client.unread_count(7).unwrap(), 1);
    let rows = client.recent_unread(7, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 2);
    let digest = client.unread_digest(7).unwrap();
    assert_eq!(digest.len(), 1);
    assert_eq!(digest[0].owner, "ada");
    assert_eq!(digest[0].total, 1);
    assert_eq!(client.count::<Message>().unwrap(), 3);
    let message = client.get::<Message>(2).unwrap().unwrap();
    assert_eq!(message.body, "again");
    assert!(client.contains::<Message>(&2).unwrap());
    assert!(!client.contains::<Message>(&9).unwrap());
    let patched = client
        .patch::<Message>(
            3,
            store::MessagePatch {
                body: Some("patched".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(patched);
    assert_eq!(client.get::<Message>(3).unwrap().unwrap().body, "patched");
    let mut scan = client.scan::<Message>().unwrap();
    scan.sort_by_key(|message| message.id);
    assert_eq!(scan.len(), 3);
    assert!(client
        .upsert(Message {
            id: 4,
            inbox_id: 7,
            sent_at: 400,
            is_read: true,
            body: "new".into(),
        })
        .unwrap());
    assert_eq!(client.count::<Message>().unwrap(), 4);
    assert_eq!(
        client
            .insert_many([
                Inbox {
                    id: 8,
                    owner: "grace".into()
                },
                Inbox {
                    id: 9,
                    owner: "linus".into()
                },
            ])
            .unwrap(),
        ()
    );
    assert_eq!(client.count::<Inbox>().unwrap(), 3);
    assert!(client.delete::<Message>(4).unwrap());
    assert!(!client.delete::<Message>(4).unwrap());
    assert_eq!(client.purge_before(150).unwrap(), 1);
    assert_eq!(client.count::<Message>().unwrap(), 2);
}

#[test]
fn unix_socket_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let server = store::Server::start(
        db,
        ServeOptions::new(CredentialsSource::Static(credentials())),
    )
    .unwrap();
    let mut client = store::Client::connect(server.endpoint(), options()).unwrap();
    assert_eq!(client.path().unwrap(), server.endpoint());
    assert!(client.remote_addr().is_none());
    exercise(&mut client);
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn tcp_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_local(0);
    let server = store::Server::start(db, options).unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    assert_eq!(server.network_access(), NetworkAccess::Local);
    let mut client = store::Client::connect_tcp(addr, self::options()).unwrap();
    assert_eq!(client.remote_addr(), Some(addr));
    exercise(&mut client);
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn reading_role_cannot_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut credentials = credentials();
    credentials.insert(Credential {
        id: "reader".into(),
        secret: Secret::new("read only"),
        role: Role::Reader,
    });
    let server = store::Server::start(
        db,
        ServeOptions::new(CredentialsSource::Static(credentials)),
    )
    .unwrap();
    let mut client = store::Client::connect(
        server.endpoint(),
        ClientOptions::new(lightshadedb::ClientCredentials::new("reader", "read only")),
    )
    .unwrap();
    assert_eq!(client.unread_count(7).unwrap(), 2);
    let error = client.mark_read(1).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(client.count::<Message>().is_ok());
    server.join().unwrap();
}

#[test]
fn bad_credentials_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let db = Db::open(&path).unwrap();
    let server = store::Server::start(
        db,
        ServeOptions::new(CredentialsSource::Static(credentials())),
    )
    .unwrap();
    let error = match store::Client::connect(
        server.endpoint(),
        ClientOptions::new(lightshadedb::ClientCredentials::new("tester", "wrong")),
    ) {
        Ok(_) => panic!("bad credentials were accepted"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), ErrorKind::Auth);
    server.join().unwrap();
}

#[test]
fn tcp_ipv6_loopback_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::Tcp {
        addr: std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 0)),
        access: NetworkAccess::Local,
        allow: Vec::new(),
    };
    let server = store::Server::start(db, options).unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    assert!(addr.ip().is_loopback());
    assert_eq!(server.network_access(), NetworkAccess::Local);
    let mut client = store::Client::connect_tcp(addr, self::options()).unwrap();
    assert_eq!(client.remote_addr(), Some(addr));
    assert_eq!(client.unread_count(7).unwrap(), 2);
    assert_eq!(client.count::<Message>().unwrap(), 3);
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn tcp_hostname_endpoint_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_local(0);
    let server = store::Server::start(db, options).unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    let endpoint =
        lightshadedb::ClientEndpoint::parse(&format!("tcp://localhost:{}", addr.port())).unwrap();
    let mut client = store::Client::connect_endpoint(endpoint, self::options()).unwrap();
    assert_eq!(
        client.remote_addr().map(|peer| peer.port()),
        Some(addr.port())
    );
    assert_eq!(client.unread_count(7).unwrap(), 2);
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn exposure_policy_is_enforced_before_serving() {
    let invalid = Listen::Tcp {
        addr: "192.0.2.1:0".parse().unwrap(),
        access: NetworkAccess::Local,
        allow: Vec::new(),
    };
    assert!(invalid.validate().is_err());

    let public = Listen::Tcp {
        addr: "203.0.113.7:0".parse().unwrap(),
        access: NetworkAccess::LocalNetwork,
        allow: Vec::new(),
    };
    assert!(public.validate().is_err());
    assert!(public.network_access() == Some(NetworkAccess::LocalNetwork));

    assert!(NetworkAccess::LocalNetwork.allows("127.0.0.1".parse().unwrap()));
    assert!(NetworkAccess::LocalNetwork.allows("10.0.0.5".parse().unwrap()));
    assert!(NetworkAccess::LocalNetwork.allows("192.168.1.10".parse().unwrap()));
    assert!(NetworkAccess::LocalNetwork.allows("::1".parse().unwrap()));
    assert!(!NetworkAccess::LocalNetwork.allows("8.8.8.8".parse().unwrap()));
    assert!(!NetworkAccess::Local.allows("10.0.0.5".parse().unwrap()));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let db = Db::open(&path).unwrap();
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = public;
    assert!(store::Server::start(db, options).is_err());
}

#[test]
fn external_policy_serves_and_reports_access() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_external(std::net::SocketAddr::from((
        std::net::Ipv4Addr::UNSPECIFIED,
        0,
    )));
    let server = store::Server::start(db, options).unwrap();
    assert_eq!(server.network_access(), NetworkAccess::External);
    let addr = server.local_addr().expect("bound TCP address");
    let mut client = store::Client::connect_tcp(
        std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, addr.port())),
        self::options(),
    )
    .unwrap();
    assert_eq!(client.unread_count(7).unwrap(), 2);
    client.close().unwrap();
    server.join().unwrap();
}

fn credentials_for(secret: &str) -> Credentials {
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "tester".into(),
        secret: Secret::new(secret),
        role: Role::Writer,
    });
    credentials
}

fn seed_named(path: &std::path::Path, inbox: i64, owner: &str, base: i64) {
    let mut db = Db::open(path).unwrap();
    db.insert(Inbox {
        id: inbox,
        owner: owner.into(),
    })
    .unwrap();
    for (n, sent_at) in [100, 200, 300].into_iter().enumerate() {
        db.insert(Message {
            id: base + n as i64,
            inbox_id: inbox,
            sent_at,
            is_read: false,
            body: format!("message-{}-{n}", base),
        })
        .unwrap();
    }
    db.close().unwrap();
}

#[test]
fn one_listener_serves_many_databases() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.lsdb");
    let b = directory.path().join("b.lsdb");
    seed_named(&a, 1, "ada", 1);
    seed_named(&b, 2, "grace", 11);

    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_local(0);
    let server = store::Server::start_all(
        vec![
            store::ServedDb {
                name: "a1".into(),
                db: Db::open(&a).unwrap(),
            },
            store::ServedDb {
                name: "b1".into(),
                db: Db::open(&b).unwrap(),
            },
        ],
        options,
    )
    .unwrap();
    let addr = server.local_addr().expect("bound TCP address");

    let mut a1 = store::Client::connect_tcp_db(addr, "a1", self::options()).unwrap();
    let mut b1 = store::Client::connect_tcp_db(addr, "b1", self::options()).unwrap();
    assert_eq!(a1.unread_count(1).unwrap(), 3);
    assert_eq!(b1.unread_count(2).unwrap(), 3);
    assert_eq!(a1.count::<Message>().unwrap(), 3);
    assert_eq!(a1.mark_read(1).unwrap(), 1);
    assert_eq!(a1.unread_count(1).unwrap(), 2);
    assert_eq!(b1.unread_count(2).unwrap(), 3);
    assert_eq!(a1.endpoint().database(), "a1");
    assert_eq!(b1.endpoint().database(), "b1");
    a1.close().unwrap();
    b1.close().unwrap();
    server.join().unwrap();
}

#[test]
fn unknown_databases_are_rejected_without_an_oracle() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_local(0);
    let server = store::Server::start_all(
        vec![store::ServedDb {
            name: "a1".into(),
            db,
        }],
        options,
    )
    .unwrap();
    let addr = server.local_addr().expect("bound TCP address");

    let missing = match store::Client::connect_tcp_db(addr, "nope", self::options()) {
        Ok(_) => panic!("unknown database was accepted"),
        Err(error) => error,
    };
    assert_eq!(missing.kind(), ErrorKind::Auth);
    let bad = match store::Client::connect_tcp_db(
        addr,
        "a1",
        ClientOptions::new(lightshadedb::ClientCredentials::new("tester", "wrong")),
    ) {
        Ok(_) => panic!("bad credentials were accepted"),
        Err(error) => error,
    };
    assert_eq!(bad.kind(), ErrorKind::Auth);
    assert_eq!(missing.to_string(), bad.to_string());
    server.join().unwrap();
}

#[test]
fn per_database_credentials_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.lsdb");
    let b = directory.path().join("b.lsdb");
    seed_named(&a, 1, "ada", 1);
    seed_named(&b, 2, "grace", 11);

    let mut options = ServeOptions::new(CredentialsSource::Static(credentials_for("aaa")));
    options.listen = Listen::tcp_local(0);
    options.credentials_overrides.insert(
        "b1".into(),
        CredentialsSource::Static(credentials_for("bbb")),
    );
    let server = store::Server::start_all(
        vec![
            store::ServedDb {
                name: "a1".into(),
                db: Db::open(&a).unwrap(),
            },
            store::ServedDb {
                name: "b1".into(),
                db: Db::open(&b).unwrap(),
            },
        ],
        options,
    )
    .unwrap();
    let addr = server.local_addr().expect("bound TCP address");

    let aaa = || ClientOptions::new(lightshadedb::ClientCredentials::new("tester", "aaa"));
    let bbb = || ClientOptions::new(lightshadedb::ClientCredentials::new("tester", "bbb"));
    assert!(matches!(
        store::Client::connect_tcp_db(addr, "a1", aaa()),
        Ok(_)
    ));
    assert!(store::Client::connect_tcp_db(addr, "a1", bbb()).is_err());
    assert!(matches!(
        store::Client::connect_tcp_db(addr, "b1", bbb()),
        Ok(_)
    ));
    assert!(store::Client::connect_tcp_db(addr, "b1", aaa()).is_err());
    server.join().unwrap();
}

#[test]
fn start_all_validates_its_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.lsdb");
    seed_named(&a, 1, "ada", 1);
    let b = directory.path().join("b.lsdb");
    seed_named(&b, 2, "grace", 11);
    let options = || ServeOptions::new(CredentialsSource::Static(credentials()));

    assert!(store::Server::start_all(vec![], options()).is_err());
    assert!(store::Server::start_all(
        vec![
            store::ServedDb {
                name: "a1".into(),
                db: Db::open(&a).unwrap()
            },
            store::ServedDb {
                name: "a1".into(),
                db: Db::open(&b).unwrap()
            },
        ],
        options(),
    )
    .is_err());
    assert!(store::Server::start_all(
        vec![store::ServedDb {
            name: "".into(),
            db: Db::open(&a).unwrap()
        }],
        options(),
    )
    .is_err());
}

fn allow_listen(networks: &[&str]) -> Listen {
    Listen::tcp_local(0)
        .with_allowlist(networks)
        .expect("test allowlist must parse")
}

#[test]
fn allowlist_permits_listed_peers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = allow_listen(&["127.0.0.1/32", "::1/128"]);
    let server = store::Server::start(db, options).unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    let mut client = store::Client::connect_tcp_db(addr, "default", self::options()).unwrap();
    assert_eq!(client.unread_count(7).unwrap(), 2);
    assert_eq!(client.count::<Message>().unwrap(), 3);
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn allowlist_drops_unlisted_peers_before_the_handshake() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = allow_listen(&["203.0.113.0/24"]);
    let server = store::Server::start(db, options).unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    let error = match store::Client::connect_tcp_db(addr, "default", self::options()) {
        Ok(_) => panic!("a dropped peer was unexpectedly served"),
        Err(error) => error,
    };
    assert!(
        !format!("{error:?}").contains("Auth"),
        "expected a dropped connection, got {error:?}"
    );
    server.join().unwrap();
}

#[test]
fn multiplexed_databases_share_one_allowlist() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.lsdb");
    let b = directory.path().join("b.lsdb");
    seed_named(&a, 1, "ada", 1);
    seed_named(&b, 2, "grace", 11);

    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = Listen::tcp_local(0)
        .with_allowlist(&["127.0.0.0/8"])
        .unwrap();
    let server = store::Server::start_all(
        vec![
            store::ServedDb {
                name: "a1".into(),
                db: Db::open(&a).unwrap(),
            },
            store::ServedDb {
                name: "b1".into(),
                db: Db::open(&b).unwrap(),
            },
        ],
        options,
    )
    .unwrap();
    let addr = server.local_addr().expect("bound TCP address");
    let mut a1 = store::Client::connect_tcp_db(addr, "a1", self::options()).unwrap();
    let mut b1 = store::Client::connect_tcp_db(addr, "b1", self::options()).unwrap();
    assert_eq!(a1.unread_count(1).unwrap(), 3);
    assert_eq!(b1.unread_count(2).unwrap(), 3);
    a1.close().unwrap();
    b1.close().unwrap();
    server.join().unwrap();
}
