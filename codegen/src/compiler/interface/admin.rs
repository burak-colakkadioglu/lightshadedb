use super::{line, rows_expression};
use crate::ir::Workload;

pub(crate) fn emit_ops(out: &mut String, workload: &Workload) -> Result<(), String> {
    let rows = rows_expression(workload, "__ls_db");
    let table_count = workload.tables.len();

    line(
        out,
        "fn __ls_cli_require_existing(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    let __ls_snapshot = Path::new(__ls_path);");
    line(
        out,
        "    let __ls_wal = __ls_sidecar(__ls_snapshot, \".wal\");",
    );
    line(
        out,
        "    if !__ls_snapshot.exists() && !__ls_wal.exists() {",
    );
    line(
        out,
        "        return Err(format!(\"database does not exist: `{__ls_path}`\"));",
    );
    line(out, "    }");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_absolute(__ls_path: &Path) -> Result<PathBuf, String> {",
    );
    line(out, "    let __ls_parent = __ls_path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new(\".\"));");
    line(out, "    let __ls_name = __ls_path.file_name().ok_or_else(|| format!(\"path `{}` has no file name\", __ls_path.display()))?;");
    line(
        out,
        "    Ok(fs::canonicalize(__ls_parent).map_err(|error| error.to_string())?.join(__ls_name))",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_aliases(__ls_source: &Path, __ls_output: &Path) -> Result<bool, String> {",
    );
    line(
        out,
        "    let __ls_source = __ls_cli_absolute(__ls_source)?;",
    );
    line(
        out,
        "    let __ls_output = __ls_cli_absolute(__ls_output)?;",
    );
    line(out, "    if __ls_source == __ls_output {");
    line(out, "        return Ok(true);");
    line(out, "    }");
    line(
        out,
        "    for __ls_suffix in [\".wal\", \".lock\", \".tmp\", \".sock\"] {",
    );
    line(out, "        let __ls_sidecar = PathBuf::from(format!(\"{}{__ls_suffix}\", __ls_source.display()));");
    line(out, "        if __ls_sidecar == __ls_output {");
    line(out, "            return Ok(true);");
    line(out, "        }");
    line(out, "    }");
    line(out, "    Ok(false)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_size(__ls_path: &Path) -> u64 {");
    line(
        out,
        "    fs::metadata(__ls_path).map(|__ls_meta| __ls_meta.len()).unwrap_or(0)",
    );
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_verify(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;",
    );
    out.push_str(&format!(
        "    println!(\"ok: lsn {{}}, {} tables, {{}} rows\", __ls_db.__ls_lsn, {rows});\n",
        table_count
    ));
    for table in &workload.tables {
        out.push_str(&format!(
            "    println!(\"  {}: {{}} rows\", __ls_db.t_{}.len());\n",
            table.name, table.name
        ));
    }
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_inspect(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(out, "    let __ls_snapshot = Path::new(__ls_path);");
    line(
        out,
        "    let __ls_wal = __ls_sidecar(__ls_snapshot, \".wal\");",
    );
    line(out, "    if __ls_snapshot.exists() {");
    line(
        out,
        "        println!(\"snapshot: {} ({} bytes)\", __ls_path, __ls_cli_size(__ls_snapshot));",
    );
    line(out, "    } else {");
    line(out, "        println!(\"snapshot: absent\");");
    line(out, "    }");
    line(out, "    if __ls_wal.exists() {");
    line(
        out,
        "        println!(\"wal: {} ({} bytes)\", __ls_wal.display(), __ls_cli_size(&__ls_wal));",
    );
    line(out, "    } else {");
    line(out, "        println!(\"wal: absent\");");
    line(out, "    }");
    line(out, "    println!(\"version: {}\", __LS_VERSION);");
    line(
        out,
        "    let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    println!(\"lsn: {}\", __ls_db.__ls_lsn);");
    out.push_str(&format!("    println!(\"rows: {{}}\", {rows});\n"));
    for table in &workload.tables {
        out.push_str(&format!(
            "    println!(\"table {}: {{}} rows\", __ls_db.t_{}.len());\n",
            table.name, table.name
        ));
        for field in &table.fields {
            let flags = if field.primary_key {
                " primary_key"
            } else {
                ""
            };
            out.push_str(&format!(
                "    println!(\"  {}: {}{}\");\n",
                field.name, field.ty, flags
            ));
        }
    }
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_compact(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(out, "    let __ls_snapshot = Path::new(__ls_path);");
    line(
        out,
        "    let __ls_wal = __ls_sidecar(__ls_snapshot, \".wal\");",
    );
    line(
        out,
        "    let __ls_snapshot_before = __ls_cli_size(__ls_snapshot);",
    );
    line(out, "    let __ls_wal_before = __ls_cli_size(&__ls_wal);");
    line(
        out,
        "    let mut __ls_db = Db::open(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let __ls_lsn = __ls_db.__ls_lsn;");
    out.push_str(&format!("    let __ls_rows = {rows};\n"));
    line(
        out,
        "    __ls_db.checkpoint().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_db.close().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"compacted: lsn {}, {} rows, snapshot {} -> {} bytes, wal {} -> {} bytes\", __ls_lsn, __ls_rows, __ls_snapshot_before, __ls_cli_size(__ls_snapshot), __ls_wal_before, __ls_cli_size(&__ls_wal));",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_recover(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    let __ls_discard = match __ls_args {");
    line(out, "        [] => false,");
    line(
        out,
        "        [__ls_flag] if __ls_flag == \"--discard-wal\" => true,",
    );
    line(
        out,
        "        _ => return Err(\"recover accepts only --discard-wal\".to_string()),",
    );
    line(out, "    };");
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(out, "    let __ls_snapshot = Path::new(__ls_path);");
    line(
        out,
        "    let __ls_wal = __ls_sidecar(__ls_snapshot, \".wal\");",
    );
    line(out, "    let __ls_wal_before = __ls_cli_size(&__ls_wal);");
    line(
        out,
        "    let mut __ls_db = Db::recover(__ls_path, __ls_discard).map_err(|error| error.to_string())?;",
    );
    line(out, "    if __ls_discard {");
    line(
        out,
        "        println!(\"discarded WAL after validating the snapshot\");",
    );
    line(out, "    }");
    line(out, "    let __ls_lsn = __ls_db.__ls_lsn;");
    out.push_str(&format!("    let __ls_rows = {rows};\n"));
    line(
        out,
        "    __ls_db.checkpoint().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_db.close().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"recovered: lsn {}, {} rows, wal {} -> {} bytes\", __ls_lsn, __ls_rows, __ls_wal_before, __ls_cli_size(&__ls_wal));",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");

    line(
        out,
        "fn __ls_cli_backup(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    let __ls_destination = match __ls_args {");
    line(
        out,
        "        [__ls_destination] => __ls_destination.as_str(),",
    );
    line(
        out,
        "        _ => return Err(\"backup expects exactly one destination path\".to_string()),",
    );
    line(out, "    };");
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_destination_path = Path::new(__ls_destination);",
    );
    line(
        out,
        "    if __ls_cli_aliases(Path::new(__ls_path), __ls_destination_path)? {",
    );
    line(out, "        return Err(format!(\"backup destination `{__ls_destination}` aliases the database or one of its sidecar files\"));");
    line(out, "    }");
    line(out, "    if __ls_destination_path.exists() {");
    line(
        out,
        "        return Err(format!(\"backup destination `{__ls_destination}` already exists\"));",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;",
    );
    out.push_str(&format!("    let __ls_rows = {rows};\n"));
    line(out, "    let __ls_lsn = __ls_db.__ls_lsn;");
    line(out, "    let __ls_parent = __ls_destination_path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new(\".\"));");
    line(out, "    let __ls_name = __ls_destination_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| \"backup destination has no file name\".to_string())?;");
    line(out, "    let __ls_temporary = __ls_parent.join(format!(\".{__ls_name}.{}.{}.tmp\", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0)));");
    line(out, "    let __ls_write_result = (|| -> io::Result<u64> {");
    line(out, "        let mut __ls_options = OpenOptions::new();");
    line(out, "        __ls_options.write(true).create_new(true);");
    line(out, "        #[cfg(unix)]");
    line(out, "        {");
    line(out, "            use std::os::unix::fs::OpenOptionsExt;");
    line(out, "            __ls_options.mode(__LS_FILE_MODE);");
    line(out, "        }");
    line(
        out,
        "        let __ls_file = __ls_options.open(&__ls_temporary)?;",
    );
    line(
        out,
        "        let mut __ls_file = io::BufWriter::new(__ls_file);",
    );
    line(out, "        __ls_db.__ls_write_snapshot(&mut __ls_file)?;");
    line(out, "        __ls_file.flush()?;");
    line(out, "        __ls_file.get_ref().sync_all()?;");
    line(out, "        Ok(__ls_file.get_ref().metadata()?.len())");
    line(out, "    })();");
    line(out, "    let __ls_size = match __ls_write_result {");
    line(out, "        Ok(__ls_size) => __ls_size,");
    line(out, "        Err(__ls_error) => {");
    line(out, "            let _ = fs::remove_file(&__ls_temporary);");
    line(out, "            return Err(__ls_error.to_string());");
    line(out, "        }");
    line(out, "    };");
    line(
        out,
        "    if let Err(__ls_error) = fs::hard_link(&__ls_temporary, __ls_destination_path) {",
    );
    line(out, "        let _ = fs::remove_file(&__ls_temporary);");
    line(
        out,
        "        if __ls_error.kind() == io::ErrorKind::AlreadyExists {",
    );
    line(out, "            return Err(format!(\"backup destination `{__ls_destination}` already exists\"));");
    line(out, "        }");
    line(out, "        return Err(__ls_error.to_string());");
    line(out, "    }");
    line(out, "    let _ = fs::remove_file(&__ls_temporary);");
    line(out, "    #[cfg(unix)]");
    line(out, "    {");
    line(
        out,
        "        File::open(__ls_parent).map_err(|error| error.to_string())?.sync_all().map_err(|error| error.to_string())?;",
    );
    line(out, "    }");
    line(
        out,
        "    println!(\"backed up: lsn {}, {} rows, {} bytes -> {}\", __ls_lsn, __ls_rows, __ls_size, __ls_destination);",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_migrate(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_db = Db::open_migrated(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_db.close().map_err(|error| error.to_string())?;",
    );
    line(out, "    println!(\"migrated\");");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    Ok(())
}
