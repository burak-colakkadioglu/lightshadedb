use super::common::{emit_field_read, literal_with_ty, type_tag};
use crate::ir::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IndexSpec {
    pub(crate) table: String,
    pub(crate) eq: Vec<String>,
    pub(crate) order: Vec<(String, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CounterSpec {
    pub(crate) table: String,
    pub(crate) eq: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Plan {
    pub(crate) indexes: Vec<IndexSpec>,
    pub(crate) counters: Vec<CounterSpec>,
    pub(crate) query_index: HashMap<String, usize>,
    pub(crate) query_counter: HashMap<String, usize>,
    pub(crate) advanced_primary: HashMap<String, usize>,
    pub(crate) advanced_join: HashMap<(String, usize), usize>,
    pub(crate) pk_range: HashSet<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct KeyField {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    pub(crate) reverse: bool,
}

pub struct Fingerprint(u64);

impl Fingerprint {
    fn new() -> Fingerprint {
        Fingerprint(0xcbf29ce484222325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= value as u64;
        self.0 = self.0.wrapping_mul(0x100000001b3);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.byte(*byte);
        }
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }

    fn text(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes(value.as_bytes());
    }

    fn ty(&mut self, ty: Ty) {
        self.byte(type_tag(ty));
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::I8(value) => {
                self.byte(3);
                self.byte(*value as u8);
            }
            Value::I16(value) => {
                self.byte(4);
                self.bytes(&value.to_le_bytes());
            }
            Value::I32(value) => {
                self.byte(5);
                self.bytes(&value.to_le_bytes());
            }
            Value::I64(value) => {
                self.byte(0);
                self.i64(*value);
            }
            Value::U8(value) => {
                self.byte(6);
                self.byte(*value);
            }
            Value::U16(value) => {
                self.byte(7);
                self.bytes(&value.to_le_bytes());
            }
            Value::U32(value) => {
                self.byte(8);
                self.u32(*value);
            }
            Value::U64(value) => {
                self.byte(9);
                self.u64(*value);
            }
            Value::F32(value) => {
                self.byte(12);
                self.bytes(&value.to_bits().to_le_bytes());
            }
            Value::F64(value) => {
                self.byte(13);
                self.bytes(&value.to_bits().to_le_bytes());
            }
            Value::Bool(value) => {
                self.byte(1);
                self.byte(*value as u8);
            }
            Value::Text(value) => {
                self.byte(2);
                self.text(value);
            }
            Value::Bytes(value) => {
                self.byte(10);
                self.u32(value.len() as u32);
                self.bytes(value);
            }
            Value::Uuid(value) => {
                self.byte(11);
                self.bytes(value);
            }
            Value::Null(ty) => {
                self.byte(14);
                self.ty(*ty);
            }
        }
    }

    fn operand(&mut self, operand: &Operand) {
        match operand {
            Operand::Field(field) => {
                self.byte(0);
                self.text(field);
            }
            Operand::Param(param) => {
                self.byte(1);
                self.text(param);
            }
            Operand::Literal(value) => {
                self.byte(2);
                self.value(value);
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Cmp { left, op, right } => {
                self.byte(0);
                self.operand(left);
                self.byte(match op {
                    Cmp::Eq => 0,
                    Cmp::Ne => 1,
                    Cmp::Lt => 2,
                    Cmp::Le => 3,
                    Cmp::Gt => 4,
                    Cmp::Ge => 5,
                });
                self.operand(right);
            }
            Expr::And(left, right) => {
                self.byte(1);
                self.expr(left);
                self.expr(right);
            }
            Expr::Or(left, right) => {
                self.byte(2);
                self.expr(left);
                self.expr(right);
            }
            Expr::IsNull { field } => {
                self.byte(3);
                self.text(field.qualifier.as_deref().unwrap_or(""));
                self.text(&field.field);
            }
            Expr::IsNotNull { field } => {
                self.byte(4);
                self.text(field.qualifier.as_deref().unwrap_or(""));
                self.text(&field.field);
            }
        }
    }

    fn limit(&mut self, limit: &Limit) {
        match limit {
            Limit::Literal(value) => {
                self.byte(0);
                self.u64(*value);
            }
            Limit::Param(param) => {
                self.byte(1);
                self.text(param);
            }
        }
    }

    fn field_ref(&mut self, reference: &FieldRef) {
        match &reference.qualifier {
            Some(qualifier) => {
                self.byte(1);
                self.text(qualifier);
            }
            None => self.byte(0),
        }
        self.text(&reference.field);
    }

    fn returns(&mut self, returns: &Returns) {
        match returns {
            Returns::None => self.byte(0),
            Returns::One(ty) => {
                self.byte(1);
                self.ty(*ty);
            }
            Returns::Many(table) => {
                self.byte(2);
                self.text(table);
            }
        }
    }

    fn body(&mut self, body: &Body) {
        match body {
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
                self.byte(0);
                self.text(table);
                match filter {
                    Some(filter) => {
                        self.byte(1);
                        self.expr(filter);
                    }
                    None => self.byte(0),
                }
                self.u32(order.len() as u32);
                for (field, descending) in order {
                    self.text(field);
                    self.byte(*descending as u8);
                }
                match (limit, offset) {
                    (None, None) => self.byte(0),
                    (Some(limit), None) => {
                        self.byte(1);
                        self.limit(limit);
                    }
                    (limit, Some(offset)) => {
                        self.byte(2);
                        match limit {
                            Some(limit) => {
                                self.byte(1);
                                self.limit(limit);
                            }
                            None => self.byte(0),
                        }
                        self.limit(offset);
                    }
                }
                self.byte(*count as u8);
                if alias.is_some()
                    || !joins.is_empty()
                    || !select.is_empty()
                    || !group_by.is_empty()
                {
                    self.byte(0xfe);
                    match alias {
                        Some(alias) => {
                            self.byte(1);
                            self.text(alias);
                        }
                        None => self.byte(0),
                    }
                    self.u32(joins.len() as u32);
                    for join in joins {
                        self.text(&join.table);
                        match &join.alias {
                            Some(alias) => {
                                self.byte(1);
                                self.text(alias);
                            }
                            None => self.byte(0),
                        }
                        self.field_ref(&join.left);
                        self.field_ref(&join.right);
                    }
                    self.u32(select.len() as u32);
                    for item in select {
                        match &item.expr {
                            SelectExpr::Field(reference) => {
                                self.byte(0);
                                self.field_ref(reference);
                            }
                            SelectExpr::Aggregate { function, field } => {
                                self.byte(1);
                                self.byte(match function {
                                    Aggregate::Count => 0,
                                    Aggregate::Sum => 1,
                                    Aggregate::Min => 2,
                                    Aggregate::Max => 3,
                                    Aggregate::Avg => 4,
                                });
                                match field {
                                    Some(field) => {
                                        self.byte(1);
                                        self.field_ref(field);
                                    }
                                    None => self.byte(0),
                                }
                            }
                        }
                        self.text(&item.alias);
                    }
                    self.u32(group_by.len() as u32);
                    for reference in group_by {
                        self.field_ref(reference);
                    }
                }
            }
            Body::Update {
                table,
                sets,
                filter,
            } => {
                self.byte(1);
                self.text(table);
                self.u32(sets.len() as u32);
                for (field, operand) in sets {
                    self.text(field);
                    self.operand(operand);
                }
                match filter {
                    Some(filter) => {
                        self.byte(1);
                        self.expr(filter);
                    }
                    None => self.byte(0),
                }
            }
            Body::Delete { table, filter } => {
                self.byte(2);
                self.text(table);
                match filter {
                    Some(filter) => {
                        self.byte(1);
                        self.expr(filter);
                    }
                    None => self.byte(0),
                }
            }
            Body::Insert { table } => {
                self.byte(3);
                self.text(table);
            }
        }
    }
}

pub fn schema_fingerprint(workload: &Workload) -> u64 {
    let mut fingerprint = Fingerprint::new();
    fingerprint.bytes(b"lightshade-schema-v2");
    let mut tables = workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
        .collect::<Vec<_>>();
    tables.sort_by(|left, right| left.name.cmp(&right.name));
    fingerprint.u32(tables.len() as u32);
    for table in tables {
        fingerprint.text(&table.name);
        let mut fields = table.fields.iter().collect::<Vec<_>>();
        fields.sort_by(|left, right| left.name.cmp(&right.name));
        fingerprint.u32(fields.len() as u32);
        for field in fields {
            fingerprint.text(&field.name);
            fingerprint.ty(field.ty);
            fingerprint.byte(field.primary_key as u8);
            if field.nullable {
                fingerprint.byte(1);
            }
        }
    }
    fingerprint.0
}

pub fn workload_fingerprint(workload: &Workload) -> u64 {
    let mut fingerprint = Fingerprint::new();
    fingerprint.bytes(b"lightshade-workload-v1");
    fingerprint.u32(
        workload
            .tables
            .iter()
            .filter(|table| !workload.is_projection(&table.name))
            .count() as u32,
    );
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        fingerprint.text(&table.name);
        fingerprint.u32(table.fields.len() as u32);
        for field in &table.fields {
            fingerprint.text(&field.name);
            fingerprint.ty(field.ty);
            fingerprint.byte(field.primary_key as u8);
            if field.nullable {
                fingerprint.byte(1);
            }
            match &field.default {
                Some(value) => {
                    fingerprint.byte(1);
                    fingerprint.value(value);
                }
                None => fingerprint.byte(0),
            }
        }
    }
    fingerprint.u32(workload.operations.len() as u32);
    for operation in &workload.operations {
        fingerprint.text(&operation.name);
        fingerprint.u32(operation.params.len() as u32);
        for param in &operation.params {
            fingerprint.text(&param.name);
            fingerprint.ty(param.ty);
            if param.nullable {
                fingerprint.byte(1);
            }
        }
        fingerprint.returns(&operation.returns);
        fingerprint.body(&operation.body);
    }
    fingerprint.0
}

pub(crate) fn is_advanced_query(body: &Body) -> bool {
    match body {
        Body::Query {
            alias,
            joins,
            select,
            group_by,
            ..
        } => alias.is_some() || !joins.is_empty() || !select.is_empty() || !group_by.is_empty(),
        _ => false,
    }
}

pub(crate) fn build_plan(workload: &Workload) -> Result<Plan, String> {
    let mut plan = Plan::default();

    for operation in &workload.operations {
        let Body::Query {
            table,
            alias,
            joins,
            select,
            group_by,
            filter,
            order,
            count,
            ..
        } = &operation.body
        else {
            continue;
        };

        if alias.is_some() || !joins.is_empty() || !select.is_empty() || !group_by.is_empty() {
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
            if let Some(constraints) = advanced_eq_constraints(filter, 0, &sources)? {
                let low_selectivity = constraints.iter().any(|(name, _)| {
                    primary
                        .field(name)
                        .is_some_and(|(_, field)| field.ty == Ty::Bool)
                });
                let mut eq = constraints
                    .iter()
                    .map(|(field, _)| field.clone())
                    .collect::<Vec<_>>();
                eq.retain(|field| !field_is_nullable(workload, &primary.name, field));
                if !eq.is_empty() && !low_selectivity {
                    let mut normalized = eq.clone();
                    normalized.sort();
                    let index = plan
                        .indexes
                        .iter()
                        .position(|existing| {
                            if existing.table != primary.name
                                || existing.eq.len() != normalized.len()
                            {
                                return false;
                            }
                            let mut existing_eq = existing.eq.clone();
                            existing_eq.sort();
                            existing_eq == normalized
                        })
                        .unwrap_or_else(|| {
                            intern(
                                &mut plan.indexes,
                                IndexSpec {
                                    table: primary.name.clone(),
                                    eq,
                                    order: Vec::new(),
                                },
                            )
                        });
                    plan.advanced_primary.insert(operation.name.clone(), index);
                }
            }
            for (offset, join) in joins.iter().enumerate() {
                let depth = offset + 1;
                let joined = sources[depth].0;
                let (left_index, _, left_field) = resolve_source(&join.left, &sources)?;
                let (right_index, _, right_field) = resolve_source(&join.right, &sources)?;
                let joined_field = if left_index == depth && right_index < depth {
                    left_field
                } else if right_index == depth && left_index < depth {
                    right_field
                } else {
                    continue;
                };
                if joined
                    .primary_key()
                    .is_some_and(|key| key.name == joined_field.name)
                {
                    continue;
                }
                if joined_field.nullable {
                    continue;
                }
                let spec = IndexSpec {
                    table: joined.name.clone(),
                    eq: vec![joined_field.name.clone()],
                    order: Vec::new(),
                };
                let index = intern(&mut plan.indexes, spec);
                plan.advanced_join
                    .insert((operation.name.clone(), depth), index);
            }
            continue;
        }

        if pk_eq_operand(filter, get_table(workload, table)?).is_some() {
            continue;
        }

        if *count {
            if let Some(constraints) = eq_constraints(filter) {
                if constraints
                    .iter()
                    .any(|(field, _)| field_is_nullable(workload, table, field))
                {
                    continue;
                }
                let eq = constraints
                    .iter()
                    .map(|(field, _)| field.clone())
                    .collect::<Vec<_>>();
                let spec = CounterSpec {
                    table: table.clone(),
                    eq,
                };
                let index = intern(&mut plan.counters, spec);
                plan.query_counter.insert(operation.name.clone(), index);
            }
        } else if !order.is_empty() {
            let primary = get_table(workload, table)?;
            let primary_key_range = primary.primary_key().is_some_and(|key| {
                order.len() == 1
                    && order[0].0 == key.name
                    && !order[0].1
                    && has_lower_bound(filter, &key.name)
            });
            if primary_key_range {
                plan.pk_range.insert(operation.name.clone());
            } else if let Some(constraints) = index_eq_constraints(filter) {
                if constraints
                    .iter()
                    .any(|(field, _)| field_is_nullable(workload, table, field))
                    || order
                        .iter()
                        .any(|(field, _)| field_is_nullable(workload, table, field))
                {
                    continue;
                }
                let eq = constraints
                    .iter()
                    .map(|(field, _)| field.clone())
                    .collect::<Vec<_>>();
                let spec = IndexSpec {
                    table: table.clone(),
                    eq,
                    order: order.clone(),
                };
                let index = intern(&mut plan.indexes, spec);
                plan.query_index.insert(operation.name.clone(), index);
            }
        }
    }

    Ok(plan)
}

pub(crate) fn intern<T: PartialEq + Clone>(items: &mut Vec<T>, item: T) -> usize {
    if let Some(index) = items.iter().position(|existing| existing == &item) {
        return index;
    }
    items.push(item);
    items.len() - 1
}

pub(crate) fn eq_constraints(filter: &Option<Expr>) -> Option<Vec<(String, Operand)>> {
    let Some(filter) = filter else {
        return Some(Vec::new());
    };
    let mut constraints = Vec::new();
    if !collect_eq(filter, &mut constraints) {
        return None;
    }
    unique_constraints(constraints)
}

pub(crate) fn index_eq_constraints(filter: &Option<Expr>) -> Option<Vec<(String, Operand)>> {
    let Some(filter) = filter else {
        return Some(Vec::new());
    };
    let mut constraints = Vec::new();
    if !collect_eq_partial(filter, &mut constraints) {
        return None;
    }
    unique_constraints(constraints)
}

pub(crate) fn unique_constraints(
    constraints: Vec<(String, Operand)>,
) -> Option<Vec<(String, Operand)>> {
    let mut seen = HashSet::new();
    for (field, _) in &constraints {
        if !seen.insert(field.clone()) {
            return None;
        }
    }
    Some(constraints)
}

pub(crate) fn collect_eq_partial(expr: &Expr, constraints: &mut Vec<(String, Operand)>) -> bool {
    match expr {
        Expr::IsNull { .. } | Expr::IsNotNull { .. } => true,
        Expr::Cmp {
            left,
            op: Cmp::Eq,
            right,
        } => {
            let (field, other) = match (left, right) {
                (Operand::Field(field), other) if !matches!(other, Operand::Field(_)) => {
                    (field, other)
                }
                (other, Operand::Field(field)) if !matches!(other, Operand::Field(_)) => {
                    (field, other)
                }
                _ => return true,
            };
            constraints.push((field.clone(), other.clone()));
            true
        }
        Expr::Cmp { .. } => true,
        Expr::And(left, right) => {
            collect_eq_partial(left, constraints) && collect_eq_partial(right, constraints)
        }
        Expr::Or(_, _) => false,
    }
}

pub(crate) fn collect_eq(expr: &Expr, constraints: &mut Vec<(String, Operand)>) -> bool {
    match expr {
        Expr::IsNull { .. } | Expr::IsNotNull { .. } => false,
        Expr::Cmp {
            left,
            op: Cmp::Eq,
            right,
        } => {
            let (field, other) = match (left, right) {
                (Operand::Field(field), other) if !matches!(other, Operand::Field(_)) => {
                    (field, other)
                }
                (other, Operand::Field(field)) if !matches!(other, Operand::Field(_)) => {
                    (field, other)
                }
                _ => return false,
            };
            constraints.push((field.clone(), other.clone()));
            true
        }
        Expr::And(left, right) => collect_eq(left, constraints) && collect_eq(right, constraints),
        _ => false,
    }
}

pub(crate) fn get_table<'a>(workload: &'a Workload, name: &str) -> Result<&'a Table, String> {
    workload
        .table(name)
        .ok_or_else(|| format!("unknown table `{name}`"))
}

pub(crate) fn get_field<'a>(table: &'a Table, name: &str) -> Result<&'a Field, String> {
    table
        .field(name)
        .map(|(_, field)| field)
        .ok_or_else(|| format!("unknown field `{name}` in table `{}`", table.name))
}

pub(crate) fn field_is_nullable(workload: &Workload, table: &str, field: &str) -> bool {
    get_table(workload, table)
        .ok()
        .and_then(|table| table.field(field))
        .is_some_and(|(_, field)| field.nullable)
}

pub(crate) fn eq_key_fields(eq: &[String], table: &Table) -> Result<Vec<KeyField>, String> {
    eq.iter()
        .map(|name| {
            let field = get_field(table, name)?;
            Ok(KeyField {
                name: field.name.clone(),
                ty: field.ty,
                reverse: false,
            })
        })
        .collect()
}

pub(crate) fn index_inner_fields(spec: &IndexSpec, table: &Table) -> Result<Vec<KeyField>, String> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();

    for (name, descending) in &spec.order {
        if spec.eq.contains(name) {
            continue;
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        let field = get_field(table, name)?;
        fields.push(KeyField {
            name: field.name.clone(),
            ty: field.ty,
            reverse: *descending,
        });
    }

    let primary_key = table
        .primary_key()
        .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
    if !fields.iter().any(|field| field.name == primary_key.name) {
        fields.push(KeyField {
            name: primary_key.name.clone(),
            ty: primary_key.ty,
            reverse: false,
        });
    }

    Ok(fields)
}

pub(crate) fn index_start_key(
    filter: &Option<Expr>,
    inner: &[KeyField],
    index: usize,
) -> Option<String> {
    let first = inner.first()?;
    if first.reverse {
        return None;
    }
    let filter = filter.as_ref()?;
    let mut bound = None;
    find_lower_bound(filter, &first.name, &mut bound);
    let value = match bound? {
        Operand::Param(name) => format!("{name}.clone()"),
        Operand::Literal(value) => literal_with_ty(&value, Some(first.ty)),
        Operand::Field(_) => return None,
    };

    let mut fields = Vec::with_capacity(inner.len());
    fields.push(format!("f0: {value}.into()"));
    for (position, field) in inner.iter().enumerate().skip(1) {
        fields.push(format!("f{position}: {}", min_key_value(field)?));
    }
    Some(format!("Idx{index}Key {{ {} }}", fields.join(", ")))
}

pub(crate) fn find_lower_bound(expr: &Expr, field: &str, bound: &mut Option<Operand>) {
    match expr {
        Expr::Cmp {
            left: Operand::Field(left),
            op: Cmp::Ge,
            right,
        } if left == field && !matches!(right, Operand::Field(_)) => {
            *bound = Some(right.clone());
        }
        Expr::Cmp {
            left: other,
            op: Cmp::Le,
            right: Operand::Field(right),
        } if right == field && !matches!(other, Operand::Field(_)) => {
            *bound = Some(other.clone());
        }
        Expr::And(left, right) => {
            find_lower_bound(left, field, bound);
            find_lower_bound(right, field, bound);
        }
        _ => {}
    }
}

pub(crate) fn has_lower_bound(filter: &Option<Expr>, field: &str) -> bool {
    let Some(filter) = filter else {
        return false;
    };
    let mut bound = None;
    find_lower_bound(filter, field, &mut bound);
    bound.is_some()
}

pub(crate) fn primary_key_start_key(filter: &Option<Expr>, table: &Table) -> Option<String> {
    let primary_key = table.primary_key()?;
    let filter = filter.as_ref()?;
    let mut bound = None;
    find_lower_bound(filter, &primary_key.name, &mut bound);
    match bound? {
        Operand::Param(name) => Some(format!("{name}.clone()")),
        Operand::Literal(value) => Some(literal_with_ty(&value, Some(primary_key.ty))),
        Operand::Field(_) => None,
    }
}

pub(crate) fn min_key_value(field: &KeyField) -> Option<String> {
    let value = match (field.ty, field.reverse) {
        (Ty::I8, false) => "i8::MIN".to_string(),
        (Ty::I8, true) => "Reverse(i8::MAX)".to_string(),
        (Ty::I16, false) => "i16::MIN".to_string(),
        (Ty::I16, true) => "Reverse(i16::MAX)".to_string(),
        (Ty::I32, false) => "i32::MIN".to_string(),
        (Ty::I32, true) => "Reverse(i32::MAX)".to_string(),
        (Ty::I64, false) => "i64::MIN".to_string(),
        (Ty::I64, true) => "Reverse(i64::MAX)".to_string(),
        (Ty::U8, false) => "u8::MIN".to_string(),
        (Ty::U8, true) => "Reverse(u8::MAX)".to_string(),
        (Ty::U16, false) => "u16::MIN".to_string(),
        (Ty::U16, true) => "Reverse(u16::MAX)".to_string(),
        (Ty::U32, false) => "u32::MIN".to_string(),
        (Ty::U32, true) => "Reverse(u32::MAX)".to_string(),
        (Ty::U64, false) => "u64::MIN".to_string(),
        (Ty::U64, true) => "Reverse(u64::MAX)".to_string(),
        (Ty::F32, false) => "__LsOrdF32(f32::NEG_INFINITY)".to_string(),
        (Ty::F64, false) => "__LsOrdF64(f64::NEG_INFINITY)".to_string(),
        (Ty::F32 | Ty::F64, true) => return None,
        (Ty::Bool, false) => "false".to_string(),
        (Ty::Bool, true) => "Reverse(true)".to_string(),
        (Ty::Text, false) => "__LsText::default()".to_string(),
        (Ty::Text | Ty::Bytes | Ty::Uuid, true) => return None,
        (Ty::Bytes, false) => "Vec::new()".to_string(),
        (Ty::Uuid, false) => "[0; 16]".to_string(),
    };
    Some(value)
}

pub(crate) fn advanced_eq_constraints(
    filter: &Option<Expr>,
    source_index: usize,
    sources: &[(&Table, String)],
) -> Result<Option<Vec<(String, Operand)>>, String> {
    let Some(filter) = filter else {
        return Ok(Some(Vec::new()));
    };
    let mut constraints = Vec::new();
    if !collect_advanced_eq(filter, source_index, sources, &mut constraints)? {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    if constraints
        .iter()
        .any(|(field, _)| !seen.insert(field.clone()))
    {
        return Ok(None);
    }
    Ok(Some(constraints))
}

pub(crate) fn collect_advanced_eq(
    expr: &Expr,
    source_index: usize,
    sources: &[(&Table, String)],
    constraints: &mut Vec<(String, Operand)>,
) -> Result<bool, String> {
    match expr {
        Expr::IsNull { .. } | Expr::IsNotNull { .. } => Ok(true),
        Expr::Cmp { left, op, right } => {
            if *op != Cmp::Eq {
                return Ok(true);
            }
            let left_field = operand_source(left, sources)?;
            let right_field = operand_source(right, sources)?;
            match (left_field, right_field) {
                (Some((index, field)), None) if index == source_index => {
                    constraints.push((field.name.clone(), right.clone()));
                }
                (None, Some((index, field))) if index == source_index => {
                    constraints.push((field.name.clone(), left.clone()));
                }
                _ => {}
            }
            Ok(true)
        }
        Expr::And(left, right) => {
            let left = collect_advanced_eq(left, source_index, sources, constraints)?;
            let right = collect_advanced_eq(right, source_index, sources, constraints)?;
            Ok(left && right)
        }
        Expr::Or(..) => Ok(false),
    }
}

pub(crate) fn operand_source<'a>(
    operand: &Operand,
    sources: &'a [(&'a Table, String)],
) -> Result<Option<(usize, &'a Field)>, String> {
    let Operand::Field(name) = operand else {
        return Ok(None);
    };
    let reference = FieldRef::parse(name).ok_or_else(|| format!("invalid field `{name}`"))?;
    let (index, _, field) = resolve_source(&reference, sources)?;
    Ok(Some((index, field)))
}

pub(crate) fn resolve_source<'a>(
    reference: &FieldRef,
    sources: &'a [(&'a Table, String)],
) -> Result<(usize, &'a Table, &'a Field), String> {
    match &reference.qualifier {
        Some(qualifier) => {
            let (index, (table, _)) = sources
                .iter()
                .enumerate()
                .find(|(_, (_, existing))| existing == qualifier)
                .ok_or_else(|| format!("unknown source `{qualifier}`"))?;
            let (_, field) = table
                .field(&reference.field)
                .ok_or_else(|| format!("unknown field `{reference}`"))?;
            Ok((index, table, field))
        }
        None => {
            let matches: Vec<(usize, &Table, &Field)> = sources
                .iter()
                .enumerate()
                .filter_map(|(index, (table, _))| {
                    table
                        .field(&reference.field)
                        .map(|(_, field)| (index, *table, field))
                })
                .collect();
            match matches.as_slice() {
                [] => Err(format!("unknown field `{}`", reference.field)),
                [only] => Ok(*only),
                many => Err(format!(
                    "field `{}` is ambiguous; it exists in {}; qualify it",
                    reference.field,
                    many.iter()
                        .map(|(_, table, _)| format!("`{}`", table.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
    }
}

pub(crate) fn pk_join_key(
    join: &Join,
    depth: usize,
    sources: &[(&Table, String)],
) -> Result<Option<String>, String> {
    let (left_index, left_table, left_field) = resolve_source(&join.left, sources)?;
    let (right_index, right_table, right_field) = resolve_source(&join.right, sources)?;
    if left_index == depth
        && right_index < depth
        && left_table
            .primary_key()
            .is_some_and(|key| key.name == left_field.name)
    {
        return Ok(Some(emit_field_read(
            &join.right,
            &sources[..=depth],
            false,
        )?));
    }
    if right_index == depth
        && left_index < depth
        && right_table
            .primary_key()
            .is_some_and(|key| key.name == right_field.name)
    {
        return Ok(Some(emit_field_read(
            &join.left,
            &sources[..=depth],
            false,
        )?));
    }
    Ok(None)
}

pub(crate) fn pk_join_key_ty(
    join: &Join,
    depth: usize,
    sources: &[(&Table, String)],
) -> Result<Option<Ty>, String> {
    let (left_index, left_table, left_field) = resolve_source(&join.left, sources)?;
    let (right_index, right_table, right_field) = resolve_source(&join.right, sources)?;
    if left_index == depth
        && right_index < depth
        && left_table
            .primary_key()
            .is_some_and(|key| key.name == left_field.name)
    {
        return Ok(Some(right_field.ty));
    }
    if right_index == depth
        && left_index < depth
        && right_table
            .primary_key()
            .is_some_and(|key| key.name == right_field.name)
    {
        return Ok(Some(left_field.ty));
    }
    Ok(None)
}

pub(crate) fn pk_eq_operand(filter: &Option<Expr>, table: &Table) -> Option<Operand> {
    let primary_key = table.primary_key()?;
    let filter = filter.as_ref()?;
    let Expr::Cmp {
        left,
        op: Cmp::Eq,
        right,
    } = filter
    else {
        return None;
    };
    match (left, right) {
        (Operand::Field(field), other)
            if field == &primary_key.name && !matches!(other, Operand::Field(_)) =>
        {
            Some(other.clone())
        }
        (other, Operand::Field(field))
            if field == &primary_key.name && !matches!(other, Operand::Field(_)) =>
        {
            Some(other.clone())
        }
        _ => None,
    }
}

pub(crate) fn advanced_filter_for(operation: &Operation) -> &Option<Expr> {
    match &operation.body {
        Body::Query { filter, .. } => filter,
        _ => &None,
    }
}
