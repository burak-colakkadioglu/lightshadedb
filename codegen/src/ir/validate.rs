use super::ast::*;
use std::collections::{HashMap, HashSet};

pub fn validate(workload: &Workload) -> Result<(), String> {
    let mut table_names = HashSet::new();
    for table in &workload.tables {
        check_name(&table.name, "table")?;
        if !table_names.insert(table.name.as_str()) {
            return Err(format!("duplicate table `{}`", table.name));
        }
        validate_table(table)?;
    }

    let mut operation_names = HashSet::new();
    for operation in &workload.operations {
        check_name(&operation.name, "operation")?;
        if !operation_names.insert(operation.name.as_str()) {
            return Err(format!("duplicate operation `{}`", operation.name));
        }
        validate_operation(workload, operation)?;
    }

    Ok(())
}

fn validate_table(table: &Table) -> Result<(), String> {
    if table.fields.is_empty() {
        return Err(format!("table `{}` has no fields", table.name));
    }

    let mut field_names = HashSet::new();
    for field in &table.fields {
        check_name(&field.name, "field")?;
        if !field_names.insert(field.name.as_str()) {
            return Err(format!(
                "table `{}` has duplicate field `{}`",
                table.name, field.name
            ));
        }
        if field.primary_key && field.default.is_some() {
            return Err(format!(
                "primary key field `{}` cannot declare a default",
                field.name
            ));
        }
    }

    let primary_keys = table
        .fields
        .iter()
        .filter(|field| field.primary_key)
        .count();
    if primary_keys != 1 {
        return Err(format!(
            "table `{}` must have exactly one `primary_key` field (found {primary_keys})",
            table.name
        ));
    }

    Ok(())
}

fn validate_operation(workload: &Workload, operation: &Operation) -> Result<(), String> {
    let prefix = |message: String| format!("operation `{}`: {message}", operation.name);

    let mut params: HashMap<&str, Ty> = HashMap::new();
    for param in &operation.params {
        check_name(&param.name, "parameter").map_err(prefix)?;
        if params.insert(param.name.as_str(), param.ty).is_some() {
            return Err(prefix(format!("duplicate parameter `{}`", param.name)));
        }
    }

    match &operation.body {
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
            let primary = workload
                .table(table)
                .ok_or_else(|| prefix(format!("unknown table `{table}`")))?;

            let mut sources: Vec<(&Table, String)> = Vec::new();
            sources.push((
                primary,
                alias.clone().unwrap_or_else(|| primary.name.clone()),
            ));
            for join in joins {
                let joined = workload
                    .table(&join.table)
                    .ok_or_else(|| prefix(format!("unknown table `{}`", join.table)))?;
                let qualifier = join.alias.clone().unwrap_or_else(|| joined.name.clone());
                if sources.iter().any(|(_, existing)| existing == &qualifier) {
                    return Err(prefix(format!("duplicate source alias `{qualifier}`")));
                }
                sources.push((joined, qualifier));
            }
            for (index, join) in joins.iter().enumerate() {
                let available = &sources[..=index + 1];
                let (_, left) = resolve_field(&join.left, available).map_err(prefix)?;
                let (_, right) = resolve_field(&join.right, available).map_err(prefix)?;
                if left.ty != right.ty {
                    return Err(prefix(format!(
                        "join compares `{}` ({}) with `{}` ({})",
                        join.left, left.ty, join.right, right.ty
                    )));
                }
            }

            if let Some(Limit::Param(name)) = limit {
                match params.get(name.as_str()) {
                    Some(Ty::I64) => {}
                    Some(_) => {
                        return Err(prefix(format!("limit parameter `{name}` must be `i64`")))
                    }
                    None => {
                        return Err(prefix(format!(
                            "limit references unknown parameter `{name}`"
                        )))
                    }
                }
            }

            if let Some(Limit::Param(name)) = offset {
                match params.get(name.as_str()) {
                    Some(Ty::I64) => {}
                    Some(_) => {
                        return Err(prefix(format!("offset parameter `{name}` must be `i64`")))
                    }
                    None => {
                        return Err(prefix(format!(
                            "offset references unknown parameter `{name}`"
                        )))
                    }
                }
            }

            let advanced =
                !select.is_empty() || !joins.is_empty() || !group_by.is_empty() || alias.is_some();

            if *count {
                if advanced {
                    return Err(prefix(
                        "`count` cannot be combined with `from`, `join`, `select`, or `group by`"
                            .to_string(),
                    ));
                }
                if !matches!(operation.returns, Returns::One(Ty::I64)) {
                    return Err(prefix(
                        "`count` operations must declare `-> one i64`".to_string(),
                    ));
                }
                if !order.is_empty() || limit.is_some() || offset.is_some() {
                    return Err(prefix(
                        "`count` operations cannot have `order by`, `limit`, or `offset`"
                            .to_string(),
                    ));
                }
                if let Some(filter) = filter {
                    check_expr(filter, primary, &params).map_err(prefix)?;
                }
                let mut ordered = HashSet::new();
                for (field, _) in order {
                    if primary.field(field).is_none() {
                        return Err(prefix(format!(
                            "`order by` references unknown field `{field}`"
                        )));
                    }
                    if !ordered.insert(field.as_str()) {
                        return Err(prefix(format!(
                            "`order by` lists field `{field}` more than once"
                        )));
                    }
                }
            } else if advanced {
                let returned = match &operation.returns {
                    Returns::Many(returned) => returned,
                    _ => {
                        return Err(prefix(
                            "projected and joined queries must declare `-> many <table>`"
                                .to_string(),
                        ))
                    }
                };
                let output = workload
                    .table(returned)
                    .ok_or_else(|| prefix(format!("query returns unknown table `{returned}`")))?;
                if select.is_empty() {
                    return Err(prefix(
                        "queries with `from`/`join` need a `select` clause".to_string(),
                    ));
                }

                let mut by_alias = HashMap::new();
                for item in select {
                    if by_alias.insert(item.alias.as_str(), item).is_some() {
                        return Err(prefix(format!(
                            "`select` assigns output field `{}` more than once",
                            item.alias
                        )));
                    }
                }
                for item in select {
                    if output.field(&item.alias).is_none() {
                        return Err(prefix(format!(
                            "`select ... as {}` does not match an output field of `{}`",
                            item.alias, output.name
                        )));
                    }
                }
                for field in &output.fields {
                    if !by_alias.contains_key(field.name.as_str()) {
                        return Err(prefix(format!(
                            "`select` is missing output field `{}`",
                            field.name
                        )));
                    }
                }
                for item in select {
                    let ty = select_type(item, &sources, &params).map_err(prefix)?;
                    let field = output
                        .field(&item.alias)
                        .map(|(_, field)| field)
                        .expect("select aliases were checked above");
                    if ty != field.ty {
                        return Err(prefix(format!(
                            "`select` field `{}` is `{ty}` but output field is `{}`",
                            item.alias, field.ty
                        )));
                    }
                }

                let mut grouped = !group_by.is_empty();
                for item in select {
                    if matches!(item.expr, SelectExpr::Aggregate { .. }) {
                        grouped = true;
                    }
                }
                if grouped {
                    for item in select {
                        if let SelectExpr::Field(reference) = &item.expr {
                            if !group_by.iter().any(|group| group == reference) {
                                return Err(prefix(format!(
                                    "selected field `{reference}` must appear in `group by`"
                                )));
                            }
                        }
                    }
                }
                let mut group_keys = HashSet::new();
                for reference in group_by {
                    resolve_field(reference, &sources).map_err(prefix)?;
                    if !group_keys.insert(reference) {
                        return Err(prefix(format!(
                            "`group by` lists `{reference}` more than once"
                        )));
                    }
                }

                if let Some(filter) = filter {
                    check_expr_sources(filter, &sources, &params).map_err(prefix)?;
                }

                let mut ordered = HashSet::new();
                for (field, _) in order {
                    if output.field(field).is_none() {
                        return Err(prefix(format!(
                            "`order by` references unknown output field `{field}`"
                        )));
                    }
                    if !ordered.insert(field.as_str()) {
                        return Err(prefix(format!(
                            "`order by` lists field `{field}` more than once"
                        )));
                    }
                }
            } else {
                match &operation.returns {
                    Returns::Many(returned) if returned == &primary.name => {}
                    Returns::Many(returned) => {
                        return Err(prefix(format!(
                            "query returns `many {returned}` but reads `{}`",
                            primary.name
                        )))
                    }
                    _ => {
                        return Err(prefix(format!(
                            "queries must declare `-> many {}`",
                            primary.name
                        )))
                    }
                }

                if let Some(filter) = filter {
                    check_expr(filter, primary, &params).map_err(prefix)?;
                }

                let mut ordered = HashSet::new();
                for (field, _) in order {
                    if primary.field(field).is_none() {
                        return Err(prefix(format!(
                            "`order by` references unknown field `{field}`"
                        )));
                    }
                    if !ordered.insert(field.as_str()) {
                        return Err(prefix(format!(
                            "`order by` lists field `{field}` more than once"
                        )));
                    }
                }
            }
        }
        Body::Update {
            table,
            sets,
            filter,
        } => {
            let table = workload
                .table(table)
                .ok_or_else(|| prefix(format!("unknown table `{table}`")))?;

            if !matches!(operation.returns, Returns::None) {
                return Err(prefix(
                    "`update` operations cannot return a value".to_string(),
                ));
            }
            if sets.is_empty() {
                return Err(prefix("`update` must set at least one field".to_string()));
            }

            let primary_key = table.primary_key().map(|field| field.name.as_str());
            let mut updated = HashSet::new();
            for (field_name, value) in sets {
                let (_, field) = table.field(field_name).ok_or_else(|| {
                    prefix(format!("`set` references unknown field `{field_name}`"))
                })?;
                if Some(field.name.as_str()) == primary_key {
                    return Err(prefix(format!(
                        "updating the primary key field `{field_name}` is not supported yet"
                    )));
                }
                if !updated.insert(field_name.as_str()) {
                    return Err(prefix(format!(
                        "field `{field_name}` is assigned more than once"
                    )));
                }
                let value_ty = operand_ty(value, table, &params).map_err(prefix)?;
                if !operand_matches(value, field.ty, table, &params)? {
                    return Err(prefix(format!(
                        "field `{field_name}` is `{}` but the assigned value is `{value_ty}`",
                        field.ty
                    )));
                }
            }

            if let Some(filter) = filter {
                check_expr(filter, table, &params).map_err(prefix)?;
            }
        }
        Body::Delete { table, filter } => {
            let table = workload
                .table(table)
                .ok_or_else(|| prefix(format!("unknown table `{table}`")))?;

            if !matches!(operation.returns, Returns::None) {
                return Err(prefix(
                    "`delete` operations cannot return a value".to_string(),
                ));
            }

            if let Some(filter) = filter {
                check_expr(filter, table, &params).map_err(prefix)?;
            }
        }
        Body::Insert { table } => {
            let table = workload
                .table(table)
                .ok_or_else(|| prefix(format!("unknown table `{table}`")))?;

            if !matches!(operation.returns, Returns::None) {
                return Err(prefix(
                    "`insert` operations cannot return a value".to_string(),
                ));
            }
            if operation.params.len() != table.fields.len() {
                return Err(prefix(format!(
                    "`insert` needs one parameter per field (`{}` fields, {} parameters)",
                    table.fields.len(),
                    operation.params.len()
                )));
            }

            for field in &table.fields {
                match params.get(field.name.as_str()) {
                    Some(ty) if *ty == field.ty => {}
                    Some(ty) => {
                        return Err(prefix(format!(
                            "insert parameter `{}` is `{ty}` but field is `{}`",
                            field.name, field.ty
                        )))
                    }
                    None => {
                        return Err(prefix(format!(
                            "`insert` is missing a parameter for field `{}`",
                            field.name
                        )))
                    }
                }
            }
        }
    }

    Ok(())
}

fn check_expr(expr: &Expr, table: &Table, params: &HashMap<&str, Ty>) -> Result<(), String> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left_ty = operand_ty(left, table, params)?;
            let right_ty = operand_ty(right, table, params)?;
            if left_ty != right_ty
                && !operand_matches(left, right_ty, table, params)?
                && !operand_matches(right, left_ty, table, params)?
            {
                return Err(format!(
                    "comparison mixes `{left_ty}` and `{right_ty}` values"
                ));
            }
            if matches!(op, Cmp::Lt | Cmp::Le | Cmp::Gt | Cmp::Ge)
                && (left_ty == Ty::Bool || right_ty == Ty::Bool)
            {
                return Err("`bool` values support only `==` and `!=` comparisons".to_string());
            }
            Ok(())
        }
        Expr::And(left, right) | Expr::Or(left, right) => {
            check_expr(left, table, params)?;
            check_expr(right, table, params)
        }
        Expr::IsNull { field } | Expr::IsNotNull { field } => {
            table
                .field(&field.field)
                .ok_or_else(|| format!("unknown field `{}`", field.field))?;
            Ok(())
        }
    }
}

fn operand_ty(operand: &Operand, table: &Table, params: &HashMap<&str, Ty>) -> Result<Ty, String> {
    match operand {
        Operand::Field(name) => table
            .field(name)
            .map(|(_, field)| field.ty)
            .ok_or_else(|| format!("unknown field `{name}`")),
        Operand::Param(name) => params
            .get(name.as_str())
            .copied()
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(value) => Ok(value.ty()),
    }
}

fn operand_matches(
    operand: &Operand,
    ty: Ty,
    table: &Table,
    params: &HashMap<&str, Ty>,
) -> Result<bool, String> {
    match operand {
        Operand::Literal(value) => Ok(value.clone().coerce_to(ty).is_some()),
        _ => Ok(operand_ty(operand, table, params)? == ty),
    }
}

fn resolve_field<'a>(
    reference: &FieldRef,
    sources: &'a [(&'a Table, String)],
) -> Result<(&'a Table, &'a Field), String> {
    match &reference.qualifier {
        Some(qualifier) => {
            let (table, _) = sources
                .iter()
                .find(|(_, existing)| existing == qualifier)
                .ok_or_else(|| format!("unknown source `{qualifier}`"))?;
            let (_, field) = table
                .field(&reference.field)
                .ok_or_else(|| format!("unknown field `{reference}`"))?;
            Ok((table, field))
        }
        None => {
            let matches: Vec<(&Table, &Field)> = sources
                .iter()
                .filter_map(|(table, _)| {
                    table
                        .field(&reference.field)
                        .map(|(_, field)| (*table, field))
                })
                .collect();
            match matches.as_slice() {
                [] => Err(format!("unknown field `{}`", reference.field)),
                [only] => Ok(*only),
                many => Err(format!(
                    "field `{}` is ambiguous; it exists in {}; qualify it with a source name",
                    reference.field,
                    many.iter()
                        .map(|(table, _)| format!("`{}`", table.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
    }
}

fn select_type(
    item: &SelectItem,
    sources: &[(&Table, String)],
    params: &HashMap<&str, Ty>,
) -> Result<Ty, String> {
    let _ = params;
    match &item.expr {
        SelectExpr::Field(reference) => Ok(resolve_field(reference, sources)?.1.ty),
        SelectExpr::Aggregate { function, field } => match function {
            Aggregate::Count => Ok(Ty::I64),
            Aggregate::Sum | Aggregate::Avg => {
                let (_, field) = resolve_field(
                    field
                        .as_ref()
                        .ok_or_else(|| format!("`{}` needs a field", function.name()))?,
                    sources,
                )?;
                if !field.ty.is_numeric() {
                    return Err(format!(
                        "`{}` needs an integer or float field, found `{}`",
                        function.name(),
                        field.ty
                    ));
                }
                Ok(if field.ty.is_float() {
                    field.ty
                } else {
                    Ty::I64
                })
            }
            Aggregate::Min | Aggregate::Max => {
                let (_, field) = resolve_field(
                    field
                        .as_ref()
                        .ok_or_else(|| format!("`{}` needs a field", function.name()))?,
                    sources,
                )?;
                if field.ty == Ty::Bool {
                    return Err(format!(
                        "`{}` does not support bool fields",
                        function.name()
                    ));
                }
                Ok(field.ty)
            }
        },
    }
}

fn check_expr_sources(
    expr: &Expr,
    sources: &[(&Table, String)],
    params: &HashMap<&str, Ty>,
) -> Result<(), String> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left_ty = source_operand_ty(left, sources, params)?;
            let right_ty = source_operand_ty(right, sources, params)?;
            if left_ty != right_ty
                && !source_operand_matches(left, right_ty, sources, params)?
                && !source_operand_matches(right, left_ty, sources, params)?
            {
                return Err(format!(
                    "comparison mixes `{left_ty}` and `{right_ty}` values"
                ));
            }
            if matches!(op, Cmp::Lt | Cmp::Le | Cmp::Gt | Cmp::Ge)
                && (left_ty == Ty::Bool || right_ty == Ty::Bool)
            {
                return Err("`bool` values support only `==` and `!=` comparisons".to_string());
            }
            Ok(())
        }
        Expr::And(left, right) | Expr::Or(left, right) => {
            check_expr_sources(left, sources, params)?;
            check_expr_sources(right, sources, params)
        }
        Expr::IsNull { field } | Expr::IsNotNull { field } => {
            let matched = sources.iter().filter(|(_, qualifier)| {
                field
                    .qualifier
                    .as_ref()
                    .is_none_or(|wanted| wanted == qualifier)
            });
            let mut found = false;
            for (table, _) in matched {
                if table.field(&field.field).is_some() {
                    found = true;
                    break;
                }
            }
            if found {
                Ok(())
            } else {
                Err(format!("is_null: unknown field `{field}`"))
            }
        }
    }
}

fn source_operand_ty(
    operand: &Operand,
    sources: &[(&Table, String)],
    params: &HashMap<&str, Ty>,
) -> Result<Ty, String> {
    match operand {
        Operand::Field(name) => {
            let reference =
                FieldRef::parse(name).ok_or_else(|| format!("invalid field reference `{name}`"))?;
            Ok(resolve_field(&reference, sources)?.1.ty)
        }
        Operand::Param(name) => params
            .get(name.as_str())
            .copied()
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(value) => Ok(value.ty()),
    }
}

fn source_operand_matches(
    operand: &Operand,
    ty: Ty,
    sources: &[(&Table, String)],
    params: &HashMap<&str, Ty>,
) -> Result<bool, String> {
    match operand {
        Operand::Literal(value) => Ok(value.clone().coerce_to(ty).is_some()),
        _ => Ok(source_operand_ty(operand, sources, params)? == ty),
    }
}

fn check_name(name: &str, kind: &str) -> Result<(), String> {
    if name.starts_with("__") {
        return Err(format!(
            "{kind} name `{name}` is reserved (names starting with `__` are internal)"
        ));
    }
    if RESERVED.contains(&name) {
        return Err(format!("{kind} name `{name}` is reserved"));
    }
    Ok(())
}

const RESERVED: &[&str] = &[
    "abstract", "and", "as", "async", "await", "become", "box", "break", "const", "continue",
    "crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "or", "override", "priv", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ir::parse::parse_workload;

    const SOURCE: &str = include_str!("../../../examples/messages.lightshade");

    #[test]
    fn accepts_the_messages_workload() {
        let workload = parse_workload(SOURCE).unwrap();
        validate(&workload).expect("example should validate");
    }

    #[test]
    fn accepts_the_suite_workloads() {
        for source in [
            include_str!("../../../examples/events.lightshade"),
            include_str!("../../../examples/sessions.lightshade"),
            include_str!("../../../examples/inbox_digest.lightshade"),
            include_str!("../../../examples/order_book.lightshade"),
            include_str!("../../../examples/catalog.lightshade"),
        ] {
            let workload = parse_workload(source).unwrap();
            validate(&workload).expect("suite example should validate");
        }
    }

    #[test]
    fn rejects_missing_primary_key() {
        let source = r#"
            table notes {
                id: i64
                body: text
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("exactly one `primary_key`"));
    }

    #[test]
    fn rejects_unknown_operation_field() {
        let source = r#"
            table notes {
                id: i64 primary_key
            }
            operation bad(x: i64) -> many notes {
                filter missing == x
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("unknown field `missing`"));
    }

    #[test]
    fn rejects_mixed_comparison_types() {
        let source = r#"
            table notes {
                id: i64 primary_key
            }
            operation bad(x: text) -> many notes {
                filter id == x
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("comparison mixes"));
    }

    #[test]
    fn coerces_integer_literals_to_unsigned_comparisons() {
        let source = r#"
            table notes {
                id: u64 primary_key
            }
            operation one() -> many notes {
                filter id == 1
            }
        "#;
        let workload = parse_workload(source).unwrap();
        validate(&workload).unwrap();
    }

    #[test]
    fn rejects_a_join_that_reads_a_later_source() {
        let source = r#"
            table first {
                id: i64 primary_key
                other: i64
            }
            table second {
                id: i64 primary_key
                other: i64
            }
            table third {
                id: i64 primary_key
                other: i64
            }
            table out {
                id: i64 primary_key
            }
            operation bad() -> many out {
                from first as f
                join second as s on third.other == f.id
                join third as third on third.id == f.id
                select f.id as id
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("unknown source `third`"), "{error}");
    }

    #[test]
    fn rejects_an_ambiguous_field_reference() {
        let source = r#"
            table left_side {
                id: i64 primary_key
                value: i64
            }
            table right_side {
                id: i64 primary_key
                value: i64
            }
            table out {
                id: i64 primary_key
            }
            operation bad() -> many out {
                from left_side
                join right_side on left_side.id == right_side.id
                select value as id
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[test]
    fn rejects_an_ungrouped_selected_field() {
        let source = r#"
            table notes {
                id: i64 primary_key
                body: text
            }
            table out {
                body: text primary_key
                total: i64
            }
            operation bad() -> many out {
                from notes
                select body, count() as total
                group by id
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("must appear in `group by`"), "{error}");
    }

    #[test]
    fn rejects_aggregating_text_with_sum() {
        let source = r#"
            table notes {
                id: i64 primary_key
                body: text
            }
            table out {
                total: i64 primary_key
            }
            operation bad() -> many out {
                from notes
                select sum(body) as total
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(
            error.contains("`sum` needs an integer or float field"),
            "{error}"
        );
    }

    #[test]
    fn rejects_updating_the_primary_key() {
        let source = r#"
            table notes {
                id: i64 primary_key
            }
            operation bad(new_id: i64) {
                update notes set id = new_id
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("primary key"));
    }

    #[test]
    fn rejects_default_on_primary_key() {
        let source = r#"
            table notes {
                id: i64 primary_key default 1
                body: text
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(
            error.contains("cannot declare a default"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn accepts_defaults_on_regular_fields() {
        let source = r#"
            table notes {
                id: i64 primary_key
                tag: text default "misc"
                weight: i64 default -7
                pinned: bool default false
            }
        "#;
        let workload = parse_workload(source).unwrap();
        validate(&workload).expect("defaults on regular fields should validate");
    }
}
