#![cfg(all(feature = "host", feature = "client", feature = "async"))]

use lightshadedb::pool::PoolOptions;
use lightshadedb::prelude::*;
use lightshadedb::secret::{Credential, Role};
use lightshadedb::table::Value;
use lightshadedb::{ClientEndpoint, Credentials, CredentialsSource, ServeOptions};
use rust_messages::store::{self, Db, Inbox, Message};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn credentials() -> Credentials {
    let mut credentials = Credentials::new();
    credentials.insert(Credential {
        id: "tester".into(),
        secret: Secret::new("correct horse battery staple"),
        role: Role::Writer,
    });
    credentials
}

fn pool_options() -> PoolOptions {
    PoolOptions::new(lightshadedb::ClientCredentials::new(
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

fn start_tcp(db: Db) -> store::Server {
    let mut options = ServeOptions::new(CredentialsSource::Static(credentials()));
    options.listen = lightshadedb::net::Listen::tcp_local(0);
    store::Server::start(db, options).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn pool_shares_one_client_across_tasks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let server = start_tcp(db);
    let addr = server.local_addr().expect("bound TCP address");

    let client = store::AsyncClient::connect_tcp(addr, pool_options())
        .await
        .unwrap();
    let completed = Arc::new(AtomicU64::new(0));
    let mut tasks = Vec::new();
    for task in 0..16u64 {
        let client = client.clone();
        let completed = completed.clone();
        tasks.push(tokio::spawn(async move {
            for i in 0..20u64 {
                let id = (task * 1000 + i + 100) as i64;
                assert!(client.unread_count(7).await.unwrap() >= 2);
                client
                    .insert(Message {
                        id,
                        inbox_id: 7,
                        sent_at: id,
                        is_read: false,
                        body: "pooled".into(),
                    })
                    .await
                    .unwrap();
                let row = client.get::<Message>(id).await.unwrap();
                assert_eq!(row.map(|row| row.body).as_deref(), Some("pooled"));
                assert!(client.delete::<Message>(id).await.unwrap());
                completed.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(completed.load(Ordering::Relaxed), 16 * 20);
    assert_eq!(client.count::<Message>().await.unwrap(), 3);
    server.join().unwrap();
}

fn scripted_peer(
    script: Vec<lightshadedb::Error>,
    finally: Option<Vec<lightshadedb::table::Value>>,
) -> (
    std::net::SocketAddr,
    Arc<AtomicU64>,
    std::thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let attempts = Arc::new(AtomicU64::new(0));
    let counting = attempts.clone();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let len = u32::from_le_bytes(header) as usize;
        let mut hello = vec![0u8; len];
        stream.read_exact(&mut hello).unwrap();
        stream.write_all(&1u32.to_le_bytes()).unwrap();
        stream.write_all(&[0u8]).unwrap();
        stream.flush().unwrap();
        for error in script {
            let frame = {
                let mut header = [0u8; 4];
                stream.read_exact(&mut header).unwrap();
                let len = u32::from_le_bytes(header) as usize;
                let mut frame = vec![0u8; len];
                stream.read_exact(&mut frame).unwrap();
                frame
            };
            let kind = frame[0];
            assert_eq!(kind, 0, "scripted peer only answers operations");
            counting.fetch_add(1, Ordering::Relaxed);
            let mut payload = vec![1u8];
            lightshadedb::wire::put_error(&mut payload, &error);
            stream
                .write_all(&(payload.len() as u32).to_le_bytes())
                .unwrap();
            stream.write_all(&payload).unwrap();
            stream.flush().unwrap();
        }
        let Some(ok) = finally else {
            return;
        };
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let len = u32::from_le_bytes(header) as usize;
        let mut frame = vec![0u8; len];
        stream.read_exact(&mut frame).unwrap();
        counting.fetch_add(1, Ordering::Relaxed);
        let mut payload = vec![0u8];
        let mut values = Vec::new();
        lightshadedb::wire::put_u32(&mut values, ok.len() as u32);
        for value in &ok {
            lightshadedb::wire::put_value(value, &mut values);
        }
        payload.extend_from_slice(&values);
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(&payload).unwrap();
        stream.flush().unwrap();
    });
    (addr, attempts, handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_retries_retryable_errors_once_each() {
    use lightshadedb::table::Value;
    let (addr, attempts, peer) =
        scripted_peer(vec![lightshadedb::Error::Busy], Some(vec![Value::I64(42)]));
    let endpoint = ClientEndpoint::tcp_db(addr.ip().to_string(), addr.port(), "default");
    let pool = lightshadedb::pool::Pool::new(
        endpoint,
        0,
        PoolOptions::new(lightshadedb::ClientCredentials::new("tester", "secret")),
    )
    .await
    .unwrap();
    let values = pool.request(0, &[]).await.unwrap();
    assert_eq!(values, vec![Value::I64(42)]);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
    peer.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_surfaces_non_retryable_errors_without_repeating() {
    let (addr, attempts, peer) = scripted_peer(
        vec![lightshadedb::Error::DuplicateKey {
            table: "widget".to_string(),
        }],
        None,
    );
    let endpoint = ClientEndpoint::tcp_db(addr.ip().to_string(), addr.port(), "default");
    let pool = lightshadedb::pool::Pool::new(
        endpoint,
        0,
        PoolOptions::new(lightshadedb::ClientCredentials::new("tester", "secret")),
    )
    .await
    .unwrap();
    let error = pool.request(0, &[]).await.unwrap_err();
    assert!(
        matches!(error, lightshadedb::Error::DuplicateKey { .. }),
        "{error:?}"
    );
    assert!(!error.is_retryable());
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
    peer.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transactions_commit_and_roll_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let server = start_tcp(db);
    let addr = server.local_addr().expect("bound TCP address");
    let client = store::AsyncClient::connect_tcp(addr, pool_options())
        .await
        .unwrap();

    let txn = client.begin().await.unwrap();
    txn.insert(Message {
        id: 9,
        inbox_id: 7,
        sent_at: 900,
        is_read: false,
        body: "txn".into(),
    })
    .await
    .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(client.count::<Message>().await.unwrap(), 4);

    let txn = client.begin().await.unwrap();
    txn.delete::<Message>(9).await.unwrap();
    txn.rollback().await.unwrap();
    assert_eq!(client.count::<Message>().await.unwrap(), 4);

    let txn = client.begin().await.unwrap();
    txn.delete::<Message>(9).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(client.count::<Message>().await.unwrap(), 3);
    let txn = client.begin().await.unwrap();
    txn.delete::<Message>(1).await.unwrap();
    txn.rollback().await.unwrap();
    assert!(client.contains::<Message>(&1).await.unwrap());
    server.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_credentials_fail_eagerly_at_connect() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("messages.lsdb");
    let mut db = Db::open(&path).unwrap();
    seed(&mut db);
    let server = start_tcp(db);
    let addr = server.local_addr().expect("bound TCP address");

    let mut options = pool_options();
    options.credentials = lightshadedb::ClientCredentials::new("tester", "wrong secret");
    let error = store::AsyncClient::connect_tcp(addr, options)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        lightshadedb::Error::Auth(_) | lightshadedb::Error::Remote(_)
    ));
    server.join().unwrap();
}
