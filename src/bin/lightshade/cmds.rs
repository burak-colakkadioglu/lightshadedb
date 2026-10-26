use lightshade_codegen::compiler::generate;
use lightshade_codegen::ir::{Body, Returns};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    backup_path, backup_raw, body_kind, cache_interface, database_path, finish_interface,
    install_cached_binary, interface_binary, load_workload, run_interface_capture, run_target,
    write_file, CliError,
};

pub(crate) fn cmd_check(args: &[String]) -> Result<(), CliError> {
    let path = args
        .first()
        .ok_or("usage: lightshade check <workload.lightshade>")?;
    let workload = load_workload(path)?;

    println!(
        "ok: {} tables, {} operations",
        workload.tables.len(),
        workload.operations.len()
    );
    for table in &workload.tables {
        let primary_key = table
            .primary_key()
            .map(|field| field.name.as_str())
            .unwrap_or("?");
        println!(
            "  table {:<20} {} fields, primary key `{}`",
            table.name,
            table.fields.len(),
            primary_key
        );
    }
    for operation in &workload.operations {
        println!(
            "  op    {:<20} {} ({})",
            operation.name,
            body_kind(&operation.body),
            operation
                .params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.ty))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
pub(crate) fn cmd_generate(args: &[String]) -> Result<(), CliError> {
    let path = args
        .first()
        .ok_or("usage: lightshade generate <workload.lightshade> [--out <file.rs>]")?;
    let out = match args.get(1).map(String::as_str) {
        Some("--out") => args.get(2).ok_or("`--out` needs a file path")?.clone(),
        Some(other) => return Err(CliError::from(format!("unexpected argument `{other}`"))),
        None => "target/lightshade/generated.rs".to_string(),
    };
    if args.len() > 3 {
        return Err(CliError::from(format!("unexpected argument `{}`", args[3])));
    }

    let workload = load_workload(path)?;
    let source = generate(&workload)?;
    write_file(Path::new(&out), &source)?;
    println!("wrote {out}");
    Ok(())
}
pub(crate) fn cmd_build(args: &[String]) -> Result<(), CliError> {
    let path = args
        .first()
        .ok_or("usage: lightshade build <workload.lightshade> [--out <file>]")?;
    let out = match args.get(1).map(String::as_str) {
        Some("--out") => args.get(2).ok_or("`--out` needs a file path")?.clone(),
        Some(other) => return Err(CliError::from(format!("unexpected argument `{other}`"))),
        None => String::new(),
    };
    if out.is_empty() && args.len() > 1 {
        return Err(CliError::from(format!("unexpected argument `{}`", args[1])));
    }
    if !out.is_empty() && args.len() > 3 {
        return Err(CliError::from(format!("unexpected argument `{}`", args[3])));
    }

    let workload = load_workload(path)?;
    let cached = cache_interface(&workload)?;
    let destination = if out.is_empty() {
        cached.clone()
    } else {
        let destination = PathBuf::from(&out);
        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create `{}`: {error}", parent.display()))?;
            }
        }
        let name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("`{}` has no file name", destination.display()))?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let temporary = destination.with_file_name(format!(
            ".{name}.{}.{stamp}.tmp{}",
            std::process::id(),
            std::env::consts::EXE_SUFFIX
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("cannot create `{}`: {error}", temporary.display()))?;
        let copy_result = (|| -> std::io::Result<()> {
            let mut source = fs::File::open(&cached)?;
            std::io::copy(&mut source, &mut file)?;
            let permissions = source.metadata()?.permissions();
            fs::set_permissions(&temporary, permissions)?;
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&temporary);
            return Err(CliError::from(format!(
                "cannot copy `{}` to `{}`: {error}",
                cached.display(),
                temporary.display()
            )));
        }
        #[cfg(windows)]
        {
            if destination.exists() {
                fs::remove_file(&destination).map_err(|error| {
                    let _ = fs::remove_file(&temporary);
                    format!("cannot replace `{}`: {error}", destination.display())
                })?;
            }
        }
        if let Err(error) = install_cached_binary(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(CliError::from(format!(
                "cannot install `{}`: {error}",
                destination.display()
            )));
        }
        destination
    };
    println!("wrote {}", destination.display());
    Ok(())
}
pub(crate) fn cmd_run(args: &[String]) -> Result<(), CliError> {
    if args.len() < 3 {
        return Err(CliError::from(
            "usage: lightshade run <workload.lightshade> <database.lsdb|database.lsdb.sock> <operation> [name=value ...]"
                .to_string(),
        ));
    }
    let database = run_target(&args[1])?;
    let workload = load_workload(&args[0])?;
    let operation = workload.operation(&args[2]).ok_or_else(|| {
        format!(
            "unknown operation `{}` (available: {})",
            args[2],
            workload
                .operations
                .iter()
                .map(|operation| operation.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    if let Body::Query { count: false, .. } = &operation.body {
        let returned = match &operation.returns {
            Returns::Many(returned) => returned,
            _ => {
                return Err(CliError::from(format!(
                    "operation `{}` must return `many <table>`",
                    operation.name
                )))
            }
        };
        let table = workload
            .table(returned)
            .ok_or_else(|| format!("unknown table `{returned}`"))?;
        println!(
            "# {}",
            table
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join("\t")
        );
    }

    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command
        .arg(&database)
        .arg("run")
        .arg(&args[2])
        .args(&args[3..]);
    finish_interface(command)
}
pub(crate) fn cmd_serve(args: &[String]) -> Result<(), CliError> {
    if args.len() != 2 {
        return Err(CliError::from(
            "usage: lightshade serve <workload.lightshade> <database.lsdb>".to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("serve");
    finish_interface(command)
}
pub(crate) fn cmd_migrate(args: &[String]) -> Result<(), CliError> {
    let mut positional: Vec<&str> = Vec::new();
    let mut dry_run = false;
    let mut apply = false;
    let mut allow_destructive = false;
    let mut no_backup = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--apply" => apply = true,
            "--allow-destructive" => allow_destructive = true,
            "--no-backup" => no_backup = true,
            other if other.starts_with('-') => {
                return Err(CliError::from(format!("unknown migrate option `{other}`")));
            }
            other => positional.push(other),
        }
    }
    if positional.len() != 2 || (dry_run && apply) {
        return Err(CliError::from(
            "usage: lightshade migrate <workload.lightshade> <database.lsdb> [--dry-run | --apply [--allow-destructive] [--no-backup]]"
                .to_string(),
        ));
    }
    if dry_run {
        return cmd_migrate_plan(positional[0], positional[1]);
    }
    if apply {
        return cmd_migrate_apply(positional[0], positional[1], allow_destructive, no_backup);
    }
    let database = database_path(positional[1])?;
    let workload = load_workload(positional[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("migrate");
    finish_interface(command)
}
fn cmd_migrate_plan(workload: &str, database: &str) -> Result<(), CliError> {
    let database = database_path(database)?;
    let workload = load_workload(workload)?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("migrate-plan");
    finish_interface(command)
}
fn cmd_migrate_apply(
    workload: &str,
    database: &str,
    allow_destructive: bool,
    no_backup: bool,
) -> Result<(), CliError> {
    let database = database_path(database)?;
    let workload = load_workload(workload)?;
    let binary = interface_binary(&workload)?;
    let plan = run_interface_capture(&binary, &[database.clone(), "migrate-plan".into()])?;
    if plan.contains("(blocked:") {
        return Err(CliError::from(format!(
            "migration cannot be applied:\n{plan}\nresolve the blocked steps first"
        )));
    }
    let destructive = plan.contains("(destructive)");
    if destructive && !allow_destructive {
        return Err(CliError::from(format!(
            "migration would drop stored data:\n{plan}\npass --allow-destructive to proceed"
        )));
    }
    let backup = if no_backup {
        None
    } else {
        let destination = backup_path(&database);
        backup_raw(&database, &destination)?;
        Some(destination)
    };
    let mut command = Command::new(&binary);
    command.arg(&database).arg("migrate");
    finish_interface(command)?;
    match backup {
        Some(destination) => {
            println!("migrated {}", database.display());
            println!("backup: {}", destination.display());
        }
        None => println!("migrated {} (no backup)", database.display()),
    }
    Ok(())
}
pub(crate) fn cmd_verify(args: &[String]) -> Result<(), CliError> {
    cmd_database_mode(
        args,
        "verify",
        "usage: lightshade verify <workload.lightshade> <database.lsdb>",
    )
}
pub(crate) fn cmd_inspect(args: &[String]) -> Result<(), CliError> {
    cmd_database_mode(
        args,
        "inspect",
        "usage: lightshade inspect <workload.lightshade> <database.lsdb>",
    )
}
pub(crate) fn cmd_compact(args: &[String]) -> Result<(), CliError> {
    cmd_database_mode(
        args,
        "compact",
        "usage: lightshade compact <workload.lightshade> <database.lsdb>",
    )
}
pub(crate) fn cmd_init(args: &[String]) -> Result<(), CliError> {
    let mut destination = None;
    let mut name = None;
    let mut features = Vec::new();
    let mut dependency_path = None;
    let mut force = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--name" => {
                index += 1;
                name = Some(args.get(index).ok_or("`--name` requires a value")?.clone());
            }
            "--features" => {
                index += 1;
                let value = args.get(index).ok_or("`--features` requires a value")?;
                for feature in value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    features.push(feature.to_string());
                }
            }
            "--path" | "--lightshade-path" => {
                index += 1;
                dependency_path = Some(args.get(index).ok_or("`--path` requires a value")?.clone());
            }
            "--force" => force = true,
            other if other.starts_with('-') => {
                return Err(CliError::from(format!("unknown init option `{other}`")));
            }
            other => {
                if destination.is_some() {
                    return Err(CliError::from(
                        "init accepts exactly one destination path".to_string(),
                    ));
                }
                destination = Some(other.to_string());
            }
        }
        index += 1;
    }
    let destination = destination.ok_or(
        "usage: lightshade init <path> [--name <name>] [--features <list>] [--path <lightshade>] [--force]",
    )?;
    for feature in &features {
        if !matches!(feature.as_str(), "embedded" | "host" | "client") {
            return Err(CliError::from(format!(
                "unknown feature `{feature}`; expected embedded, host, or client"
            )));
        }
    }
    if features.is_empty() {
        features.push("embedded".to_string());
    }
    if features.iter().any(|feature| feature == "host")
        && !features.iter().any(|feature| feature == "embedded")
    {
        features.push("embedded".to_string());
    }
    let directory = Path::new(&destination);
    let crate_name = match name {
        Some(name) => name,
        None => directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("cannot derive a crate name from the destination path; pass `--name`")?
            .to_string(),
    };
    let crate_name = sanitize_crate_name(&crate_name)?;
    if directory.exists() && !directory.is_dir() {
        return Err(CliError::from(format!(
            "`{destination}` exists and is not a directory"
        )));
    }
    let manifest = directory.join("Cargo.toml");
    let source = directory.join("src").join("main.rs");
    if !force && (manifest.exists() || source.exists()) {
        return Err(CliError::from(format!(
            "`{destination}` already contains a Lightshade scaffold; pass `--force` to overwrite"
        )));
    }
    fs::create_dir_all(directory.join("src")).map_err(|error| error.to_string())?;
    let dependency = match dependency_path {
        Some(path) => format!(
            "lightshade = {{ path = {:?}, default-features = false }}",
            path
        ),
        None => format!(
            "lightshade = {{ version = {:?}, default-features = false }}",
            env!("CARGO_PKG_VERSION")
        ),
    };
    let default_features = features
        .iter()
        .map(|feature| format!("\"{feature}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let manifest_text = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
{dependency}

[features]
default = [{default_features}]
embedded = ["lightshade/embedded"]
host = ["embedded", "lightshade/host"]
client = ["lightshade/client"]
"#
    );
    fs::write(&manifest, manifest_text).map_err(|error| error.to_string())?;
    fs::write(source, scaffold_source()).map_err(|error| error.to_string())?;
    println!("initialized {crate_name} at {destination}");
    Ok(())
}
fn sanitize_crate_name(name: &str) -> Result<String, CliError> {
    let mut out = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            if index == 0 && character.is_ascii_digit() {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
        } else if character.is_whitespace() || character == '.' {
            out.push('-');
        } else {
            return Err(CliError::from(format!(
                "`{name}` is not a valid crate name; use letters, digits, `-`, or `_`"
            )));
        }
    }
    if out.is_empty() {
        return Err(CliError::from("crate name cannot be empty".to_string()));
    }
    Ok(out)
}
fn scaffold_source() -> &'static str {
    r#"use lightshade::prelude::*;

#[lightshade::schema]
pub mod store {
    use lightshade::prelude::*;

    pub struct Note {
        #[lightshade(primary_key)]
        pub id: i64,
        pub title: String,
        pub done: bool,
    }

    #[lightshade::operations]
    pub trait Notes {
        #[lightshade::query(
            filter = Note::done.eq(false),
            order = Note::id.asc(),
        )]
        fn open_notes(&self) -> Result<Vec<Note>>;
    }
}

fn run(app: &mut impl store::Backend) -> lightshade::Result<()> {
    app.insert(store::Note {
        id: 1,
        title: "Try Lightshade".into(),
        done: false,
    })?;
    let notes = app.open_notes()?;
    println!("{} open note(s)", notes.len());
    Ok(())
}

#[cfg(feature = "embedded")]
fn main() -> lightshade::Result<()> {
    let mut app = store::App::open(store::AppConfig::embedded("notes.lsdb"))?;
    run(&mut app)
}

#[cfg(all(feature = "client", not(feature = "embedded")))]
fn main() -> lightshade::Result<()> {
    let mut app = store::App::from_env()?;
    run(&mut app)
}
"#
}
fn cmd_database_mode(args: &[String], mode: &str, usage: &str) -> Result<(), CliError> {
    if args.len() != 2 {
        return Err(CliError::from(usage.to_string()));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg(mode);
    finish_interface(command)
}
pub(crate) fn cmd_recover(args: &[String]) -> Result<(), CliError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(CliError::from(
            "usage: lightshade recover <workload.lightshade> <database.lsdb> [--discard-wal]"
                .to_string(),
        ));
    }
    let discard = match args.get(2).map(String::as_str) {
        None => false,
        Some("--discard-wal") => true,
        Some(other) => return Err(CliError::from(format!("unexpected argument `{other}`"))),
    };
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("recover");
    if discard {
        command.arg("--discard-wal");
    }
    finish_interface(command)
}
pub(crate) fn cmd_backup(args: &[String]) -> Result<(), CliError> {
    if args.len() != 3 {
        return Err(CliError::from(
            "usage: lightshade backup <workload.lightshade> <database.lsdb> <destination.lsdb>"
                .to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let destination = database_path(&args[2])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("backup").arg(&destination);
    finish_interface(command)
}
pub(crate) fn cmd_dump(args: &[String]) -> Result<(), CliError> {
    if args.len() != 3 {
        return Err(CliError::from(
            "usage: lightshade dump <workload.lightshade> <database.lsdb> <destination.dump>"
                .to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("dump").arg(&args[2]);
    finish_interface(command)
}
pub(crate) fn cmd_restore(args: &[String]) -> Result<(), CliError> {
    if args.len() != 3 {
        return Err(CliError::from(
            "usage: lightshade restore <workload.lightshade> <database.lsdb> <source.dump>"
                .to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg(&database).arg("restore").arg(&args[2]);
    finish_interface(command)
}
pub(crate) fn cmd_export(args: &[String]) -> Result<(), CliError> {
    if args.len() != 4 {
        return Err(CliError::from(
            "usage: lightshade export <workload.lightshade> <database.lsdb> <table> <destination.csv>"
                .to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command
        .arg(&database)
        .arg("export")
        .arg(&args[2])
        .arg(&args[3]);
    finish_interface(command)
}
pub(crate) fn cmd_import(args: &[String]) -> Result<(), CliError> {
    if args.len() != 4 {
        return Err(CliError::from(
            "usage: lightshade import <workload.lightshade> <database.lsdb> <table> <source.csv>"
                .to_string(),
        ));
    }
    let database = database_path(&args[1])?;
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command
        .arg(&database)
        .arg("import")
        .arg(&args[2])
        .arg(&args[3]);
    finish_interface(command)
}
pub(crate) fn cmd_explain(args: &[String]) -> Result<(), CliError> {
    if args.is_empty() || args.len() > 2 {
        return Err(CliError::from(
            "usage: lightshade explain <workload.lightshade> [operation]".to_string(),
        ));
    }
    let workload = load_workload(&args[0])?;
    let binary = interface_binary(&workload)?;
    let mut command = Command::new(&binary);
    command.arg("--explain");
    if let Some(operation) = args.get(1) {
        command.arg(operation);
    }
    finish_interface(command)
}
