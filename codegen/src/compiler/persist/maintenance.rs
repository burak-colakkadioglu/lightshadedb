use super::super::common::row_type;
use super::super::plan::{index_inner_fields, Plan};
use super::{row_eq_ctor, row_key_ctor};
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_maintenance(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    table: &Table,
) -> Result<(), String> {
    let row = row_type(&table.name);

    writeln!(
        out,
        "    fn __ls_insert_{}(&mut self, __row: &{}) {{",
        table.name, row
    )
    .unwrap();

    for (index, spec) in plan.indexes.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let inner_fields = index_inner_fields(spec, table)?;
        let eq_ctor = row_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, "__row");
        let key_ctor = row_key_ctor(&format!("Idx{index}Key"), &inner_fields, "__row");
        writeln!(
            out,
            "        self.idx{index}.write().unwrap().insert({eq_ctor}, {key_ctor});",
        )
        .unwrap();
    }

    for (index, spec) in plan.counters.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let key_ctor = row_eq_ctor(&format!("Cnt{index}Key"), &spec.eq, "__row");
        writeln!(
            out,
            "        *self.cnt{index}.entry({key_ctor}).or_insert(0) += 1;"
        )
        .unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    fn __ls_remove_{}(&mut self, __row: &{}) {{",
        table.name, row
    )
    .unwrap();

    for (index, spec) in plan.indexes.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let inner_fields = index_inner_fields(spec, table)?;
        let eq_ctor = row_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, "__row");
        let key_ctor = row_key_ctor(&format!("Idx{index}Key"), &inner_fields, "__row");
        writeln!(out, "        {{").unwrap();
        writeln!(out, "            let __ls_eq = {eq_ctor};").unwrap();
        writeln!(out, "            let __ls_key = {key_ctor};").unwrap();
        writeln!(
            out,
            "            self.idx{index}.write().unwrap().remove(&__ls_eq, &__ls_key);"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }

    for (index, spec) in plan.counters.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let key_ctor = row_eq_ctor(&format!("Cnt{index}Key"), &spec.eq, "__row");
        writeln!(out, "        {{").unwrap();
        writeln!(out, "            let __ls_key = {key_ctor};").unwrap();
        writeln!(
            out,
            "            if let Some(__count) = self.cnt{index}.get_mut(&__ls_key) {{"
        )
        .unwrap();
        writeln!(out, "                *__count = __count.saturating_sub(1);").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            if self.cnt{index}.get(&__ls_key).map_or(false, |count| *count == 0) {{"
        )
        .unwrap();
        writeln!(out, "                self.cnt{index}.remove(&__ls_key);").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    let _ = workload;
    Ok(())
}

pub(crate) fn emit_transaction_support(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
) -> Result<(), String> {
    writeln!(
        out,
        "    fn __ls_clone_committed(&self) -> io::Result<Db> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_ready()?;").unwrap();
    writeln!(out, "        let mut __ls_clone = Db::__ls_new_spilled()?;").unwrap();
    writeln!(out, "        __ls_clone.__ls_in_transaction = true;").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(
            out,
            "        __ls_clone.t_{}.__ls_clone_from(&self.t_{})?;",
            table.name, table.name
        )
        .unwrap();
    }
    for (index, _) in plan.indexes.iter().enumerate() {
        writeln!(
            out,
            "        *__ls_clone.idx{index}.write().unwrap() = self.idx{index}.read().unwrap().clone();"
        )
        .unwrap();
    }
    for (index, _) in plan.counters.iter().enumerate() {
        writeln!(
            out,
            "        __ls_clone.cnt{index} = self.cnt{index}.clone();"
        )
        .unwrap();
    }
    writeln!(
        out,
        "        __ls_clone.__ls_versions = self.__ls_versions.clone();"
    )
    .unwrap();
    writeln!(out, "        __ls_clone.__ls_lsn = self.__ls_lsn;").unwrap();
    writeln!(
        out,
        "        __ls_clone.__ls_max_rows = self.__ls_max_rows;"
    )
    .unwrap();
    writeln!(
        out,
        "        __ls_clone.__ls_max_wal_bytes = self.__ls_max_wal_bytes;"
    )
    .unwrap();
    writeln!(out, "        Ok(__ls_clone)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    fn __ls_row_count(&self) -> u64 {{").unwrap();
    write!(out, "        0u64").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        write!(out, " + self.t_{}.len() as u64", table.name).unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    fn __ls_take_effects(&mut self) -> io::Result<Vec<u8>> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_ready()?;").unwrap();
    writeln!(out, "        Ok(std::mem::take(&mut self.__ls_effects))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    fn __ls_commit_effects(&mut self, __ls_effects: &[u8]) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(out, "        if __ls_effects.is_empty() {{").unwrap();
    writeln!(out, "            return Ok(());").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        if let Some(message) = self.__ls_wal_quota_error(__ls_effects.len()) {{"
    )
    .unwrap();
    writeln!(out, "            return Err(io::Error::other(message));").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        if let Err(error) = self.__ls_write_batch(__ls_effects) {{"
    )
    .unwrap();
    writeln!(
        out,
        "            self.__ls_broken = Some(error.to_string());"
    )
    .unwrap();
    writeln!(out, "            return Err(error);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        let mut __ls_reader = __LsReader::new(__ls_effects);"
    )
    .unwrap();
    writeln!(out, "        while !__ls_reader.is_empty() {{").unwrap();
    writeln!(
        out,
        "            if let Err(error) = self.__ls_replay_effect(&mut __ls_reader) {{"
    )
    .unwrap();
    writeln!(
        out,
        "                self.__ls_broken = Some(error.to_string());"
    )
    .unwrap();
    writeln!(out, "                return Err(error);").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
    writeln!(out, "        Ok(())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_commit_wal(&mut self, __ls_effects: &[u8]) -> io::Result<u64> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(out, "        if __ls_effects.is_empty() {{").unwrap();
    writeln!(out, "            return Ok(self.__ls_lsn);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        if let Some(message) = self.__ls_wal_quota_error(__ls_effects.len()) {{"
    )
    .unwrap();
    writeln!(out, "            return Err(io::Error::other(message));").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        if let Err(error) = self.__ls_write_batch(__ls_effects) {{"
    )
    .unwrap();
    writeln!(
        out,
        "            self.__ls_broken = Some(error.to_string());"
    )
    .unwrap();
    writeln!(out, "            return Err(error);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
    writeln!(out, "        Ok(self.__ls_lsn)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_clone(&self) -> io::Result<Db> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_clone_committed()").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_take(&mut self) -> io::Result<Vec<u8>> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_take_effects()").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    #[doc(hidden)] pub fn __ls_shard_commit(&mut self, __ls_effects: &[u8]) -> io::Result<()> {{").unwrap();
    writeln!(out, "        self.__ls_commit_effects(__ls_effects)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_reload(&mut self) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_reload()").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_row_count(&self) -> u64 {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_row_count()").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    #[doc(hidden)] pub fn __ls_shard_check_row_limit(&self, __ls_rows: u64) -> io::Result<()> {{").unwrap();
    writeln!(out, "        self.__ls_check_row_limit(__ls_rows)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_broken(&self) -> Option<String> {{"
    )
    .unwrap();
    writeln!(
        out,
        "        self.__ls_broken_message().map(|__ls_msg| __ls_msg.to_string())"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_lsn(&self) -> u64 {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_lsn").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_versions(&self) -> Vec<u64> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_versions.clone()").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    #[doc(hidden)] pub fn __ls_shard_bump(&mut self, __ls_tables: &[usize]) {{"
    )
    .unwrap();
    writeln!(out, "        for &__ls_table in __ls_tables {{ if let Some(__ls_version) = self.__ls_versions.get_mut(__ls_table) {{ *__ls_version = __ls_version.wrapping_add(1); }} }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}
