use super::common::{
    emit_expr, emit_slow_sample, lookup_operand, operand_value, param_rust, row_type,
};
use super::persist::{effect_delete_call, effect_upsert_call, row_eq_ctor, row_key_ctor};
use super::plan::{get_table, index_inner_fields, pk_eq_operand, KeyField, Plan};
use crate::ir::*;
use std::collections::HashSet;
use std::fmt::Write;

pub(crate) fn emit_bulk_insert(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    table: &Table,
) -> Result<(), String> {
    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
    let row = row_type(&table.name);

    let mut table_indexes: Vec<(usize, Vec<KeyField>)> = Vec::new();
    for (index, spec) in plan.indexes.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        table_indexes.push((index, index_inner_fields(spec, table)?));
    }
    let mut table_counters: Vec<usize> = Vec::new();
    for (index, spec) in plan.counters.iter().enumerate() {
        if spec.table == table.name {
            table_counters.push(index);
        }
    }

    writeln!(
        out,
        "    pub(crate) fn __ls_bulk_insert_{}(&mut self, __ls_rows: Vec<{}>) -> io::Result<u64> {{",
        table.name, row
    )
    .unwrap();
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(out, "        let __ls_total = __ls_rows.len() as u64;").unwrap();
    writeln!(out, "        if __ls_total == 0 {{ return Ok(0); }}").unwrap();
    writeln!(
        out,
        "        self.__ls_check_row_limit(self.__ls_row_count().saturating_add(__ls_total))?;"
    )
    .unwrap();
    writeln!(out, "        for __ls_row in &__ls_rows {{").unwrap();
    writeln!(
        out,
        "            if self.t_{}.contains_key(&__ls_row.{}) {{ return Ok(0); }}",
        table.name, primary_key.name
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        self.__ls_effects.reserve(__ls_rows.len().saturating_mul(64));"
    )
    .unwrap();
    for (index, _) in &table_indexes {
        writeln!(
            out,
            "        let mut __ls_pairs_{index}: Vec<(Idx{index}Eq, Idx{index}Key)> = Vec::with_capacity(__ls_rows.len());"
        )
        .unwrap();
    }
    writeln!(out, "        let mut __ls_inserted = 0u64;").unwrap();
    writeln!(out, "        for mut __ls_row in __ls_rows {{").unwrap();
    if let Some(ttl) = table.ttl_secs {
        writeln!(out, "            if __ls_row.expires_at == 0 {{").unwrap();
        writeln!(out, "                __ls_row.expires_at = (__ls_now_millis().saturating_add({}u64.saturating_mul(1000))) as i64;", ttl).unwrap();
        writeln!(out, "            }}").unwrap();
    }
    writeln!(out, "            {}", effect_upsert_call(table, "__ls_row")).unwrap();
    for (index, inner) in &table_indexes {
        let spec = &plan.indexes[*index];
        let eq = row_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, "__ls_row");
        let key = row_key_ctor(&format!("Idx{index}Key"), inner, "__ls_row");
        writeln!(out, "            __ls_pairs_{index}.push(({eq}, {key}));").unwrap();
    }
    for index in &table_counters {
        let key = row_eq_ctor(
            &format!("Cnt{index}Key"),
            &plan.counters[*index].eq,
            "__ls_row",
        );
        writeln!(
            out,
            "            *self.cnt{index}.entry({key}).or_insert(0) += 1;"
        )
        .unwrap();
    }
    writeln!(
        out,
        "            self.t_{}.__ls_insert(__ls_row.{}.clone(), __ls_row);",
        table.name, primary_key.name
    )
    .unwrap();
    writeln!(out, "            __ls_inserted += 1;").unwrap();
    writeln!(out, "        }}").unwrap();
    for (index, _) in &table_indexes {
        writeln!(out, "        {{").unwrap();
        writeln!(
            out,
            "            let mut __ls_group = self.idx{index}.write().unwrap();"
        )
        .unwrap();
        writeln!(
            out,
            "            for (__ls_eq, __ls_key) in __ls_pairs_{index} {{ __ls_group.insert(__ls_eq, __ls_key); }}"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        Ok(__ls_inserted)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    let _ = workload;
    Ok(())
}

pub(crate) fn emit_insert(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Insert { table } = &operation.body else {
        return Err(format!("operation `{}` is not an insert", operation.name));
    };
    let table = get_table(workload, table)?;
    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        out,
        "    pub fn {}(&mut self, {}) -> io::Result<bool> {{",
        operation.name, params
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(
        out,
        "        if self.t_{}.contains_key(&{}) {{",
        table.name, primary_key.name
    )
    .unwrap();
    writeln!(out, "            __ls_slow.__ls_set_rows(0);").unwrap();
    writeln!(out, "            return Ok(false);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        self.__ls_check_row_quota()?;").unwrap();
    if table.ttl_secs.is_some() {
        writeln!(out, "        let mut __row = {} {{", row_type(&table.name)).unwrap();
    } else {
        writeln!(out, "        let __row = {} {{", row_type(&table.name)).unwrap();
    }
    for field in &table.fields {
        writeln!(out, "            {}: {},", field.name, field.name).unwrap();
    }
    writeln!(out, "        }};").unwrap();
    if let Some(ttl) = table.ttl_secs {
        writeln!(out, "        if __row.expires_at == 0 {{").unwrap();
        writeln!(out, "            __row.expires_at = (__ls_now_millis().saturating_add({}u64.saturating_mul(1000))) as i64;", ttl).unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        {}", effect_upsert_call(table, "__row")).unwrap();
    writeln!(out, "        self.__ls_finish_effects()?;").unwrap();
    writeln!(out, "        self.__ls_insert_{}(&__row);", table.name).unwrap();
    writeln!(
        out,
        "        self.t_{}.__ls_insert(__row.{}.clone(), __row);",
        table.name, primary_key.name
    )
    .unwrap();
    writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
    writeln!(out, "        __ls_slow.__ls_set_rows(1);").unwrap();
    writeln!(out, "        Ok(true)").unwrap();
    writeln!(out, "    }}").unwrap();
    Ok(())
}

pub(crate) fn emit_update(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Update {
        table,
        sets,
        filter,
    } = &operation.body
    else {
        return Err(format!("operation `{}` is not an update", operation.name));
    };
    let table = get_table(workload, table)?;
    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");
    let args = operation
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let changed = sets
        .iter()
        .map(|(field, _)| field.as_str())
        .collect::<HashSet<_>>();

    let mut affected_indexes = Vec::new();
    for (index, spec) in plan.indexes.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let inner = index_inner_fields(spec, table)?;
        let affected = spec.eq.iter().any(|field| changed.contains(field.as_str()))
            || inner
                .iter()
                .any(|field| changed.contains(field.name.as_str()));
        if affected {
            affected_indexes.push((index, inner));
        }
    }

    let mut affected_counters = Vec::new();
    for (index, spec) in plan.counters.iter().enumerate() {
        if spec.table == table.name && spec.eq.iter().any(|field| changed.contains(field.as_str()))
        {
            affected_counters.push(index);
        }
    }

    writeln!(
        out,
        "    fn __ls_update_{}(&mut self, {}) -> u64 {{",
        operation.name, params
    )
    .unwrap();

    if let Some(pk_operand) = pk_eq_operand(filter, table) {
        let pk_key = lookup_operand(
            &pk_operand,
            table
                .primary_key()
                .ok_or_else(|| format!("table `{}` has no primary key", table.name))?
                .ty,
        )?;
        writeln!(out, "        let __ls_pk = {pk_key};").unwrap();
        writeln!(
            out,
            "        let Some(mut __ls_row) = self.t_{}.remove(&__ls_pk) else {{ return 0; }};",
            table.name
        )
        .unwrap();
        emit_row_update(
            out,
            "        ",
            table,
            primary_key,
            sets,
            &affected_indexes,
            &affected_counters,
            plan,
            &["return 1;"],
            &operation.params,
        )?;
        writeln!(out, "        1").unwrap();
    } else {
        let predicate = match filter {
            Some(filter) => emit_expr(filter, "__row", table, &operation.params)?,
            None => "true".to_string(),
        };
        writeln!(out, "        let mut __ls_affected = 0u64;").unwrap();
        writeln!(
            out,
            "        let mut __ls_keys: Vec<{}> = Vec::new();",
            primary_key.ty.rust()
        )
        .unwrap();
        writeln!(
            out,
            "        for __row in self.t_{}.values() {{",
            table.name
        )
        .unwrap();
        writeln!(out, "            if {predicate} {{").unwrap();
        writeln!(
            out,
            "                __ls_keys.push(__row.{}.clone());",
            primary_key.name
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        for __ls_key in __ls_keys {{").unwrap();
        writeln!(
            out,
            "            let Some(mut __ls_row) = self.t_{}.remove(&__ls_key) else {{ continue; }};",
            table.name
        )
        .unwrap();
        emit_row_update(
            out,
            "            ",
            table,
            primary_key,
            sets,
            &affected_indexes,
            &affected_counters,
            plan,
            &["__ls_affected += 1;", "continue;"],
            &operation.params,
        )?;
        writeln!(out, "            __ls_affected += 1;").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        __ls_affected").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    pub fn {}(&mut self, {}) -> io::Result<u64> {{",
        operation.name, params
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;
    writeln!(out, "        self.__ls_writable()?;").unwrap();
    writeln!(
        out,
        "        let __ls_count = self.__ls_update_{}({});",
        operation.name, args
    )
    .unwrap();
    writeln!(out, "        self.__ls_finish_effects()?;").unwrap();
    writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
    writeln!(out, "        __ls_slow.__ls_set_rows(__ls_count);").unwrap();
    writeln!(out, "        Ok(__ls_count)").unwrap();
    writeln!(out, "    }}").unwrap();
    Ok(())
}

pub(crate) fn emit_delete(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Delete { table, filter } = &operation.body else {
        return Err(format!("operation `{}` is not a delete", operation.name));
    };
    let table = get_table(workload, table)?;
    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        out,
        "    pub fn {}(&mut self, {}) -> io::Result<u64> {{",
        operation.name, params
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;
    writeln!(out, "        self.__ls_writable()?;").unwrap();

    if let Some(pk_operand) = pk_eq_operand(filter, table) {
        let pk_key = lookup_operand(
            &pk_operand,
            table
                .primary_key()
                .ok_or_else(|| format!("table `{}` has no primary key", table.name))?
                .ty,
        )?;
        writeln!(out, "        let __ls_pk = {pk_key};").unwrap();
        writeln!(
            out,
            "        if !self.t_{}.contains_key(&__ls_pk) {{",
            table.name
        )
        .unwrap();
        writeln!(out, "            __ls_slow.__ls_set_rows(0);").unwrap();
        writeln!(out, "            return Ok(0);").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        {}", effect_delete_call(table, "__ls_pk")).unwrap();
        writeln!(out, "        self.__ls_finish_effects()?;").unwrap();
        writeln!(
            out,
            "        let Some(__ls_row) = self.t_{}.remove(&__ls_pk) else {{",
            table.name
        )
        .unwrap();
        writeln!(out, "            __ls_slow.__ls_set_rows(0);").unwrap();
        writeln!(out, "            return Ok(0);").unwrap();
        writeln!(out, "        }};").unwrap();
        writeln!(out, "        self.__ls_remove_{}(&__ls_row);", table.name).unwrap();
        writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
        writeln!(out, "        __ls_slow.__ls_set_rows(1);").unwrap();
        writeln!(out, "        Ok(1)").unwrap();
    } else {
        let predicate = match filter {
            Some(filter) => emit_expr(filter, "__row", table, &operation.params)?,
            None => "true".to_string(),
        };
        writeln!(
            out,
            "        let mut __ls_keys: Vec<{}> = Vec::new();",
            primary_key.ty.rust()
        )
        .unwrap();
        writeln!(
            out,
            "        for __row in self.t_{}.values() {{",
            table.name
        )
        .unwrap();
        writeln!(out, "            if {predicate} {{").unwrap();
        writeln!(
            out,
            "                __ls_keys.push(__row.{}.clone());",
            primary_key.name
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        for __ls_key in &__ls_keys {{").unwrap();
        writeln!(
            out,
            "            if let Some(__ls_row) = self.t_{}.get(__ls_key) {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "                {}",
            effect_delete_call(table, &format!("__ls_row.{}", primary_key.name))
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        self.__ls_finish_effects()?;").unwrap();
        writeln!(out, "        let mut __ls_affected = 0u64;").unwrap();
        writeln!(out, "        for __ls_key in __ls_keys {{").unwrap();
        writeln!(
            out,
            "            let Some(__ls_row) = self.t_{}.remove(&__ls_key) else {{ continue; }};",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "            self.__ls_remove_{}(&__ls_row);",
            table.name
        )
        .unwrap();
        writeln!(out, "            __ls_affected += 1;").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        self.__ls_maybe_checkpoint();").unwrap();
        writeln!(out, "        __ls_slow.__ls_set_rows(__ls_affected);").unwrap();
        writeln!(out, "        Ok(__ls_affected)").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_row_update(
    out: &mut String,
    indent: &str,
    table: &Table,
    primary_key: &Field,
    sets: &[(String, Operand)],
    affected_indexes: &[(usize, Vec<KeyField>)],
    affected_counters: &[usize],
    plan: &Plan,
    noop_action: &[&str],
    params: &[Param],
) -> Result<(), String> {
    for (position, (field, operand)) in sets.iter().enumerate() {
        let hint = table.field(field).map(|(_, field)| field.ty);
        let value = operand_value(operand, "__ls_row", hint)?;
        let target_nullable = table
            .field(field)
            .map(|(_, field)| field.nullable)
            .unwrap_or(false);
        let yields_option = match operand {
            Operand::Literal(Value::Null(_)) => true,
            Operand::Literal(_) => false,
            Operand::Param(name) => params
                .iter()
                .find(|param| &param.name == name)
                .is_some_and(|param| param.nullable),
            Operand::Field(name) => table
                .field(name)
                .map(|(_, field)| field.nullable)
                .unwrap_or(false),
        };
        let value = if target_nullable && !yields_option {
            format!("Some({value})")
        } else {
            value
        };
        writeln!(out, "{indent}let __ls_set_{position} = {value};").unwrap();
    }

    if !sets.is_empty() {
        let unchanged = sets
            .iter()
            .enumerate()
            .map(|(position, (field, _))| format!("__ls_row.{field} == __ls_set_{position}"))
            .collect::<Vec<_>>()
            .join(" && ");
        writeln!(out, "{indent}if {unchanged} {{").unwrap();
        writeln!(
            out,
            "{indent}    self.t_{}.__ls_insert(__ls_row.{}.clone(), __ls_row);",
            table.name, primary_key.name
        )
        .unwrap();
        for action in noop_action {
            writeln!(out, "{indent}    {action}").unwrap();
        }
        writeln!(out, "{indent}}}").unwrap();
    }

    for (index, inner) in affected_indexes {
        let spec = &plan.indexes[*index];
        let eq = row_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, "__ls_row");
        let key = row_key_ctor(&format!("Idx{index}Key"), inner, "__ls_row");
        writeln!(out, "{indent}let __ls_idx{index}_old_eq = {eq};").unwrap();
        writeln!(out, "{indent}let __ls_idx{index}_old_key = {key};").unwrap();
    }
    for index in affected_counters {
        let key = row_eq_ctor(
            &format!("Cnt{index}Key"),
            &plan.counters[*index].eq,
            "__ls_row",
        );
        writeln!(out, "{indent}let __ls_cnt{index}_old_key = {key};").unwrap();
    }

    for (position, (field, _)) in sets.iter().enumerate() {
        writeln!(out, "{indent}__ls_row.{field} = __ls_set_{position};").unwrap();
    }
    writeln!(out, "{indent}{}", effect_upsert_call(table, "__ls_row")).unwrap();

    for (index, inner) in affected_indexes {
        let spec = &plan.indexes[*index];
        let eq = row_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, "__ls_row");
        let key = row_key_ctor(&format!("Idx{index}Key"), inner, "__ls_row");
        writeln!(out, "{indent}{{").unwrap();
        writeln!(out, "{indent}    let __ls_new_eq = {eq};").unwrap();
        writeln!(out, "{indent}    let __ls_new_key = {key};").unwrap();
        writeln!(
            out,
            "{indent}    if __ls_idx{index}_old_eq != __ls_new_eq || __ls_idx{index}_old_key != __ls_new_key {{"
        )
        .unwrap();
        writeln!(
            out,
            "{indent}        self.idx{index}.write().unwrap().remove(&__ls_idx{index}_old_eq, &__ls_idx{index}_old_key);"
        )
        .unwrap();
        writeln!(
            out,
            "{indent}        self.idx{index}.write().unwrap().insert(__ls_new_eq, __ls_new_key);",
        )
        .unwrap();
        writeln!(out, "{indent}    }}").unwrap();
        writeln!(out, "{indent}}}").unwrap();
    }

    for index in affected_counters {
        let key = row_eq_ctor(
            &format!("Cnt{index}Key"),
            &plan.counters[*index].eq,
            "__ls_row",
        );
        writeln!(out, "{indent}{{").unwrap();
        writeln!(out, "{indent}    let __ls_cnt{index}_new_key = {key};").unwrap();
        writeln!(
            out,
            "{indent}    if __ls_cnt{index}_old_key != __ls_cnt{index}_new_key {{"
        )
        .unwrap();
        writeln!(
            out,
            "{indent}        if let Some(__ls_count) = self.cnt{index}.get_mut(&__ls_cnt{index}_old_key) {{"
        )
        .unwrap();
        writeln!(
            out,
            "{indent}            *__ls_count = __ls_count.saturating_sub(1);"
        )
        .unwrap();
        writeln!(out, "{indent}        }}").unwrap();
        writeln!(
            out,
            "{indent}        if self.cnt{index}.get(&__ls_cnt{index}_old_key).map_or(false, |__ls_count| *__ls_count == 0) {{"
        )
        .unwrap();
        writeln!(
            out,
            "{indent}            self.cnt{index}.remove(&__ls_cnt{index}_old_key);"
        )
        .unwrap();
        writeln!(out, "{indent}        }}").unwrap();
        writeln!(
            out,
            "{indent}        *self.cnt{index}.entry(__ls_cnt{index}_new_key).or_insert(0) += 1;"
        )
        .unwrap();
        writeln!(out, "{indent}    }}").unwrap();
        writeln!(out, "{indent}}}").unwrap();
    }

    writeln!(
        out,
        "{indent}self.t_{}.__ls_insert(__ls_row.{}.clone(), __ls_row);",
        table.name, primary_key.name
    )
    .unwrap();
    Ok(())
}
