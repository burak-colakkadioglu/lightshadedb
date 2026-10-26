#![cfg(feature = "embedded")]

use self::dataplane_store::{Blob, Db, Nums, Opts};
use lightshadedb::dataplane;
use lightshadedb::table::Value;
use lightshadedb::{Error, FieldType};

#[lightshadedb::schema]
pub mod dataplane_store {
    use lightshadedb::prelude::*;

    pub struct Nums {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub tiny: i8,
        pub small: i16,
        pub normal: i32,
        pub big: u64,
        pub ratio: f64,
    }

    pub struct Blob {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub approx: f32,
        pub flag: bool,
        pub name: String,
        pub data: Vec<u8>,
        pub uuid: [u8; 16],
    }

    pub struct Opts {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub maybe_note: Option<String>,
        pub maybe_score: Option<i64>,
        pub maybe_flag: Option<bool>,
        pub maybe_blob: Option<Vec<u8>>,
        pub maybe_ratio: Option<f32>,
    }

    #[lightshadedb::operations]
    pub trait DataplaneOps {
        #[lightshadedb::query(filter = Nums::id.eq(id))]
        fn num_by_id(&self, id: i64) -> Result<Vec<Nums>>;
    }
}

fn seed() -> Db {
    let mut db = Db::open_in_memory().unwrap();
    for id in 1..=3 {
        db.insert(Nums {
            id,
            tiny: -8,
            small: 1600,
            normal: -100_000,
            big: u64::MAX,
            ratio: 2.5,
        })
        .unwrap();
        db.insert(Blob {
            id,
            approx: 0.5,
            flag: id % 2 == 0,
            name: format!("widget-{id}, \"quoted\"\nnewline"),
            data: vec![0, 1, 2, 255],
            uuid: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        })
        .unwrap();
        db.insert(Opts {
            id,
            maybe_note: if id % 2 == 0 {
                Some("note, with comma".to_string())
            } else {
                None
            },
            maybe_score: Some(id * 10),
            maybe_flag: None,
            maybe_blob: Some(vec![9, 8, 7]),
            maybe_ratio: Some(1.25),
        })
        .unwrap();
    }
    db
}

fn values_sorted(db: &Db) -> Vec<Vec<Value>> {
    use lightshadedb::Table as _;
    let mut out = Vec::new();
    let mut nums = db.scan::<Nums>().unwrap();
    nums.sort_by_key(|row| row.id);
    out.extend(nums.iter().map(|row| row.to_values()));
    let mut blobs = db.scan::<Blob>().unwrap();
    blobs.sort_by_key(|row| row.id);
    out.extend(blobs.iter().map(|row| row.to_values()));
    let mut opts = db.scan::<Opts>().unwrap();
    opts.sort_by_key(|row| row.id);
    out.extend(opts.iter().map(|row| row.to_values()));
    out
}

fn table_values<T>(db: &Db) -> Vec<Vec<Value>>
where
    T: lightshadedb::table::TableOps<Db>,
{
    let mut rows = db.scan::<T>().unwrap();
    rows.sort_by_key(|row| format!("{:?}", <T as lightshadedb::Table>::to_values(row)));
    rows.iter()
        .map(|row| <T as lightshadedb::Table>::to_values(row))
        .collect()
}

#[test]
fn dump_and_restore_round_trip() {
    let db = seed();
    let directory = tempfile::tempdir().unwrap();
    let dump = directory.path().join("tables.dump");
    let report = db.dump(&dump).unwrap();
    assert_eq!(report.tables, 3);
    assert_eq!(report.rows, 9);
    assert!(report.bytes > 8);

    let raw = std::fs::read(&dump).unwrap();
    assert!(raw.starts_with(b"LIGHTDMP"));

    let restored = directory.path().join("restored.lsdb");
    let report = Db::restore(&restored, &dump).unwrap();
    assert_eq!(report.tables, 3);
    assert_eq!(report.rows, 9);

    let reopened = Db::open(&restored).unwrap();
    assert_eq!(values_sorted(&reopened), values_sorted(&db));
}

#[test]
fn dump_refuses_overwrite_and_restore_refuses_existing() {
    let db = seed();
    let directory = tempfile::tempdir().unwrap();
    let dump = directory.path().join("tables.dump");
    db.dump(&dump).unwrap();
    let error = db.dump(&dump).unwrap_err();
    assert!(error.to_string().contains("already exists"), "{error}");

    let target = directory.path().join("exists.lsdb");
    Db::restore(&target, &dump).unwrap();
    let error = Db::restore(&target, &dump).unwrap_err();
    assert!(error.to_string().contains("already exists"), "{error}");
}

#[test]
fn restore_rejects_foreign_dumps() {
    let directory = tempfile::tempdir().unwrap();
    let bad = directory.path().join("bad.dump");
    std::fs::write(&bad, b"definitely not a dump").unwrap();
    let target = directory.path().join("out.lsdb");
    assert!(Db::restore(&target, &bad).is_err());

    let cut = directory.path().join("cut.dump");
    std::fs::write(&cut, b"LIGHTDMP\x01\x00\x00\x00").unwrap();
    assert!(Db::restore(&target, &cut).is_err());

    let mut versioned = b"LIGHTDMP".to_vec();
    versioned.extend_from_slice(&2u32.to_le_bytes());
    let wrong = directory.path().join("wrong.dump");
    std::fs::write(&wrong, versioned).unwrap();
    assert!(Db::restore(&target, &wrong).is_err());
}

#[test]
fn golden_bytes_decode_to_the_documented_layout() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LIGHTDMP");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(b"t");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(b"id");
    bytes.extend_from_slice(&[0u8, 0, 1]);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(b"note");
    bytes.extend_from_slice(&[2u8, 1, 0]);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&1i64.to_le_bytes());
    bytes.push(0);
    let dump = dataplane::read_dump(&mut bytes.as_slice()).unwrap();
    assert_eq!(dump.len(), 1);
    assert_eq!(dump[0].name, "t");
    assert_eq!(dump[0].columns.len(), 2);
    assert_eq!(dump[0].columns[0].ty, FieldType::I64);
    assert!(dump[0].columns[0].primary_key);
    assert!(!dump[0].columns[0].nullable);
    assert_eq!(dump[0].columns[1].ty, FieldType::Text);
    assert!(dump[0].columns[1].nullable);
    assert_eq!(
        dump[0].rows,
        vec![vec![Value::I64(1), Value::Null(FieldType::Text)]]
    );
}

#[test]
fn export_and_import_round_trip_through_csv() {
    let db = seed();
    let directory = tempfile::tempdir().unwrap();
    let csv = directory.path().join("blobs.csv");
    let report = db.export_csv::<Blob>(&csv).unwrap();
    assert_eq!(report.rows, 3);

    let text = std::fs::read(&csv).unwrap();
    let text = String::from_utf8(text).unwrap();
    let header = text.lines().next().unwrap();
    assert_eq!(header, "id,approx,flag,name,data,uuid");
    assert!(text.contains("\"widget-1, \"\"quoted\"\"\nnewline\""));

    let mut fresh = Db::open_in_memory().unwrap();
    let report = fresh.import_csv::<Blob>(&csv).unwrap();
    assert_eq!(report.rows, 3);
    assert_eq!(table_values::<Blob>(&fresh), table_values::<Blob>(&db));

    let error = fresh.import_csv::<Blob>(&csv).unwrap_err();
    assert!(matches!(error, Error::DuplicateKey { .. }), "{error}");
    assert_eq!(fresh.scan::<Blob>().unwrap().len(), 3);
}

#[test]
fn import_rejects_bad_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| {
        let path = directory.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    };

    let csv = write("unknown.csv", "id,nope\n1,2\n");
    let mut db = Db::open_in_memory().unwrap();
    assert!(db.import_csv::<Blob>(&csv).is_err());

    let csv = write(
        "bad.csv",
        "id,approx,flag,name,data,uuid\n1,not-a-number,true,x,,0102030405060708090a0b0c0d0e0f10\n",
    );
    let error = db.import_csv::<Blob>(&csv).unwrap_err();
    assert!(error.to_string().contains("row 2"), "{error}");
    assert!(error.to_string().contains("approx"), "{error}");

    let csv = write("missing.csv", "maybe_note,maybe_score\nfoo,1\n");
    let error = db.import_csv::<Opts>(&csv).unwrap_err();
    let text = error.to_string();
    assert!(text.contains("opts") && text.contains("`id`"), "{text}");

    let csv = write("empty.csv", "id,approx,flag,name,data,uuid\n1,,true,,,\n");
    let error = db.import_csv::<Blob>(&csv).unwrap_err();
    assert!(error.to_string().contains("needs a value"), "{error}");

    let csv = write("empty-file.csv", "");
    assert!(db.import_csv::<Blob>(&csv).is_err());
}

#[test]
fn nulls_survive_csv_with_documented_rules() {
    let db = seed();
    let directory = tempfile::tempdir().unwrap();
    let csv = directory.path().join("nulls.csv");
    db.export_csv::<Opts>(&csv).unwrap();

    let text = std::fs::read(&csv).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains(",,10,,"), "{text}");
    assert!(text.contains(",\"note, with comma\",20,,"), "{text}");

    let mut fresh = Db::open_in_memory().unwrap();
    fresh.import_csv::<Opts>(&csv).unwrap();
    assert_eq!(table_values::<Opts>(&fresh), table_values::<Opts>(&db));
}
