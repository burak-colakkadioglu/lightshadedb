use crate::model::{
    DeleteArgs, Operation, OperationKind, Param, Projection, QueryArgs, Table, UpdateArgs,
};
use lightshadedb_codegen::ir::{
    Body, Cmp, Expr, Field, Limit, Operand, Returns, Ty, Value, Workload,
};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{BinOp, Expr as RustExpr, Lit, Type, UnOp};

pub struct LoweredSchema {
    pub workload: Workload,
    pub tables: Vec<LoweredTable>,
    pub projections: Vec<LoweredProjection>,
}

pub struct LoweredProjection {
    pub db_name: String,
    pub struct_name: String,
}

pub struct LoweredTable {
    pub db_name: String,
    pub struct_name: String,
    pub primary_key: String,
    pub columns: Vec<LoweredColumn>,
    pub crud: CrudNames,
}

pub struct LoweredColumn {
    pub name: String,
    pub primary_key: bool,
}

pub struct CrudNames {
    pub insert: String,
    pub get: String,
    pub contains: String,
    pub scan: String,
    pub count: String,
    pub update: String,
    pub delete: String,
}

pub fn lower(
    tables: &[Table],
    projections: &[Projection],
    operations: &[Operation],
) -> syn::Result<LoweredSchema> {
    let mut workload = Workload::default();
    let mut lowered_tables = Vec::new();
    let mut lowered_projections = Vec::new();

    for table in tables {
        let mut fields = Vec::new();
        for field in &table.fields {
            let default = match &field.default {
                Some(expr) => Some(default_value(expr, field.ir)?),
                None => None,
            };
            fields.push(Field {
                name: field.ident.to_string(),
                ty: field.ir,
                primary_key: field.primary_key,
                default,
                nullable: field.nullable,
            });
        }
        workload.tables.push(lightshadedb_codegen::ir::Table {
            name: table.db_name.clone(),
            fields,
            ttl_secs: table.ttl_secs,
        });
        let crud = crud_names(lowered_tables.len());
        lowered_tables.push(LoweredTable {
            db_name: table.db_name.clone(),
            struct_name: table.ident.to_string(),
            primary_key: table.fields[table.primary_key].ident.to_string(),
            columns: table
                .fields
                .iter()
                .map(|field| LoweredColumn {
                    name: field.ident.to_string(),
                    primary_key: field.primary_key,
                })
                .collect(),
            crud,
        });
    }

    for projection in projections {
        let db_name = snake_case(&projection.ident.to_string());
        workload.tables.push(lightshadedb_codegen::ir::Table {
            name: db_name.clone(),
            ttl_secs: None,
            fields: projection
                .fields
                .iter()
                .map(|field| Field {
                    name: field.ident.to_string(),
                    ty: field.ir,
                    primary_key: false,
                    default: None,
                    nullable: false,
                })
                .collect(),
        });
        workload.projections.push(db_name.clone());
        lowered_projections.push(LoweredProjection {
            db_name,
            struct_name: projection.ident.to_string(),
        });
    }

    for operation in operations {
        let ir = lower_operation(operation, tables, projections, &lowered_tables)?;
        workload.operations.push(ir);
    }

    for (index, table) in tables.iter().enumerate() {
        let names = &lowered_tables[index].crud;
        push_hidden_crud(&mut workload, table, names);
    }

    Ok(LoweredSchema {
        workload,
        tables: lowered_tables,
        projections: lowered_projections,
    })
}

fn snake_case(value: &str) -> String {
    let mut out = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            for lower in character.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn crud_names(index: usize) -> CrudNames {
    CrudNames {
        insert: format!("__ls_crud_{index}_insert"),
        get: format!("__ls_crud_{index}_get"),
        contains: format!("__ls_crud_{index}_contains"),
        scan: format!("__ls_crud_{index}_scan"),
        count: format!("__ls_crud_{index}_count"),
        update: format!("__ls_crud_{index}_update"),
        delete: format!("__ls_crud_{index}_delete"),
    }
}

fn push_hidden_crud(workload: &mut Workload, table: &Table, names: &CrudNames) {
    let all_params = table
        .fields
        .iter()
        .map(|field| lightshadedb_codegen::ir::Param {
            name: field.ident.to_string(),
            ty: field.ir,
            nullable: field.nullable,
        })
        .collect::<Vec<_>>();
    let key = &table.fields[table.primary_key];
    let key_param = lightshadedb_codegen::ir::Param {
        name: key.ident.to_string(),
        ty: key.ir,
        nullable: false,
    };
    let key_eq = Expr::Cmp {
        left: Operand::Field(key.ident.to_string()),
        op: Cmp::Eq,
        right: Operand::Param(key.ident.to_string()),
    };

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.insert.clone(),
            params: all_params.clone(),
            returns: Returns::None,
            body: Body::Insert {
                table: table.db_name.clone(),
            },
        });

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.get.clone(),
            params: vec![key_param.clone()],
            returns: Returns::Many(table.db_name.clone()),
            body: Body::Query {
                table: table.db_name.clone(),
                alias: None,
                joins: Vec::new(),
                select: Vec::new(),
                group_by: Vec::new(),
                filter: Some(key_eq.clone()),
                order: vec![(key.ident.to_string(), false)],
                limit: Some(Limit::Literal(1)),
                offset: None,
                count: false,
            },
        });

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.contains.clone(),
            params: vec![key_param.clone()],
            returns: Returns::One(Ty::I64),
            body: Body::Query {
                table: table.db_name.clone(),
                alias: None,
                joins: Vec::new(),
                select: Vec::new(),
                group_by: Vec::new(),
                filter: Some(key_eq.clone()),
                order: Vec::new(),
                limit: None,
                offset: None,
                count: true,
            },
        });

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.scan.clone(),
            params: Vec::new(),
            returns: Returns::Many(table.db_name.clone()),
            body: Body::Query {
                table: table.db_name.clone(),
                alias: None,
                joins: Vec::new(),
                select: Vec::new(),
                group_by: Vec::new(),
                filter: None,
                order: vec![(key.ident.to_string(), false)],
                limit: None,
                offset: None,
                count: false,
            },
        });

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.count.clone(),
            params: Vec::new(),
            returns: Returns::One(Ty::I64),
            body: Body::Query {
                table: table.db_name.clone(),
                alias: None,
                joins: Vec::new(),
                select: Vec::new(),
                group_by: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: None,
                count: true,
            },
        });

    let sets = table
        .fields
        .iter()
        .filter(|field| !field.primary_key)
        .map(|field| {
            (
                field.ident.to_string(),
                Operand::Param(field.ident.to_string()),
            )
        })
        .collect();
    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.update.clone(),
            params: all_params,
            returns: Returns::None,
            body: Body::Update {
                table: table.db_name.clone(),
                sets,
                filter: Some(key_eq.clone()),
            },
        });

    workload
        .operations
        .push(lightshadedb_codegen::ir::Operation {
            name: names.delete.clone(),
            params: vec![key_param],
            returns: Returns::None,
            body: Body::Delete {
                table: table.db_name.clone(),
                filter: Some(key_eq),
            },
        });
}

fn lower_operation(
    operation: &Operation,
    tables: &[Table],
    projections: &[Projection],
    lowered_tables: &[LoweredTable],
) -> syn::Result<lightshadedb_codegen::ir::Operation> {
    let name = operation.method.sig.ident.to_string();
    let ir_params = operation
        .params
        .iter()
        .map(|param| lightshadedb_codegen::ir::Param {
            name: param.ident.to_string(),
            ty: param.ir,
            nullable: false,
        })
        .collect::<Vec<_>>();
    let (body, returns) = match &operation.kind {
        OperationKind::Query(args) => {
            if let Some(projection) = projection_output(operation, projections) {
                let table = projection_source(
                    args,
                    &operation.params,
                    tables,
                    projections,
                    lowered_tables,
                )?;
                let body = lower_projection_query(
                    args,
                    &operation.params,
                    tables,
                    projections,
                    lowered_tables,
                    table,
                    projection,
                )?;
                (
                    body,
                    Returns::Many(snake_case(&projection.ident.to_string())),
                )
            } else {
                let table = query_table(operation, args, tables, lowered_tables)?;
                let body = lower_query(
                    args,
                    &operation.params,
                    tables,
                    lowered_tables,
                    table,
                    false,
                )?;
                (body, Returns::Many(table.db_name.clone()))
            }
        }
        OperationKind::Count(args) => {
            let table = query_table(operation, args, tables, lowered_tables)?;
            let body = lower_query(args, &operation.params, tables, lowered_tables, table, true)?;
            (body, Returns::One(Ty::I64))
        }
        OperationKind::Update(args) => {
            let table = update_table(args, &operation.params, tables, lowered_tables)?;
            let sets = lower_sets(args, &operation.params, tables, lowered_tables, table)?;
            let single = [(table, table.db_name.clone())];
            let filter = lower_filter(
                &args.filter,
                &operation.params,
                tables,
                lowered_tables,
                &single,
            )?;
            (
                Body::Update {
                    table: table.db_name.clone(),
                    sets,
                    filter,
                },
                Returns::None,
            )
        }
        OperationKind::Delete(args) => {
            let table = delete_table(args, &operation.params, tables, lowered_tables)?;
            let single = [(table, table.db_name.clone())];
            let filter = lower_filter(
                &args.filter,
                &operation.params,
                tables,
                lowered_tables,
                &single,
            )?;
            (
                Body::Delete {
                    table: table.db_name.clone(),
                    filter,
                },
                Returns::None,
            )
        }
    };
    Ok(lightshadedb_codegen::ir::Operation {
        name,
        params: ir_params,
        returns,
        body,
    })
}

fn projection_output<'a>(
    operation: &Operation,
    projections: &'a [Projection],
) -> Option<&'a Projection> {
    let syn::ReturnType::Type(_, ty) = &operation.method.sig.output else {
        return None;
    };
    let Type::Path(path) = ty.as_ref() else {
        return None;
    };
    let result = path.path.segments.last()?;
    if result.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &result.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(Type::Path(inner)) = arguments.args.first()? else {
        return None;
    };
    let vec = inner.path.segments.last()?;
    if vec.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(items) = &vec.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(Type::Path(row)) = items.args.first()? else {
        return None;
    };
    let ident = &row.path.segments.last()?.ident;
    projections
        .iter()
        .find(|projection| projection.ident == *ident)
}

fn projection_source<'a>(
    args: &QueryArgs,
    params: &[Param],
    tables: &'a [Table],
    projections: &[Projection],
    lowered: &[LoweredTable],
) -> syn::Result<&'a Table> {
    if let Some(from) = &args.from {
        return table_from_type(from, tables);
    }
    let _ = projections;
    if let Some(filter) = &args.filter {
        if let Some(table_ident) = first_field_table(filter) {
            return tables
                .iter()
                .find(|table| table.ident == table_ident)
                .ok_or_else(|| {
                    syn::Error::new(filter.span(), "unknown table in projection filter")
                });
        }
    }
    if let Some(select) = &args.select {
        if let Some(table_ident) = first_field_table(select) {
            return tables
                .iter()
                .find(|table| table.ident == table_ident)
                .ok_or_else(|| {
                    syn::Error::new(select.span(), "unknown table in projection select")
                });
        }
    }
    let _ = (params, lowered);
    Err(syn::Error::new(
        args.select
            .as_ref()
            .map(|expr| expr.span())
            .unwrap_or_else(Span::call_site),
        "projection queries need `from = <Table>` when the source cannot be inferred",
    ))
}

fn lower_projection_query(
    args: &QueryArgs,
    params: &[Param],
    tables: &[Table],
    projections: &[Projection],
    lowered: &[LoweredTable],
    table: &Table,
    output: &Projection,
) -> syn::Result<Body> {
    let joins = lower_joins(&args.joins, tables)?;
    let mut sources = vec![(table, table.db_name.clone())];
    for join in &joins {
        let joined = tables
            .iter()
            .find(|candidate| candidate.db_name == join.table)
            .ok_or_else(|| syn::Error::new(Span::call_site(), "unknown joined table"))?;
        sources.push((joined, joined.db_name.clone()));
    }
    let select_expr = args.select.as_ref().ok_or_else(|| {
        syn::Error::new(
            args.from
                .as_ref()
                .map(|from| from.span())
                .unwrap_or_else(Span::call_site),
            "a projection query needs `select = <Projection>::select()...`",
        )
    })?;
    let select = lower_select(select_expr, output, &sources, tables, projections, lowered)?;
    let filter = lower_filter(&args.filter, params, tables, lowered, &sources)?;
    let mut group_by = Vec::new();
    for expr in &args.group_by {
        group_by.push(lower_group(expr, tables, lowered, &sources)?);
    }
    let mut order = Vec::new();
    for expr in &args.order {
        order.push(lower_projection_order(expr, output)?);
    }
    let limit = match &args.limit {
        Some(expr) => Some(lower_limit(expr, params)?),
        None => None,
    };
    let offset = match &args.offset {
        Some(expr) => Some(lower_bound(expr, params, "offset")?),
        None => None,
    };
    Ok(Body::Query {
        table: table.db_name.clone(),
        alias: None,
        joins,
        select,
        group_by,
        filter,
        order,
        limit,
        offset,
        count: false,
    })
}

fn resolve_field_ref(
    expr: &RustExpr,
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<(lightshadedb_codegen::ir::FieldRef, Ty)> {
    let (struct_ident, field_ident) = crate::model::field_path(expr).ok_or_else(|| {
        syn::Error::new(
            expr.span(),
            "expected a field handle such as `Message::sent_at`",
        )
    })?;
    let table = tables
        .iter()
        .find(|table| table.ident == struct_ident)
        .ok_or_else(|| syn::Error::new(expr.span(), format!("unknown table `{struct_ident}`")))?;
    let source = sources
        .iter()
        .find(|(candidate, _)| candidate.db_name == table.db_name)
        .ok_or_else(|| {
            syn::Error::new(
                expr.span(),
                format!("table `{}` is not part of this query", table.db_name),
            )
        })?;
    let field = table
        .fields
        .iter()
        .find(|field| field.ident == field_ident)
        .ok_or_else(|| {
            syn::Error::new(
                expr.span(),
                format!("unknown field `{struct_ident}::{field_ident}`"),
            )
        })?;
    let _ = lowered;
    Ok((
        lightshadedb_codegen::ir::FieldRef {
            qualifier: (sources.len() > 1).then(|| source.1.clone()),
            field: field.ident.to_string(),
        },
        field.ir,
    ))
}

fn resolve_field_any(
    expr: &RustExpr,
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<(String, Ty)> {
    let (field, ty) = resolve_field_ref(expr, tables, lowered, sources)?;
    let name = match field.qualifier {
        Some(qualifier) => format!("{qualifier}.{}", field.field),
        None => field.field,
    };
    Ok((name, ty))
}

fn lower_joins(
    exprs: &[RustExpr],
    tables: &[Table],
) -> syn::Result<Vec<lightshadedb_codegen::ir::Join>> {
    let mut joins = Vec::new();
    for expr in exprs {
        let RustExpr::MethodCall(call) = expr else {
            return Err(syn::Error::new(
                expr.span(),
                "joins are `Table::join_on(field.eq_field(other))`",
            ));
        };
        if call.method != "join_on" {
            return Err(syn::Error::new(
                call.method.span(),
                "joins are `Table::join_on(field.eq_field(other))`",
            ));
        }
        let RustExpr::Path(path) = call.receiver.as_ref() else {
            return Err(syn::Error::new(
                call.receiver.span(),
                "expected a table name",
            ));
        };
        let table_ident = path
            .path
            .segments
            .last()
            .ok_or_else(|| syn::Error::new(call.receiver.span(), "expected a table name"))?
            .ident
            .clone();
        let joined = tables
            .iter()
            .find(|table| table.ident == table_ident)
            .ok_or_else(|| {
                syn::Error::new(
                    call.receiver.span(),
                    format!("unknown table `{table_ident}`"),
                )
            })?;
        let predicate = call
            .args
            .first()
            .ok_or_else(|| syn::Error::new(call.span(), "`join_on` needs an equality predicate"))?;
        let mut keys = Vec::new();
        for leaf in split_join_conjunction(predicate) {
            let RustExpr::MethodCall(comparison) = leaf else {
                return Err(syn::Error::new(
                    leaf.span(),
                    "`join_on` needs `Field::eq_field(Field)`",
                ));
            };
            if comparison.method != "eq_field" {
                return Err(syn::Error::new(
                    comparison.method.span(),
                    "inner joins use `Field::eq_field(Field)`",
                ));
            }
            let (left_table, left_field_ident) = crate::model::field_path(&comparison.receiver)
                .ok_or_else(|| {
                    syn::Error::new(comparison.receiver.span(), "expected a field handle")
                })?;
            let right_expr = comparison.args.first().ok_or_else(|| {
                syn::Error::new(comparison.span(), "`eq_field` needs a field argument")
            })?;
            let (right_table, right_field_ident) = crate::model::field_path(right_expr)
                .ok_or_else(|| syn::Error::new(right_expr.span(), "expected a field handle"))?;
            let left_model = tables
                .iter()
                .find(|table| table.ident == left_table)
                .ok_or_else(|| {
                    syn::Error::new(comparison.receiver.span(), "unknown table in join")
                })?;
            let right_model = tables
                .iter()
                .find(|table| table.ident == right_table)
                .ok_or_else(|| syn::Error::new(right_expr.span(), "unknown table in join"))?;
            let left_field = left_model
                .fields
                .iter()
                .find(|field| field.ident == left_field_ident)
                .ok_or_else(|| {
                    syn::Error::new(comparison.receiver.span(), "unknown field in join")
                })?;
            let left_ty = left_field.ir;
            let right_field = right_model
                .fields
                .iter()
                .find(|field| field.ident == right_field_ident)
                .ok_or_else(|| syn::Error::new(right_expr.span(), "unknown field in join"))?;
            let right_ty = right_field.ir;
            if left_ty != right_ty {
                return Err(syn::Error::new(
                    leaf.span(),
                    "join fields have different types",
                ));
            }
            keys.push(lightshadedb_codegen::ir::JoinKey {
                left: lightshadedb_codegen::ir::FieldRef {
                    qualifier: Some(left_model.db_name.clone()),
                    field: left_field_ident.to_string(),
                },
                right: lightshadedb_codegen::ir::FieldRef {
                    qualifier: Some(right_model.db_name.clone()),
                    field: right_field_ident.to_string(),
                },
            });
        }
        joins.push(lightshadedb_codegen::ir::Join {
            table: joined.db_name.clone(),
            alias: None,
            keys,
        });
    }
    Ok(joins)
}

fn split_join_conjunction(predicate: &RustExpr) -> Vec<&RustExpr> {
    match predicate {
        RustExpr::Paren(paren) => split_join_conjunction(&paren.expr),
        RustExpr::Binary(binary) if matches!(binary.op, BinOp::BitAnd(_) | BinOp::And(_)) => {
            let mut leaves = split_join_conjunction(&binary.left);
            leaves.extend(split_join_conjunction(&binary.right));
            leaves
        }
        _ => vec![predicate],
    }
}

fn lower_select(
    expr: &RustExpr,
    output: &Projection,
    sources: &[(&Table, String)],
    tables: &[Table],
    projections: &[Projection],
    lowered: &[LoweredTable],
) -> syn::Result<Vec<lightshadedb_codegen::ir::SelectItem>> {
    let mut calls = Vec::new();
    let mut current = expr;
    let base;
    loop {
        match current {
            RustExpr::Call(call) if call.args.is_empty() => {
                let RustExpr::Path(path) = call.func.as_ref() else {
                    return Err(syn::Error::new(
                        call.span(),
                        "expected `<Projection>::select()`",
                    ));
                };
                let mut segments = path.path.segments.iter();
                let projection = segments
                    .next()
                    .ok_or_else(|| syn::Error::new(call.span(), "expected a projection type"))?;
                let method = segments.next().ok_or_else(|| {
                    syn::Error::new(call.span(), "expected `<Projection>::select()`")
                })?;
                if method.ident != "select" || segments.next().is_some() {
                    return Err(syn::Error::new(
                        call.span(),
                        "expected `<Projection>::select()`",
                    ));
                }
                base = projection.ident.clone();
                break;
            }
            RustExpr::MethodCall(call) => {
                let argument = call.args.first().ok_or_else(|| {
                    syn::Error::new(call.span(), "projection builder methods need one value")
                })?;
                calls.push((call.method.clone(), argument));
                current = call.receiver.as_ref();
            }
            _ => {
                return Err(syn::Error::new(
                    current.span(),
                    "expected a `<Projection>::select()` builder chain",
                ))
            }
        }
    }
    if base != output.ident {
        return Err(syn::Error::new(
            expr.span(),
            format!(
                "select builder is for `{base}` but the method returns `{}`",
                output.ident
            ),
        ));
    }
    calls.reverse();
    let _ = projections;
    let mut items = Vec::new();
    for field in &output.fields {
        let matches = calls
            .iter()
            .filter(|(method, _)| *method == field.ident)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(syn::Error::new(
                expr.span(),
                format!(
                    "projection field `{}` must be selected exactly once",
                    field.ident
                ),
            ));
        }
        let (_, value) = matches[0];
        let (select_expr, result_ty, value_nullable) =
            lower_select_value(value, sources, tables, lowered)?;
        if result_ty != field.ir {
            return Err(syn::Error::new(
                value.span(),
                format!(
                    "projection field `{}` is {} but the selected value is {}",
                    field.ident, field.ir, result_ty
                ),
            ));
        }
        if value_nullable && !field.nullable {
            return Err(syn::Error::new(
                value.span(),
                format!(
                    "projection field `{}` must be `Option<{}>` to receive nullable `{}`",
                    field.ident, field.ir, result_ty,
                ),
            ));
        }
        items.push(lightshadedb_codegen::ir::SelectItem {
            expr: select_expr,
            alias: field.ident.to_string(),
        });
    }
    Ok(items)
}

fn lower_select_value(
    expr: &RustExpr,
    sources: &[(&Table, String)],
    tables: &[Table],
    lowered: &[LoweredTable],
) -> syn::Result<(lightshadedb_codegen::ir::SelectExpr, Ty, bool)> {
    if let RustExpr::MethodCall(call) = expr {
        if call.method == "select" {
            let (field, ty) = resolve_field_ref(&call.receiver, tables, lowered, sources)?;
            let (struct_ident, field_ident) = crate::model::field_path(&call.receiver)
                .ok_or_else(|| syn::Error::new(call.receiver.span(), "expected a field handle"))?;
            let nullable = tables
                .iter()
                .find(|table| table.ident == struct_ident)
                .and_then(|table| table.fields.iter().find(|field| field.ident == field_ident))
                .is_some_and(|field| field.nullable);
            return Ok((
                lightshadedb_codegen::ir::SelectExpr::Field(field),
                ty,
                nullable,
            ));
        }
    }
    let (method, arguments): (String, Vec<&RustExpr>) = match expr {
        RustExpr::Call(call) => {
            let RustExpr::Path(path) = call.func.as_ref() else {
                return Err(syn::Error::new(expr.span(), "expected an aggregate call"));
            };
            let method = path
                .path
                .segments
                .last()
                .ok_or_else(|| syn::Error::new(expr.span(), "expected an aggregate call"))?
                .ident
                .to_string();
            (method, call.args.iter().collect())
        }
        RustExpr::MethodCall(call) => (call.method.to_string(), call.args.iter().collect()),
        _ => {
            return Err(syn::Error::new(
                expr.span(),
                "selected values are `Field::select()` or an aggregate",
            ))
        }
    };
    let aggregate = match method.as_str() {
        "count" => lightshadedb_codegen::ir::Aggregate::Count,
        "min" => lightshadedb_codegen::ir::Aggregate::Min,
        "max" => lightshadedb_codegen::ir::Aggregate::Max,
        "sum" => lightshadedb_codegen::ir::Aggregate::Sum,
        "avg" => lightshadedb_codegen::ir::Aggregate::Avg,
        other => {
            return Err(syn::Error::new(
                expr.span(),
                format!("unknown aggregate `{other}`"),
            ))
        }
    };
    if aggregate == lightshadedb_codegen::ir::Aggregate::Count {
        return Ok((
            lightshadedb_codegen::ir::SelectExpr::Aggregate {
                function: aggregate,
                field: None,
            },
            Ty::I64,
            false,
        ));
    }
    let argument = arguments.first().copied().ok_or_else(|| {
        syn::Error::new(expr.span(), format!("`{method}` needs a field argument"))
    })?;
    let (field, ty) = resolve_field_ref(argument, tables, lowered, sources)?;
    if !ty.is_numeric()
        && !matches!(
            aggregate,
            lightshadedb_codegen::ir::Aggregate::Min | lightshadedb_codegen::ir::Aggregate::Max
        )
    {
        return Err(syn::Error::new(
            argument.span(),
            "this aggregate requires a numeric field",
        ));
    }
    let result_ty = ty;
    Ok((
        lightshadedb_codegen::ir::SelectExpr::Aggregate {
            function: aggregate,
            field: Some(field),
        },
        result_ty,
        false,
    ))
}

fn lower_group(
    expr: &RustExpr,
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<lightshadedb_codegen::ir::FieldRef> {
    let RustExpr::MethodCall(call) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "group terms are `Field::group()`",
        ));
    };
    if call.method != "group" {
        return Err(syn::Error::new(
            call.method.span(),
            "group terms are `Field::group()`",
        ));
    }
    let (field, _) = resolve_field_ref(&call.receiver, tables, lowered, sources)?;
    Ok(field)
}

fn lower_projection_order(expr: &RustExpr, output: &Projection) -> syn::Result<(String, bool)> {
    let RustExpr::MethodCall(call) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "order terms are `Field::asc()` or `Field::desc()`",
        ));
    };
    let descending = match call.method.to_string().as_str() {
        "asc" => false,
        "desc" => true,
        other => {
            return Err(syn::Error::new(
                call.method.span(),
                format!("unknown order method `{other}`"),
            ))
        }
    };
    let (struct_ident, field) = crate::model::field_path(&call.receiver).ok_or_else(|| {
        syn::Error::new(
            call.receiver.span(),
            "projection order terms name a projection field",
        )
    })?;
    if struct_ident != output.ident || !output.fields.iter().any(|model| model.ident == field) {
        return Err(syn::Error::new(
            call.receiver.span(),
            "projection order terms name a field on the projection type",
        ));
    }
    Ok((field.to_string(), descending))
}

fn query_table<'a>(
    operation: &Operation,
    args: &QueryArgs,
    tables: &'a [Table],
    lowered: &[LoweredTable],
) -> syn::Result<&'a Table> {
    if let Some(from) = &args.from {
        return table_from_type(from, tables);
    }
    if let OperationKind::Query(_) = operation.kind {
        if let syn::ReturnType::Type(_, ty) = &operation.method.sig.output {
            if let Type::Path(path) = ty.as_ref() {
                if let Some(segment) = path.path.segments.last() {
                    if segment.ident == "Result" {
                        if let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments {
                            if let Some(syn::GenericArgument::Type(Type::Path(inner))) =
                                arguments.args.first()
                            {
                                if let Some(vec) = inner.path.segments.last() {
                                    if vec.ident == "Vec" {
                                        if let syn::PathArguments::AngleBracketed(items) =
                                            &vec.arguments
                                        {
                                            if let Some(syn::GenericArgument::Type(Type::Path(
                                                row,
                                            ))) = items.args.first()
                                            {
                                                if let Some(ident) = row.path.segments.last() {
                                                    if let Some(table) = tables
                                                        .iter()
                                                        .find(|table| table.ident == ident.ident)
                                                    {
                                                        return Ok(table);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    infer_table(operation, args, tables, lowered)
}

fn infer_table<'a>(
    operation: &Operation,
    args: &QueryArgs,
    tables: &'a [Table],
    lowered: &[LoweredTable],
) -> syn::Result<&'a Table> {
    let mut candidate: Option<&Table> = None;
    let mut consider = |expr: &RustExpr| -> syn::Result<()> {
        let Some(table_ident) = first_field_table(expr) else {
            return Ok(());
        };
        let table = tables
            .iter()
            .find(|table| table.ident == table_ident)
            .ok_or_else(|| syn::Error::new(expr.span(), "unknown table in query expression"))?;
        match candidate {
            Some(existing) if existing.db_name != table.db_name => {
                return Err(syn::Error::new(
                    expr.span(),
                    "query expressions reference more than one table; use `from` and a projection",
                ))
            }
            _ => candidate = Some(table),
        }
        Ok(())
    };
    if let Some(filter) = &args.filter {
        consider(filter)?;
    }
    for order in &args.order {
        consider(order)?;
    }
    if let Some(limit) = &args.limit {
        let _ = limit;
    }
    if let Some(offset) = &args.offset {
        consider(offset)?;
    }
    if let Some(table) = candidate {
        return Ok(table);
    }
    let span = operation.method.sig.span();
    let hint = lowered
        .iter()
        .map(|table| table.struct_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(syn::Error::new(
        span,
        format!("cannot infer the table for this operation; add `from = <Table>` (tables: {hint})"),
    ))
}

fn update_table<'a>(
    args: &UpdateArgs,
    params: &[Param],
    tables: &'a [Table],
    lowered: &[LoweredTable],
) -> syn::Result<&'a Table> {
    let mut candidate: Option<&Table> = None;
    let expressions = args.set.iter().chain(args.filter.iter());
    for expr in expressions {
        let Some(table_ident) = first_field_table(expr) else {
            continue;
        };
        let table = tables
            .iter()
            .find(|table| table.ident == table_ident)
            .ok_or_else(|| syn::Error::new(expr.span(), "unknown table in update expression"))?;
        match candidate {
            Some(existing) if existing.db_name != table.db_name => {
                return Err(syn::Error::new(
                    expr.span(),
                    "update expressions reference more than one table",
                ));
            }
            _ => candidate = Some(table),
        }
    }
    let _ = (params, lowered);
    candidate.ok_or_else(|| {
        syn::Error::new(
            args.set
                .first()
                .map(|expr| expr.span())
                .unwrap_or_else(Span::call_site),
            "cannot infer the table for this update; reference a field in `set` or `filter`",
        )
    })
}

fn delete_table<'a>(
    args: &DeleteArgs,
    params: &[Param],
    tables: &'a [Table],
    lowered: &[LoweredTable],
) -> syn::Result<&'a Table> {
    let _ = (params, lowered);
    let filter = args.filter.as_ref().ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
            "delete requires a `filter` so the table can be inferred",
        )
    })?;
    let table_ident = first_field_table(filter).ok_or_else(|| {
        syn::Error::new(
            filter.span(),
            "delete filter must reference a field so the table can be inferred",
        )
    })?;
    tables
        .iter()
        .find(|table| table.ident == table_ident)
        .ok_or_else(|| syn::Error::new(filter.span(), "unknown table in delete filter"))
}

fn table_from_type<'a>(ty: &Type, tables: &'a [Table]) -> syn::Result<&'a Table> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new(ty.span(), "expected a table name"));
    };
    let ident = path
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(ty.span(), "expected a table name"))?
        .ident
        .clone();
    tables
        .iter()
        .find(|table| table.ident == ident)
        .ok_or_else(|| syn::Error::new(ty.span(), format!("unknown table `{ident}`")))
}

fn lower_query(
    args: &QueryArgs,
    params: &[Param],
    tables: &[Table],
    lowered: &[LoweredTable],
    table: &Table,
    count: bool,
) -> syn::Result<Body> {
    let single = [(table, table.db_name.clone())];
    let filter = lower_filter(&args.filter, params, tables, lowered, &single)?;
    let mut order = Vec::new();
    for expr in &args.order {
        let (field, descending) = lower_order(expr, tables, lowered, table)?;
        order.push((field, descending));
    }
    let limit = match &args.limit {
        Some(expr) => Some(lower_limit(expr, params)?),
        None => None,
    };
    let offset = match &args.offset {
        Some(expr) => Some(lower_bound(expr, params, "offset")?),
        None => None,
    };
    Ok(Body::Query {
        table: table.db_name.clone(),
        alias: None,
        joins: Vec::new(),
        select: Vec::new(),
        group_by: Vec::new(),
        filter,
        order,
        limit,
        offset,
        count,
    })
}

fn lower_filter(
    filter: &Option<RustExpr>,
    params: &[Param],
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<Option<Expr>> {
    let Some(filter) = filter else {
        return Ok(None);
    };
    let lowered = lower_predicate(filter, params, tables, lowered, sources)?;
    reject_null_comparisons(&lowered, filter.span())?;
    Ok(Some(lowered))
}

fn reject_null_comparisons(expr: &Expr, span: Span) -> syn::Result<()> {
    match expr {
        Expr::Cmp { left, right, .. } => {
            for operand in [left, right] {
                if matches!(operand, Operand::Literal(Value::Null(_))) {
                    return Err(syn::Error::new(
                        span,
                        "comparisons with `None` never match; use `is_null()` or `is_not_null()` instead",
                    ));
                }
            }
            Ok(())
        }
        Expr::And(left, right) | Expr::Or(left, right) => {
            reject_null_comparisons(left, span)?;
            reject_null_comparisons(right, span)
        }
        Expr::IsNull { .. } | Expr::IsNotNull { .. } => Ok(()),
    }
}

fn lower_predicate(
    expr: &RustExpr,
    params: &[Param],
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<Expr> {
    match expr {
        RustExpr::Paren(paren) => lower_predicate(&paren.expr, params, tables, lowered, sources),
        RustExpr::Group(group) => lower_predicate(&group.expr, params, tables, lowered, sources),
        RustExpr::Binary(binary) => match binary.op {
            BinOp::BitAnd(_) | BinOp::And(_) => Ok(Expr::And(
                Box::new(lower_predicate(
                    &binary.left,
                    params,
                    tables,
                    lowered,
                    sources,
                )?),
                Box::new(lower_predicate(
                    &binary.right,
                    params,
                    tables,
                    lowered,
                    sources,
                )?),
            )),
            BinOp::BitOr(_) | BinOp::Or(_) => Ok(Expr::Or(
                Box::new(lower_predicate(
                    &binary.left,
                    params,
                    tables,
                    lowered,
                    sources,
                )?),
                Box::new(lower_predicate(
                    &binary.right,
                    params,
                    tables,
                    lowered,
                    sources,
                )?),
            )),
            _ => Err(syn::Error::new(
                binary.op.span(),
                "unsupported boolean operator in query filter",
            )),
        },
        RustExpr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => negate(lower_predicate(
            &unary.expr,
            params,
            tables,
            lowered,
            sources,
        )?),
        RustExpr::MethodCall(call) => {
            let method = call.method.to_string();
            if matches!(method.as_str(), "and" | "or" | "not") {
                let receiver = lower_predicate(&call.receiver, params, tables, lowered, sources)?;
                return match method.as_str() {
                    "not" => negate(receiver),
                    "and" => Ok(Expr::And(
                        Box::new(receiver),
                        Box::new(lower_predicate(
                            call.args.first().ok_or_else(|| {
                                syn::Error::new(call.span(), "`and` needs one argument")
                            })?,
                            params,
                            tables,
                            lowered,
                            sources,
                        )?),
                    )),
                    "or" => Ok(Expr::Or(
                        Box::new(receiver),
                        Box::new(lower_predicate(
                            call.args.first().ok_or_else(|| {
                                syn::Error::new(call.span(), "`or` needs one argument")
                            })?,
                            params,
                            tables,
                            lowered,
                            sources,
                        )?),
                    )),
                    _ => unreachable!(),
                };
            }
            lower_comparison(
                &call.receiver,
                &method,
                call.args.first(),
                call.span(),
                params,
                tables,
                lowered,
                sources,
            )
        }
        _ => Err(syn::Error::new(
            expr.span(),
            "unsupported query expression; use typed field methods such as `Message::id.eq(value)`",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn reject_null_comparison(operand: &Operand, span: Span, method: &str) -> syn::Result<()> {
    if matches!(operand, Operand::Literal(Value::Null(_))) {
        return Err(syn::Error::new(
            span,
            format!(
                "`{method}` with `None` never matches; use `is_null()` or `is_not_null()` instead"
            ),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_comparison(
    receiver: &RustExpr,
    method: &str,
    argument: Option<&RustExpr>,
    span: Span,
    params: &[Param],
    tables: &[Table],
    lowered: &[LoweredTable],
    sources: &[(&Table, String)],
) -> syn::Result<Expr> {
    if method == "is_null" || method == "is_not_null" {
        if argument.is_some() {
            return Err(syn::Error::new(
                span,
                format!("`{method}` takes no argument"),
            ));
        }
        let (field, _) = resolve_field_ref(receiver, tables, lowered, sources)?;
        return Ok(if method == "is_null" {
            Expr::IsNull { field }
        } else {
            Expr::IsNotNull { field }
        });
    }
    let (field, field_ty) = resolve_field_any(receiver, tables, lowered, sources)?;
    if matches!(method, "eq_field" | "ne_field") {
        let argument = argument
            .ok_or_else(|| syn::Error::new(span, format!("`{method}` needs a field argument")))?;
        let (other, other_ty) = resolve_field_any(argument, tables, lowered, sources)?;
        if field_ty != other_ty {
            return Err(syn::Error::new(
                argument.span(),
                "comparison fields have different types",
            ));
        }
        return Ok(Expr::Cmp {
            left: Operand::Field(field),
            op: if method == "eq_field" {
                Cmp::Eq
            } else {
                Cmp::Ne
            },
            right: Operand::Field(other),
        });
    }
    if method == "between" {
        let argument =
            argument.ok_or_else(|| syn::Error::new(span, "`between` needs a range argument"))?;
        let RustExpr::Range(range) = argument else {
            return Err(syn::Error::new(
                argument.span(),
                "`between` needs an inclusive range such as `1..=9`",
            ));
        };
        if !matches!(range.limits, syn::RangeLimits::Closed(_)) {
            return Err(syn::Error::new(
                argument.span(),
                "`between` uses an inclusive range (`..=`)",
            ));
        }
        let start = range
            .start
            .as_deref()
            .ok_or_else(|| syn::Error::new(argument.span(), "range has no start"))?;
        let end = range
            .end
            .as_deref()
            .ok_or_else(|| syn::Error::new(argument.span(), "range has no end"))?;
        return Ok(Expr::And(
            Box::new(Expr::Cmp {
                left: Operand::Field(field.clone()),
                op: Cmp::Ge,
                right: lower_operand(start, field_ty, params)?,
            }),
            Box::new(Expr::Cmp {
                left: Operand::Field(field),
                op: Cmp::Le,
                right: lower_operand(end, field_ty, params)?,
            }),
        ));
    }
    if method == "in_" {
        let argument =
            argument.ok_or_else(|| syn::Error::new(span, "`in_` needs a collection argument"))?;
        let values = match argument {
            RustExpr::Array(array) => array.elems.iter().collect::<Vec<_>>(),
            RustExpr::Tuple(tuple) => tuple.elems.iter().collect::<Vec<_>>(),
            _ => {
                return Err(syn::Error::new(
                    argument.span(),
                    "`in_` accepts an array or tuple literal in a schema query",
                ))
            }
        };
        if values.is_empty() {
            return Err(syn::Error::new(
                argument.span(),
                "`in_` requires at least one value",
            ));
        }
        let mut predicates = values
            .into_iter()
            .map(|value| {
                Ok(Expr::Cmp {
                    left: Operand::Field(field.clone()),
                    op: Cmp::Eq,
                    right: lower_operand(value, field_ty, params)?,
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let mut result = predicates.remove(0);
        for predicate in predicates {
            result = Expr::Or(Box::new(result), Box::new(predicate));
        }
        return Ok(result);
    }
    let op = match method {
        "eq" => Cmp::Eq,
        "ne" => Cmp::Ne,
        "lt" => Cmp::Lt,
        "le" => Cmp::Le,
        "gt" => Cmp::Gt,
        "ge" => Cmp::Ge,
        other => {
            return Err(syn::Error::new(
                span,
                format!("unknown field method `{other}` in a query"),
            ))
        }
    };
    let argument =
        argument.ok_or_else(|| syn::Error::new(span, format!("`{method}` needs one argument")))?;
    let right = lower_operand(argument, field_ty, params)?;
    reject_null_comparison(&right, argument.span(), method)?;
    Ok(Expr::Cmp {
        left: Operand::Field(field),
        op,
        right,
    })
}

fn negate(expr: Expr) -> syn::Result<Expr> {
    Ok(match expr {
        Expr::Cmp { left, op, right } => Expr::Cmp {
            left,
            op: match op {
                Cmp::Eq => Cmp::Ne,
                Cmp::Ne => Cmp::Eq,
                Cmp::Lt => Cmp::Ge,
                Cmp::Le => Cmp::Gt,
                Cmp::Gt => Cmp::Le,
                Cmp::Ge => Cmp::Lt,
            },
            right,
        },
        Expr::And(left, right) => Expr::Or(Box::new(negate(*left)?), Box::new(negate(*right)?)),
        Expr::Or(left, right) => Expr::And(Box::new(negate(*left)?), Box::new(negate(*right)?)),
        Expr::IsNull { field } => Expr::IsNotNull { field },
        Expr::IsNotNull { field } => Expr::IsNull { field },
    })
}

fn lower_operand(expr: &RustExpr, ty: Ty, params: &[Param]) -> syn::Result<Operand> {
    match expr {
        RustExpr::Paren(paren) => lower_operand(&paren.expr, ty, params),
        RustExpr::Group(group) => lower_operand(&group.expr, ty, params),
        RustExpr::Call(call) => {
            let is_some = match &*call.func {
                RustExpr::Path(path) => {
                    let segments = &path.path.segments;
                    segments.len() == 1 && segments[0].ident == "Some"
                        || segments.len() == 2
                            && segments[0].ident == "Option"
                            && segments[1].ident == "Some"
                }
                _ => false,
            };
            if !is_some || call.args.len() != 1 {
                return Err(syn::Error::new(
                    expr.span(),
                    "expected a parameter, literal, or field in a query",
                ));
            }
            lower_operand(&call.args[0], ty, params)
        }
        RustExpr::Path(path) => {
            if path.path.segments.len() == 1 {
                let ident = &path.path.segments[0].ident;
                if let Some(param) = params.iter().find(|param| param.ident == *ident) {
                    if param.ir != ty {
                        return Err(syn::Error::new(
                            expr.span(),
                            format!(
                                "parameter `{ident}` is {} but the field is {}",
                                param.ir, ty
                            ),
                        ));
                    }
                    return Ok(Operand::Param(ident.to_string()));
                }
                if ident == "None" {
                    return Ok(Operand::Literal(Value::Null(ty)));
                }
            }
            Err(syn::Error::new(
                expr.span(),
                "expected a parameter, literal, or field in a query",
            ))
        }
        RustExpr::Lit(literal) => crate::model::literal_for_ty(&literal.lit, ty)
            .map(Operand::Literal)
            .ok_or_else(|| {
                syn::Error::new(expr.span(), format!("literal is not a valid {ty} value"))
            }),
        RustExpr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => {
            let RustExpr::Lit(literal) = unary.expr.as_ref() else {
                return Err(syn::Error::new(
                    expr.span(),
                    "negative literals must be written directly",
                ));
            };
            let value = match (&literal.lit, ty) {
                (Lit::Int(value), Ty::I8) => value
                    .base10_parse::<i8>()
                    .ok()
                    .map(|value| Value::I8(-value)),
                (Lit::Int(value), Ty::I16) => value
                    .base10_parse::<i16>()
                    .ok()
                    .map(|value| Value::I16(-value)),
                (Lit::Int(value), Ty::I32) => value
                    .base10_parse::<i32>()
                    .ok()
                    .map(|value| Value::I32(-value)),
                (Lit::Int(value), Ty::I64) => value
                    .base10_parse::<i64>()
                    .ok()
                    .map(|value| Value::I64(-value)),
                (Lit::Float(value), Ty::F32) => value
                    .base10_parse::<f32>()
                    .ok()
                    .map(|value| Value::F32(-value)),
                (Lit::Float(value), Ty::F64) => value
                    .base10_parse::<f64>()
                    .ok()
                    .map(|value| Value::F64(-value)),
                _ => None,
            };
            value.map(Operand::Literal).ok_or_else(|| {
                syn::Error::new(
                    expr.span(),
                    format!("negative literal is not a valid {ty} value"),
                )
            })
        }
        _ => Err(syn::Error::new(
            expr.span(),
            "expected a parameter, literal, or field in a query",
        )),
    }
}

fn resolve_field(
    expr: &RustExpr,
    tables: &[Table],
    lowered: &[LoweredTable],
    expected: &Table,
) -> syn::Result<(String, Ty)> {
    let (struct_ident, field_ident) = crate::model::field_path(expr).ok_or_else(|| {
        syn::Error::new(
            expr.span(),
            "expected a field handle such as `Message::sent_at`",
        )
    })?;
    let table = tables
        .iter()
        .find(|table| table.ident == struct_ident)
        .ok_or_else(|| syn::Error::new(expr.span(), format!("unknown table `{struct_ident}`")))?;
    if table.db_name != expected.db_name {
        return Err(syn::Error::new(
            expr.span(),
            format!(
                "field belongs to table `{}` but the operation reads `{}`",
                table.db_name, expected.db_name
            ),
        ));
    }
    let field = table
        .fields
        .iter()
        .find(|field| field.ident == field_ident)
        .ok_or_else(|| {
            syn::Error::new(
                expr.span(),
                format!("unknown field `{struct_ident}::{field_ident}`"),
            )
        })?;
    let _ = lowered;
    Ok((field.ident.to_string(), field.ir))
}

fn lower_order(
    expr: &RustExpr,
    tables: &[Table],
    lowered: &[LoweredTable],
    table: &Table,
) -> syn::Result<(String, bool)> {
    let RustExpr::MethodCall(call) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "order terms must be `Field::asc()` or `Field::desc()`",
        ));
    };
    let descending = match call.method.to_string().as_str() {
        "asc" => false,
        "desc" => true,
        other => {
            return Err(syn::Error::new(
                call.method.span(),
                format!("unknown order method `{other}`"),
            ))
        }
    };
    let (field, _) = resolve_field(&call.receiver, tables, lowered, table)?;
    Ok((field, descending))
}

fn lower_limit(expr: &RustExpr, params: &[Param]) -> syn::Result<Limit> {
    lower_bound(expr, params, "limit")
}

fn lower_bound(expr: &RustExpr, params: &[Param], what: &str) -> syn::Result<Limit> {
    match expr {
        RustExpr::Paren(paren) => lower_bound(&paren.expr, params, what),
        RustExpr::Group(group) => lower_bound(&group.expr, params, what),
        RustExpr::Lit(literal) => match &literal.lit {
            Lit::Int(value) => Ok(Limit::Literal(value.base10_parse()?)),
            _ => Err(syn::Error::new(
                expr.span(),
                format!("{what} must be a non-negative integer or integer parameter"),
            )),
        },
        RustExpr::Path(path) => {
            let ident = path.path.get_ident().ok_or_else(|| {
                syn::Error::new(expr.span(), format!("{what} must name a parameter"))
            })?;
            let param = params
                .iter()
                .find(|param| param.ident == *ident)
                .ok_or_else(|| {
                    syn::Error::new(expr.span(), format!("unknown parameter `{ident}`"))
                })?;
            if !param.ir.is_integer() {
                return Err(syn::Error::new(
                    expr.span(),
                    format!("{what} parameter must be an integer"),
                ));
            }
            Ok(Limit::Param(ident.to_string()))
        }
        _ => Err(syn::Error::new(
            expr.span(),
            format!("{what} must be a literal or an integer parameter"),
        )),
    }
}

fn lower_sets(
    args: &UpdateArgs,
    params: &[Param],
    tables: &[Table],
    lowered: &[LoweredTable],
    table: &Table,
) -> syn::Result<Vec<(String, Operand)>> {
    let mut sets = Vec::new();
    for expr in &args.set {
        let RustExpr::MethodCall(call) = expr else {
            return Err(syn::Error::new(
                expr.span(),
                "update assignments are `Field::set(value)`",
            ));
        };
        if call.method != "set" {
            return Err(syn::Error::new(
                call.method.span(),
                "update assignments are `Field::set(value)`",
            ));
        }
        let (field, ty) = resolve_field(&call.receiver, tables, lowered, table)?;
        if table.fields[table.primary_key].ident == field {
            return Err(syn::Error::new(
                call.receiver.span(),
                "update cannot assign the primary key",
            ));
        }
        let value = call
            .args
            .first()
            .ok_or_else(|| syn::Error::new(call.span(), "`set` needs one argument"))?;
        let value = lower_operand(value, ty, params)?;
        if matches!(value, Operand::Literal(Value::Null(_))) {
            let nullable = table
                .fields
                .iter()
                .find(|candidate| candidate.ident == field)
                .is_some_and(|candidate| candidate.nullable);
            if !nullable {
                return Err(syn::Error::new(
                    call.span(),
                    format!("cannot assign `None` to non-nullable field `{field}`"),
                ));
            }
        }
        sets.push((field, value));
    }
    if sets.is_empty() {
        return Err(syn::Error::new(
            args.set
                .first()
                .map(|expr| expr.span())
                .unwrap_or_else(Span::call_site),
            "update requires at least one assignment",
        ));
    }
    Ok(sets)
}

fn default_value(expr: &RustExpr, ty: Ty) -> syn::Result<Value> {
    match expr {
        RustExpr::Lit(literal) => crate::model::literal_for_ty(&literal.lit, ty).ok_or_else(|| {
            syn::Error::new(expr.span(), format!("default is not a valid {ty} literal"))
        }),
        RustExpr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => {
            let RustExpr::Lit(literal) = unary.expr.as_ref() else {
                return Err(syn::Error::new(expr.span(), "invalid default"));
            };
            match (&literal.lit, ty) {
                (Lit::Int(value), Ty::I64) => Ok(Value::I64(-value.base10_parse::<i64>()?)),
                (Lit::Int(value), Ty::I32) => Ok(Value::I32(-value.base10_parse::<i32>()?)),
                (Lit::Int(value), Ty::I16) => Ok(Value::I16(-value.base10_parse::<i16>()?)),
                (Lit::Int(value), Ty::I8) => Ok(Value::I8(-value.base10_parse::<i8>()?)),
                (Lit::Float(value), Ty::F64) => Ok(Value::F64(-value.base10_parse::<f64>()?)),
                (Lit::Float(value), Ty::F32) => Ok(Value::F32(-value.base10_parse::<f32>()?)),
                _ => Err(syn::Error::new(expr.span(), "invalid default")),
            }
        }
        RustExpr::Path(path) if path.path.is_ident("String") => Err(syn::Error::new(
            expr.span(),
            "text defaults must be string literals",
        )),
        _ => Err(syn::Error::new(
            expr.span(),
            "defaults must be integer, float, string, or bool literals",
        )),
    }
}

fn first_field_table(expr: &RustExpr) -> Option<syn::Ident> {
    match expr {
        RustExpr::Path(_) => crate::model::field_path(expr).map(|(table, _)| table),
        RustExpr::MethodCall(call) => first_field_table(&call.receiver)
            .or_else(|| call.args.first().and_then(first_field_table)),
        RustExpr::Binary(binary) => {
            first_field_table(&binary.left).or_else(|| first_field_table(&binary.right))
        }
        RustExpr::Unary(unary) => first_field_table(&unary.expr),
        RustExpr::Paren(paren) => first_field_table(&paren.expr),
        RustExpr::Group(group) => first_field_table(&group.expr),
        RustExpr::Tuple(tuple) => tuple.elems.iter().find_map(first_field_table),
        RustExpr::Array(array) => array.elems.iter().find_map(first_field_table),
        RustExpr::Range(range) => range
            .start
            .as_deref()
            .and_then(first_field_table)
            .or_else(|| range.end.as_deref().and_then(first_field_table)),
        RustExpr::Reference(reference) => first_field_table(&reference.expr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_snake_case_struct_names() {
        assert_eq!(snake_case("Message"), "message");
        assert_eq!(snake_case("UnreadDigest"), "unread_digest");
        assert_eq!(snake_case("ABC"), "a_b_c");
        assert_eq!(snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn hidden_crud_names_are_namespaced_by_table_index() {
        let first = crud_names(0);
        let second = crud_names(1);
        assert_eq!(first.insert, "__ls_crud_0_insert");
        assert_eq!(second.scan, "__ls_crud_1_scan");
        assert_ne!(first.get, second.get);
    }
}
