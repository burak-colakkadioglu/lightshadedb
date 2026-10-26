use rust_messages::store;

fn run(app: &mut impl store::Backend) -> lightshadedb::Result<()> {
    app.insert(store::Inbox {
        id: 7,
        owner: "ada".into(),
    })?;
    app.insert(store::Message {
        id: 1,
        inbox_id: 7,
        sent_at: 100,
        is_read: false,
        body: "hello".into(),
    })?;
    app.insert(store::Message {
        id: 2,
        inbox_id: 7,
        sent_at: 200,
        is_read: false,
        body: "again".into(),
    })?;
    assert_eq!(app.unread_count(7)?, 2);
    assert_eq!(app.mark_read(1)?, 1);
    assert_eq!(app.unread_count(7)?, 1);
    let rows = app.recent_unread(7, 10)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].body, "again");
    let patch = store::MessagePatch {
        is_read: Some(true),
        ..Default::default()
    };
    assert!(app.patch::<store::Message>(2, patch)?);
    assert_eq!(app.scan::<store::Message>()?.len(), 2);
    assert_eq!(app.purge_before(150)?, 1);
    assert_eq!(app.count::<store::Message>()?, 1);
    Ok(())
}

#[cfg(feature = "embedded")]
fn main() -> lightshadedb::Result<()> {
    let mut app = store::App::open(store::AppConfig::embedded(":memory:"))?;
    run(&mut app)
}

#[cfg(all(feature = "client", not(feature = "embedded")))]
fn main() -> lightshadedb::Result<()> {
    let mut app = store::App::from_env()?;
    run(&mut app)
}
