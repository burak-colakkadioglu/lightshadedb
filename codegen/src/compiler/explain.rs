use super::plan::{
    advanced_eq_constraints, advanced_filter_for, eq_constraints, field_is_nullable, get_table,
    index_eq_constraints, index_inner_fields, index_start_key, is_advanced_query, pk_eq_operand,
    pk_join_key, primary_key_start_key, Plan,
};
use crate::ir::*;

pub struct OpPlan {
    pub strategy: String,
    pub details: Vec<String>,
}

impl OpPlan {
    pub fn lines(&self, name: &str, what: &str) -> Vec<String> {
        let mut out = vec![
            format!("operation `{name}` ({what}):"),
            format!("  strategy: {}", self.strategy),
        ];
        out.extend(self.details.iter().map(|detail| format!("  {detail}")));
        out
    }
}

pub fn describe_operation(
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<OpPlan, String> {
    match &operation.body {
        Body::Insert { table } => describe_insert(workload, plan, table),
        Body::Update {
            table,
            sets,
            filter,
        } => describe_update(workload, plan, operation, table, sets, filter),
        Body::Delete { table, filter } => describe_delete(workload, operation, table, filter),
        Body::Query {
            table,
            filter,
            count: true,
            ..
        } => describe_count(workload, plan, operation, table, filter),
        Body::Query { .. } if is_advanced_query(&operation.body) => {
            describe_advanced(workload, plan, operation)
        }
        Body::Query {
            table,
            filter,
            order,
            limit,
            offset,
            count: false,
            ..
        } => describe_many(
            workload, plan, operation, table, filter, order, limit, offset,
        ),
    }
}

fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Field(name) => name.clone(),
        Operand::Param(name) => format!("${name}"),
        Operand::Literal(value) => value_text(value),
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::I8(value) => value.to_string(),
        Value::I16(value) => value.to_string(),
        Value::I32(value) => value.to_string(),
        Value::I64(value) => value.to_string(),
        Value::U8(value) => value.to_string(),
        Value::U16(value) => value.to_string(),
        Value::U32(value) => value.to_string(),
        Value::U64(value) => value.to_string(),
        Value::F32(value) => value.to_string(),
        Value::F64(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Text(text) => format!("{text:?}"),
        Value::Bytes(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                hex.push_str(&format!("{byte:02x}"));
            }
            format!("hex:{hex}")
        }
        Value::Uuid(id) => {
            let mut hex = String::with_capacity(32);
            for byte in id {
                hex.push_str(&format!("{byte:02x}"));
            }
            format!("uuid:{hex}")
        }
        Value::Null(_) => "NULL".to_string(),
    }
}

fn limit_text(limit: &Option<Limit>) -> Option<String> {
    match limit {
        None => None,
        Some(Limit::Literal(value)) => Some(value.to_string()),
        Some(Limit::Param(name)) => Some(format!("${name}")),
    }
}

fn order_text(order: &[(String, bool)]) -> String {
    order
        .iter()
        .map(|(field, descending)| {
            if *descending {
                format!("{field} desc")
            } else {
                field.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn limit_details(limit: &Option<Limit>, offset: &Option<Limit>, early: bool) -> Vec<String> {
    let mut details = Vec::new();
    match (limit_text(limit), limit_text(offset)) {
        (None, None) => {}
        (limit, offset) => {
            let mut parts = Vec::new();
            if let Some(limit) = limit {
                parts.push(format!("limit {limit}"));
            }
            if let Some(offset) = offset {
                parts.push(format!("offset {offset}"));
            }
            let how = if early {
                "stops the scan early"
            } else {
                "applied after materializing all matches"
            };
            details.push(format!("{} {how}", parts.join(" + ")));
        }
    }
    details
}

fn nullable_blockers(workload: &Workload, table: &str, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .filter(|field| field_is_nullable(workload, table, field))
        .cloned()
        .collect()
}

fn describe_insert(workload: &Workload, plan: &Plan, table_name: &str) -> Result<OpPlan, String> {
    let table = get_table(workload, table_name)?;
    let details = maintained_details(
        plan_indexes(plan, &table.name),
        plan_counters(plan, &table.name),
    );
    Ok(OpPlan {
        strategy: format!(
            "point put into {} (duplicate key returns false)",
            table.name
        ),
        details,
    })
}

fn maintained_details(
    indexes: Vec<(usize, Vec<String>)>,
    counters: Vec<(usize, Vec<String>)>,
) -> Vec<String> {
    let mut details = Vec::new();
    for (index, fields) in indexes {
        details.push(format!(
            "maintains index Idx{index} on ({})",
            fields.join(", ")
        ));
    }
    for (index, fields) in counters {
        details.push(format!(
            "maintains counter Cnt{index} on ({})",
            fields.join(", ")
        ));
    }
    if details.is_empty() {
        details.push("touches no maintained indexes or counters".to_string());
    }
    details
}

fn plan_indexes(plan: &Plan, table: &str) -> Vec<(usize, Vec<String>)> {
    plan.indexes
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.table == table)
        .map(|(index, spec)| (index, spec.eq.clone()))
        .collect()
}

fn plan_counters(plan: &Plan, table: &str) -> Vec<(usize, Vec<String>)> {
    plan.counters
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.table == table)
        .map(|(index, spec)| (index, spec.eq.clone()))
        .collect()
}

fn describe_update(
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
    table_name: &str,
    sets: &[(String, Operand)],
    filter: &Option<Expr>,
) -> Result<OpPlan, String> {
    let table = get_table(workload, table_name)?;
    let key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
    let strategy = match pk_eq_operand(filter, table) {
        Some(operand) => format!(
            "point update of {} by primary key ({} = {})",
            table.name,
            key.name,
            operand_text(&operand)
        ),
        None => format!(
            "full scan of {} with filter (updates rows in place)",
            table.name
        ),
    };
    let changed: Vec<&str> = sets.iter().map(|(field, _)| field.as_str()).collect();
    let mut indexes = Vec::new();
    for (index, spec) in plan.indexes.iter().enumerate() {
        if spec.table != table.name {
            continue;
        }
        let inner = index_inner_fields(spec, table)?;
        let affected = spec
            .eq
            .iter()
            .any(|field| changed.contains(&field.as_str()))
            || inner
                .iter()
                .any(|field| changed.contains(&field.name.as_str()));
        if affected {
            indexes.push((index, spec.eq.clone()));
        }
    }
    let mut counters = Vec::new();
    for (index, spec) in plan.counters.iter().enumerate() {
        if spec.table == table.name
            && spec
                .eq
                .iter()
                .any(|field| changed.contains(&field.as_str()))
        {
            counters.push((index, spec.eq.clone()));
        }
    }
    let _ = &operation.name;
    Ok(OpPlan {
        strategy,
        details: maintained_details(indexes, counters),
    })
}

fn describe_delete(
    workload: &Workload,
    operation: &Operation,
    table_name: &str,
    filter: &Option<Expr>,
) -> Result<OpPlan, String> {
    let table = get_table(workload, table_name)?;
    let key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
    let strategy = match pk_eq_operand(filter, table) {
        Some(operand) => format!(
            "point delete from {} by primary key ({} = {})",
            table.name,
            key.name,
            operand_text(&operand)
        ),
        None => format!("full scan of {} with filter (deletes matches)", table.name),
    };
    let _ = &operation.name;
    Ok(OpPlan {
        strategy,
        details: vec!["removes rows from every index and counter on the table".to_string()],
    })
}

fn describe_count(
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
    table_name: &str,
    filter: &Option<Expr>,
) -> Result<OpPlan, String> {
    let table = get_table(workload, table_name)?;
    if let Some(pk) = pk_eq_operand(filter, table) {
        let key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        return Ok(OpPlan {
            strategy: format!(
                "primary-key point lookup on {} ({} = {})",
                table.name,
                key.name,
                operand_text(&pk)
            ),
            details: vec!["counts 0 or 1 without scanning".to_string()],
        });
    }
    if let Some(index) = plan.query_counter.get(&operation.name) {
        let spec = &plan.counters[*index];
        let constraints = eq_constraints(filter).ok_or_else(|| {
            format!(
                "internal: operation `{}` lost its counter constraints",
                operation.name
            )
        })?;
        return Ok(OpPlan {
            strategy: format!(
                "maintained counter Cnt{index} on {} ({})",
                table.name,
                spec.eq.join(", ")
            ),
            details: vec![format!(
                "equality: {}",
                equality_text(&spec.eq, &constraints)
            )],
        });
    }
    let mut details = Vec::new();
    match eq_constraints(filter) {
        None => details.push(
            "no counter: filter has no usable equality prefix (needs field = value conjuncts)"
                .to_string(),
        ),
        Some(constraints) => {
            let fields: Vec<String> = constraints.iter().map(|(field, _)| field.clone()).collect();
            let blockers = nullable_blockers(workload, &table.name, &fields);
            if !blockers.is_empty() {
                details.push(format!(
                    "no counter: {} nullable (counters never cover nullable columns)",
                    blockers.join(", ")
                ));
            } else if fields.is_empty() {
                details.push("no counter: no equality filter to count by".to_string());
            }
        }
    }
    details.push("counts by scanning and testing every row".to_string());
    Ok(OpPlan {
        strategy: format!("full scan of {}", table.name),
        details,
    })
}

fn equality_text(eq: &[String], constraints: &[(String, Operand)]) -> String {
    eq.iter()
        .map(|field| {
            let operand = constraints
                .iter()
                .find(|(name, _)| name == field)
                .map(|(_, operand)| operand_text(operand))
                .unwrap_or_else(|| "?".to_string());
            format!("{field} = {operand}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn bound_text(bound: &str) -> String {
    match bound.strip_suffix(".clone()") {
        Some(param) => format!("${param}"),
        None => bound.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn describe_many(
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
    table_name: &str,
    filter: &Option<Expr>,
    order: &[(String, bool)],
    limit: &Option<Limit>,
    offset: &Option<Limit>,
) -> Result<OpPlan, String> {
    let table = get_table(workload, table_name)?;
    if let Some(pk) = pk_eq_operand(filter, table) {
        let key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        return Ok(OpPlan {
            strategy: format!(
                "primary-key point lookup on {} ({} = {})",
                table.name,
                key.name,
                operand_text(&pk)
            ),
            details: vec!["returns 0 or 1 rows; limit and offset do not scan".to_string()],
        });
    }
    if plan.pk_range.contains(&operation.name) {
        let start = primary_key_start_key(filter, table).ok_or_else(|| {
            format!(
                "internal: operation `{}` has no primary-key lower bound",
                operation.name
            )
        })?;
        let mut details = vec![
            "rows arrive in primary-key order; no sort".to_string(),
            "residual filter still tested per row".to_string(),
        ];
        details.extend(limit_details(limit, offset, true));
        return Ok(OpPlan {
            strategy: format!(
                "ordered primary-key range scan on {} from lower bound {}",
                table.name,
                bound_text(&start)
            ),
            details,
        });
    }
    if let Some(index) = plan.query_index.get(&operation.name) {
        let spec = &plan.indexes[*index];
        let constraints = index_eq_constraints(filter).ok_or_else(|| {
            format!(
                "internal: operation `{}` has no equality constraints for its index",
                operation.name
            )
        })?;
        let mut details = Vec::new();
        if spec.eq.is_empty() {
            details.push("no equality prefix: full index in order".to_string());
        } else {
            details.push(format!(
                "equality: {}",
                equality_text(&spec.eq, &constraints)
            ));
        }
        details.push(format!(
            "order: {} (satisfied by the index; no sort)",
            order_text(&spec.order)
        ));
        let inner = index_inner_fields(spec, table)?;
        if index_start_key(filter, &inner, *index).is_some() {
            details.push(
                "range bound: scan starts at the first key at or after the bound".to_string(),
            );
        }
        details.extend(limit_details(limit, offset, true));
        return Ok(OpPlan {
            strategy: format!(
                "index scan on {} via Idx{index} ({} equality, {} order)",
                table.name,
                spec.eq.len(),
                spec.order.len()
            ),
            details,
        });
    }
    let mut details = Vec::new();
    match index_eq_constraints(filter) {
        None => details.push(
            "no index: filter has no usable equality prefix (needs field = value conjuncts)"
                .to_string(),
        ),
        Some(constraints) => {
            let mut fields: Vec<String> =
                constraints.iter().map(|(field, _)| field.clone()).collect();
            for (field, _) in order {
                if !fields.contains(field) {
                    fields.push(field.clone());
                }
            }
            let blockers = nullable_blockers(workload, &table.name, &fields);
            if !blockers.is_empty() {
                details.push(format!(
                    "no index: {} nullable (indexes never cover nullable columns)",
                    blockers.join(", ")
                ));
            } else if order.is_empty() {
                if fields.is_empty() {
                    details.push("no index: no filter or order to index".to_string());
                } else {
                    details.push(format!(
                        "no index: equality on ({}) without ORDER BY (unordered queries always scan)",
                        constraints
                            .iter()
                            .map(|(field, _)| field.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
    }
    if filter.is_some() {
        details.push("residual filter tested per row".to_string());
    }
    if order.is_empty() {
        details.push("no sort".to_string());
    } else {
        details.push(format!(
            "in-memory sort by {} (tiebreak: primary key)",
            order_text(order)
        ));
    }
    details.extend(limit_details(limit, offset, false));
    Ok(OpPlan {
        strategy: format!("full scan of {}", table.name),
        details,
    })
}

fn describe_advanced(
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<OpPlan, String> {
    let Body::Query {
        table,
        alias,
        joins,
        filter,
        order,
        limit,
        offset,
        group_by,
        ..
    } = &operation.body
    else {
        return Err(format!(
            "operation `{}` is not an advanced query",
            operation.name
        ));
    };
    let primary = get_table(workload, table)?;
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
    let mut details = Vec::new();
    let primary_strategy = if let Some(index) = plan.advanced_primary.get(&operation.name) {
        let spec = &plan.indexes[*index];
        let constraints = advanced_eq_constraints(advanced_filter_for(operation), 0, &sources)?
            .ok_or_else(|| {
                format!(
                    "internal: operation `{}` lost its primary constraints",
                    operation.name
                )
            })?;
        details.push(format!(
            "primary equality: {}",
            equality_text(&spec.eq, &constraints)
        ));
        format!(
            "index scan on {} via Idx{index} ({} equality)",
            sources[0].1,
            spec.eq.len()
        )
    } else {
        let mut bools = Vec::new();
        if let Ok(Some(constraints)) =
            advanced_eq_constraints(advanced_filter_for(operation), 0, &sources)
        {
            for (field, _) in &constraints {
                let ty = sources[0].0.field(field).map(|(_, field)| field.ty);
                if ty == Some(Ty::Bool) && !bools.contains(field) {
                    bools.push(field.clone());
                }
            }
        }
        if !bools.is_empty() {
            details.push(format!(
                "no index: {} bool (low-selectivity columns are never indexed)",
                bools.join(", ")
            ));
        }
        format!("full scan of {}", sources[0].1)
    };
    for (depth, join) in joins.iter().enumerate() {
        let depth = depth + 1;
        let joined_name = &sources[depth].1;
        if pk_join_key(join, depth, &sources)?.is_some() {
            details.push(format!(
                "join {joined_name}: primary-key point lookup ({} = {})",
                join.keys[0].left, join.keys[0].right
            ));
        } else if let Some(index) = plan.advanced_join.get(&(operation.name.clone(), depth)) {
            let spec = &plan.indexes[*index];
            details.push(format!(
                "join {joined_name}: index lookup via Idx{index} on ({})",
                spec.eq.join(", ")
            ));
        } else if join.keys.len() == 1 {
            details.push(format!("join {joined_name}: nested loop with filter"));
        } else {
            let pairs = join
                .keys
                .iter()
                .map(|key| format!("{} = {}", key.left, key.right))
                .collect::<Vec<_>>()
                .join(", ");
            details.push(format!(
                "join {joined_name}: nested loop with filter ({pairs})"
            ));
        }
    }
    if !group_by.is_empty() {
        details.push(format!(
            "in-memory group by ({}) with aggregates",
            group_by
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if filter.is_some() {
        details.push("residual filter tested per joined row".to_string());
    }
    if !order.is_empty() {
        details.push(format!("in-memory sort by {}", order_text(order)));
    }
    details.extend(limit_details(limit, offset, false));
    Ok(OpPlan {
        strategy: format!(
            "advanced query over {} source(s): {primary_strategy}",
            sources.len()
        ),
        details,
    })
}

#[cfg(test)]
mod tests {
    use super::super::plan::build_plan;
    use super::*;
    use crate::ir::parse::parse_workload;

    const SOURCE: &str = include_str!("../../../examples/messages.lightshade");

    fn nullable_workload() -> Workload {
        let mut workload = parse_workload(SOURCE).unwrap();
        let table = workload
            .tables
            .iter_mut()
            .find(|table| table.name == "messages")
            .unwrap();
        table
            .fields
            .iter_mut()
            .find(|field| field.name == "inbox_id")
            .unwrap()
            .nullable = true;
        workload
    }

    #[test]
    fn nullable_columns_fall_back_to_scan_with_a_reason() {
        let workload = nullable_workload();
        let plan = build_plan(&workload).unwrap();
        let operation = workload
            .operations
            .iter()
            .find(|operation| operation.name == "recent_messages")
            .unwrap();
        let described = describe_operation(&workload, &plan, operation).unwrap();
        assert_eq!(described.strategy, "full scan of messages");
        assert!(
            described.details.iter().any(|detail| detail
                == "no index: inbox_id nullable (indexes never cover nullable columns)"),
            "{:?}",
            described.details
        );
    }

    #[test]
    fn nullable_columns_fall_back_to_count_scan_with_a_reason() {
        let workload = nullable_workload();
        let plan = build_plan(&workload).unwrap();
        let operation = workload
            .operations
            .iter()
            .find(|operation| operation.name == "unread_count")
            .unwrap();
        let described = describe_operation(&workload, &plan, operation).unwrap();
        assert_eq!(described.strategy, "full scan of messages");
        assert!(
            described.details.iter().any(|detail| detail
                == "no counter: inbox_id nullable (counters never cover nullable columns)"),
            "{:?}",
            described.details
        );
    }
}
