use super::super::plan::KeyField;
use super::super::GenerateOptions;
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_engine_accessors(out: &mut String, options: &GenerateOptions) {
    let visibility = if options.public_db { "pub " } else { "" };
    for (signature, body) in [
        ("fn is_read_only(&self) -> bool", "self.__ls_read_only"),
        (
            "fn in_transaction(&self) -> bool",
            "self.__ls_in_transaction",
        ),
        ("fn lsn(&self) -> u64", "self.__ls_lsn"),
        (
            "fn path(&self) -> Option<&Path>",
            "self.__ls_store.as_ref().map(|store| store.path.as_path())",
        ),
    ] {
        writeln!(out, "    {visibility}{signature} {{").unwrap();
        writeln!(out, "        {body}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }
    writeln!(
        out,
        "    {visibility}fn open_in_memory() -> io::Result<Db> {{"
    )
    .unwrap();
    writeln!(out, "        let mut db = Db::__ls_new_spilled()?;").unwrap();
    writeln!(out, "        db.__ls_store = None;").unwrap();
    writeln!(out, "        Ok(db)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

pub(crate) fn stored_tables(workload: &Workload) -> impl Iterator<Item = &Table> {
    workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
}

pub(crate) fn stored_rows_expression(workload: &Workload, database: &str) -> String {
    let terms = stored_tables(workload)
        .map(|table| format!("{database}.t_{}.len() as u64", table.name))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        "0u64".to_string()
    } else {
        terms.join(" + ")
    }
}

pub(crate) fn index_row_expression(
    table: &Table,
    key: &str,
    inner: &[KeyField],
) -> Result<String, String> {
    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
    let position = inner
        .iter()
        .position(|field| field.name == primary_key.name)
        .ok_or_else(|| {
            format!(
                "internal: index on `{}` does not carry its primary key",
                table.name
            )
        })?;
    let field = &inner[position];
    let value = format!("{key}.f{position}");
    let value = if field.reverse {
        format!("{value}.0")
    } else {
        value
    };
    match primary_key.ty {
        Ty::Text => Ok(format!(
            "self.t_{}.__ls_get_str({value}.as_str())",
            table.name
        )),
        Ty::F32 | Ty::F64 => Ok(format!("self.t_{}.get(&{value}.0)", table.name)),
        _ => Ok(format!("self.t_{}.get(&{value})", table.name)),
    }
}
