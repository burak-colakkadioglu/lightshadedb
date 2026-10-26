use super::generate_library;
use crate::ir::{Body, Field, Operation, Param, Returns, Ty, Workload};

mod admin;
mod dataplane;
mod explain;
mod migrate;
mod serve;

use admin::emit_ops;
use dataplane::emit_data_plane;
use explain::emit_explain;
use migrate::emit_migrate_plan;
use serve::emit_protocol;

pub fn generate_interface(workload: &Workload) -> Result<String, String> {
    let mut out = generate_library(workload)?;
    out.push('\n');
    emit_wrapper(&mut out, workload)?;
    Ok(out)
}

pub(crate) fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

pub(crate) fn emit_wrapper(out: &mut String, workload: &Workload) -> Result<(), String> {
    let names = workload
        .operations
        .iter()
        .map(|operation| operation.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("const __LS_CLI_OPERATIONS: &str = {names:?};\n\n"));

    line(out, "fn main() {");
    line(
        out,
        "    let __ls_args: Vec<String> = std::env::args().collect();",
    );
    line(
        out,
        "    if __ls_args.len() >= 2 && __ls_args[1] == \"--explain\" {",
    );
    line(
        out,
        "        let __ls_result = __ls_cli_explain(&__ls_args[2..]);",
    );
    line(out, "        if let Err(__ls_message) = __ls_result {");
    line(out, "            eprintln!(\"error: {__ls_message}\");");
    line(out, "            std::process::exit(1);");
    line(out, "        }");
    line(out, "        return;");
    line(out, "    }");
    line(out, "    if __ls_args.len() < 3 {");
    line(out, "        eprintln!(\"usage: interface <database-or-socket> run <operation> [name=value ...] | serve | migrate | migrate-plan | verify | inspect | compact | recover [--discard-wal] | backup <destination> | dump <destination> | restore <dump> | export <table> <csv> | import <table> <csv>\");");
    line(out, "        std::process::exit(2);");
    line(out, "    }");
    line(out, "    let __ls_path = __ls_args[1].as_str();");
    line(out, "    let __ls_result = match __ls_args[2].as_str() {");
    line(out, "        \"run\" => {");
    line(out, "            if __ls_path.ends_with(\".sock\") {");
    line(
        out,
        "                __ls_cli_client_run(__ls_path, &__ls_args[3..])",
    );
    line(out, "            } else {");
    line(
        out,
        "                __ls_cli_run(__ls_path, &__ls_args[3..])",
    );
    line(out, "            }");
    line(out, "        }");
    line(out, "        \"serve\" => __ls_cli_serve(__ls_path),");
    line(out, "        \"migrate\" => __ls_cli_migrate(__ls_path),");
    line(
        out,
        "        \"migrate-plan\" => __ls_cli_migrate_plan(__ls_path),",
    );
    line(out, "        \"verify\" => __ls_cli_verify(__ls_path),");
    line(out, "        \"inspect\" => __ls_cli_inspect(__ls_path),");
    line(out, "        \"compact\" => __ls_cli_compact(__ls_path),");
    line(
        out,
        "        \"recover\" => __ls_cli_recover(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        \"backup\" => __ls_cli_backup(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        \"dump\" => __ls_cli_dump(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        \"restore\" => __ls_cli_restore(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        \"export\" => __ls_cli_export(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        \"import\" => __ls_cli_import(__ls_path, &__ls_args[3..]),",
    );
    line(
        out,
        "        __ls_other => Err(format!(\"unknown interface mode `{__ls_other}`\")),",
    );
    line(out, "    };");
    line(out, "    if let Err(__ls_message) = __ls_result {");
    line(out, "        eprintln!(\"error: {__ls_message}\");");
    line(out, "        std::process::exit(1);");
    line(out, "    }");
    line(out, "}");
    line(out, "");

    emit_run(out, workload)?;
    emit_helpers(out);
    emit_ops(out, workload)?;
    emit_migrate_plan(out, workload)?;
    emit_data_plane(out, workload)?;
    emit_explain(out, workload)?;
    emit_protocol(out, workload)?;
    Ok(())
}

pub(crate) fn emit_run(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(
        out,
        "fn __ls_cli_run(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(
        out,
        "    let (__ls_operation, __ls_rest) = match __ls_args.split_first() {",
    );
    line(
        out,
        "        Some((__ls_operation, __ls_rest)) => (__ls_operation.as_str(), __ls_rest),",
    );
    line(
        out,
        "        None => return Err(\"run expects an operation name\".to_string()),",
    );
    line(out, "    };");
    line(out, "    match __ls_operation {");
    for operation in &workload.operations {
        let known = operation
            .params
            .iter()
            .map(|param| format!("{:?}", param.name))
            .collect::<Vec<_>>()
            .join(", ");
        line(out, &format!("        {:?} => {{", operation.name));
        line(
            out,
            &format!(
                "            let __ls_pairs = __ls_cli_pairs(__ls_rest, &[{known}], {:?})?;",
                operation.name
            ),
        );
        match &operation.body {
            Body::Query { count: true, .. } => {
                line(out, "            __ls_cli_require_existing(__ls_path)?;");
                line(out, "            let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;");
                out.push_str(&format!(
                    "            println!(\"{{}}\", __ls_db.{}({}));\n",
                    operation.name,
                    call_args(operation)
                ));
            }
            Body::Query { count: false, .. } => {
                let returned = match &operation.returns {
                    Returns::Many(returned) => returned,
                    _ => {
                        return Err(format!(
                            "operation `{}` must return `many <table>`",
                            operation.name
                        ))
                    }
                };
                let table = workload
                    .table(returned)
                    .ok_or_else(|| format!("unknown table `{returned}`"))?;
                line(out, "            __ls_cli_require_existing(__ls_path)?;");
                line(out, "            let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;");
                out.push_str(&format!(
                    "            for __ls_row in __ls_db.{}({}) {{\n",
                    operation.name,
                    call_args(operation)
                ));
                line(out, "                let __ls_cells = vec![");
                for field in &table.fields {
                    line(
                        out,
                        &format!("                    {},", sqlite_row_cell(field)),
                    );
                }
                line(out, "                ];");
                line(
                    out,
                    "                println!(\"{}\", __ls_cells.join(\"\\t\"));",
                );
                line(out, "            }");
            }
            Body::Insert { .. } | Body::Update { .. } | Body::Delete { .. } => {
                line(out, "            let mut __ls_db = Db::open(__ls_path).map_err(|error| error.to_string())?;");
                out.push_str(&format!(
                    "            let __ls_outcome = __ls_db.{}({}).map_err(|error| error.to_string())?;\n",
                    operation.name,
                    call_args(operation)
                ));
                line(
                    out,
                    "            __ls_db.close().map_err(|error| error.to_string())?;",
                );
                line(out, "            println!(\"ok {__ls_outcome}\");");
            }
        }
        line(out, "            Ok(())");
        line(out, "        }");
    }
    line(
        out,
        "        __ls_other => Err(format!(\"unknown operation `{__ls_other}` (available: {})\", __LS_CLI_OPERATIONS)),",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    Ok(())
}

pub(crate) fn call_args(operation: &Operation) -> String {
    operation
        .params
        .iter()
        .map(argument_expr)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn argument_expr(param: &Param) -> String {
    match param.ty {
        Ty::I8 => format!("__ls_cli_parse::<i8>(&__ls_pairs, {:?})?", param.name),
        Ty::I16 => format!("__ls_cli_parse::<i16>(&__ls_pairs, {:?})?", param.name),
        Ty::I32 => format!("__ls_cli_parse::<i32>(&__ls_pairs, {:?})?", param.name),
        Ty::I64 => format!("__ls_cli_i64(&__ls_pairs, {:?})?", param.name),
        Ty::U8 => format!("__ls_cli_parse::<u8>(&__ls_pairs, {:?})?", param.name),
        Ty::U16 => format!("__ls_cli_parse::<u16>(&__ls_pairs, {:?})?", param.name),
        Ty::U32 => format!("__ls_cli_parse::<u32>(&__ls_pairs, {:?})?", param.name),
        Ty::U64 => format!("__ls_cli_parse::<u64>(&__ls_pairs, {:?})?", param.name),
        Ty::F32 => format!("__ls_cli_parse::<f32>(&__ls_pairs, {:?})?", param.name),
        Ty::F64 => format!("__ls_cli_parse::<f64>(&__ls_pairs, {:?})?", param.name),
        Ty::Bool => format!("__ls_cli_bool(&__ls_pairs, {:?})?", param.name),
        Ty::Text => format!("__ls_cli_text(&__ls_pairs, {:?})?", param.name),
        Ty::Bytes => format!("__ls_cli_bytes(&__ls_pairs, {:?})?", param.name),
        Ty::Uuid => format!("__ls_cli_uuid(&__ls_pairs, {:?})?", param.name),
    }
}

pub(crate) fn sqlite_row_cell(field: &Field) -> String {
    match field.ty {
        Ty::I8
        | Ty::I16
        | Ty::I32
        | Ty::I64
        | Ty::U8
        | Ty::U16
        | Ty::U32
        | Ty::U64
        | Ty::F32
        | Ty::F64
        | Ty::Bool => format!("__ls_row.{}.to_string()", field.name),
        Ty::Text => format!("__ls_cli_escape(&__ls_row.{})", field.name),
        Ty::Bytes => format!("__ls_cli_hex(&__ls_row.{})", field.name),
        Ty::Uuid => format!("__ls_cli_uuid_text(&__ls_row.{})", field.name),
    }
}

pub(crate) fn emit_helpers(out: &mut String) {
    line(out, "fn __ls_cli_pairs(__ls_args: &[String], __ls_known: &[&str], __ls_operation: &str) -> Result<Vec<(String, String)>, String> {");
    line(out, "    let mut __ls_pairs = Vec::new();");
    line(out, "    for __ls_arg in __ls_args {");
    line(
        out,
        "        let (__ls_key, __ls_value) = __ls_arg.split_once('=').ok_or_else(|| format!(\"argument `{__ls_arg}` is not name=value\"))?;",
    );
    line(out, "        if !__ls_known.contains(&__ls_key) {");
    line(
        out,
        "            return Err(format!(\"unknown argument `{__ls_key}` for operation `{__ls_operation}`\"));",
    );
    line(out, "        }");
    line(
        out,
        "        __ls_pairs.push((__ls_key.to_string(), __ls_value.to_string()));",
    );
    line(out, "    }");
    line(out, "    Ok(__ls_pairs)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_required<'a>(__ls_pairs: &'a [(String, String)], __ls_name: &str) -> Result<&'a str, String> {");
    line(out, "    __ls_pairs");
    line(out, "        .iter()");
    line(out, "        .find(|(__ls_key, _)| __ls_key == __ls_name)");
    line(out, "        .map(|(_, __ls_value)| __ls_value.as_str())");
    line(
        out,
        "        .ok_or_else(|| format!(\"missing argument `{__ls_name}`\"))",
    );
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_i64(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<i64, String> {");
    line(
        out,
        "    let __ls_value = __ls_cli_required(__ls_pairs, __ls_name)?;",
    );
    line(
        out,
        "    __ls_value.parse().map_err(|_| format!(\"argument `{__ls_name}` is not an i64: `{__ls_value}`\"))",
    );
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_parse<T: std::str::FromStr>(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<T, String> {");
    line(
        out,
        "    let __ls_value = __ls_cli_required(__ls_pairs, __ls_name)?;",
    );
    line(
        out,
        "    __ls_value.parse().map_err(|_| format!(\"argument `{__ls_name}` has an invalid value: `{__ls_value}`\"))",
    );
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_hex(__ls_value: &[u8]) -> String {");
    line(
        out,
        "    let mut __ls_out = String::with_capacity(__ls_value.len() * 2);",
    );
    line(out, "    for __ls_byte in __ls_value {");
    line(out, "        use std::fmt::Write as _;");
    line(
        out,
        "        let _ = write!(__ls_out, \"{__ls_byte:02x}\");",
    );
    line(out, "    }");
    line(out, "    __ls_out");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_uuid_text(__ls_value: &[u8; 16]) -> String {",
    );
    line(out, "    let __ls_hex = __ls_cli_hex(__ls_value);");
    line(out, "    format!(\"{}-{}-{}-{}-{}\" , &__ls_hex[0..8], &__ls_hex[8..12], &__ls_hex[12..16], &__ls_hex[16..20], &__ls_hex[20..32])");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_bytes(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<Vec<u8>, String> {");
    line(
        out,
        "    let __ls_value = __ls_cli_required(__ls_pairs, __ls_name)?;",
    );
    line(out, "    if __ls_value.len() % 2 != 0 { return Err(format!(\"argument `{__ls_name}` must contain an even number of hex digits\")); }");
    line(
        out,
        "    let mut __ls_out = Vec::with_capacity(__ls_value.len() / 2);",
    );
    line(
        out,
        "    for __ls_index in (0..__ls_value.len()).step_by(2) {",
    );
    line(out, "        __ls_out.push(u8::from_str_radix(&__ls_value[__ls_index..__ls_index + 2], 16).map_err(|_| format!(\"argument `{__ls_name}` is not valid hex\"))?);");
    line(out, "    }");
    line(out, "    Ok(__ls_out)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_uuid(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<[u8; 16], String> {");
    line(
        out,
        "    let __ls_value = __ls_cli_required(__ls_pairs, __ls_name)?;",
    );
    line(out, "    let __ls_hex: String = __ls_value.chars().filter(|character| *character != '-').collect();");
    line(out, "    if __ls_hex.len() != 32 { return Err(format!(\"argument `{__ls_name}` is not a UUID\")); }");
    line(out, "    let mut __ls_out = [0u8; 16];");
    line(out, "    for __ls_index in 0..16 {");
    line(out, "        __ls_out[__ls_index] = u8::from_str_radix(&__ls_hex[__ls_index * 2..__ls_index * 2 + 2], 16).map_err(|_| format!(\"argument `{__ls_name}` is not a UUID\"))?;");
    line(out, "    }");
    line(out, "    Ok(__ls_out)");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_bool(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<bool, String> {");
    line(
        out,
        "    let __ls_value = __ls_cli_required(__ls_pairs, __ls_name)?;",
    );
    line(out, "    match __ls_value {");
    line(out, "        \"true\" => Ok(true),");
    line(out, "        \"false\" => Ok(false),");
    line(
        out,
        "        _ => Err(format!(\"argument `{__ls_name}` is not a bool: `{__ls_value}`\")),",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_text(__ls_pairs: &[(String, String)], __ls_name: &str) -> Result<String, String> {");
    line(
        out,
        "    Ok(__ls_cli_required(__ls_pairs, __ls_name)?.to_string())",
    );
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_cli_escape(__ls_value: &str) -> String {");
    line(
        out,
        "    let mut __ls_out = String::with_capacity(__ls_value.len());",
    );
    line(out, "    for __ls_character in __ls_value.chars() {");
    line(out, "        match __ls_character {");
    line(
        out,
        "            '\\\\' => __ls_out.push_str(\"\\\\\\\\\"),",
    );
    line(out, "            '\\t' => __ls_out.push_str(\"\\\\t\"),");
    line(out, "            '\\n' => __ls_out.push_str(\"\\\\n\"),");
    line(out, "            '\\r' => __ls_out.push_str(\"\\\\r\"),");
    line(out, "            __ls_other => __ls_out.push(__ls_other),");
    line(out, "        }");
    line(out, "    }");
    line(out, "    __ls_out");
    line(out, "}");
    line(out, "");
}

pub(crate) fn rows_expression(workload: &Workload, database: &str) -> String {
    if workload.tables.is_empty() {
        return "0u64".to_string();
    }
    workload
        .tables
        .iter()
        .map(|table| format!("{database}.t_{}.len() as u64", table.name))
        .collect::<Vec<_>>()
        .join(" + ")
}
