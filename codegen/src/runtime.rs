use crate::ir::*;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum OpResult {
    None,
    One(Value),
    Many(Vec<Row>),
}

#[derive(Clone, Debug)]
pub struct DbError(pub String);

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DbError {}

impl From<String> for DbError {
    fn from(message: String) -> Self {
        DbError(message)
    }
}

impl From<&str> for DbError {
    fn from(message: &str) -> Self {
        DbError(message.to_string())
    }
}

struct TableData {
    rows: BTreeMap<Value, Row>,
}

pub struct Database {
    workload: Workload,
    tables: HashMap<String, TableData>,
}

impl Database {
    pub fn new(workload: Workload) -> Database {
        let tables = workload
            .tables
            .iter()
            .map(|table| {
                (
                    table.name.clone(),
                    TableData {
                        rows: BTreeMap::new(),
                    },
                )
            })
            .collect();
        Database { workload, tables }
    }

    pub fn execute(&mut self, name: &str, args: &[Value]) -> Result<OpResult, DbError> {
        let operation = self
            .workload
            .operation(name)
            .cloned()
            .ok_or_else(|| DbError(format!("unknown operation `{name}`")))?;
        let bindings = bind_params(&operation, args)?;

        match &operation.body {
            Body::Insert { table } => {
                let schema = self.schema(table)?;
                let pk_index = schema
                    .primary_key_index()
                    .ok_or_else(|| DbError(format!("table `{table}` has no primary key")))?;

                let mut row = Vec::with_capacity(schema.fields.len());
                for field in &schema.fields {
                    let param_index = operation
                        .params
                        .iter()
                        .position(|param| param.name == field.name)
                        .ok_or_else(|| {
                            DbError(format!(
                                "insert operation `{}` has no parameter for field `{}`",
                                operation.name, field.name
                            ))
                        })?;
                    row.push(args[param_index].clone());
                }

                let primary_key = row[pk_index].clone();
                let table_data = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;
                if table_data.rows.contains_key(&primary_key) {
                    return Err(DbError(format!(
                        "duplicate primary key {primary_key} in table `{table}`"
                    )));
                }
                table_data.rows.insert(primary_key, row);
                Ok(OpResult::None)
            }
            Body::Update {
                table,
                sets,
                filter,
            } => {
                let schema = self.schema(table)?;
                let pk_index = schema
                    .primary_key_index()
                    .ok_or_else(|| DbError(format!("table `{table}` has no primary key")))?;

                let mut replacements: Vec<(Value, Row)> = Vec::new();
                {
                    let table_data = self
                        .tables
                        .get(table)
                        .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;
                    for row in table_data.rows.values() {
                        if let Some(filter) = filter {
                            if eval_expr(filter, &schema, row, &bindings)? != Value::Bool(true) {
                                continue;
                            }
                        }

                        let mut new_row = row.clone();
                        for (field_name, operand) in sets {
                            let (index, field) = schema
                                .field(field_name)
                                .ok_or_else(|| DbError(format!("unknown field `{field_name}`")))?;
                            let value = eval_operand(operand, &schema, row, &bindings)?;
                            let value_ty = value.ty();
                            new_row[index] = coerce_value(value, field.ty).ok_or_else(|| {
                                DbError(format!(
                                    "field `{field_name}` expects `{}`, got `{}`",
                                    field.ty, value_ty
                                ))
                            })?;
                        }
                        replacements.push((row[pk_index].clone(), new_row));
                    }
                }

                let table_data = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;
                for (old_key, new_row) in replacements {
                    let new_key = new_row[pk_index].clone();
                    table_data.rows.remove(&old_key);
                    table_data.rows.insert(new_key, new_row);
                }
                Ok(OpResult::None)
            }
            Body::Delete { table, filter } => {
                let schema = self.schema(table)?;
                let pk_index = schema
                    .primary_key_index()
                    .ok_or_else(|| DbError(format!("table `{table}` has no primary key")))?;

                let mut keys = Vec::new();
                {
                    let table_data = self
                        .tables
                        .get(table)
                        .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;
                    for row in table_data.rows.values() {
                        if let Some(filter) = filter {
                            if eval_expr(filter, &schema, row, &bindings)? != Value::Bool(true) {
                                continue;
                            }
                        }
                        keys.push(row[pk_index].clone());
                    }
                }

                let table_data = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;
                for key in keys {
                    table_data.rows.remove(&key);
                }
                Ok(OpResult::None)
            }
            Body::Query {
                table,
                alias,
                joins,
                select,
                group_by,
                filter,
                order,
                limit,
                offset,
                count,
            } => {
                if !select.is_empty()
                    || !joins.is_empty()
                    || !group_by.is_empty()
                    || alias.is_some()
                {
                    return self.advanced_query(AdvancedQuery {
                        table,
                        alias,
                        joins,
                        select,
                        group_by,
                        filter,
                        order,
                        limit,
                        offset,
                        bindings: &bindings,
                        operation: &operation,
                    });
                }
                let schema = self.schema(table)?;
                let pk_index = schema
                    .primary_key_index()
                    .ok_or_else(|| DbError(format!("table `{table}` has no primary key")))?;
                let table_data = self
                    .tables
                    .get(table)
                    .ok_or_else(|| DbError(format!("unknown table `{table}`")))?;

                if *count {
                    let mut total: i64 = 0;
                    for row in table_data.rows.values() {
                        if let Some(filter) = filter {
                            if eval_expr(filter, &schema, row, &bindings)? != Value::Bool(true) {
                                continue;
                            }
                        }
                        total += 1;
                    }
                    return Ok(OpResult::One(Value::I64(total)));
                }

                let mut rows: Vec<Row> = Vec::new();
                for row in table_data.rows.values() {
                    if let Some(filter) = filter {
                        if eval_expr(filter, &schema, row, &bindings)? != Value::Bool(true) {
                            continue;
                        }
                    }
                    rows.push(row.clone());
                }

                if !order.is_empty() {
                    let mut order_fields = Vec::with_capacity(order.len());
                    for (field_name, descending) in order {
                        let (index, _) = schema.field(field_name).ok_or_else(|| {
                            DbError(format!("unknown order field `{field_name}`"))
                        })?;
                        order_fields.push((index, *descending));
                    }

                    rows.sort_by(|left, right| {
                        for (index, descending) in &order_fields {
                            let ordering = compare_nullable(&left[*index], &right[*index]);
                            if ordering != Ordering::Equal {
                                return if *descending {
                                    ordering.reverse()
                                } else {
                                    ordering
                                };
                            }
                        }
                        left[pk_index].cmp(&right[pk_index])
                    });
                }

                if let Some(offset) = offset {
                    let skip = offset_count(offset, &bindings)?;
                    rows.drain(..skip.min(rows.len()));
                }

                if let Some(limit) = limit {
                    let count = match limit {
                        Limit::Literal(value) => *value as usize,
                        Limit::Param(name) => match bindings.get(name) {
                            Some(Value::I64(value)) if *value >= 0 => *value as usize,
                            Some(Value::I64(_)) => {
                                return Err(DbError(format!(
                                    "limit parameter `{name}` must not be negative"
                                )))
                            }
                            Some(_) => {
                                return Err(DbError(format!(
                                    "limit parameter `{name}` must be an i64"
                                )))
                            }
                            None => {
                                return Err(DbError(format!(
                                    "limit references unknown parameter `{name}`"
                                )))
                            }
                        },
                    };
                    rows.truncate(count);
                }

                Ok(OpResult::Many(rows))
            }
        }
    }

    fn advanced_query(&self, query: AdvancedQuery<'_>) -> Result<OpResult, DbError> {
        let AdvancedQuery {
            table,
            alias,
            joins,
            select,
            group_by,
            filter,
            order,
            limit,
            offset,
            bindings,
            operation,
        } = query;

        let output_name = match &operation.returns {
            Returns::Many(name) => name,
            _ => {
                return Err(DbError(
                    "projected and joined queries must declare `-> many <table>`".to_string(),
                ))
            }
        };
        let output = self.schema(output_name)?;
        let primary = self.schema(table)?;
        let primary_rows = self
            .tables
            .get(table)
            .ok_or_else(|| DbError(format!("unknown table `{table}`")))?
            .rows
            .values()
            .cloned()
            .collect::<Vec<Row>>();

        let mut sources = vec![QuerySource {
            table: primary,
            qualifier: alias.clone().unwrap_or_else(|| table.to_string()),
            offset: 0,
        }];
        let mut tuples = primary_rows;

        for join in joins {
            let joined = self.schema(&join.table)?;
            let offset = sources
                .iter()
                .map(|source| source.offset + source.table.fields.len())
                .max()
                .unwrap_or(0);
            let qualifier = join.alias.clone().unwrap_or_else(|| join.table.clone());
            if sources.iter().any(|source| source.qualifier == qualifier) {
                return Err(DbError(format!("duplicate source alias `{qualifier}`")));
            }
            sources.push(QuerySource {
                table: joined,
                qualifier,
                offset,
            });
            let joined_rows = self
                .tables
                .get(&join.table)
                .ok_or_else(|| DbError(format!("unknown table `{}`", join.table)))?
                .rows
                .values()
                .cloned()
                .collect::<Vec<Row>>();
            let mut next = Vec::new();
            for tuple in &tuples {
                for row in &joined_rows {
                    let mut candidate = tuple.clone();
                    candidate.extend(row.iter().cloned());
                    let mut matched = true;
                    for key in &join.keys {
                        let left = eval_field_value(&key.left, &sources, &candidate)?;
                        let right = eval_field_value(&key.right, &sources, &candidate)?;
                        if left != right {
                            matched = false;
                            break;
                        }
                    }
                    if matched {
                        next.push(candidate);
                    }
                }
            }
            tuples = next;
        }

        let items = output
            .fields
            .iter()
            .map(|field| {
                select
                    .iter()
                    .find(|item| item.alias == field.name)
                    .ok_or_else(|| {
                        DbError(format!("`select` is missing output field `{}`", field.name))
                    })
            })
            .collect::<Result<Vec<&SelectItem>, DbError>>()?;
        let grouped = !group_by.is_empty()
            || items
                .iter()
                .any(|item| matches!(item.expr, SelectExpr::Aggregate { .. }));

        let mut rows = Vec::new();
        if grouped {
            let mut groups: BTreeMap<Vec<Value>, Vec<Accumulator>> = BTreeMap::new();
            if group_by.is_empty() {
                groups.insert(Vec::new(), init_accumulators(&items, &sources));
            }
            for tuple in &tuples {
                if let Some(filter) = filter {
                    if eval_expr_sources(filter, &sources, tuple, bindings)? != Value::Bool(true) {
                        continue;
                    }
                }
                let mut key = Vec::with_capacity(group_by.len());
                for reference in group_by {
                    key.push(eval_field_value(reference, &sources, tuple)?);
                }
                let accumulators = groups
                    .entry(key)
                    .or_insert_with(|| init_accumulators(&items, &sources));
                for (item, accumulator) in items.iter().zip(accumulators.iter_mut()) {
                    update_accumulator(accumulator, item, &sources, tuple)?;
                }
            }
            for (_, accumulators) in groups {
                let mut row = Vec::with_capacity(output.fields.len());
                for (field, accumulator) in output.fields.iter().zip(accumulators) {
                    row.push(finish_accumulator(accumulator, field.ty));
                }
                rows.push(row);
            }
        } else {
            for tuple in &tuples {
                if let Some(filter) = filter {
                    if eval_expr_sources(filter, &sources, tuple, bindings)? != Value::Bool(true) {
                        continue;
                    }
                }
                let mut row = Vec::with_capacity(output.fields.len());
                for item in &items {
                    row.push(match &item.expr {
                        SelectExpr::Field(reference) => {
                            eval_field_value(reference, &sources, tuple)?
                        }
                        SelectExpr::Aggregate { .. } => {
                            return Err(DbError(
                                "aggregate select items require `group by`".to_string(),
                            ))
                        }
                    });
                }
                rows.push(row);
            }
        }

        if !order.is_empty() {
            let mut order_fields = Vec::with_capacity(order.len());
            for (field_name, descending) in order {
                let (index, _) = output
                    .field(field_name)
                    .ok_or_else(|| DbError(format!("unknown output order field `{field_name}`")))?;
                order_fields.push((index, *descending));
            }
            rows.sort_by(|left, right| {
                for (index, descending) in &order_fields {
                    let ordering = compare_nullable(&left[*index], &right[*index]);
                    if ordering != Ordering::Equal {
                        return if *descending {
                            ordering.reverse()
                        } else {
                            ordering
                        };
                    }
                }
                Ordering::Equal
            });
        }

        if let Some(offset) = offset {
            let skip = offset_count(offset, bindings)?;
            rows.drain(..skip.min(rows.len()));
        }
        if let Some(limit) = limit {
            rows.truncate(limit_count(limit, bindings)?);
        }
        Ok(OpResult::Many(rows))
    }

    fn schema(&self, table: &str) -> Result<Table, DbError> {
        self.workload
            .table(table)
            .cloned()
            .ok_or_else(|| DbError(format!("unknown table `{table}`")))
    }
}

struct QuerySource {
    table: Table,
    qualifier: String,
    offset: usize,
}

struct AdvancedQuery<'a> {
    table: &'a str,
    alias: &'a Option<String>,
    joins: &'a [Join],
    select: &'a [SelectItem],
    group_by: &'a [FieldRef],
    filter: &'a Option<Expr>,
    order: &'a [(String, bool)],
    limit: &'a Option<Limit>,
    offset: &'a Option<Limit>,
    bindings: &'a HashMap<String, Value>,
    operation: &'a Operation,
}

enum Accumulator {
    Field(Option<Value>),
    Count(i64),
    Sum { total: i128, seen: bool },
    FloatSum { total: f64, seen: bool },
    Min(Option<Value>),
    Max(Option<Value>),
    Avg { total: i128, count: i64 },
    FloatAvg { total: f64, count: i64 },
}

fn init_accumulators(items: &[&SelectItem], sources: &[QuerySource]) -> Vec<Accumulator> {
    items
        .iter()
        .map(|item| match &item.expr {
            SelectExpr::Field(_) => Accumulator::Field(None),
            SelectExpr::Aggregate { function, .. } => match function {
                Aggregate::Count => Accumulator::Count(0),
                Aggregate::Sum if aggregate_field_ty(item, sources).is_some_and(Ty::is_float) => {
                    Accumulator::FloatSum {
                        total: 0.0,
                        seen: false,
                    }
                }
                Aggregate::Sum => Accumulator::Sum {
                    total: 0,
                    seen: false,
                },
                Aggregate::Min => Accumulator::Min(None),
                Aggregate::Max => Accumulator::Max(None),
                Aggregate::Avg if aggregate_field_ty(item, sources).is_some_and(Ty::is_float) => {
                    Accumulator::FloatAvg {
                        total: 0.0,
                        count: 0,
                    }
                }
                Aggregate::Avg => Accumulator::Avg { total: 0, count: 0 },
            },
        })
        .collect()
}

fn aggregate_field_ty(item: &SelectItem, sources: &[QuerySource]) -> Option<Ty> {
    let SelectExpr::Aggregate {
        field: Some(reference),
        ..
    } = &item.expr
    else {
        return None;
    };
    let (source, index) = resolve_query_field(reference, sources).ok()?;
    Some(source.table.fields[index].ty)
}

fn update_accumulator(
    accumulator: &mut Accumulator,
    item: &SelectItem,
    sources: &[QuerySource],
    tuple: &Row,
) -> Result<(), DbError> {
    match (&item.expr, accumulator) {
        (SelectExpr::Field(reference), Accumulator::Field(slot)) => {
            if slot.is_none() {
                *slot = Some(eval_field_value(reference, sources, tuple)?);
            }
            Ok(())
        }
        (SelectExpr::Aggregate { function, .. }, accumulator) => match function {
            Aggregate::Count => {
                if let Accumulator::Count(count) = accumulator {
                    *count = count.saturating_add(1);
                }
                Ok(())
            }
            Aggregate::Sum | Aggregate::Avg => {
                let SelectExpr::Aggregate {
                    field: Some(reference),
                    ..
                } = &item.expr
                else {
                    return Err(DbError("aggregate needs a field".to_string()));
                };
                let value = eval_field_value(reference, sources, tuple)?;
                if matches!(value, Value::Null(_)) {
                    return Ok(());
                }
                match accumulator {
                    Accumulator::Sum { total, seen } => {
                        let value = integer_to_i128(&value).ok_or_else(|| {
                            DbError("aggregate needs an integer field".to_string())
                        })?;
                        *total += value;
                        *seen = true;
                    }
                    Accumulator::Avg { total, count } => {
                        let value = integer_to_i128(&value).ok_or_else(|| {
                            DbError("aggregate needs an integer field".to_string())
                        })?;
                        *total += value;
                        *count = count.saturating_add(1);
                    }
                    Accumulator::FloatSum { total, seen } => {
                        *total += float_to_f64(&value)
                            .ok_or_else(|| DbError("aggregate needs a float field".to_string()))?;
                        *seen = true;
                    }
                    Accumulator::FloatAvg { total, count } => {
                        *total += float_to_f64(&value)
                            .ok_or_else(|| DbError("aggregate needs a float field".to_string()))?;
                        *count = count.saturating_add(1);
                    }
                    _ => {}
                }
                Ok(())
            }
            Aggregate::Min | Aggregate::Max => {
                let SelectExpr::Aggregate {
                    field: Some(reference),
                    ..
                } = &item.expr
                else {
                    return Err(DbError("aggregate needs a field".to_string()));
                };
                let value = eval_field_value(reference, sources, tuple)?;
                if matches!(value, Value::Null(_)) {
                    return Ok(());
                }
                let slot = match accumulator {
                    Accumulator::Min(slot) | Accumulator::Max(slot) => slot,
                    _ => return Ok(()),
                };
                let replace = match slot {
                    Some(existing) => match function {
                        Aggregate::Min => &value < existing,
                        _ => &value > existing,
                    },
                    None => true,
                };
                if replace {
                    *slot = Some(value);
                }
                Ok(())
            }
        },
        _ => Ok(()),
    }
}

fn finish_accumulator(accumulator: Accumulator, ty: Ty) -> Value {
    match accumulator {
        Accumulator::Field(value) | Accumulator::Min(value) | Accumulator::Max(value) => {
            value.unwrap_or_else(|| default_value(ty))
        }
        Accumulator::Count(count) => Value::I64(count),
        Accumulator::Sum { total, .. } => Value::I64(clamp_i128(total)),
        Accumulator::FloatSum { total, seen } => float_value(ty, if seen { total } else { 0.0 }),
        Accumulator::Avg { total, count } => {
            if count == 0 {
                Value::I64(0)
            } else {
                Value::I64(clamp_i128(total / count as i128))
            }
        }
        Accumulator::FloatAvg { total, count } => {
            if count == 0 {
                float_value(ty, 0.0)
            } else {
                float_value(ty, total / count as f64)
            }
        }
    }
}

fn float_value(ty: Ty, value: f64) -> Value {
    if ty == Ty::F32 {
        Value::F32(value as f32)
    } else {
        Value::F64(value)
    }
}

fn float_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::F32(value) => Some(*value as f64),
        Value::F64(value) => Some(*value),
        _ => None,
    }
}

fn clamp_i128(value: i128) -> i64 {
    value.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn default_value(ty: Ty) -> Value {
    match ty {
        Ty::I8 => Value::I8(0),
        Ty::I16 => Value::I16(0),
        Ty::I32 => Value::I32(0),
        Ty::I64 => Value::I64(0),
        Ty::U8 => Value::U8(0),
        Ty::U16 => Value::U16(0),
        Ty::U32 => Value::U32(0),
        Ty::U64 => Value::U64(0),
        Ty::F32 => Value::F32(0.0),
        Ty::F64 => Value::F64(0.0),
        Ty::Bool => Value::Bool(false),
        Ty::Text => Value::Text(String::new()),
        Ty::Bytes => Value::Bytes(Vec::new()),
        Ty::Uuid => Value::Uuid([0; 16]),
    }
}

fn integer_to_i128(value: &Value) -> Option<i128> {
    Some(match value {
        Value::I8(value) => *value as i128,
        Value::I16(value) => *value as i128,
        Value::I32(value) => *value as i128,
        Value::I64(value) => *value as i128,
        Value::U8(value) => *value as i128,
        Value::U16(value) => *value as i128,
        Value::U32(value) => *value as i128,
        Value::U64(value) => *value as i128,
        _ => return None,
    })
}

fn coerce_value(value: Value, ty: Ty) -> Option<Value> {
    value.coerce_to(ty)
}

fn compare_values(left: Value, right: Value) -> Option<Ordering> {
    if left.ty() == right.ty() {
        return Some(left.cmp(&right));
    }
    if let Some(converted) = left.clone().coerce_to(right.ty()) {
        return Some(converted.cmp(&right));
    }
    if let Some(converted) = right.clone().coerce_to(left.ty()) {
        return Some(left.cmp(&converted));
    }
    None
}

fn compare_nullable(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Null(_), Value::Null(_)) => Ordering::Equal,
        (Value::Null(_), _) => Ordering::Less,
        (_, Value::Null(_)) => Ordering::Greater,
        _ => left.cmp(right),
    }
}

fn eval_field_value(
    reference: &FieldRef,
    sources: &[QuerySource],
    tuple: &Row,
) -> Result<Value, DbError> {
    let (source, field) = resolve_query_field(reference, sources)?;
    Ok(tuple[source.offset + field].clone())
}

fn resolve_query_field<'a>(
    reference: &FieldRef,
    sources: &'a [QuerySource],
) -> Result<(&'a QuerySource, usize), DbError> {
    match &reference.qualifier {
        Some(qualifier) => {
            let source = sources
                .iter()
                .find(|source| &source.qualifier == qualifier)
                .ok_or_else(|| DbError(format!("unknown source `{qualifier}`")))?;
            let (index, _) = source
                .table
                .field(&reference.field)
                .ok_or_else(|| DbError(format!("unknown field `{reference}`")))?;
            Ok((source, index))
        }
        None => {
            let matches = sources
                .iter()
                .filter_map(|source| {
                    source
                        .table
                        .field(&reference.field)
                        .map(|(index, _)| (source, index))
                })
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => Err(DbError(format!("unknown field `{}`", reference.field))),
                [only] => Ok(*only),
                _ => Err(DbError(format!(
                    "field `{}` is ambiguous; qualify it with a source name",
                    reference.field
                ))),
            }
        }
    }
}

fn kleene_and(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
        _ => Value::Null(Ty::Bool),
    }
}

fn kleene_or(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
        _ => Value::Null(Ty::Bool),
    }
}

fn eval_expr_sources(
    expr: &Expr,
    sources: &[QuerySource],
    tuple: &Row,
    bindings: &HashMap<String, Value>,
) -> Result<Value, DbError> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left = eval_operand_sources(left, sources, tuple, bindings)?;
            let right = eval_operand_sources(right, sources, tuple, bindings)?;
            if matches!(left, Value::Null(_)) || matches!(right, Value::Null(_)) {
                return Ok(Value::Null(Ty::Bool));
            }
            let ordering = compare_values(left.clone(), right.clone()).ok_or_else(|| {
                DbError(format!(
                    "cannot compare `{}` with `{}`",
                    left.ty(),
                    right.ty()
                ))
            })?;
            Ok(Value::Bool(match op {
                Cmp::Eq => ordering == Ordering::Equal,
                Cmp::Ne => ordering != Ordering::Equal,
                Cmp::Lt => ordering == Ordering::Less,
                Cmp::Le => ordering != Ordering::Greater,
                Cmp::Gt => ordering == Ordering::Greater,
                Cmp::Ge => ordering != Ordering::Less,
            }))
        }
        Expr::And(left, right) => Ok(kleene_and(
            eval_expr_sources(left, sources, tuple, bindings)?,
            eval_expr_sources(right, sources, tuple, bindings)?,
        )),
        Expr::Or(left, right) => Ok(kleene_or(
            eval_expr_sources(left, sources, tuple, bindings)?,
            eval_expr_sources(right, sources, tuple, bindings)?,
        )),
        Expr::IsNull { field } => Ok(Value::Bool(matches!(
            eval_field_value(field, sources, tuple)?,
            Value::Null(_)
        ))),
        Expr::IsNotNull { field } => Ok(Value::Bool(!matches!(
            eval_field_value(field, sources, tuple)?,
            Value::Null(_)
        ))),
    }
}

fn eval_operand_sources(
    operand: &Operand,
    sources: &[QuerySource],
    tuple: &Row,
    bindings: &HashMap<String, Value>,
) -> Result<Value, DbError> {
    match operand {
        Operand::Field(name) => {
            let reference = FieldRef::parse(name)
                .ok_or_else(|| DbError(format!("invalid field reference `{name}`")))?;
            eval_field_value(&reference, sources, tuple)
        }
        Operand::Param(name) => bindings
            .get(name)
            .cloned()
            .ok_or_else(|| DbError(format!("unbound parameter `{name}`"))),
        Operand::Literal(value) => Ok(value.clone()),
    }
}

fn limit_count(limit: &Limit, bindings: &HashMap<String, Value>) -> Result<usize, DbError> {
    match limit {
        Limit::Literal(value) => Ok(*value as usize),
        Limit::Param(name) => match bindings.get(name) {
            Some(Value::I64(value)) if *value >= 0 => Ok(*value as usize),
            Some(Value::I64(_)) => Err(DbError(format!(
                "limit parameter `{name}` must not be negative"
            ))),
            Some(_) => Err(DbError(format!("limit parameter `{name}` must be an i64"))),
            None => Err(DbError(format!(
                "limit references unknown parameter `{name}`"
            ))),
        },
    }
}

fn offset_count(offset: &Limit, bindings: &HashMap<String, Value>) -> Result<usize, DbError> {
    match offset {
        Limit::Literal(value) => Ok(*value as usize),
        Limit::Param(name) => match bindings.get(name) {
            Some(Value::I64(value)) if *value >= 0 => Ok(*value as usize),
            Some(Value::I64(_)) => Err(DbError(format!(
                "offset parameter `{name}` must not be negative"
            ))),
            Some(_) => Err(DbError(format!("offset parameter `{name}` must be an i64"))),
            None => Err(DbError(format!(
                "offset references unknown parameter `{name}`"
            ))),
        },
    }
}

fn bind_params(operation: &Operation, args: &[Value]) -> Result<HashMap<String, Value>, DbError> {
    if args.len() != operation.params.len() {
        return Err(DbError(format!(
            "operation `{}` expects {} arguments, got {}",
            operation.name,
            operation.params.len(),
            args.len()
        )));
    }

    let mut bindings = HashMap::with_capacity(args.len());
    for (param, value) in operation.params.iter().zip(args) {
        if value.ty() != param.ty {
            return Err(DbError(format!(
                "parameter `{}` expects `{}`, got `{}`",
                param.name,
                param.ty,
                value.ty()
            )));
        }
        bindings.insert(param.name.clone(), value.clone());
    }
    Ok(bindings)
}

fn eval_expr(
    expr: &Expr,
    table: &Table,
    row: &Row,
    bindings: &HashMap<String, Value>,
) -> Result<Value, DbError> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left = eval_operand(left, table, row, bindings)?;
            let right = eval_operand(right, table, row, bindings)?;
            if matches!(left, Value::Null(_)) || matches!(right, Value::Null(_)) {
                return Ok(Value::Null(Ty::Bool));
            }
            let ordering = compare_values(left.clone(), right.clone()).ok_or_else(|| {
                DbError(format!(
                    "cannot compare `{}` with `{}`",
                    left.ty(),
                    right.ty()
                ))
            })?;
            Ok(Value::Bool(match op {
                Cmp::Eq => ordering == Ordering::Equal,
                Cmp::Ne => ordering != Ordering::Equal,
                Cmp::Lt => ordering == Ordering::Less,
                Cmp::Le => ordering != Ordering::Greater,
                Cmp::Gt => ordering == Ordering::Greater,
                Cmp::Ge => ordering != Ordering::Less,
            }))
        }
        Expr::And(left, right) => Ok(kleene_and(
            eval_expr(left, table, row, bindings)?,
            eval_expr(right, table, row, bindings)?,
        )),
        Expr::Or(left, right) => Ok(kleene_or(
            eval_expr(left, table, row, bindings)?,
            eval_expr(right, table, row, bindings)?,
        )),
        Expr::IsNull { field } => {
            let (index, _) = table
                .field(&field.field)
                .ok_or_else(|| DbError(format!("unknown field `{}`", field.field)))?;
            Ok(Value::Bool(matches!(row[index], Value::Null(_))))
        }
        Expr::IsNotNull { field } => {
            let (index, _) = table
                .field(&field.field)
                .ok_or_else(|| DbError(format!("unknown field `{}`", field.field)))?;
            Ok(Value::Bool(!matches!(row[index], Value::Null(_))))
        }
    }
}

fn eval_operand(
    operand: &Operand,
    table: &Table,
    row: &Row,
    bindings: &HashMap<String, Value>,
) -> Result<Value, DbError> {
    match operand {
        Operand::Field(name) => table
            .field(name)
            .map(|(index, _)| row[index].clone())
            .ok_or_else(|| DbError(format!("unknown field `{name}`"))),
        Operand::Param(name) => bindings
            .get(name)
            .cloned()
            .ok_or_else(|| DbError(format!("unbound parameter `{name}`"))),
        Operand::Literal(value) => Ok(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::parse::parse_workload;

    const SOURCE: &str = include_str!("../../examples/messages.lightshade");

    fn database() -> Database {
        let workload = parse_workload(SOURCE).unwrap();
        validate(&workload).unwrap();
        Database::new(workload)
    }

    fn insert(db: &mut Database, id: i64, inbox: i64, sent: i64, read: bool, body: &str) {
        db.execute(
            "insert_message",
            &[
                Value::I64(id),
                Value::I64(inbox),
                Value::I64(sent),
                Value::Bool(read),
                Value::Text(body.to_string()),
            ],
        )
        .unwrap();
    }

    #[test]
    fn queries_orders_counts_and_updates() {
        let mut db = database();
        insert(&mut db, 1, 1, 100, false, "first");
        insert(&mut db, 2, 1, 300, true, "second");
        insert(&mut db, 3, 1, 200, false, "third");
        insert(&mut db, 4, 2, 999, false, "other inbox");

        let recent = db
            .execute("recent_messages", &[Value::I64(1), Value::I64(10)])
            .unwrap();
        match recent {
            OpResult::Many(rows) => {
                let ids: Vec<i64> = rows
                    .iter()
                    .map(|row| match &row[0] {
                        Value::I64(id) => *id,
                        _ => panic!("id should be i64"),
                    })
                    .collect();
                assert_eq!(ids, vec![2, 3, 1]);
            }
            other => panic!("unexpected result: {other:?}"),
        }

        let unread = db.execute("unread_count", &[Value::I64(1)]).unwrap();
        assert_eq!(unread, OpResult::One(Value::I64(2)));

        db.execute("mark_read", &[Value::I64(3)]).unwrap();
        let unread = db.execute("unread_count", &[Value::I64(1)]).unwrap();
        assert_eq!(unread, OpResult::One(Value::I64(1)));
    }

    #[test]
    fn limits_results() {
        let mut db = database();
        insert(&mut db, 1, 1, 100, false, "first");
        insert(&mut db, 2, 1, 200, false, "second");
        insert(&mut db, 3, 1, 300, false, "third");

        let recent = db
            .execute("recent_messages", &[Value::I64(1), Value::I64(2)])
            .unwrap();
        match recent {
            OpResult::Many(rows) => assert_eq!(rows.len(), 2),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn offsets_skip_before_limits_apply() {
        const SOURCE: &str = r#"
table notes {
    id: i64 primary_key
    rank: i64
}

operation add(id: i64, rank: i64) {
    insert notes
}

operation page(limit: i64, skip: i64) -> many notes {
    order by rank asc
    limit limit
    offset skip
}

operation tail() -> many notes {
    order by rank asc
    limit 2
    offset 2
}
"#;
        let workload = parse_workload(SOURCE).unwrap();
        validate(&workload).unwrap();
        let mut db = Database::new(workload);
        for (id, rank) in [(1, 10), (2, 20), (3, 30), (4, 40)] {
            db.execute("add", &[Value::I64(id), Value::I64(rank)])
                .unwrap();
        }
        let ids = |result: OpResult| match result {
            OpResult::Many(rows) => rows
                .iter()
                .map(|row| match &row[0] {
                    Value::I64(id) => *id,
                    _ => panic!("id should be i64"),
                })
                .collect::<Vec<_>>(),
            other => panic!("unexpected result: {other:?}"),
        };

        let page = db.execute("tail", &[]).unwrap();
        assert_eq!(ids(page), vec![3, 4]);

        let page = db.execute("page", &[Value::I64(2), Value::I64(1)]).unwrap();
        assert_eq!(ids(page), vec![2, 3]);

        let page = db
            .execute("page", &[Value::I64(100), Value::I64(3)])
            .unwrap();
        assert_eq!(ids(page), vec![4]);

        let page = db
            .execute("page", &[Value::I64(100), Value::I64(9)])
            .unwrap();
        assert_eq!(ids(page), Vec::<i64>::new());

        let error = db
            .execute("page", &[Value::I64(2), Value::I64(-1)])
            .unwrap_err();
        assert!(error
            .0
            .contains("offset parameter `skip` must not be negative"));
    }

    #[test]
    fn empty_tables_are_normal_relations() {
        let mut db = database();
        let recent = db
            .execute("recent_messages", &[Value::I64(42), Value::I64(10)])
            .unwrap();
        assert_eq!(recent, OpResult::Many(Vec::new()));
        let unread = db.execute("unread_count", &[Value::I64(42)]).unwrap();
        assert_eq!(unread, OpResult::One(Value::I64(0)));
    }

    #[test]
    fn deletes_rows_by_primary_key_and_filter() {
        const SOURCE: &str = r#"
table items {
    id: i64 primary_key
    value: i64
}

operation add(id: i64, value: i64) {
    insert items
}

operation remove(wanted: i64) {
    delete items where id == wanted
}

operation purge(max: i64) {
    delete items where value <= max
}

operation total() -> one i64 {
    count items where value >= 0
}
"#;
        let workload = parse_workload(SOURCE).unwrap();
        validate(&workload).unwrap();
        let mut db = Database::new(workload);
        for (id, value) in [(1, 10), (2, 20), (3, 30)] {
            db.execute("add", &[Value::I64(id), Value::I64(value)])
                .unwrap();
        }

        db.execute("remove", &[Value::I64(2)]).unwrap();
        assert_eq!(
            db.execute("total", &[]).unwrap(),
            OpResult::One(Value::I64(2))
        );

        db.execute("purge", &[Value::I64(10)]).unwrap();
        assert_eq!(
            db.execute("total", &[]).unwrap(),
            OpResult::One(Value::I64(1))
        );
    }

    const INBOX_SOURCE: &str = include_str!("../../examples/inbox_digest.lightshade");

    fn inbox_database() -> Database {
        let workload = parse_workload(INBOX_SOURCE).unwrap();
        validate(&workload).unwrap();
        Database::new(workload)
    }

    fn add_message(db: &mut Database, id: i64, inbox: i64, sent: i64, read: bool, body: &str) {
        db.execute(
            "add_message",
            &[
                Value::I64(id),
                Value::I64(inbox),
                Value::I64(sent),
                Value::Bool(read),
                Value::Text(body.to_string()),
            ],
        )
        .unwrap();
    }

    fn rows(db: &mut Database, operation: &str, args: &[Value]) -> Vec<Vec<Value>> {
        match db.execute(operation, args).unwrap() {
            OpResult::Many(rows) => rows,
            other => panic!("unexpected result: {other:?}"),
        }
    }

    fn integers(row: &[Value]) -> Vec<i64> {
        row.iter()
            .map(|value| match value {
                Value::I64(value) => *value,
                other => panic!("expected i64, found {other:?}"),
            })
            .collect()
    }

    #[test]
    fn projects_joins_groups_and_aggregates() {
        let mut db = inbox_database();
        db.execute("add_inbox", &[Value::I64(1), Value::Text("alice".into())])
            .unwrap();
        db.execute("add_inbox", &[Value::I64(2), Value::Text("bob".into())])
            .unwrap();
        add_message(&mut db, 1, 1, 100, false, "first");
        add_message(&mut db, 2, 1, 200, true, "second");
        add_message(&mut db, 3, 1, 300, false, "third");
        add_message(&mut db, 4, 2, 400, false, "other inbox");

        let digest = rows(&mut db, "unread_digest", &[]);
        assert_eq!(digest.len(), 2);
        assert_eq!(digest[0][0], Value::Text("alice".into()));
        assert_eq!(integers(&digest[0][1..]), vec![2, 300, 100, 400]);
        assert_eq!(digest[1][0], Value::Text("bob".into()));
        assert_eq!(integers(&digest[1][1..]), vec![1, 400, 400, 400]);

        let overall = rows(&mut db, "overall", &[]);
        assert_eq!(overall.len(), 1);
        assert_eq!(integers(&overall[0]), vec![4, 100, 400, 250, 1000]);

        let excerpts = rows(&mut db, "excerpts", &[Value::I64(150)]);
        assert_eq!(excerpts.len(), 2);
        assert_eq!(excerpts[0][0], Value::I64(3));
        assert_eq!(excerpts[0][1], Value::Text("alice".into()));
        assert_eq!(excerpts[0][2], Value::Text("third".into()));
        assert_eq!(excerpts[1][0], Value::I64(2));
        assert_eq!(excerpts[1][1], Value::Text("alice".into()));
        assert_eq!(excerpts[1][2], Value::Text("second".into()));
    }

    #[test]
    fn empty_aggregate_queries_return_one_default_row() {
        let mut db = inbox_database();
        let overall = rows(&mut db, "overall", &[]);
        assert_eq!(overall.len(), 1);
        assert_eq!(integers(&overall[0]), vec![0, 0, 0, 0, 0]);
        assert!(rows(&mut db, "unread_digest", &[]).is_empty());
    }

    #[test]
    fn rejects_duplicate_primary_keys() {
        let mut db = database();
        insert(&mut db, 1, 1, 100, false, "first");
        let error = db
            .execute(
                "insert_message",
                &[
                    Value::I64(1),
                    Value::I64(2),
                    Value::I64(200),
                    Value::Bool(false),
                    Value::Text("duplicate".to_string()),
                ],
            )
            .unwrap_err();
        assert!(error.0.contains("duplicate primary key"));
    }
}
