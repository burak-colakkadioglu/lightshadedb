use lightshade_codegen::compiler::generate_interface;
use lightshade_codegen::ir::{parse_workload, validate, Body, Workload};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

mod cmds;

struct CliError {
    message: String,
    code: i32,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 2,
        }
    }
}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        let code = if message.starts_with("usage:") { 2 } else { 1 };
        Self { message, code }
    }
}

impl From<&str> for CliError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        std::process::exit(2);
    }

    let result = match args[0].as_str() {
        "check" => cmds::cmd_check(&args[1..]),
        "generate" => cmds::cmd_generate(&args[1..]),
        "build" => cmds::cmd_build(&args[1..]),
        "run" => cmds::cmd_run(&args[1..]),
        "serve" => cmds::cmd_serve(&args[1..]),
        "migrate" => cmds::cmd_migrate(&args[1..]),
        "verify" => cmds::cmd_verify(&args[1..]),
        "inspect" => cmds::cmd_inspect(&args[1..]),
        "compact" => cmds::cmd_compact(&args[1..]),
        "recover" => cmds::cmd_recover(&args[1..]),
        "backup" => cmds::cmd_backup(&args[1..]),
        "dump" => cmds::cmd_dump(&args[1..]),
        "restore" => cmds::cmd_restore(&args[1..]),
        "export" => cmds::cmd_export(&args[1..]),
        "import" => cmds::cmd_import(&args[1..]),
        "explain" => cmds::cmd_explain(&args[1..]),
        "init" => cmds::cmd_init(&args[1..]),
        "version" | "-V" | "--version" => {
            println!("lightshade {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "-h" | "--help" => {
            usage();
            Ok(())
        }
        other => Err(CliError::usage(format!("unknown command `{other}`"))),
    };

    if let Err(error) = result {
        eprintln!("error: {}", error.message);
        std::process::exit(error.code);
    }
}

fn usage() {
    println!(
        "         lightshade - Lightshade Database Software\n\n\
         Usage:\n  \
         lightshade --version\n  \
         lightshade check <workload.lightshade>\n  \
         lightshade generate <workload.lightshade> [--out <file.rs>]\n  \
         lightshade build <workload.lightshade> [--out <file>]\n  \
         lightshade run <workload.lightshade> <database.lsdb|database.lsdb.sock> <operation> [name=value ...]\n  \
         lightshade serve <workload.lightshade> <database.lsdb>\n  \
         lightshade migrate <workload.lightshade> <database.lsdb> [--dry-run | --apply [--allow-destructive] [--no-backup]]\n  \
         lightshade verify <workload.lightshade> <database.lsdb>\n  \
         lightshade inspect <workload.lightshade> <database.lsdb>\n  \
         lightshade compact <workload.lightshade> <database.lsdb>\n  \
         lightshade recover <workload.lightshade> <database.lsdb> [--discard-wal]\n  \
         lightshade backup <workload.lightshade> <database.lsdb> <destination.lsdb>\n  \
         lightshade dump <workload.lightshade> <database.lsdb> <destination.dump>\n  \
         lightshade restore <workload.lightshade> <database.lsdb> <source.dump>\n  \
         lightshade export <workload.lightshade> <database.lsdb> <table> <destination.csv>\n  \
         lightshade import <workload.lightshade> <database.lsdb> <table> <source.csv>\n  \
         lightshade explain <workload.lightshade> [operation]\n  \
         lightshade init <path> [--name <name>] [--features <list>] [--path <lightshade>] [--force]\n\n\
         Commands:\n  \
         check      Parse and validate a workload\n  \
         generate   Emit specialized Rust for a workload\n  \
         build      Compile and cache the interface binary for a workload\n  \
         run        Execute one operation against a database or server\n  \
         serve      Host a database over a Unix socket\n  \
         migrate    Upgrade a database created by an older build\n  \
         verify     Check a database for damage without modifying it\n  \
         inspect    Show schema, sizes, LSN, and row counts\n  \
         compact    Rewrite the snapshot and truncate the WAL\n  \
         recover    Repair a torn WAL, or discard it with --discard-wal\n  \
         backup     Write a consistent copy of a database\n  \
         dump       Write a portable, self-describing backup file\n  \
         restore    Rebuild a database from a dump file\n  \
         export     Write one table to CSV (empty field means NULL)\n  \
         import     Append CSV rows into one table\n  \
         explain    Show each operation's access path without a database\n  \
         init       Scaffold a Rust-native Lightshade application"
    );
}

fn run_target(raw: &str) -> Result<PathBuf, CliError> {
    let path = Path::new(raw);
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("lsdb") | Some("sock") => Ok(path.to_path_buf()),
        _ => Err(CliError::from(format!(
            "run targets must end in `.lsdb` or `.sock`: `{raw}`"
        ))),
    }
}

fn backup_path(database: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database.lsdb");
    database.with_file_name(format!("{file_name}.pre-migrate-{nanos}.lsdb"))
}

fn backup_raw(database: &Path, destination: &Path) -> Result<(), CliError> {
    if destination.exists() {
        return Err(CliError::from(format!(
            "backup destination `{}` already exists",
            destination.display()
        )));
    }
    for sidecar in [".wal", ".lock"] {
        let mut candidate = database.as_os_str().to_os_string();
        candidate.push(sidecar);
        if same_file(&candidate, destination) {
            return Err(CliError::from(format!(
                "backup destination `{}` aliases the database or one of its sidecar files",
                destination.display()
            )));
        }
    }
    copy_file_synced(database, destination)?;
    let mut wal = database.as_os_str().to_os_string();
    wal.push(".wal");
    let wal = Path::new(&wal);
    if wal.exists() {
        let mut backup_wal = destination.as_os_str().to_os_string();
        backup_wal.push(".wal");
        copy_file_synced(wal, Path::new(&backup_wal))?;
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(left: &std::ffi::OsStr, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(left), std::fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => canonical_equal(left, right),
    }
}

#[cfg(not(unix))]
fn same_file(left: &std::ffi::OsStr, right: &Path) -> bool {
    canonical_equal(left, right)
}

fn canonical_equal(left: &std::ffi::OsStr, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn copy_file_synced(source: &Path, destination: &Path) -> Result<(), CliError> {
    if let Some(parent) = destination.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("cannot create directory `{}`: {error}", parent.display())
            })?;
        }
    }
    let bytes = std::fs::read(source)
        .map_err(|error| format!("cannot read `{}`: {error}", source.display()))?;
    {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| format!("cannot write `{}`: {error}", destination.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("cannot tighten `{}`: {error}", destination.display()))?;
        }
        let mut file = std::io::BufWriter::new(file);
        file.write_all(&bytes)
            .map_err(|error| format!("cannot write `{}`: {error}", destination.display()))?;
        file.flush()
            .map_err(|error| format!("cannot write `{}`: {error}", destination.display()))?;
        file.get_ref()
            .sync_all()
            .map_err(|error| format!("cannot sync `{}`: {error}", destination.display()))?;
    }
    sync_parent(destination)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), CliError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    match parent {
        Some(parent) => std::fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|error| {
                CliError::from(format!("cannot sync `{}`: {error}", parent.display()))
            }),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn run_interface_capture(binary: &Path, args: &[PathBuf]) -> Result<String, CliError> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run interface binary: {error}"))?;
    if !output.status.success() {
        return Err(CliError::from(format!(
            "interface failed: {}",
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn database_path(raw: &str) -> Result<PathBuf, CliError> {
    let path = Path::new(raw);
    if path.extension().and_then(|extension| extension.to_str()) != Some("lsdb") {
        return Err(CliError::from(format!(
            "database paths must end in `.lsdb`: `{raw}`"
        )));
    }
    Ok(path.to_path_buf())
}

fn interface_binary(workload: &Workload) -> Result<PathBuf, CliError> {
    if let Ok(path) = env::var("LIGHTSHADE_INTERFACE") {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(CliError::from(format!(
                "LIGHTSHADE_INTERFACE `{}` is not a file",
                path.display()
            )));
        }
        return Ok(path);
    }
    cache_interface(workload)
}

fn cache_interface(workload: &Workload) -> Result<PathBuf, CliError> {
    let source = generate_interface(workload)?;
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for byte in rustc.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if let Ok(metadata) = fs::metadata(&rustc) {
        for value in [
            metadata.len(),
            metadata
                .modified()
                .ok()
                .and_then(|time| {
                    time.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|duration| duration.as_nanos() as u64)
                })
                .unwrap_or(0),
        ] {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }

    let directory = cache_directory();
    secure_cache_directory(&directory)?;
    let stem = format!("interface-{hash:016x}");
    let binary = directory.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
    match fs::symlink_metadata(&binary) {
        Ok(metadata) if metadata.file_type().is_file() => return Ok(binary),
        Ok(_) => {
            return Err(CliError::from(format!(
                "refusing unsafe cache entry `{}`",
                binary.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CliError::from(format!(
                "cannot inspect `{}`: {error}",
                binary.display()
            )))
        }
    }

    let source_path = directory.join(format!("{stem}.rs"));
    write_cache_file(&directory, &format!("{stem}.rs"), source.as_bytes())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let build_directory = directory.join(format!(
        ".{stem}.{}.{nonce}.build{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    fs::create_dir(&build_directory).map_err(|error| {
        format!(
            "cannot create build directory `{}`: {error}",
            build_directory.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&build_directory, fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                let _ = fs::remove_dir_all(&build_directory);
                format!(
                    "cannot secure build directory `{}`: {error}",
                    build_directory.display()
                )
            },
        )?;
    }
    let temporary = build_directory.join(format!("interface{}", std::env::consts::EXE_SUFFIX));

    let output = Command::new(&rustc)
        .arg("--edition=2021")
        .arg("-O")
        .arg("--out-dir")
        .arg(&build_directory)
        .arg(&source_path)
        .arg("-o")
        .arg(&temporary)
        .output()
        .map_err(|error| {
            let _ = fs::remove_dir_all(&build_directory);
            format!("cannot run `{rustc}`: {error}")
        })?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&build_directory);
        let workload_name = workload
            .tables
            .first()
            .map(|table| table.name.as_str())
            .unwrap_or("workload");
        return Err(CliError::from(format!(
            "cannot compile the interface for `{workload_name}`:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let metadata = match fs::symlink_metadata(&temporary) {
        Ok(metadata) => metadata,
        Err(error) => {
            let _ = fs::remove_dir_all(&build_directory);
            return Err(CliError::from(format!(
                "cannot inspect `{}`: {error}",
                temporary.display()
            )));
        }
    };
    if !metadata.file_type().is_file() {
        let _ = fs::remove_dir_all(&build_directory);
        return Err(CliError::from(format!(
            "rustc did not produce a regular file at `{}`",
            temporary.display()
        )));
    }

    if let Ok(existing) = fs::read(&binary) {
        if let Ok(fresh) = fs::read(&temporary) {
            if existing == fresh {
                let _ = fs::remove_file(&temporary);
                let _ = fs::remove_dir_all(&build_directory);
                return Ok(binary);
            }
        }
    }
    if let Err(error) = install_cached_binary(&temporary, &binary) {
        let _ = fs::remove_dir_all(&build_directory);
        return Err(CliError::from(format!(
            "cannot install `{}`: {error}",
            binary.display()
        )));
    }
    let _ = fs::remove_dir_all(&build_directory);
    Ok(binary)
}

fn secure_cache_directory(directory: &Path) -> Result<(), CliError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CliError::from(format!(
                "cache directory `{}` is a symlink",
                directory.display()
            )));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(CliError::from(format!(
                "cache path `{}` is not a directory",
                directory.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(directory)
                .map_err(|error| format!("cannot create `{}`: {error}", directory.display()))?;
            if fs::symlink_metadata(directory)
                .map_err(|error| format!("cannot inspect `{}`: {error}", directory.display()))?
                .file_type()
                .is_symlink()
            {
                return Err(CliError::from(format!(
                    "cache directory `{}` is a symlink",
                    directory.display()
                )));
            }
        }
        Err(error) => {
            return Err(CliError::from(format!(
                "cannot inspect `{}`: {error}",
                directory.display()
            )))
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot secure `{}`: {error}", directory.display()))?;
    }
    Ok(())
}

fn install_cached_binary(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    let mut wait = std::time::Duration::from_millis(50);
    for _ in 0..40 {
        match fs::rename(temporary, destination) {
            Ok(()) => return Ok(()),
            Err(_) => {
                std::thread::sleep(wait);
                wait = std::cmp::min(wait * 2, std::time::Duration::from_millis(500));
            }
        }
    }
    fs::rename(temporary, destination)
}

fn write_cache_file(directory: &Path, name: &str, contents: &[u8]) -> Result<(), CliError> {
    let temporary = directory.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot create `{}`: {error}", temporary.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("cannot write `{}`: {error}", temporary.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync `{}`: {error}", temporary.display()))?;
    drop(file);
    let destination = directory.join(name);
    if let Ok(existing) = fs::read(&destination) {
        if existing.as_slice() == contents {
            let _ = fs::remove_file(&temporary);
            return Ok(());
        }
    }
    install_cached_binary(&temporary, &destination)
        .map_err(|error| format!("cannot install `{}`: {error}", destination.display()))?;
    Ok(())
}

fn cache_directory() -> PathBuf {
    if let Ok(directory) = env::var("LIGHTSHADE_CACHE") {
        return PathBuf::from(directory);
    }
    if let Ok(directory) = env::var("XDG_CACHE_HOME") {
        return PathBuf::from(directory).join("lightshade");
    }
    if let Ok(directory) = env::var("LOCALAPPDATA") {
        return PathBuf::from(directory).join("lightshade");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("lightshade");
    }
    if let Ok(home) = env::var("USERPROFILE") {
        return PathBuf::from(home).join(".cache").join("lightshade");
    }
    env::temp_dir().join(format!("lightshade-cache-{}", std::process::id()))
}

fn finish_interface(mut command: Command) -> Result<(), CliError> {
    let status = command
        .status()
        .map_err(|error| format!("cannot run interface binary: {error}"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn load_workload(path: &str) -> Result<Workload, CliError> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read workload `{path}`: {error}"))?;
    let workload = parse_workload(&source).map_err(|error| error.to_string())?;
    validate(&workload)?;
    Ok(workload)
}

fn body_kind(body: &Body) -> &'static str {
    match body {
        Body::Query { .. } => "query",
        Body::Update { .. } => "mutation",
        Body::Delete { .. } => "mutation",
        Body::Insert { .. } => "mutation",
    }
}

fn write_file(path: &Path, contents: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create directory `{}`: {error}", parent.display()))?;
    }
    fs::write(path, contents)
        .map_err(|error| format!("cannot write `{}`: {error}", path.display()))?;
    Ok(())
}
