use super::super::explain::describe_operation;
use super::super::plan::{build_plan, is_advanced_query};
use super::line;
use crate::ir::{Body, Operation, Returns, Workload};

pub(crate) fn explain_what(workload: &Workload, operation: &Operation) -> Result<String, String> {
    match &operation.body {
        Body::Insert { table } => Ok(format!("insert into {table}")),
        Body::Update { table, .. } => Ok(format!("update of {table}")),
        Body::Delete { table, .. } => Ok(format!("delete from {table}")),
        Body::Query {
            table,
            count: false,
            ..
        } => {
            if is_advanced_query(&operation.body) {
                let returned = match &operation.returns {
                    Returns::Many(returned) => returned,
                    _ => {
                        return Err(format!(
                            "operation `{}` must return `many <table>`",
                            operation.name
                        ));
                    }
                };
                let _ = workload
                    .table(returned)
                    .ok_or_else(|| format!("unknown table `{returned}`"))?;
                Ok(format!("advanced query returning many {returned}"))
            } else {
                let _ = workload
                    .table(table)
                    .ok_or_else(|| format!("unknown table `{table}`"))?;
                Ok(format!("query returning many {table}"))
            }
        }
        Body::Query {
            table, count: true, ..
        } => Ok(format!("count query on {table}")),
    }
}

pub(crate) fn emit_explain(out: &mut String, workload: &Workload) -> Result<(), String> {
    let plan = build_plan(workload)?;
    let mut rendered: Vec<(String, Vec<String>)> = Vec::new();
    for operation in &workload.operations {
        let described = super::explain::describe_operation(workload, &plan, operation)?;
        let what = explain_what(workload, operation)?;
        rendered.push((
            operation.name.clone(),
            described.lines(&operation.name, &what),
        ));
    }
    line(
        out,
        "fn __ls_cli_explain(__ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    match __ls_args {");
    line(out, "        [] => {");
    for (_, texts) in &rendered {
        for text in texts {
            line(out, &format!("            println!({text:?});"));
        }
    }
    line(out, "            Ok(())");
    line(out, "        }");
    line(out, "        [__ls_name] => {");
    let mut first = true;
    for (name, texts) in &rendered {
        line(
            out,
            &format!(
                "            {} __ls_name == {name:?} {{",
                if first { "if" } else { "} else if" }
            ),
        );
        first = false;
        for text in texts {
            line(out, &format!("                println!({text:?});"));
        }
    }
    if rendered.is_empty() {
        line(
            out,
            "            return Err(\"no operations to explain\".to_string());",
        );
    } else {
        line(out, "            } else {");
        line(
            out,
            "                return Err(format!(\"unknown operation `{__ls_name}` (available: {})\", __LS_CLI_OPERATIONS));",
        );
        line(out, "            }");
    }
    line(out, "            Ok(())");
    line(out, "        }");
    line(
        out,
        "        _ => Err(\"explain expects at most one operation name\".to_string()),",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
    Ok(())
}
