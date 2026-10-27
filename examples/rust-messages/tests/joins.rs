#![cfg(feature = "embedded")]

use self::join_store::{Author, Book, Db};

#[lightshadedb::schema]
pub mod join_store {
    use lightshadedb::prelude::*;

    pub struct Author {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub lang: i64,
        pub nick: Option<String>,
    }

    pub struct Book {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub author_id: i64,
        pub lang: i64,
        pub author_nick: Option<String>,
        pub title: String,
    }

    #[lightshadedb(skip)]
    pub struct Pair {
        pub title: String,
        pub nick: Option<String>,
    }

    #[lightshadedb::operations]
    pub trait Joins {
        #[lightshadedb::query(
            from = Book,
            joins = (Author.join_on(Book::author_id.eq_field(Author::id) & Book::lang.eq_field(Author::lang)),),
            filter = Book::id.eq(id),
            select = Pair::select().title(Book::title.select()).nick(Author::nick.select()),
        )]
        fn by_author_lang(&self, id: i64) -> Result<Vec<Pair>>;

        #[lightshadedb::query(
            from = Book,
            joins = (Author.join_on(Book::author_nick.eq_field(Author::nick)),),
            filter = Book::id.eq(id),
            select = Pair::select().title(Book::title.select()).nick(Author::nick.select()),
        )]
        fn by_nick(&self, id: i64) -> Result<Vec<Pair>>;
    }
}

fn author(id: i64, lang: i64, nick: Option<&str>) -> Author {
    Author {
        id,
        lang,
        nick: nick.map(str::to_string),
    }
}

fn book(id: i64, author_id: i64, lang: i64, author_nick: Option<&str>) -> Book {
    Book {
        id,
        author_id,
        lang,
        author_nick: author_nick.map(str::to_string),
        title: format!("book-{id}"),
    }
}

#[test]
fn multi_key_join_needs_every_key() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(author(1, 7, Some("ada"))).unwrap();
    db.insert(book(10, 1, 7, Some("ada"))).unwrap();
    db.insert(book(11, 1, 9, Some("ada"))).unwrap();

    let matched = db.by_author_lang(10).unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].title, "book-10");
    assert_eq!(matched[0].nick.as_deref(), Some("ada"));

    assert!(db.by_author_lang(11).unwrap().is_empty());
}

#[test]
fn null_join_keys_never_match() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert(author(2, 7, Some("grace"))).unwrap();
    db.insert(author(3, 7, None)).unwrap();
    db.insert(book(20, 2, 7, Some("grace"))).unwrap();
    db.insert(book(21, 3, 7, None)).unwrap();

    let matched = db.by_nick(20).unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].nick.as_deref(), Some("grace"));

    assert!(db.by_nick(21).unwrap().is_empty());
}
