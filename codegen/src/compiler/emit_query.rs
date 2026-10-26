use super::common::{
    emit_expr, emit_field_read, emit_nullable_cmp, emit_slow_sample, group_key_rust,
    literal_with_ty, lookup_eq_ctor, lookup_operand, param_rust, row_type,
};
use super::persist::index_row_expression;
use super::plan::advanced_filter_for;
use super::plan::{
    advanced_eq_constraints, eq_constraints, get_table, index_eq_constraints, index_inner_fields,
    index_start_key, pk_eq_operand, pk_join_key, pk_join_key_ty, primary_key_start_key,
    resolve_source, Plan,
};
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_advanced_loops(
    out: &mut String,
    plan: &Plan,
    operation: &Operation,
    sources: &[(&Table, String)],
    joins: &[Join],
    row_closure: bool,
    pre_join_predicate: Option<&str>,
) -> Result<(), String> {
    if row_closure {
        for depth in 1..sources.len() {
            let join = &joins[depth - 1];
            if let Some(key_ty) = pk_join_key_ty(join, depth, sources)? {
                let joined = &sources[depth].0;
                writeln!(
                    out,
                    "        let mut __ls_join_{depth}: std::collections::HashMap<{}, {}> = std::collections::HashMap::new();",
                    key_ty.rust(),
                    row_type(&joined.name)
                )
                .unwrap();
            }
        }
    }
    for (depth, (table, _)) in sources.iter().enumerate() {
        if depth == 0 {
            if let Some(index) = plan.advanced_primary.get(&operation.name) {
                let spec = &plan.indexes[*index];
                let constraints =
                    advanced_eq_constraints(advanced_filter_for(operation), 0, sources)?
                        .ok_or_else(|| {
                            format!(
                                "internal: operation `{}` lost its primary constraints",
                                operation.name
                            )
                        })?;
                let key = lookup_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, &constraints, table)?;
                let inner = index_inner_fields(spec, table)?;
                let row = index_row_expression(table, "__ls_key", &inner)?;
                writeln!(out, "        let __ls_idx_key = {key};").unwrap();
                writeln!(
                    out,
                    "        let mut __ls_idx = self.idx{index}.write().unwrap();"
                )
                .unwrap();
                writeln!(out, "        __ls_idx.prepare(&__ls_idx_key);").unwrap();
                writeln!(
                    out,
                    "        for __ls_row_0 in __ls_idx.get(&__ls_idx_key).into_iter().flat_map(|__ls_inner| __ls_inner.values()).filter_map(|__ls_key| {row}) {{"
                )
                .unwrap();
            } else if row_closure {
                writeln!(
                    out,
                    "        self.t_{}.for_each_value(|__ls_row_0| {{",
                    table.name
                )
                .unwrap();
                if let Some(predicate) = pre_join_predicate {
                    writeln!(out, "            if !({predicate}) {{ return; }}").unwrap();
                }
            } else {
                writeln!(
                    out,
                    "        for __ls_row_0 in self.t_{}.values() {{",
                    table.name
                )
                .unwrap();
            }
            continue;
        }
        let join = &joins[depth - 1];
        if let Some(key) = pk_join_key(join, depth, sources)? {
            if row_closure {
                writeln!(out, "        let __ls_join_key_{depth} = {key};").unwrap();
                writeln!(
                    out,
                    "        if !__ls_join_{depth}.contains_key(&__ls_join_key_{depth}) {{"
                )
                .unwrap();
                writeln!(
                    out,
                    "            if let Some(__ls_join_row) = self.t_{}.get(&__ls_join_key_{depth}) {{",
                    table.name
                )
                .unwrap();
                writeln!(
                    out,
                    "                __ls_join_{depth}.insert(__ls_join_key_{depth}.clone(), __ls_join_row);"
                )
                .unwrap();
                writeln!(out, "            }}").unwrap();
                writeln!(out, "        }}").unwrap();
                writeln!(
                    out,
                    "        if let Some(__ls_row_{depth}) = __ls_join_{depth}.get(&__ls_join_key_{depth}) {{"
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "        if let Some(__ls_row_{depth}) = self.t_{}.get(&{key}) {{",
                    table.name
                )
                .unwrap();
            }
            continue;
        }
        if let Some(index) = plan.advanced_join.get(&(operation.name.clone(), depth)) {
            let spec = &plan.indexes[*index];
            let inner = index_inner_fields(spec, table)?;
            let row = index_row_expression(table, "__ls_key", &inner)?;
            let (left_index, _, _) = resolve_source(&join.left, sources)?;
            let (right_index, _, _) = resolve_source(&join.right, sources)?;
            let outer = if left_index == depth {
                &join.right
            } else {
                let _ = right_index;
                &join.left
            };
            let (_, _, field) = resolve_source(outer, &sources[..=depth])?;
            let read = emit_field_read(outer, &sources[..=depth], false)?;
            let read = if field.ty == Ty::Text {
                format!("{read}.clone()")
            } else {
                read
            };
            writeln!(
                out,
                "        let __ls_join_key_{depth} = Idx{index}Eq {{ e0: {read} }};"
            )
            .unwrap();
            writeln!(
                out,
                "        let mut __ls_idx_{depth} = self.idx{index}.write().unwrap();"
            )
            .unwrap();
            writeln!(
                out,
                "        __ls_idx_{depth}.prepare(&__ls_join_key_{depth});"
            )
            .unwrap();
            writeln!(
                out,
                "        for __ls_row_{depth} in __ls_idx_{depth}.get(&__ls_join_key_{depth}).into_iter().flat_map(|__ls_inner| __ls_inner.values()).filter_map(|__ls_key| {row}) {{"
            )
            .unwrap();
            continue;
        }
        writeln!(
            out,
            "        for __ls_row_{depth} in self.t_{}.values() {{",
            table.name
        )
        .unwrap();
        let left = emit_field_read(&join.left, &sources[..=depth], false)?;
        let right = emit_field_read(&join.right, &sources[..=depth], false)?;
        writeln!(out, "            if !({left} == {right}) {{ continue; }}").unwrap();
    }
    Ok(())
}

pub(crate) fn emit_advanced_query(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Query {
        table,
        alias,
        joins,
        select,
        group_by,
        filter,
        order,
        limit,
        offset,
        count: false,
    } = &operation.body
    else {
        return Err(format!(
            "operation `{}` is not an advanced query",
            operation.name
        ));
    };
    let primary = get_table(workload, table)?;
    let returned = match &operation.returns {
        Returns::Many(returned) => returned,
        _ => {
            return Err(format!(
                "operation `{}` must return `many <table>`",
                operation.name
            ))
        }
    };
    let output = get_table(workload, returned)?;

    let mut sources: Vec<(&Table, String)> = vec![(
        primary,
        alias.clone().unwrap_or_else(|| primary.name.clone()),
    )];
    for join in joins {
        let joined = get_table(workload, &join.table)?;
        sources.push((
            joined,
            join.alias.clone().unwrap_or_else(|| joined.name.clone()),
        ));
    }

    let mut items: Vec<&SelectItem> = Vec::new();
    for field in &output.fields {
        let item = select
            .iter()
            .find(|item| item.alias == field.name)
            .ok_or_else(|| {
                format!(
                    "operation `{}`: select is missing output field `{}`",
                    operation.name, field.name
                )
            })?;
        items.push(item);
    }
    let grouped = !group_by.is_empty()
        || items
            .iter()
            .any(|item| matches!(item.expr, SelectExpr::Aggregate { .. }));

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        out,
        "    pub fn {}(&self, {}) -> Vec<{}> {{",
        operation.name,
        params,
        row_type(&output.name)
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;

    match limit {
        Some(Limit::Literal(value)) => {
            writeln!(out, "        let __ls_take = {value}usize;").unwrap();
        }
        Some(Limit::Param(name)) => {
            writeln!(
                out,
                "        let __ls_take = if {name} < 0 {{ 0usize }} else {{ {name} as usize }};"
            )
            .unwrap();
        }
        None => {
            writeln!(out, "        let __ls_take = usize::MAX;").unwrap();
        }
    }
    match offset {
        Some(Limit::Literal(value)) => {
            writeln!(out, "        let __ls_skip = {value}usize;").unwrap();
        }
        Some(Limit::Param(name)) => {
            writeln!(
                out,
                "        let __ls_skip = if {name} < 0 {{ 0usize }} else {{ {name} as usize }};"
            )
            .unwrap();
        }
        None => {
            writeln!(out, "        let __ls_skip = 0usize;").unwrap();
        }
    }

    if grouped {
        let key_type = if group_by.is_empty() {
            "()".to_string()
        } else {
            let mut parts = Vec::new();
            for reference in group_by {
                let (_, _, field) = resolve_source(reference, &sources)?;
                if field.nullable {
                    parts.push(format!("Option<{}>", group_key_rust(field.ty)));
                } else {
                    parts.push(group_key_rust(field.ty));
                }
            }
            format!("({})", parts.join(", "))
        };
        writeln!(out, "        type __LsKey = {key_type};").unwrap();
        writeln!(out, "        struct __LsAcc {{").unwrap();
        for (index, item) in items.iter().enumerate() {
            writeln!(
                out,
                "            a{index}: {},",
                accumulator_type(item, &sources)?
            )
            .unwrap();
        }
        writeln!(out, "        }}").unwrap();
        let init = items
            .iter()
            .enumerate()
            .map(|(index, item)| format!("a{index}: {}", accumulator_init(item, &sources)))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            out,
            "        let mut __ls_groups: BTreeMap<__LsKey, __LsAcc> = BTreeMap::new();"
        )
        .unwrap();
        if group_by.is_empty() {
            writeln!(out, "        __ls_groups.insert((), __LsAcc {{ {init} }});").unwrap();
        }

        let order_independent = items.iter().all(|item| match &item.expr {
            SelectExpr::Aggregate { .. } => true,
            SelectExpr::Field(reference) => group_by.contains(reference),
        });
        let row_closure = order_independent
            && (!order.is_empty() || group_by.is_empty())
            && !plan.advanced_primary.contains_key(&operation.name)
            && joins.iter().all(|join| join.table != primary.name)
            && joins
                .iter()
                .enumerate()
                .all(|(index, join)| matches!(pk_join_key(join, index + 1, &sources), Ok(Some(_))));
        let pre_join_predicate = match filter {
            Some(filter) if row_closure && expr_within_sources(filter, 0, &sources)? => {
                Some(emit_expr_sources(filter, &sources[..1], &operation.params)?)
            }
            _ => None,
        };
        emit_advanced_loops(
            out,
            plan,
            operation,
            &sources,
            joins,
            row_closure,
            pre_join_predicate.as_deref(),
        )?;
        let predicate = match filter {
            _ if pre_join_predicate.is_some() => "true".to_string(),
            Some(filter) => emit_expr_sources(filter, &sources, &operation.params)?,
            None => "true".to_string(),
        };
        let skip = if row_closure { "return" } else { "continue" };
        writeln!(out, "            if !({predicate}) {{ {skip}; }}").unwrap();
        let borrowed_text_group = group_by.len() == 1
            && resolve_source(&group_by[0], &sources)
                .is_ok_and(|(_, _, field)| field.ty == Ty::Text && !field.nullable);
        if borrowed_text_group {
            let key_ref = emit_field_read(&group_by[0], &sources, false)?;
            writeln!(out, "            let __ls_key_ref = &{key_ref};").unwrap();
            writeln!(
                out,
                "            let __ls_acc = if let Some(__ls_acc) = __ls_groups.get_mut(__ls_key_ref.as_str()) {{"
            )
            .unwrap();
            writeln!(out, "                __ls_acc").unwrap();
            writeln!(out, "            }} else {{").unwrap();
            writeln!(
                out,
                "                let __ls_key = __ls_key_ref.to_string();"
            )
            .unwrap();
            writeln!(
                out,
                "                __ls_groups.entry(__ls_key).or_insert_with(|| __LsAcc {{ {init} }})"
            )
            .unwrap();
            writeln!(out, "            }};").unwrap();
        } else {
            let key = group_by
                .iter()
                .map(|reference| emit_group_key_read(reference, &sources))
                .collect::<Result<Vec<_>, String>>()?
                .join(", ");
            let key = if group_by.is_empty() {
                "()".to_string()
            } else {
                format!("({key})")
            };
            writeln!(out, "            let __ls_key = {key};").unwrap();
            writeln!(
                out,
                "            let __ls_acc = __ls_groups.entry(__ls_key).or_insert_with(|| __LsAcc {{ {init} }});"
            )
            .unwrap();
        }
        for (index, item) in items.iter().enumerate() {
            match &item.expr {
                SelectExpr::Field(reference) => {
                    let value = emit_field_read(reference, &sources, true)?;
                    let (_, _, field) = resolve_source(reference, &sources)?;
                    let assigned = if field.nullable {
                        value.to_string()
                    } else {
                        format!("Some({value})")
                    };
                    writeln!(out, "            if __ls_acc.a{index}.is_none() {{").unwrap();
                    writeln!(out, "                __ls_acc.a{index} = {assigned};").unwrap();
                    writeln!(out, "            }}").unwrap();
                }
                SelectExpr::Aggregate { function, field } => match function {
                    Aggregate::Count => {
                        writeln!(
                            out,
                            "            __ls_acc.a{index} = __ls_acc.a{index}.saturating_add(1);"
                        )
                        .unwrap();
                    }
                    Aggregate::Sum | Aggregate::Avg => {
                        let reference = field.as_ref().ok_or_else(|| {
                            format!("operation `{}`: aggregate needs a field", operation.name)
                        })?;
                        let value = emit_field_read(reference, &sources, false)?;
                        let (_, _, source_field) = resolve_source(reference, &sources)?;
                        let field_ty = aggregate_field_ty(item, &sources)?;
                        let cast = if field_ty.is_float() { "f64" } else { "i128" };
                        writeln!(out, "            {{").unwrap();
                        writeln!(out, "                let __ls_value = {value};").unwrap();
                        if source_field.nullable {
                            writeln!(
                                out,
                                "                if let ::std::option::Option::Some(__ls_value) = __ls_value {{"
                            )
                            .unwrap();
                        }
                        writeln!(
                            out,
                            "                __ls_acc.a{index}.0 += __ls_value as {cast};"
                        )
                        .unwrap();
                        if *function == Aggregate::Sum {
                            writeln!(out, "                __ls_acc.a{index}.1 = true;").unwrap();
                        } else {
                            writeln!(
                                out,
                                "                __ls_acc.a{index}.1 = __ls_acc.a{index}.1.saturating_add(1);"
                            )
                            .unwrap();
                        }
                        if source_field.nullable {
                            writeln!(out, "                }}").unwrap();
                        }
                        writeln!(out, "            }}").unwrap();
                    }
                    Aggregate::Min | Aggregate::Max => {
                        let reference = field.as_ref().ok_or_else(|| {
                            format!("operation `{}`: aggregate needs a field", operation.name)
                        })?;
                        let value = emit_field_read(reference, &sources, true)?;
                        let (_, _, source_field) = resolve_source(reference, &sources)?;
                        let comparison = if *function == Aggregate::Min {
                            "<="
                        } else {
                            ">="
                        };
                        writeln!(out, "            {{").unwrap();
                        writeln!(out, "                let __ls_value = {value};").unwrap();
                        if source_field.nullable {
                            writeln!(
                                out,
                                "                if let ::std::option::Option::Some(__ls_value) = __ls_value {{"
                            )
                            .unwrap();
                        }
                        writeln!(out, "                match &__ls_acc.a{index} {{").unwrap();
                        writeln!(
                            out,
                            "                    Some(__ls_existing) if __ls_existing {comparison} &__ls_value => {{}}"
                        )
                        .unwrap();
                        writeln!(
                            out,
                            "                    _ => __ls_acc.a{index} = Some(__ls_value),"
                        )
                        .unwrap();
                        writeln!(out, "                }}").unwrap();
                        if source_field.nullable {
                            writeln!(out, "                }}").unwrap();
                        }
                        writeln!(out, "            }}").unwrap();
                    }
                },
            }
        }
        if row_closure {
            for _ in 1..sources.len() {
                writeln!(out, "        }}").unwrap();
            }
            writeln!(out, "        }});").unwrap();
        } else {
            for _ in sources.iter().rev() {
                writeln!(out, "        }}").unwrap();
            }
        }
        writeln!(
            out,
            "        let mut __ls_rows: Vec<{}> = Vec::new();",
            row_type(&output.name)
        )
        .unwrap();
        writeln!(out, "        for (_, __ls_acc) in __ls_groups {{").unwrap();
        writeln!(
            out,
            "            __ls_rows.push({} {{",
            row_type(&output.name)
        )
        .unwrap();
        for (index, item) in items.iter().enumerate() {
            let field = &output.fields[index];
            writeln!(
                out,
                "                {}: {},",
                field.name,
                accumulator_finish(index, item, field.ty, &sources)?
            )
            .unwrap();
        }
        writeln!(out, "            }});").unwrap();
        writeln!(out, "        }}").unwrap();
    } else {
        writeln!(
            out,
            "        let mut __ls_rows: Vec<{}> = Vec::new();",
            row_type(&output.name)
        )
        .unwrap();
        if !order.is_empty() {
            writeln!(
                out,
                "        let __ls_order = |__ls_left: &{}, __ls_right: &{}| -> std::cmp::Ordering {{",
                row_type(&output.name),
                row_type(&output.name)
            )
            .unwrap();
            for (field, descending) in order {
                let (ty, nullable) = output
                    .field(field)
                    .map(|(_, field)| (field.ty, field.nullable))
                    .ok_or_else(|| format!("unknown output order field `{field}`"))?;
                let comparison = order_compare(
                    ty,
                    &format!("__ls_left.{field}"),
                    &format!("__ls_right.{field}"),
                    nullable,
                );
                writeln!(out, "            let __ls_ordering = {comparison};").unwrap();
                writeln!(
                    out,
                    "            if __ls_ordering != std::cmp::Ordering::Equal {{"
                )
                .unwrap();
                if *descending {
                    writeln!(out, "                return __ls_ordering.reverse();").unwrap();
                } else {
                    writeln!(out, "                return __ls_ordering;").unwrap();
                }
                writeln!(out, "            }}").unwrap();
            }
            writeln!(out, "            std::cmp::Ordering::Equal").unwrap();
            writeln!(out, "        }};").unwrap();
        }
        emit_advanced_loops(out, plan, operation, &sources, joins, false, None)?;
        let predicate = match filter {
            Some(filter) => emit_expr_sources(filter, &sources, &operation.params)?,
            None => "true".to_string(),
        };
        writeln!(out, "            if !({predicate}) {{ continue; }}").unwrap();
        if order.is_empty() {
            writeln!(
                out,
                "            if __ls_rows.len() >= __ls_skip.saturating_add(__ls_take) {{ continue; }}"
            )
            .unwrap();
        }
        writeln!(
            out,
            "            __ls_rows.push({} {{",
            row_type(&output.name)
        )
        .unwrap();
        for (index, item) in items.iter().enumerate() {
            let field = &output.fields[index];
            let value = match &item.expr {
                SelectExpr::Field(reference) => emit_field_read(reference, &sources, true)?,
                SelectExpr::Aggregate { .. } => {
                    return Err(format!(
                        "operation `{}`: aggregate outside a grouped query",
                        operation.name
                    ))
                }
            };
            writeln!(out, "                {}: {value},", field.name).unwrap();
        }
        writeln!(out, "            }});").unwrap();
        if !order.is_empty() {
            writeln!(
                out,
                "            if __ls_rows.len() >= __ls_skip.saturating_add(__ls_take).saturating_mul(2).max(1) {{"
            )
            .unwrap();
            writeln!(out, "                __ls_rows.sort_by(__ls_order);").unwrap();
            writeln!(
                out,
                "                __ls_rows.truncate(__ls_skip.saturating_add(__ls_take));"
            )
            .unwrap();
            writeln!(out, "            }}").unwrap();
        }
        for _ in sources.iter().rev() {
            writeln!(out, "        }}").unwrap();
        }
        if !order.is_empty() {
            writeln!(out, "        __ls_rows.sort_by(__ls_order);").unwrap();
        }
    }

    if grouped && !order.is_empty() {
        writeln!(out, "        __ls_rows.sort_by(|__ls_left, __ls_right| {{").unwrap();
        for (field, descending) in order {
            let (ty, nullable) = output
                .field(field)
                .map(|(_, field)| (field.ty, field.nullable))
                .ok_or_else(|| format!("unknown output order field `{field}`"))?;
            let comparison = order_compare(
                ty,
                &format!("__ls_left.{field}"),
                &format!("__ls_right.{field}"),
                nullable,
            );
            writeln!(out, "            let __ls_ordering = {comparison};").unwrap();
            writeln!(
                out,
                "            if __ls_ordering != std::cmp::Ordering::Equal {{"
            )
            .unwrap();
            if *descending {
                writeln!(out, "                return __ls_ordering.reverse();").unwrap();
            } else {
                writeln!(out, "                return __ls_ordering;").unwrap();
            }
            writeln!(out, "            }}").unwrap();
        }
        writeln!(out, "            std::cmp::Ordering::Equal").unwrap();
        writeln!(out, "        }});").unwrap();
    }
    writeln!(
        out,
        "        __ls_rows.drain(..__ls_skip.min(__ls_rows.len()));"
    )
    .unwrap();
    writeln!(out, "        __ls_rows.truncate(__ls_take);").unwrap();
    writeln!(
        out,
        "        __ls_slow.__ls_set_rows(__ls_rows.len() as u64);"
    )
    .unwrap();
    writeln!(out, "        __ls_rows").unwrap();
    writeln!(out, "    }}").unwrap();
    Ok(())
}

pub(crate) fn expr_within_sources(
    expr: &Expr,
    maximum: usize,
    sources: &[(&Table, String)],
) -> Result<bool, String> {
    match expr {
        Expr::Cmp { left, right, .. } => Ok(operand_within_sources(left, maximum, sources)?
            && operand_within_sources(right, maximum, sources)?),
        Expr::And(left, right) | Expr::Or(left, right) => {
            Ok(expr_within_sources(left, maximum, sources)?
                && expr_within_sources(right, maximum, sources)?)
        }
        Expr::IsNull { field } | Expr::IsNotNull { field } => Ok(resolve_source(field, sources)
            .map(|(index, _, _)| index <= maximum)
            .unwrap_or(false)),
    }
}

pub(crate) fn operand_within_sources(
    operand: &Operand,
    maximum: usize,
    sources: &[(&Table, String)],
) -> Result<bool, String> {
    match operand {
        Operand::Field(name) => {
            let reference =
                FieldRef::parse(name).ok_or_else(|| format!("invalid field `{name}`"))?;
            let (index, _, _) = resolve_source(&reference, sources)?;
            Ok(index <= maximum)
        }
        Operand::Param(_) | Operand::Literal(_) => Ok(true),
    }
}

pub(crate) fn operand_nullable_sources(
    operand: &Operand,
    sources: &[(&Table, String)],
    params: &[Param],
) -> Result<bool, String> {
    match operand {
        Operand::Field(name) => {
            let reference =
                FieldRef::parse(name).ok_or_else(|| format!("invalid field `{name}`"))?;
            let (_, _, field) = resolve_source(&reference, sources)?;
            Ok(field.nullable)
        }
        Operand::Param(name) => params
            .iter()
            .find(|param| &param.name == name)
            .map(|param| param.nullable)
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(Value::Null(_)) => Ok(true),
        Operand::Literal(_) => Ok(false),
    }
}

pub(crate) fn emit_expr_sources(
    expr: &Expr,
    sources: &[(&Table, String)],
    params: &[Param],
) -> Result<String, String> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left_ty = source_operand_ty(left, sources, params)?;
            let right_ty = source_operand_ty(right, sources, params)?;
            let left_nullable = operand_nullable_sources(left, sources, params)?;
            let right_nullable = operand_nullable_sources(right, sources, params)?;
            let left = emit_operand_sources(left, sources, Some(right_ty))?;
            let right = emit_operand_sources(right, sources, Some(left_ty))?;
            let symbol = match op {
                Cmp::Eq => "==",
                Cmp::Ne => "!=",
                Cmp::Lt => "<",
                Cmp::Le => "<=",
                Cmp::Gt => ">",
                Cmp::Ge => ">=",
            };
            if left_nullable || right_nullable {
                Ok(emit_nullable_cmp(
                    &left,
                    left_nullable,
                    &right,
                    right_nullable,
                    symbol,
                ))
            } else {
                Ok(format!("({left} {symbol} {right})"))
            }
        }
        Expr::And(left, right) => Ok(format!(
            "({} && {})",
            emit_expr_sources(left, sources, params)?,
            emit_expr_sources(right, sources, params)?
        )),
        Expr::Or(left, right) => Ok(format!(
            "({} || {})",
            emit_expr_sources(left, sources, params)?,
            emit_expr_sources(right, sources, params)?
        )),
        Expr::IsNull { field } => {
            let (value, nullable) = is_null_operand(field, sources)?;
            if nullable {
                Ok(format!("{value}.is_none()"))
            } else {
                Ok("false".to_string())
            }
        }
        Expr::IsNotNull { field } => {
            let (value, nullable) = is_null_operand(field, sources)?;
            if nullable {
                Ok(format!("{value}.is_some()"))
            } else {
                Ok("true".to_string())
            }
        }
    }
}

pub(crate) fn is_null_operand(
    field: &FieldRef,
    sources: &[(&Table, String)],
) -> Result<(String, bool), String> {
    let value = emit_field_read(field, sources, false)?;
    let (_, _, resolved) = resolve_source(field, sources)?;
    Ok((value, resolved.nullable))
}

pub(crate) fn emit_operand_sources(
    operand: &Operand,
    sources: &[(&Table, String)],
    hint: Option<Ty>,
) -> Result<String, String> {
    match operand {
        Operand::Field(name) => {
            let reference =
                FieldRef::parse(name).ok_or_else(|| format!("invalid field `{name}`"))?;
            emit_field_read(&reference, sources, false)
        }
        Operand::Param(name) => Ok(name.clone()),
        Operand::Literal(value) => Ok(literal_with_ty(value, hint)),
    }
}

pub(crate) fn source_operand_ty(
    operand: &Operand,
    sources: &[(&Table, String)],
    params: &[Param],
) -> Result<Ty, String> {
    match operand {
        Operand::Field(name) => {
            let reference =
                FieldRef::parse(name).ok_or_else(|| format!("invalid field `{name}`"))?;
            Ok(resolve_source(&reference, sources)?.2.ty)
        }
        Operand::Param(name) => params
            .iter()
            .find(|param| &param.name == name)
            .map(|param| param.ty)
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(value) => Ok(value.ty()),
    }
}

pub(crate) fn emit_group_key_read(
    reference: &FieldRef,
    sources: &[(&Table, String)],
) -> Result<String, String> {
    let value = emit_field_read(reference, sources, true)?;
    let (_, _, field) = resolve_source(reference, sources)?;
    if field.ty.is_float() {
        if field.nullable {
            return Ok(format!("{value}.map(|__ls_v| __ls_v.into())"));
        }
        return Ok(format!("{value}.into()"));
    }
    Ok(value)
}

pub(crate) fn order_compare(ty: Ty, left: &str, right: &str, nullable: bool) -> String {
    if !nullable {
        if ty.is_float() {
            return format!("{left}.total_cmp(&{right})");
        }
        return format!("{left}.cmp(&{right})");
    }
    let inner = if ty.is_float() {
        "__ls_a.total_cmp(__ls_b)".to_string()
    } else {
        "__ls_a.cmp(__ls_b)".to_string()
    };
    format!(
        "(match (&{left}, &{right}) {{ (None, None) => ::std::cmp::Ordering::Equal, (None, Some(_)) => ::std::cmp::Ordering::Less, (Some(_), None) => ::std::cmp::Ordering::Greater, (Some(__ls_a), Some(__ls_b)) => {inner} }})"
    )
}

pub(crate) fn aggregate_field_ty(
    item: &SelectItem,
    sources: &[(&Table, String)],
) -> Result<Ty, String> {
    let SelectExpr::Aggregate { field, .. } = &item.expr else {
        return Err("internal: expected an aggregate".to_string());
    };
    let reference = field
        .as_ref()
        .ok_or_else(|| "aggregate needs a field".to_string())?;
    Ok(resolve_source(reference, sources)?.2.ty)
}

pub(crate) fn accumulator_type(
    item: &SelectItem,
    sources: &[(&Table, String)],
) -> Result<String, String> {
    match &item.expr {
        SelectExpr::Field(reference) => {
            let (_, _, field) = resolve_source(reference, sources)?;
            Ok(format!("Option<{}>", field.ty.rust()))
        }
        SelectExpr::Aggregate { function, field } => match function {
            Aggregate::Count => Ok("i64".to_string()),
            Aggregate::Sum if aggregate_field_ty(item, sources)?.is_float() => {
                Ok("(f64, bool)".to_string())
            }
            Aggregate::Sum => Ok("(i128, bool)".to_string()),
            Aggregate::Min | Aggregate::Max => {
                let reference = field
                    .as_ref()
                    .ok_or_else(|| "aggregate needs a field".to_string())?;
                let (_, _, field) = resolve_source(reference, sources)?;
                Ok(format!("Option<{}>", field.ty.rust()))
            }
            Aggregate::Avg if aggregate_field_ty(item, sources)?.is_float() => {
                Ok("(f64, i64)".to_string())
            }
            Aggregate::Avg => Ok("(i128, i64)".to_string()),
        },
    }
}

pub(crate) fn accumulator_init(item: &SelectItem, sources: &[(&Table, String)]) -> String {
    match &item.expr {
        SelectExpr::Field(_) => "None".to_string(),
        SelectExpr::Aggregate { function, .. } => match function {
            Aggregate::Count => "0i64".to_string(),
            Aggregate::Sum if aggregate_field_ty(item, sources).is_ok_and(Ty::is_float) => {
                "(0.0f64, false)".to_string()
            }
            Aggregate::Sum => "(0i128, false)".to_string(),
            Aggregate::Min | Aggregate::Max => "None".to_string(),
            Aggregate::Avg if aggregate_field_ty(item, sources).is_ok_and(Ty::is_float) => {
                "(0.0f64, 0i64)".to_string()
            }
            Aggregate::Avg => "(0i128, 0i64)".to_string(),
        },
    }
}

pub(crate) fn accumulator_finish(
    index: usize,
    item: &SelectItem,
    ty: Ty,
    sources: &[(&Table, String)],
) -> Result<String, String> {
    let default = match ty {
        Ty::I8 => "0i8",
        Ty::I16 => "0i16",
        Ty::I32 => "0i32",
        Ty::I64 => "0i64",
        Ty::U8 => "0u8",
        Ty::U16 => "0u16",
        Ty::U32 => "0u32",
        Ty::U64 => "0u64",
        Ty::F32 => "0.0f32",
        Ty::F64 => "0.0f64",
        Ty::Bool => "false",
        Ty::Text => "String::new()",
        Ty::Bytes => "Vec::new()",
        Ty::Uuid => "[0; 16]",
    };
    match &item.expr {
        SelectExpr::Field(reference) => {
            let (_, _, field) = resolve_source(reference, sources)?;
            if field.nullable {
                Ok(format!("__ls_acc.a{index}.clone()"))
            } else {
                let _ = sources;
                Ok(format!(
                    "__ls_acc.a{index}.clone().unwrap_or_else(|| {default})"
                ))
            }
        }
        SelectExpr::Aggregate {
            function: Aggregate::Min | Aggregate::Max,
            ..
        } => {
            let _ = sources;
            Ok(format!(
                "__ls_acc.a{index}.clone().unwrap_or_else(|| {default})"
            ))
        }
        SelectExpr::Aggregate {
            function: Aggregate::Count,
            ..
        } => Ok(format!("__ls_acc.a{index}")),
        SelectExpr::Aggregate {
            function: Aggregate::Sum,
            ..
        } => {
            if aggregate_field_ty(item, sources)?.is_float() {
                let value = if ty == Ty::F32 {
                    format!("__ls_acc.a{index}.0 as f32")
                } else {
                    format!("__ls_acc.a{index}.0")
                };
                Ok(format!(
                    "if __ls_acc.a{index}.1 {{ {value} }} else {{ {default} }}"
                ))
            } else {
                Ok(format!(
                    "if __ls_acc.a{index}.1 {{ __ls_acc.a{index}.0.clamp(i64::MIN as i128, i64::MAX as i128) as i64 }} else {{ 0i64 }}"
                ))
            }
        }
        SelectExpr::Aggregate {
            function: Aggregate::Avg,
            ..
        } => {
            if aggregate_field_ty(item, sources)?.is_float() {
                let value = if ty == Ty::F32 {
                    format!("(__ls_acc.a{index}.0 / __ls_acc.a{index}.1 as f64) as f32")
                } else {
                    format!("__ls_acc.a{index}.0 / __ls_acc.a{index}.1 as f64")
                };
                Ok(format!(
                    "if __ls_acc.a{index}.1 == 0 {{ {default} }} else {{ {value} }}"
                ))
            } else {
                Ok(format!(
                    "if __ls_acc.a{index}.1 == 0 {{ 0i64 }} else {{ (__ls_acc.a{index}.0 / __ls_acc.a{index}.1 as i128) as i64 }}"
                ))
            }
        }
    }
}

pub(crate) fn emit_count_query(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Query {
        table,
        filter,
        count: true,
        ..
    } = &operation.body
    else {
        return Err(format!(
            "operation `{}` is not a count query",
            operation.name
        ));
    };
    let table = get_table(workload, table)?;

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        out,
        "    pub fn {}(&self, {}) -> i64 {{",
        operation.name, params
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;

    if let Some(pk_operand) = pk_eq_operand(filter, table) {
        let key = lookup_operand(
            &pk_operand,
            table
                .primary_key()
                .ok_or_else(|| format!("table `{}` has no primary key", table.name))?
                .ty,
        )?;
        writeln!(
            out,
            "        let __ls_hit = self.t_{}.contains_key(&{key});",
            table.name
        )
        .unwrap();
        writeln!(out, "        __ls_slow.__ls_set_rows(__ls_hit as u64);").unwrap();
        writeln!(out, "        __ls_hit as i64").unwrap();
    } else if let Some(index) = plan.query_counter.get(&operation.name) {
        let spec = &plan.counters[*index];
        let constraints = eq_constraints(filter).ok_or_else(|| {
            format!(
                "internal: operation `{}` has no equality constraints for its counter",
                operation.name
            )
        })?;
        let key = lookup_eq_ctor(&format!("Cnt{index}Key"), &spec.eq, &constraints, table)?;
        writeln!(out, "        let __ls_key = {key};").unwrap();
        writeln!(
            out,
            "        let __ls_n: u64 = *self.cnt{index}.get(&__ls_key).unwrap_or(&0);"
        )
        .unwrap();
        writeln!(out, "        __ls_slow.__ls_set_rows(__ls_n);").unwrap();
        writeln!(out, "        __ls_n as i64").unwrap();
    } else {
        let predicate = match filter {
            Some(filter) => emit_expr(filter, "__row", table, &operation.params)?,
            None => "true".to_string(),
        };
        writeln!(
            out,
            "        let __ls_n = self.t_{}.values().filter(|__row| {predicate}).count();",
            table.name
        )
        .unwrap();
        writeln!(out, "        __ls_slow.__ls_set_rows(__ls_n as u64);").unwrap();
        writeln!(out, "        __ls_n as i64").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    Ok(())
}

pub(crate) fn emit_many_query(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let Body::Query {
        table,
        filter,
        order,
        limit,
        offset,
        count: false,
        ..
    } = &operation.body
    else {
        return Err(format!("operation `{}` is not a row query", operation.name));
    };
    let table = get_table(workload, table)?;

    let params = operation
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_rust(param)))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        out,
        "    pub fn {}(&self, {}) -> Vec<{}> {{",
        operation.name,
        params,
        row_type(&table.name)
    )
    .unwrap();
    emit_slow_sample(out, workload, plan, operation)?;

    match limit {
        Some(Limit::Literal(value)) => {
            writeln!(out, "        let __ls_take = {value}usize;").unwrap();
        }
        Some(Limit::Param(name)) => {
            writeln!(
                out,
                "        let __ls_take = if {name} < 0 {{ 0usize }} else {{ {name} as usize }};"
            )
            .unwrap();
        }
        None => {
            writeln!(out, "        let __ls_take = usize::MAX;").unwrap();
        }
    }
    match offset {
        Some(Limit::Literal(value)) => {
            writeln!(out, "        let __ls_skip = {value}usize;").unwrap();
        }
        Some(Limit::Param(name)) => {
            writeln!(
                out,
                "        let __ls_skip = if {name} < 0 {{ 0usize }} else {{ {name} as usize }};"
            )
            .unwrap();
        }
        None => {
            writeln!(out, "        let __ls_skip = 0usize;").unwrap();
        }
    }

    let predicate = match filter {
        Some(filter) => emit_expr(filter, "__row", table, &operation.params)?,
        None => "true".to_string(),
    };

    if let Some(pk_operand) = pk_eq_operand(filter, table) {
        let key = lookup_operand(
            &pk_operand,
            table
                .primary_key()
                .ok_or_else(|| format!("table `{}` has no primary key", table.name))?
                .ty,
        )?;
        writeln!(
            out,
            "        if __ls_take == 0 || __ls_skip > 0 {{ __ls_slow.__ls_set_rows(0); return Vec::new(); }}"
        )
        .unwrap();
        writeln!(
            out,
            "        let __ls_out: Vec<{}> = self.t_{}.get(&{key}).into_iter().collect();",
            row_type(&table.name),
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "        __ls_slow.__ls_set_rows(__ls_out.len() as u64);"
        )
        .unwrap();
        writeln!(out, "        __ls_out").unwrap();
    } else if plan.pk_range.contains(&operation.name) {
        let start = primary_key_start_key(filter, table).ok_or_else(|| {
            format!(
                "internal: operation `{}` has no primary-key lower bound",
                operation.name
            )
        })?;
        writeln!(
            out,
            "        let mut __ls_out: Vec<{}> = Vec::new();",
            row_type(&table.name)
        )
        .unwrap();
        writeln!(out, "        let mut __ls_skipped = 0usize;").unwrap();
        writeln!(
            out,
            "        for __row in self.t_{}.values_from(&{start}) {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "            if __ls_out.len() >= __ls_take {{ break; }}"
        )
        .unwrap();
        writeln!(out, "            if {predicate} {{").unwrap();
        writeln!(
            out,
            "                if __ls_skipped < __ls_skip {{ __ls_skipped += 1; }}"
        )
        .unwrap();
        writeln!(out, "                else {{ __ls_out.push(__row); }}").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        __ls_slow.__ls_set_rows(__ls_out.len() as u64);"
        )
        .unwrap();
        writeln!(out, "        __ls_out").unwrap();
    } else if let Some(index) = plan.query_index.get(&operation.name) {
        let spec = &plan.indexes[*index];
        let constraints = index_eq_constraints(filter).ok_or_else(|| {
            format!(
                "internal: operation `{}` has no equality constraints for its index",
                operation.name
            )
        })?;
        let key = lookup_eq_ctor(&format!("Idx{index}Eq"), &spec.eq, &constraints, table)?;
        let inner = index_inner_fields(spec, table)?;
        let start = index_start_key(filter, &inner, *index);
        let row = index_row_expression(table, "__ls_key", &inner)?;

        writeln!(
            out,
            "        let mut __ls_out: Vec<{}> = Vec::new();",
            row_type(&table.name)
        )
        .unwrap();
        writeln!(out, "        let mut __ls_skipped = 0usize;").unwrap();
        writeln!(
            out,
            "        let mut __ls_idx = self.idx{index}.write().unwrap();"
        )
        .unwrap();
        writeln!(out, "        __ls_idx.prepare(&{key});").unwrap();
        writeln!(
            out,
            "        if let Some(__ls_inner) = __ls_idx.get(&{key}) {{"
        )
        .unwrap();
        if let Some(start) = start {
            writeln!(out, "            let __ls_start = {start};").unwrap();
            writeln!(
                out,
                "            for __ls_key in __ls_inner.values_from(&__ls_start) {{"
            )
            .unwrap();
        } else {
            writeln!(out, "            for __ls_key in __ls_inner.values() {{").unwrap();
        }
        writeln!(
            out,
            "                if __ls_out.len() >= __ls_take {{ break; }}"
        )
        .unwrap();
        writeln!(out, "                if let Some(__row) = {row} {{").unwrap();
        writeln!(out, "                    if {predicate} {{").unwrap();
        writeln!(
            out,
            "                        if __ls_skipped < __ls_skip {{ __ls_skipped += 1; }}"
        )
        .unwrap();
        writeln!(
            out,
            "                        else {{ __ls_out.push(__row.clone()); }}"
        )
        .unwrap();
        writeln!(out, "                    }}").unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        __ls_slow.__ls_set_rows(__ls_out.len() as u64);"
        )
        .unwrap();
        writeln!(out, "        __ls_out").unwrap();
    } else {
        writeln!(
            out,
            "        let mut __ls_out: Vec<{}> = self.t_{}.values().filter(|__row| {predicate}).collect();",
            row_type(&table.name),
            table.name
        )
        .unwrap();

        if !order.is_empty() {
            let primary_key = table
                .primary_key()
                .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
            let mut chain = String::new();
            for (field, descending) in order {
                let (ty, nullable) = table
                    .field(field)
                    .map(|(_, field)| (field.ty, field.nullable))
                    .ok_or_else(|| format!("unknown order field `{field}`"))?;
                let comparison = order_compare(
                    ty,
                    &format!("__ls_a.{field}"),
                    &format!("__ls_b.{field}"),
                    nullable,
                );
                let comparison = if *descending {
                    format!("{comparison}.reverse()")
                } else {
                    comparison
                };
                if chain.is_empty() {
                    chain = comparison;
                } else {
                    chain = format!("{chain}.then({comparison})");
                }
            }
            let pk_comparison = order_compare(
                primary_key.ty,
                &format!("__ls_a.{}", primary_key.name),
                &format!("__ls_b.{}", primary_key.name),
                false,
            );
            chain = format!("{chain}.then({pk_comparison})");
            writeln!(out, "        __ls_out.sort_by(|__ls_a, __ls_b| {{").unwrap();
            writeln!(out, "            {chain}").unwrap();
            writeln!(out, "        }});").unwrap();
        }

        writeln!(
            out,
            "        __ls_out.drain(..__ls_skip.min(__ls_out.len()));"
        )
        .unwrap();
        writeln!(out, "        __ls_out.truncate(__ls_take);").unwrap();
        writeln!(
            out,
            "        __ls_slow.__ls_set_rows(__ls_out.len() as u64);"
        )
        .unwrap();
        writeln!(out, "        __ls_out").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    Ok(())
}
