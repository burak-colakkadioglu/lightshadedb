use super::{stored_rows_expression, stored_tables};
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_admin_methods(out: &mut String, workload: &Workload) {
    let rows = stored_rows_expression(workload, "db");
    let table_count = stored_tables(workload).count();
    let per_table = stored_tables(workload)
        .map(|table| format!("db.t_{}.len() as u64", table.name))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_verify(path: &Path) -> io::Result<(u64, u64, u64, u64, u64, Vec<u64>)> {{"
    )
    .unwrap();
    writeln!(out, "        let db = Db::open_read_only(path)?;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(
        out,
        "        let snapshot = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(
        out,
        "        let wal = fs::metadata(__ls_sidecar(path, \".wal\")).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(out, "        let counts = vec![{per_table}];").unwrap();
    writeln!(
        out,
        "        Ok((db.__ls_lsn, rows, snapshot, wal, __LS_SCHEMA, counts))"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_inspect(path: &Path) -> io::Result<(u32, u32, u64, u64, u64, u64, u64, u64, Vec<u64>)> {{"
    )
    .unwrap();
    writeln!(out, "        let db = Db::open_read_only(path)?;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(
        out,
        "        let snapshot = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(
        out,
        "        let wal = fs::metadata(__ls_sidecar(path, \".wal\")).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(out, "        let counts = vec![{per_table}];").unwrap();
    writeln!(
        out,
        "        Ok((__LS_VERSION, __LS_WAL_VERSION, __LS_SCHEMA, __LS_WORKLOAD, db.__ls_lsn, rows, snapshot, wal, counts))"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_backup(source: &Path, destination: &Path, overwrite: bool) -> io::Result<(u64, u64, u64)> {{"
    )
    .unwrap();
    writeln!(out, "        if __ls_admin_alias(source, destination)? {{").unwrap();
    writeln!(
        out,
        "            return Err(io::Error::new(io::ErrorKind::InvalidInput, \"backup destination aliases the database or one of its sidecar files\"));"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        if destination.exists() && !overwrite {{").unwrap();
    writeln!(
        out,
        "            return Err(io::Error::new(io::ErrorKind::AlreadyExists, \"backup destination already exists\"));"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        let db = Db::open_read_only(source)?;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(out, "        let lsn = db.__ls_lsn;").unwrap();
    writeln!(out, "        let parent = destination.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new(\".\"));").unwrap();
    writeln!(out, "        let name = destination.file_name().and_then(|name| name.to_str()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, \"backup destination has no file name\"))?;").unwrap();
    writeln!(out, "        let temporary = parent.join(format!(\".{{name}}.{{}}.{{}}.tmp\", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0)));").unwrap();
    writeln!(out, "        let write_result = (|| -> io::Result<u64> {{").unwrap();
    writeln!(out, "            let mut options = __ls_private_options();").unwrap();
    writeln!(out, "            options.write(true).create_new(true);").unwrap();
    writeln!(out, "            let file = options.open(&temporary)?;").unwrap();
    writeln!(out, "            let mut file = io::BufWriter::new(file);").unwrap();
    writeln!(out, "            db.__ls_write_snapshot(&mut file)?;").unwrap();
    writeln!(out, "            file.flush()?;").unwrap();
    writeln!(out, "            file.get_ref().sync_all()?;").unwrap();
    writeln!(out, "            Ok(file.get_ref().metadata()?.len())").unwrap();
    writeln!(out, "        }})();").unwrap();
    writeln!(out, "        let bytes = match write_result {{").unwrap();
    writeln!(out, "            Ok(bytes) => bytes,").unwrap();
    writeln!(out, "            Err(error) => {{").unwrap();
    writeln!(out, "                let _ = fs::remove_file(&temporary);").unwrap();
    writeln!(out, "                return Err(error);").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        if overwrite {{").unwrap();
    writeln!(
        out,
        "            if let Err(error) = fs::rename(&temporary, destination) {{"
    )
    .unwrap();
    writeln!(out, "                let _ = fs::remove_file(&temporary);").unwrap();
    writeln!(out, "                return Err(error);").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(
        out,
        "        }} else if let Err(error) = fs::hard_link(&temporary, destination) {{"
    )
    .unwrap();
    writeln!(out, "            let _ = fs::remove_file(&temporary);").unwrap();
    writeln!(out, "            return Err(error);").unwrap();
    writeln!(out, "        }} else {{").unwrap();
    writeln!(out, "            let _ = fs::remove_file(&temporary);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        #[cfg(unix)] {{").unwrap();
    writeln!(out, "            File::open(parent)?.sync_all()?;").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Ok((lsn, rows, bytes))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_stored_schema(path: &Path) -> io::Result<(u64, u64, Vec<(String, Vec<(String, u8, bool, bool)>)>)> {{"
    )
    .unwrap();
    writeln!(out, "        let file = File::open(path)?;").unwrap();
    writeln!(
        out,
        "        let mut input = __LsInput::new(io::BufReader::new(file));"
    )
    .unwrap();
    writeln!(out, "        if input.take(8)?[..] != __LS_MAGIC[..] {{ return Err(__ls_invalid(\"bad snapshot magic\")); }}").unwrap();
    writeln!(out, "        let version = input.u32()?;").unwrap();
    writeln!(out, "        if version != __LS_VERSION && version != __LS_PREVIOUS_VERSION && version != __LS_OLD_VERSION {{ return Err(__ls_invalid(\"unsupported snapshot version\")); }}").unwrap();
    writeln!(out, "        let stored_fingerprint = input.u64()?;").unwrap();
    writeln!(out, "        let lsn = input.u64()?;").unwrap();
    writeln!(out, "        let count = input.u32()? as usize;").unwrap();
    writeln!(out, "        let mut tables = Vec::with_capacity(count);").unwrap();
    writeln!(out, "        for _ in 0..count {{").unwrap();
    writeln!(out, "            let name = input.text()?;").unwrap();
    writeln!(out, "            let field_count = input.u32()? as usize;").unwrap();
    writeln!(
        out,
        "            let mut fields = Vec::with_capacity(field_count);"
    )
    .unwrap();
    writeln!(out, "            for _ in 0..field_count {{").unwrap();
    writeln!(out, "                let field = input.text()?;").unwrap();
    writeln!(out, "                let tag = input.u8()?;").unwrap();
    writeln!(out, "                let flags = input.u8()?;").unwrap();
    writeln!(out, "                if tag > 13 {{ return Err(__ls_invalid(\"snapshot column has unknown type\")); }}").unwrap();
    writeln!(
        out,
        "                fields.push((field, tag, (flags & 1) == 1, (flags & 2) == 2));"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "            let rows = input.u64()?;").unwrap();
    writeln!(out, "            let mut parsed = 0u64;").unwrap();
    writeln!(out, "            while parsed < rows {{").unwrap();
    writeln!(
        out,
        "                __ls_read_chunk(&mut input, rows, &mut parsed)?;"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "            tables.push((name, fields));").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        input.finish()?;").unwrap();
    writeln!(out, "        Ok((stored_fingerprint, lsn, tables))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_migrate(path: &Path, backup: Option<&Path>) -> io::Result<(u64, u64)> {{"
    )
    .unwrap();
    writeln!(out, "        if let Some(backup) = backup {{").unwrap();
    writeln!(out, "            let mut options = __ls_private_options();").unwrap();
    writeln!(out, "            options.write(true).create_new(true);").unwrap();
    writeln!(out, "            let mut source = File::open(path)?;").unwrap();
    writeln!(
        out,
        "            let mut destination = options.open(backup)?;"
    )
    .unwrap();
    writeln!(out, "            io::copy(&mut source, &mut destination)?;").unwrap();
    writeln!(out, "            destination.sync_all()?;").unwrap();
    writeln!(
        out,
        "            let source_wal = __ls_sidecar(path, \".wal\");"
    )
    .unwrap();
    writeln!(
        out,
        "            if let Ok(mut source) = File::open(&source_wal) {{"
    )
    .unwrap();
    writeln!(
        out,
        "                let backup_wal = __ls_sidecar(backup, \".wal\");"
    )
    .unwrap();
    writeln!(
        out,
        "                let mut options = __ls_private_options();"
    )
    .unwrap();
    writeln!(out, "                options.write(true).create_new(true);").unwrap();
    writeln!(
        out,
        "                let mut destination = options.open(&backup_wal)?;"
    )
    .unwrap();
    writeln!(
        out,
        "                io::copy(&mut source, &mut destination)?;"
    )
    .unwrap();
    writeln!(out, "                destination.sync_all()?;").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        let mut db = Db::open_migrated(path)?;").unwrap();
    writeln!(out, "        let lsn = db.__ls_lsn;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(out, "        db.checkpoint()?;").unwrap();
    writeln!(out, "        db.close()?;").unwrap();
    writeln!(out, "        Ok((lsn, rows))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_compact(path: &Path) -> io::Result<(u64, u64, u64, u64, u64, u64)> {{"
    )
    .unwrap();
    writeln!(out, "        let wal_path = __ls_sidecar(path, \".wal\");").unwrap();
    writeln!(
        out,
        "        let snapshot_before = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(
        out,
        "        let wal_before = fs::metadata(&wal_path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(out, "        let mut db = Db::open(path)?;").unwrap();
    writeln!(out, "        let lsn = db.__ls_lsn;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(out, "        db.checkpoint()?;").unwrap();
    writeln!(out, "        db.close()?;").unwrap();
    writeln!(
        out,
        "        let snapshot_after = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(
        out,
        "        let wal_after = fs::metadata(&wal_path).map(|meta| meta.len()).unwrap_or(0);"
    )
    .unwrap();
    writeln!(
        out,
        "        Ok((lsn, rows, snapshot_before, snapshot_after, wal_before, wal_after))"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_recover(path: &Path, discard_wal: bool) -> io::Result<(u64, u64)> {{"
    )
    .unwrap();
    writeln!(out, "        let mut db = Db::recover(path, discard_wal)?;").unwrap();
    writeln!(out, "        let lsn = db.__ls_lsn;").unwrap();
    writeln!(out, "        let rows = {rows};").unwrap();
    writeln!(out, "        db.checkpoint()?;").unwrap();
    writeln!(out, "        db.close()?;").unwrap();
    writeln!(out, "        Ok((lsn, rows))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_admin_table_count(&self) -> usize {{"
    )
    .unwrap();
    writeln!(out, "        {table_count}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}
