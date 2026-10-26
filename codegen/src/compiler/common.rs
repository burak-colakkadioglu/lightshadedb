use super::explain;
use super::plan::{resolve_source, Plan};
use super::{ACTIVE_MIGRATIONS, ACTIVE_ROW_TYPES};
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn type_tag(ty: Ty) -> u8 {
    match ty {
        Ty::I64 => 0,
        Ty::Bool => 1,
        Ty::Text => 2,
        Ty::I8 => 3,
        Ty::I16 => 4,
        Ty::I32 => 5,
        Ty::U8 => 6,
        Ty::U16 => 7,
        Ty::U32 => 8,
        Ty::U64 => 9,
        Ty::Bytes => 10,
        Ty::Uuid => 11,
        Ty::F32 => 12,
        Ty::F64 => 13,
    }
}

pub(crate) fn default_expression(value: &Value) -> String {
    match value {
        Value::I8(value) => format!("{value}i8"),
        Value::I16(value) => format!("{value}i16"),
        Value::I32(value) => format!("{value}i32"),
        Value::I64(value) => format!("{value}i64"),
        Value::U8(value) => format!("{value}u8"),
        Value::U16(value) => format!("{value}u16"),
        Value::U32(value) => format!("{value}u32"),
        Value::U64(value) => format!("{value}u64"),
        Value::F32(value) => format!("f32::from_bits({}u32)", value.to_bits()),
        Value::F64(value) => format!("f64::from_bits({}u64)", value.to_bits()),
        Value::Bool(value) => value.to_string(),
        Value::Text(value) => format!("{value:?}.to_string()"),
        Value::Bytes(value) => format!("vec!{value:?}"),
        Value::Uuid(value) => format!("{value:?}"),
        Value::Null(_) => "None".to_string(),
    }
}

pub(crate) fn owned_pattern(ty: Ty) -> &'static str {
    match ty {
        Ty::I8 => "I8(value)",
        Ty::I16 => "I16(value)",
        Ty::I32 => "I32(value)",
        Ty::I64 => "I64(value)",
        Ty::U8 => "U8(value)",
        Ty::U16 => "U16(value)",
        Ty::U32 => "U32(value)",
        Ty::U64 => "U64(value)",
        Ty::F32 => "F32(value)",
        Ty::F64 => "F64(value)",
        Ty::Bool => "Bool(value)",
        Ty::Text => "Text(value)",
        Ty::Bytes => "Bytes(value)",
        Ty::Uuid => "Uuid(value)",
    }
}

pub(crate) fn owned_argument(ty: Ty) -> &'static str {
    match ty {
        Ty::Text | Ty::Bytes => "value.clone()",
        _ => "*value",
    }
}

pub(crate) fn owned_extraction(ty: Ty) -> &'static str {
    match ty {
        Ty::I8 => "I8(value) => *value",
        Ty::I16 => "I16(value) => *value",
        Ty::I32 => "I32(value) => *value",
        Ty::I64 => "I64(value) => *value",
        Ty::U8 => "U8(value) => *value",
        Ty::U16 => "U16(value) => *value",
        Ty::U32 => "U32(value) => *value",
        Ty::U64 => "U64(value) => *value",
        Ty::F32 => "F32(value) => *value",
        Ty::F64 => "F64(value) => *value",
        Ty::Bool => "Bool(value) => *value",
        Ty::Text => "Text(value) => value.clone()",
        Ty::Bytes => "Bytes(value) => value.clone()",
        Ty::Uuid => "Uuid(value) => *value",
    }
}

pub(crate) fn row_type(table: &str) -> String {
    ACTIVE_ROW_TYPES.with(|types| {
        types
            .borrow()
            .get(table)
            .cloned()
            .unwrap_or_else(|| format!("Row_{table}"))
    })
}

pub(crate) fn stored_table_name(table: &str) -> String {
    ACTIVE_MIGRATIONS.with(|mappings| {
        mappings
            .borrow()
            .get(table)
            .map(|mapping| mapping.stored_name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| table.to_string())
    })
}

pub(crate) fn stored_field_name(table: &str, field: &str) -> String {
    ACTIVE_MIGRATIONS.with(|mappings| {
        mappings
            .borrow()
            .get(table)
            .and_then(|mapping| mapping.fields.get(field))
            .map(|mapping| mapping.stored_name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| field.to_string())
    })
}

pub(crate) fn field_conversion(table: &str, field: &str) -> Option<(crate::ir::Ty, String)> {
    ACTIVE_MIGRATIONS.with(|mappings| {
        mappings
            .borrow()
            .get(table)
            .and_then(|mapping| mapping.fields.get(field))
            .and_then(|mapping| mapping.convert.clone())
    })
}

pub(crate) fn key_rust(ty: Ty) -> String {
    match ty {
        Ty::Text => "__LsText".to_string(),
        Ty::F32 => "__LsOrdF32".to_string(),
        Ty::F64 => "__LsOrdF64".to_string(),
        _ => ty.rust().to_string(),
    }
}

pub(crate) fn param_rust(param: &Param) -> String {
    if param.nullable {
        format!("Option<{}>", param.ty.rust())
    } else {
        param.ty.rust().to_string()
    }
}

pub(crate) fn group_key_rust(ty: Ty) -> String {
    match ty {
        Ty::F32 => "__LsOrdF32".to_string(),
        Ty::F64 => "__LsOrdF64".to_string(),
        _ => ty.rust().to_string(),
    }
}

pub(crate) fn emit_slow_sample(
    out: &mut String,
    workload: &Workload,
    plan: &Plan,
    operation: &Operation,
) -> Result<(), String> {
    let described = explain::describe_operation(workload, plan, operation)?;
    writeln!(
        out,
        "        let __ls_slow = __LsSlowSample::__ls_new({:?}, {:?});",
        operation.name, described.strategy
    )
    .unwrap();
    Ok(())
}

pub(crate) fn literal_with_ty(value: &Value, ty: Option<Ty>) -> String {
    match ty.and_then(|ty| value.clone().coerce_to(ty)) {
        Some(value) => literal(&value),
        None => literal(value),
    }
}

pub(crate) fn literal(value: &Value) -> String {
    match value {
        Value::I8(value) => format!("{value}i8"),
        Value::I16(value) => format!("{value}i16"),
        Value::I32(value) => format!("{value}i32"),
        Value::I64(value) if *value == i64::MIN => "i64::MIN".to_string(),
        Value::I64(value) => format!("{value}i64"),
        Value::U8(value) => format!("{value}u8"),
        Value::U16(value) => format!("{value}u16"),
        Value::U32(value) => format!("{value}u32"),
        Value::U64(value) => format!("{value}u64"),
        Value::F32(value) => format!("f32::from_bits({}u32)", value.to_bits()),
        Value::F64(value) => format!("f64::from_bits({}u64)", value.to_bits()),
        Value::Bool(value) => value.to_string(),
        Value::Text(value) => format!("{value:?}.to_string()"),
        Value::Bytes(value) => format!("vec!{value:?}"),
        Value::Uuid(value) => format!("{value:?}"),
        Value::Null(_) => "None".to_string(),
    }
}

pub(crate) fn emit_expr(
    expr: &Expr,
    row: &str,
    table: &Table,
    params: &[Param],
) -> Result<String, String> {
    match expr {
        Expr::Cmp { left, op, right } => {
            let left_ty = table_operand_ty(left, table, params)?;
            let right_ty = table_operand_ty(right, table, params)?;
            let left_nullable = operand_nullable_table(left, table, params)?;
            let right_nullable = operand_nullable_table(right, table, params)?;
            let left = operand_expr(left, row, Some(right_ty))?;
            let right = operand_expr(right, row, Some(left_ty))?;
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
            emit_expr(left, row, table, params)?,
            emit_expr(right, row, table, params)?
        )),
        Expr::Or(left, right) => Ok(format!(
            "({} || {})",
            emit_expr(left, row, table, params)?,
            emit_expr(right, row, table, params)?
        )),
        Expr::IsNull { field } => {
            let nullable = table
                .field(&field.field)
                .map(|(_, field)| field.nullable)
                .unwrap_or(true);
            if nullable {
                Ok(format!("{row}.{field}.is_none()", field = field.field))
            } else {
                Ok("false".to_string())
            }
        }
        Expr::IsNotNull { field } => {
            let nullable = table
                .field(&field.field)
                .map(|(_, field)| field.nullable)
                .unwrap_or(true);
            if nullable {
                Ok(format!("{row}.{field}.is_some()", field = field.field))
            } else {
                Ok("true".to_string())
            }
        }
    }
}

pub(crate) fn table_operand_ty(
    operand: &Operand,
    table: &Table,
    params: &[Param],
) -> Result<Ty, String> {
    match operand {
        Operand::Field(name) => table
            .field(name)
            .map(|(_, field)| field.ty)
            .ok_or_else(|| format!("unknown field `{name}`")),
        Operand::Param(name) => params
            .iter()
            .find(|param| &param.name == name)
            .map(|param| param.ty)
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(value) => Ok(value.ty()),
    }
}

pub(crate) fn operand_expr(
    operand: &Operand,
    row: &str,
    hint: Option<Ty>,
) -> Result<String, String> {
    match operand {
        Operand::Field(name) => Ok(format!("{row}.{name}")),
        Operand::Param(name) => Ok(name.clone()),
        Operand::Literal(value) => Ok(literal_with_ty(value, hint)),
    }
}

pub(crate) fn operand_value(
    operand: &Operand,
    row: &str,
    hint: Option<Ty>,
) -> Result<String, String> {
    match operand {
        Operand::Field(name) => Ok(format!("{row}.{name}.clone()")),
        Operand::Param(name) => Ok(format!("{name}.clone()")),
        Operand::Literal(value) => Ok(literal_with_ty(value, hint)),
    }
}

pub(crate) fn lookup_operand(operand: &Operand, ty: Ty) -> Result<String, String> {
    match operand {
        Operand::Param(name) => Ok(format!("{name}.clone()")),
        Operand::Literal(value) => Ok(literal_with_ty(value, Some(ty))),
        Operand::Field(name) => Err(format!("internal: field `{name}` cannot be a lookup value")),
    }
}

pub(crate) fn lookup_eq_ctor(
    name: &str,
    eq_fields: &[String],
    constraints: &[(String, Operand)],
    table: &Table,
) -> Result<String, String> {
    if eq_fields.is_empty() {
        return Ok(name.to_string());
    }
    if eq_fields.len() != constraints.len() {
        return Err("internal: index key does not match its constraints".to_string());
    }
    let mut fields = Vec::with_capacity(eq_fields.len());
    for (index, (field, _)) in constraints.iter().enumerate() {
        if &eq_fields[index] != field {
            return Err("internal: index key field order mismatch".to_string());
        }
        let ty = table
            .field(&eq_fields[index])
            .map(|(_, field)| field.ty)
            .ok_or_else(|| format!("internal: unknown index field `{}`", eq_fields[index]))?;
        fields.push(format!(
            "e{index}: {}.into()",
            lookup_operand(&constraints[index].1, ty)?
        ));
    }
    Ok(format!("{name} {{ {} }}", fields.join(", ")))
}

pub(crate) fn operand_nullable_table(
    operand: &Operand,
    table: &Table,
    params: &[Param],
) -> Result<bool, String> {
    match operand {
        Operand::Field(name) => table
            .field(name)
            .map(|(_, field)| field.nullable)
            .ok_or_else(|| format!("unknown field `{name}`")),
        Operand::Param(name) => params
            .iter()
            .find(|param| &param.name == name)
            .map(|param| param.nullable)
            .ok_or_else(|| format!("unknown parameter `{name}`")),
        Operand::Literal(Value::Null(_)) => Ok(true),
        Operand::Literal(_) => Ok(false),
    }
}

pub(crate) fn emit_nullable_cmp(
    left: &str,
    left_nullable: bool,
    right: &str,
    right_nullable: bool,
    symbol: &str,
) -> String {
    let left = if left_nullable {
        format!("{left}.as_ref()")
    } else {
        format!("Some(&({left}))")
    };
    let right = if right_nullable {
        format!("{right}.as_ref()")
    } else {
        format!("Some(&({right}))")
    };
    format!("(match ({left}, {right}) {{ (Some(__ls_l), Some(__ls_r)) => __ls_l {symbol} __ls_r, _ => false }})")
}

pub(crate) fn emit_field_read(
    reference: &FieldRef,
    sources: &[(&Table, String)],
    clone_text: bool,
) -> Result<String, String> {
    let (index, _, field) = resolve_source(reference, sources)?;
    if clone_text && field.ty == Ty::Text {
        Ok(format!("__ls_row_{index}.{}.clone()", field.name))
    } else {
        Ok(format!("__ls_row_{index}.{}", field.name))
    }
}
