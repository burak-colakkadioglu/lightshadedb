use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in <workspace>/macros")
        .to_path_buf()
}

fn scratch_dir(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lightshade-compile-fail-{}-{case}",
        std::process::id()
    ))
}

fn check_fails(case: &str, module: &str) -> String {
    let root = workspace_root();
    let directory = scratch_dir(case);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(
        directory.join("Cargo.toml"),
        format!(
            r#"[package]
name = "compile-fail-{case}"
version = "0.1.0"
edition = "2021"

[dependencies]
lightshadedb = {{ path = "{}" }}

[features]
default = ["embedded"]
embedded = ["lightshadedb/embedded"]
host = ["embedded", "lightshadedb/host"]
client = ["lightshadedb/client"]

[workspace]
"#,
            root.clone().display().to_string().replace('\\', "/"),
        ),
    )
    .unwrap();
    std::fs::write(
        directory.join("src").join("lib.rs"),
        format!(
            "use lightshadedb::prelude::*;\n\n#[lightshadedb::schema]\npub mod store {{\n{module}\n}}\n"
        ),
    )
    .unwrap();

    let target = std::env::temp_dir().join(format!(
        "lightshade-compile-fail-target-{}",
        std::process::id()
    ));
    let output = Command::new("cargo")
        .args(["check", "--offline", "--message-format", "short"])
        .current_dir(&directory)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("failed to run cargo check");
    assert!(
        !output.status.success(),
        "{case} unexpectedly compiled:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&directory);
    stderr
}

fn check_case(case: &str, module: &str, expected: &str) {
    let stderr = check_fails(case, module);
    assert!(
        stderr.contains(expected),
        "{case} failed without the expected diagnostic `{expected}`:\n{stderr}",
    );
}

fn check_case_files(case: &str, files: &[(&str, &str)], lib: &str, expected: &str) {
    let root = workspace_root();
    let directory = scratch_dir(case);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(
        directory.join("Cargo.toml"),
        format!(
            r#"[package]
name = "compile-fail-{case}"
version = "0.1.0"
edition = "2021"

[dependencies]
lightshadedb = {{ path = "{}" }}

[features]
default = ["embedded"]
embedded = ["lightshadedb/embedded"]
host = ["embedded", "lightshadedb/host"]
client = ["lightshadedb/client"]

[workspace]
"#,
            root.clone().display().to_string().replace('\\', "/"),
        ),
    )
    .unwrap();
    for (path, contents) in files {
        let path = directory.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    std::fs::write(directory.join("src").join("lib.rs"), lib).unwrap();

    let target = std::env::temp_dir().join(format!(
        "lightshade-compile-fail-target-{}",
        std::process::id()
    ));
    let output = Command::new("cargo")
        .args(["check", "--offline", "--message-format", "short"])
        .current_dir(&directory)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("failed to run cargo check");
    assert!(
        !output.status.success(),
        "{case} unexpectedly compiled:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        stderr.contains(expected),
        "{case} failed without the expected diagnostic `{expected}`:\n{stderr}",
    );
}

const TABLE: &str = r#"
    use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub title: String,
    }
"#;

#[test]
fn tables_need_exactly_one_primary_key() {
    check_case(
        "missing-primary-key",
        r#"
    use lightshadedb::prelude::*;

    pub struct Note {
        pub id: i64,
    }
"#,
        "must have exactly one `#[lightshadedb(primary_key)]` field",
    );
}

#[test]
fn updates_reject_limit_and_offset() {
    check_case(
        "update-with-limit",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::update(
            filter = Note::id.eq(note),
            set = (Note::title.set(title),),
            limit = 1,
        )]
        fn retitle(&mut self, note: i64, title: String) -> Result<u64>;
    }}"#
        ),
        "update accepts only `filter` and `set`",
    );
    check_case(
        "update-with-offset",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::update(
            filter = Note::id.eq(note),
            set = (Note::title.set(title),),
            offset = 1,
        )]
        fn retitle(&mut self, note: i64, title: String) -> Result<u64>;
    }}"#
        ),
        "update accepts only `filter` and `set`",
    );
}

#[test]
fn updates_require_set() {
    check_case(
        "update-without-set",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::update(filter = Note::id.eq(note))]
        fn touch(&mut self, note: i64) -> Result<u64>;
    }}"#
        ),
        "update requires `set`",
    );
}

#[test]
fn deletes_accept_only_a_filter() {
    check_case(
        "delete-with-set",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::delete(
            filter = Note::id.eq(note),
            set = (Note::title.set(title),),
        )]
        fn remove(&mut self, note: i64, title: String) -> Result<u64>;
    }}"#
        ),
        "delete accepts only `filter`",
    );
    check_case(
        "delete-with-offset",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::delete(filter = Note::id.eq(note), offset = 1)]
        fn remove(&mut self, note: i64) -> Result<u64>;
    }}"#
        ),
        "delete accepts only `filter`",
    );
}

#[test]
fn unknown_fields_are_rejected() {
    check_case(
        "unknown-field",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::query(filter = Note::missing.eq(note))]
        fn find(&self, note: i64) -> Result<Vec<Note>>;
    }}"#
        ),
        "unknown field `Note::missing`",
    );
}

#[test]
fn limits_need_integers() {
    check_case(
        "limit-text",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::query(filter = Note::id.eq(note), limit = title)]
        fn find(&self, note: i64, title: String) -> Result<Vec<Note>>;
    }}"#
        ),
        "limit parameter must be an integer",
    );
}

#[test]
fn counts_reject_order_limit_and_offset() {
    check_case(
        "count-with-limit",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::query(count, filter = Note::id.eq(note), limit = 1)]
        fn count(&self, note: i64) -> Result<i64>;
    }}"#
        ),
        "`count` operations cannot have `order by`, `limit`, or `offset`",
    );
    check_case(
        "count-with-offset",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::query(count, filter = Note::id.eq(note), offset = 1)]
        fn count(&self, note: i64) -> Result<i64>;
    }}"#
        ),
        "`count` operations cannot have `order by`, `limit`, or `offset`",
    );
}

#[test]
fn duplicate_arguments_are_rejected() {
    check_case(
        "duplicate-offset",
        &format!(
            r#"{TABLE}

    #[lightshadedb::operations]
    pub trait Notes {{
        #[lightshadedb::query(filter = Note::id.eq(note), offset = 1, offset = 2)]
        fn find(&self, note: i64) -> Result<Vec<Note>>;
    }}"#
        ),
        "duplicate argument `offset`",
    );
}

#[test]
fn aggregates_need_numeric_fields() {
    check_case(
        "sum-text",
        r#"
    use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub title: String,
    }

    #[lightshadedb(skip)]
    pub struct Total {
        pub total: i64,
    }

    #[lightshadedb::operations]
    pub trait Notes {
        #[lightshadedb::query(
            from = Note,
            select = Total::select().total(sum(Note::title)),
            filter = Note::id.ge(0),
        )]
        fn total(&self) -> Result<Vec<Total>>;
    }
"#,
        "this aggregate requires a numeric field",
    );
}

#[test]
fn registries_reject_bad_allow_entries() {
    check_case_files(
        "registry-bad-allow",
        &[(
            "lightshade.toml",
            r#"[database.a1]
schema = "store"
mode = "host"
path = "data/a1.lsdb"
listen = "tcp://127.0.0.1:7101"
credentials_file = "creds.toml"
allow = ["999.0.0.1"]
"#,
        )],
        r#"use lightshadedb::prelude::*;

#[lightshadedb::schema]
pub mod store {
    use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
    }
}

lightshadedb::registry!("lightshade.toml");
"#,
        "has an invalid allowlist entry",
    );
    check_case_files(
        "registry-allow-needs-host",
        &[(
            "lightshade.toml",
            r#"[database.a1]
schema = "store"
mode = "embedded"
path = "data/a1.lsdb"
allow = ["10.0.0.0/8"]
"#,
        )],
        r#"use lightshadedb::prelude::*;

#[lightshadedb::schema]
pub mod store {
    use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
    }
}

lightshadedb::registry!("lightshade.toml");
"#,
        "`allow` needs `mode = \"host\"`",
    );
}

#[test]
fn nullable_primary_keys_are_rejected() {
    check_case(
        "nullable-primary-key",
        r#"use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: Option<i64>,
        pub title: String,
    }"#,
        "cannot be nullable",
    );
}

#[test]
fn nullable_option_params_are_rejected() {
    check_case(
        "nullable-param",
        r#"use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub score: Option<i64>,
    }

    #[lightshadedb::operations]
    pub trait NoteOps {
        #[lightshadedb::query(
            filter = Note::id.eq(id),
        )]
        fn by_id(&self, id: Option<i64>) -> Result<Vec<Note>>;
    }"#,
        "cannot be Option",
    );
}

#[test]
fn comparisons_against_none_are_rejected() {
    check_case(
        "none-comparison",
        r#"use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub score: Option<i64>,
    }

    #[lightshadedb::operations]
    pub trait NoteOps {
        #[lightshadedb::query(
            filter = Note::score.eq(None),
        )]
        fn nones(&self) -> Result<Vec<Note>>;
    }"#,
        "use `is_null()`",
    );
}

#[test]
fn nested_options_are_rejected() {
    check_case(
        "nested-option",
        r#"use lightshadedb::prelude::*;

    pub struct Note {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub score: Option<Option<i64>>,
    }"#,
        "unsupported Lightshade DB field type",
    );
}
