#![cfg(feature = "embedded")]

use self::nullable_store::{Db, Note, NotePatch};

#[lightshade::schema]
pub mod nullable_store {
    use lightshade::prelude::*;

    pub struct Note {
        #[lightshade(primary_key)]
        pub id: i64,
        pub title: String,
        pub score: Option<i64>,
        pub tag: Option<String>,
        pub blob: Option<Vec<u8>>,
        pub pinned: Option<bool>,
    }

    #[lightshade(skip)]
    pub struct TagCount {
        pub tag: Option<String>,
        pub total: i64,
    }

    #[lightshade(skip)]
    pub struct ScoreStats {
        pub total: i64,
        pub sum: i64,
        pub lowest: i64,
    }

    #[lightshade::operations]
    pub trait NoteOps {
        #[lightshade::query(
            filter = Note::score.eq(Some(score)),
            order = (Note::id.asc()),
        )]
        fn by_score(&self, score: i64) -> Result<Vec<Note>>;

        #[lightshade::query(
            filter = Note::score.is_null(),
            order = (Note::id.asc()),
        )]
        fn null_scores(&self) -> Result<Vec<Note>>;

        #[lightshade::query(
            filter = Note::tag.is_not_null(),
            order = (Note::id.asc()),
        )]
        fn tagged(&self) -> Result<Vec<Note>>;

        #[lightshade::query(
            order = (Note::score.asc(), Note::id.asc()),
        )]
        fn ordered(&self) -> Result<Vec<Note>>;

        #[lightshade::query(
            from = Note,
            select = TagCount::select()
                .tag(Note::tag.select())
                .total(count()),
            group_by = (Note::tag.group(),),
            order = (TagCount::total.desc(),),
        )]
        fn count_by_tag(&self) -> Result<Vec<TagCount>>;

        #[lightshade::query(
            from = Note,
            select = ScoreStats::select()
                .total(count())
                .sum(sum(Note::score))
                .lowest(min(Note::score)),
        )]
        fn score_stats(&self) -> Result<Vec<ScoreStats>>;

        #[lightshade::query(
            from = Note,
            select = ScoreStats::select()
                .total(count())
                .sum(sum(Note::score))
                .lowest(min(Note::score)),
            filter = Note::score.is_not_null(),
        )]
        fn present_score_stats(&self) -> Result<Vec<ScoreStats>>;

        #[lightshade::query(count, filter = Note::score.is_null())]
        fn null_count(&self) -> Result<i64>;

        #[lightshade::update(
            filter = Note::id.eq(id),
            set = (Note::score.set(Some(score)),),
        )]
        fn set_score(&mut self, id: i64, score: i64) -> Result<u64>;

        #[lightshade::update(
            filter = Note::id.eq(id),
            set = (Note::score.set(None),),
        )]
        fn clear_score(&mut self, id: i64) -> Result<u64>;

        #[lightshade::update(
            filter = Note::score.lt(Some(cutoff)),
            set = (Note::pinned.set(true),),
        )]
        fn pin_low_scores(&mut self, cutoff: i64) -> Result<u64>;

        #[lightshade::delete(filter = Note::tag.is_null())]
        fn delete_untagged(&mut self) -> Result<u64>;
    }
}

fn note(id: i64, score: Option<i64>, tag: Option<&str>) -> Note {
    Note {
        id,
        title: format!("note-{id}"),
        score,
        tag: tag.map(str::to_string),
        blob: None,
        pinned: None,
    }
}

#[test]
fn nulls_round_trip_through_memory() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();

    let first = db.get::<Note>(1).unwrap().unwrap();
    assert_eq!(first.score, Some(7));
    assert_eq!(first.tag.as_deref(), Some("a"));
    assert_eq!(first.blob, None);

    let second = db.get::<Note>(2).unwrap().unwrap();
    assert_eq!(second.score, None);
    assert_eq!(second.tag, None);
    assert_eq!(db.count::<Note>().unwrap(), 2);
}

#[test]
fn nulls_survive_snapshot_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(note(1, Some(7), Some("a"))).unwrap();
        db.insert(note(2, None, None)).unwrap();
        db.insert(Note {
            id: 3,
            title: "blob".into(),
            score: None,
            tag: None,
            blob: Some(vec![1, 2, 3]),
            pinned: Some(true),
        })
        .unwrap();
        db.close().unwrap();
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get::<Note>(2).unwrap().unwrap().score, None);
        let third = db.get::<Note>(3).unwrap().unwrap();
        assert_eq!(third.blob, Some(vec![1, 2, 3]));
        assert_eq!(third.pinned, Some(true));
        assert_eq!(db.count::<Note>().unwrap(), 3);
        db.close().unwrap();
    }
}

#[test]
fn filters_use_three_valued_logic() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();
    db.insert(note(3, Some(7), Some("b"))).unwrap();
    db.insert(note(4, Some(3), None)).unwrap();

    let rows = db.by_score(7).unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 3]
    );

    assert_eq!(
        db.null_scores()
            .unwrap()
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(
        db.tagged()
            .unwrap()
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(db.null_count().unwrap(), 1);
}

#[test]
fn order_puts_nulls_first() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();
    db.insert(note(3, Some(3), Some("b"))).unwrap();

    let rows = db.ordered().unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![2, 3, 1]
    );
}

#[test]
fn aggregates_skip_nulls() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();
    db.insert(note(3, Some(3), Some("a"))).unwrap();
    db.insert(note(4, None, Some("b"))).unwrap();

    let stats = db.score_stats().unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].total, 4);
    assert_eq!(stats[0].sum, 10);
    assert_eq!(stats[0].lowest, 3);

    let present = db.present_score_stats().unwrap();
    assert_eq!(present.len(), 1);
    assert_eq!(present[0].total, 2);
    assert_eq!(present[0].sum, 10);
    assert_eq!(present[0].lowest, 3);

    let mut groups = db.count_by_tag().unwrap();
    groups.sort_by_key(|group| group.total);
    assert_eq!(groups.len(), 3);
    assert!(groups
        .iter()
        .any(|group| group.tag.as_deref() == Some("a") && group.total == 2));
    assert!(groups
        .iter()
        .any(|group| group.tag.as_deref() == Some("b") && group.total == 1));
    assert!(groups
        .iter()
        .any(|group| group.tag.is_none() && group.total == 1));
}

#[test]
fn updates_and_deletes_filter_on_nulls() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();
    db.insert(note(3, Some(3), Some("b"))).unwrap();

    assert_eq!(db.set_score(2, 9).unwrap(), 1);
    assert_eq!(db.get::<Note>(2).unwrap().unwrap().score, Some(9));
    assert_eq!(db.clear_score(2).unwrap(), 1);
    assert_eq!(db.get::<Note>(2).unwrap().unwrap().score, None);

    assert_eq!(db.pin_low_scores(5).unwrap(), 1);
    assert!(!db.get::<Note>(1).unwrap().unwrap().pinned.unwrap_or(false));
    assert!(db.get::<Note>(3).unwrap().unwrap().pinned.unwrap_or(false));

    assert_eq!(db.delete_untagged().unwrap(), 1);
    assert_eq!(db.count::<Note>().unwrap(), 2);
}

#[cfg(all(feature = "embedded", feature = "host", feature = "client"))]
#[test]
fn nulls_round_trip_over_the_wire() {
    use lightshade::secret::{Credential, Role};
    use lightshade::{ClientOptions, Credentials, CredentialsSource, ServeOptions};

    fn credentials() -> Credentials {
        let mut credentials = Credentials::new();
        credentials.insert(Credential {
            id: "tester".into(),
            secret: lightshade::Secret::new("correct-horse-battery-staple"),
            role: Role::Writer,
        });
        credentials
    }

    fn options() -> ClientOptions {
        ClientOptions::new(lightshade::ClientCredentials::new(
            "tester",
            "correct-horse-battery-staple",
        ))
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    let mut db = Db::open(&path).unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();
    db.insert(note(2, None, None)).unwrap();
    db.close().unwrap();

    let db = Db::open(&path).unwrap();
    let mut serve = ServeOptions::new(CredentialsSource::Static(credentials()));
    serve.listen = lightshade::Listen::tcp_local(0);
    let server = nullable_store::Server::start(db, serve).unwrap();
    let addr = server.local_addr().expect("bound TCP address");

    let mut client = nullable_store::Client::connect_tcp(addr, options()).unwrap();
    let rows = client.ordered().unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(rows[0].score, None);
    assert_eq!(rows[1].tag.as_deref(), Some("a"));
    client.close().unwrap();
    server.join().unwrap();
}

#[test]
fn nulls_survive_wal_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert(note(1, Some(7), Some("a"))).unwrap();
        db.insert(note(2, None, None)).unwrap();
    }
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get::<Note>(1).unwrap().unwrap().score, Some(7));
        assert_eq!(db.get::<Note>(2).unwrap().unwrap().score, None);
        assert_eq!(db.count::<Note>().unwrap(), 2);
        db.close().unwrap();
    }
}

#[test]
fn patch_sets_and_clears_null() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(note(1, Some(7), Some("a"))).unwrap();

    assert!(db
        .patch::<Note>(
            1,
            NotePatch {
                score: Some(None),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(db.get::<Note>(1).unwrap().unwrap().score, None);

    assert!(db
        .patch::<Note>(
            1,
            NotePatch {
                score: Some(Some(42)),
                tag: Some(Some("b".into())),
                ..Default::default()
            }
        )
        .unwrap());
    let row = db.get::<Note>(1).unwrap().unwrap();
    assert_eq!(row.score, Some(42));
    assert_eq!(row.tag.as_deref(), Some("b"));

    assert!(db.patch::<Note>(1, NotePatch::default()).unwrap());
    assert_eq!(db.get::<Note>(1).unwrap().unwrap().score, Some(42));
}

#[test]
fn nullable_field_migrates_without_a_default() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.lsdb");

    #[lightshade::schema]
    pub mod before {
        pub struct Note {
            #[lightshade(primary_key)]
            pub id: i64,
            pub title: String,
        }
    }

    {
        let mut db = before::Db::open(&path).unwrap();
        db.insert(before::Note {
            id: 1,
            title: "old".into(),
        })
        .unwrap();
        db.close().unwrap();
    }

    let db = Db::open_migrated(&path).unwrap();
    let row = db.get::<Note>(1).unwrap().unwrap();
    assert_eq!(row.title, "old");
    assert_eq!(row.score, None);
    assert_eq!(row.tag, None);
    db.close().unwrap();

    let db = Db::open(&path).unwrap();
    assert_eq!(db.count::<Note>().unwrap(), 1);
    db.close().unwrap();
}
