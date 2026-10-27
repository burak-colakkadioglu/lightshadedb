use super::replay::effect_delete_call;
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_ttl(out: &mut String, workload: &Workload) -> Result<(), String> {
    let ttl_tables: Vec<&Table> = workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
        .filter(|table| table.ttl_secs.is_some())
        .collect();
    if ttl_tables.is_empty() {
        writeln!(
            out,
            "    fn __ls_purge_expired(&mut self) -> io::Result<u64> {{"
        )
        .unwrap();
        writeln!(out, "        Ok(0)").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "    pub fn purge_expired(&mut self) -> io::Result<u64> {{"
        )
        .unwrap();
        writeln!(out, "        Ok(0)").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
        return Ok(());
    }
    writeln!(
        out,
        "    fn __ls_expire_table(&mut self, __ls_now: u64) -> io::Result<u64> {{"
    )
    .unwrap();
    writeln!(out, "        let mut __ls_total = 0u64;").unwrap();
    for table in &ttl_tables {
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        writeln!(out, "        {{").unwrap();
        writeln!(
            out,
            "            let __ls_keys = self.t_{}.__ls_expired_keys(__ls_now);",
            table.name
        )
        .unwrap();
        writeln!(out, "            for __ls_key in __ls_keys {{").unwrap();
        writeln!(
            out,
            "                if let Some(__ls_old) = self.t_{}.remove(&__ls_key) {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "                    self.__ls_remove_{}(&__ls_old);",
            table.name
        )
        .unwrap();
        let call = effect_delete_call(table, "__ls_key");
        let call = call.replace("__ls_key", "&__ls_key");
        let call = if primary_key.ty == Ty::Text {
            format!(
                "self.__ls_effect_delete({:?}, __LsValue::Text(__ls_key.as_str()));",
                table.name
            )
        } else if primary_key.ty == Ty::Bytes {
            format!(
                "self.__ls_effect_delete({:?}, __LsValue::Bytes(&__ls_key));",
                table.name
            )
        } else if primary_key.ty == Ty::Uuid {
            format!(
                "self.__ls_effect_delete({:?}, __LsValue::Uuid(&__ls_key));",
                table.name
            )
        } else {
            let _ = &call;
            match primary_key.ty {
                Ty::I8 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::I8(__ls_key));",
                    table.name
                ),
                Ty::I16 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::I16(__ls_key));",
                    table.name
                ),
                Ty::I32 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::I32(__ls_key));",
                    table.name
                ),
                Ty::I64 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::I64(__ls_key));",
                    table.name
                ),
                Ty::U8 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::U8(__ls_key));",
                    table.name
                ),
                Ty::U16 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::U16(__ls_key));",
                    table.name
                ),
                Ty::U32 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::U32(__ls_key));",
                    table.name
                ),
                Ty::U64 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::U64(__ls_key));",
                    table.name
                ),
                Ty::F32 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::F32(__ls_key));",
                    table.name
                ),
                Ty::F64 => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::F64(__ls_key));",
                    table.name
                ),
                Ty::Bool => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::Bool(__ls_key));",
                    table.name
                ),
                _ => format!(
                    "self.__ls_effect_delete({:?}, __LsValue::I64(__ls_key as i64));",
                    table.name
                ),
            }
        };
        writeln!(out, "                    {call}").unwrap();
        writeln!(out, "                    __ls_total += 1;").unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        if __ls_total > 0 {{").unwrap();
    writeln!(out, "            self.__ls_finish_effects()?;").unwrap();
    writeln!(out, "            self.__ls_maybe_checkpoint();").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Ok(__ls_total)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    fn __ls_purge_expired(&mut self) -> io::Result<u64> {{"
    )
    .unwrap();
    writeln!(out, "        if self.__ls_in_transaction {{").unwrap();
    writeln!(out, "            return Ok(0);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        if self.__ls_read_only {{").unwrap();
    writeln!(out, "            return Ok(0);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        if let Err(error) = self.__ls_ready() {{").unwrap();
    writeln!(out, "            let _ = error;").unwrap();
    writeln!(out, "            return Ok(0);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        let __ls_now = __ls_now_millis();").unwrap();
    writeln!(out, "        self.__ls_expire_table(__ls_now)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    pub fn purge_expired(&mut self) -> io::Result<u64> {{"
    )
    .unwrap();
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(out, "        if self.__ls_in_transaction {{").unwrap();
    writeln!(out, "            return Err(io::Error::new(io::ErrorKind::InvalidInput, \"cannot purge expired rows during a transaction\"));").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        let __ls_now = __ls_now_millis();").unwrap();
    writeln!(out, "        self.__ls_expire_table(__ls_now)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}
