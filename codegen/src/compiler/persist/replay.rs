use super::super::common::{default_expression, row_type, type_tag};
use super::super::plan::Plan;
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_replay(out: &mut String, workload: &Workload) -> Result<(), String> {
    writeln!(
        out,
        "    fn __ls_replay_op(&mut self, __ls_reader: &mut __LsReader<'_>) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        let operation = __ls_reader.u32()?;").unwrap();
    writeln!(
        out,
        "        let parameter_count = __ls_reader.u32()? as usize;"
    )
    .unwrap();
    writeln!(out, "        match operation {{").unwrap();
    for (operation_index, operation) in workload.operations.iter().enumerate() {
        if matches!(
            operation.body,
            Body::Insert { .. } | Body::Update { .. } | Body::Delete { .. }
        ) {
            writeln!(out, "            {operation_index}u32 => {{").unwrap();
            writeln!(
                out,
                "                if parameter_count != {} {{ return Err(__ls_invalid(\"WAL parameter count mismatch\")); }}",
                operation.params.len()
            )
            .unwrap();
            for (param_index, param) in operation.params.iter().enumerate() {
                let (tag, value) = match param.ty {
                    Ty::I8 => (3, "__ls_reader.i8()?"),
                    Ty::I16 => (4, "__ls_reader.i16()?"),
                    Ty::I32 => (5, "__ls_reader.i32()?"),
                    Ty::I64 => (0, "__ls_reader.i64()?"),
                    Ty::U8 => (6, "__ls_reader.u8()?"),
                    Ty::U16 => (7, "__ls_reader.u16()?"),
                    Ty::U32 => (8, "__ls_reader.u32()?"),
                    Ty::U64 => (9, "__ls_reader.u64()?"),
                    Ty::F32 => (12, "__ls_reader.f32()?"),
                    Ty::F64 => (13, "__ls_reader.f64()?"),
                    Ty::Bool => (1, "__ls_reader.bool()?"),
                    Ty::Text => (2, "__ls_reader.text()?"),
                    Ty::Bytes => (10, "__ls_reader.bytes()?"),
                    Ty::Uuid => (11, "__ls_reader.uuid()?"),
                };
                if param.nullable {
                    writeln!(
                        out,
                        "                let __ls_tag_{param_index} = __ls_reader.u8()?;"
                    )
                    .unwrap();
                    writeln!(
                        out,
                        "                let p{param_index} = if __ls_tag_{param_index} == 14 {{"
                    )
                    .unwrap();
                    writeln!(
                        out,
                        "                    if __ls_reader.u8()? != {tag} {{ return Err(__ls_invalid(\"WAL type mismatch\")); }}"
                    )
                    .unwrap();
                    writeln!(out, "                    None").unwrap();
                    writeln!(out, "                }} else {{").unwrap();
                    writeln!(
                        out,
                        "                    if __ls_tag_{param_index} != {tag} {{ return Err(__ls_invalid(\"WAL type mismatch\")); }}"
                    )
                    .unwrap();
                    writeln!(out, "                    Some({value})").unwrap();
                    writeln!(out, "                }};").unwrap();
                } else {
                    writeln!(
                        out,
                        "                if __ls_reader.u8()? != {tag} {{ return Err(__ls_invalid(\"WAL type mismatch\")); }}"
                    )
                    .unwrap();
                    writeln!(out, "                let p{param_index} = {value};").unwrap();
                }
            }
            let args = (0..operation.params.len())
                .map(|index| format!("p{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "                self.{}({args})?;", operation.name).unwrap();
            writeln!(out, "            }}").unwrap();
        }
    }
    writeln!(
        out,
        "            _ => return Err(__ls_invalid(\"unknown WAL operation\")),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Ok(())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}

pub(crate) fn emit_replay_effects(out: &mut String, workload: &Workload) -> Result<(), String> {
    writeln!(
        out,
        "    fn __ls_replay_effect(&mut self, __ls_reader: &mut __LsReader<'_>) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        let kind = __ls_reader.u8()?;").unwrap();
    writeln!(out, "        let table = __ls_reader.text()?;").unwrap();
    writeln!(out, "        match kind {{").unwrap();
    writeln!(out, "            0 => {{").unwrap();
    writeln!(
        out,
        "                let count = __ls_reader.u32()? as usize;"
    )
    .unwrap();
    writeln!(
        out,
        "                let mut columns: Vec<(String, __LsOwnedValue)> = Vec::with_capacity(count);"
    )
    .unwrap();
    writeln!(out, "                for _ in 0..count {{").unwrap();
    writeln!(out, "                    let name = __ls_reader.text()?;").unwrap();
    writeln!(
        out,
        "                    let value = __ls_reader.owned_value()?;"
    )
    .unwrap();
    writeln!(out, "                    columns.push((name, value));").unwrap();
    writeln!(out, "                }}").unwrap();
    writeln!(
        out,
        "                self.__ls_replay_upsert(&table, &columns)"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "            1 => {{").unwrap();
    writeln!(
        out,
        "                let value = __ls_reader.owned_value()?;"
    )
    .unwrap();
    writeln!(
        out,
        "                self.__ls_replay_delete(&table, value)"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(
        out,
        "            _ => Err(__ls_invalid(\"unknown WAL effect kind\")),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    fn __ls_replay_upsert(&mut self, table: &str, columns: &[(String, __LsOwnedValue)]) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        match table {{").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(
            out,
            "            {:?} => self.__ls_replay_upsert_{}(columns),",
            table.name, table.name
        )
        .unwrap();
    }
    writeln!(
        out,
        "            _ => Err(__ls_invalid(\"unknown table in WAL effect\")),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    fn __ls_replay_delete(&mut self, table: &str, value: __LsOwnedValue) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        match table {{").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(
            out,
            "            {:?} => self.__ls_replay_delete_{}(value),",
            table.name, table.name
        )
        .unwrap();
    }
    writeln!(
        out,
        "            _ => Err(__ls_invalid(\"unknown table in WAL effect\")),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        writeln!(
            out,
            "    fn __ls_replay_upsert_{}(&mut self, columns: &[(String, __LsOwnedValue)]) -> io::Result<()> {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "        let find = |name: &str| columns.iter().find(|(column, _)| column == name).map(|(_, value)| value);"
        )
        .unwrap();
        writeln!(out, "        let row = {} {{", row_type(&table.name)).unwrap();
        for field in &table.fields {
            let pattern = match field.ty {
                Ty::I8 => "__LsOwnedValue::I8(value)",
                Ty::I16 => "__LsOwnedValue::I16(value)",
                Ty::I32 => "__LsOwnedValue::I32(value)",
                Ty::I64 => "__LsOwnedValue::I64(value)",
                Ty::U8 => "__LsOwnedValue::U8(value)",
                Ty::U16 => "__LsOwnedValue::U16(value)",
                Ty::U32 => "__LsOwnedValue::U32(value)",
                Ty::U64 => "__LsOwnedValue::U64(value)",
                Ty::F32 => "__LsOwnedValue::F32(value)",
                Ty::F64 => "__LsOwnedValue::F64(value)",
                Ty::Bool => "__LsOwnedValue::Bool(value)",
                Ty::Text => "__LsOwnedValue::Text(value)",
                Ty::Bytes => "__LsOwnedValue::Bytes(value)",
                Ty::Uuid => "__LsOwnedValue::Uuid(value)",
            };
            let assignment = match field.ty {
                Ty::Text | Ty::Bytes => "value.clone()".to_string(),
                _ => "*value".to_string(),
            };
            let (pattern, assignment) = if field.nullable {
                (
                    format!("Some(__LsOwnedValue::Null(_)) => None, Some({pattern})"),
                    format!("Some({assignment})"),
                )
            } else {
                (format!("Some({pattern})"), assignment)
            };
            writeln!(
                out,
                "            {}: match find({:?}) {{",
                field.name, field.name
            )
            .unwrap();
            writeln!(out, "                {pattern} => {assignment},").unwrap();
            writeln!(
                out,
                "                Some(_) => return Err(__ls_invalid(\"WAL effect type mismatch for `{}.{}`\")),",
                table.name, field.name
            )
            .unwrap();
            if field.name == primary_key.name {
                writeln!(
                    out,
                    "                None => return Err(__ls_invalid(\"WAL effect is missing `{}.{}`\")),",
                    table.name, field.name
                )
                .unwrap();
            } else if let Some(default) = &field.default {
                writeln!(
                    out,
                    "                None => {},",
                    default_expression(default)
                )
                .unwrap();
            } else if field.nullable {
                writeln!(out, "                None => None,").unwrap();
            } else {
                writeln!(
                    out,
                    "                None => return Err(__ls_invalid(\"WAL effect is missing `{}.{}`\")),",
                    table.name, field.name
                )
                .unwrap();
            }
            writeln!(out, "            }},").unwrap();
        }
        writeln!(out, "        }};").unwrap();
        writeln!(out, "        let key = row.{}.clone();", primary_key.name).unwrap();
        writeln!(
            out,
            "        if let Some(__ls_old) = self.t_{}.remove(&key) {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "            self.__ls_remove_{}(&__ls_old);",
            table.name
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        self.__ls_insert_{}(&row);", table.name).unwrap();
        writeln!(out, "        self.t_{}.__ls_insert(key, row);", table.name).unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        let (pattern, assignment) = match primary_key.ty {
            Ty::I8 => ("__LsOwnedValue::I8(value)", "value"),
            Ty::I16 => ("__LsOwnedValue::I16(value)", "value"),
            Ty::I32 => ("__LsOwnedValue::I32(value)", "value"),
            Ty::I64 => ("__LsOwnedValue::I64(value)", "value"),
            Ty::U8 => ("__LsOwnedValue::U8(value)", "value"),
            Ty::U16 => ("__LsOwnedValue::U16(value)", "value"),
            Ty::U32 => ("__LsOwnedValue::U32(value)", "value"),
            Ty::U64 => ("__LsOwnedValue::U64(value)", "value"),
            Ty::F32 => ("__LsOwnedValue::F32(value)", "value"),
            Ty::F64 => ("__LsOwnedValue::F64(value)", "value"),
            Ty::Bool => ("__LsOwnedValue::Bool(value)", "value"),
            Ty::Text => ("__LsOwnedValue::Text(value)", "value.clone()"),
            Ty::Bytes => ("__LsOwnedValue::Bytes(value)", "value.clone()"),
            Ty::Uuid => ("__LsOwnedValue::Uuid(value)", "value"),
        };
        writeln!(
            out,
            "    fn __ls_replay_delete_{}(&mut self, value: __LsOwnedValue) -> io::Result<()> {{",
            table.name
        )
        .unwrap();
        writeln!(out, "        let key = match value {{").unwrap();
        writeln!(out, "            {pattern} => {assignment},").unwrap();
        writeln!(
            out,
            "            _ => return Err(__ls_invalid(\"WAL effect key type mismatch for `{}`\")),",
            table.name
        )
        .unwrap();
        writeln!(out, "        }};").unwrap();
        writeln!(
            out,
            "        if let Some(__ls_old) = self.t_{}.remove(&key) {{",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "            self.__ls_remove_{}(&__ls_old);",
            table.name
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }
    Ok(())
}

pub(crate) fn emit_adopt(out: &mut String, workload: &Workload, plan: &Plan) {
    writeln!(out, "    fn __ls_adopt(&mut self, __ls_other: Db) {{").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(
            out,
            "        self.t_{} = __ls_other.t_{};",
            table.name, table.name
        )
        .unwrap();
    }
    for (index, _) in plan.indexes.iter().enumerate() {
        writeln!(out, "        self.idx{index} = __ls_other.idx{index};").unwrap();
    }
    for (index, _) in plan.counters.iter().enumerate() {
        writeln!(out, "        self.cnt{index} = __ls_other.cnt{index};").unwrap();
    }
    writeln!(
        out,
        "        self.__ls_versions = __ls_other.__ls_versions;"
    )
    .unwrap();
    writeln!(out, "        self.__ls_lsn = __ls_other.__ls_lsn;").unwrap();
    writeln!(out, "        self.__ls_effects.clear();").unwrap();
    writeln!(out, "        self.__ls_in_transaction = false;").unwrap();
    writeln!(out, "        self.__ls_memory_backup = None;").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

pub(crate) fn effect_upsert_call(table: &Table, row: &str) -> String {
    let columns = table
        .fields
        .iter()
        .map(|field| {
            let value = match field.ty {
                Ty::I8 => format!("__LsValue::I8({row}.{})", field.name),
                Ty::I16 => format!("__LsValue::I16({row}.{})", field.name),
                Ty::I32 => format!("__LsValue::I32({row}.{})", field.name),
                Ty::I64 => format!("__LsValue::I64({row}.{})", field.name),
                Ty::U8 => format!("__LsValue::U8({row}.{})", field.name),
                Ty::U16 => format!("__LsValue::U16({row}.{})", field.name),
                Ty::U32 => format!("__LsValue::U32({row}.{})", field.name),
                Ty::U64 => format!("__LsValue::U64({row}.{})", field.name),
                Ty::F32 => format!("__LsValue::F32({row}.{})", field.name),
                Ty::F64 => format!("__LsValue::F64({row}.{})", field.name),
                Ty::Bool => format!("__LsValue::Bool({row}.{})", field.name),
                Ty::Text => format!("__LsValue::Text({row}.{}.as_str())", field.name),
                Ty::Bytes => format!("__LsValue::Bytes(&{row}.{})", field.name),
                Ty::Uuid => format!("__LsValue::Uuid(&{row}.{})", field.name),
            };
            if field.nullable {
                let present = value
                    .replace(&format!("&{row}.{}", field.name), "value")
                    .replace(&format!("{row}.{}.as_str()", field.name), "value.as_str()")
                    .replace(&format!("{row}.{}", field.name), "(*value)");
                let value = format!(
                    "match &{row}.{field} {{ ::std::option::Option::Some(value) => {present}, ::std::option::Option::None => __LsValue::Null({tag}) }}",
                    row = row,
                    field = field.name,
                    tag = type_tag(field.ty),
                );
                format!("({:?}, {value})", field.name)
            } else {
                format!("({:?}, {value})", field.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("self.__ls_effect_upsert({:?}, &[{columns}]);", table.name)
}

pub(crate) fn effect_delete_call(table: &Table, expression: &str) -> String {
    let primary_key = table
        .primary_key()
        .expect("validated tables have a primary key");
    let value = match primary_key.ty {
        Ty::I8 => format!("__LsValue::I8({expression})"),
        Ty::I16 => format!("__LsValue::I16({expression})"),
        Ty::I32 => format!("__LsValue::I32({expression})"),
        Ty::I64 => format!("__LsValue::I64({expression})"),
        Ty::U8 => format!("__LsValue::U8({expression})"),
        Ty::U16 => format!("__LsValue::U16({expression})"),
        Ty::U32 => format!("__LsValue::U32({expression})"),
        Ty::U64 => format!("__LsValue::U64({expression})"),
        Ty::F32 => format!("__LsValue::F32({expression})"),
        Ty::F64 => format!("__LsValue::F64({expression})"),
        Ty::Bool => format!("__LsValue::Bool({expression})"),
        Ty::Text => format!("__LsValue::Text({expression}.as_str())"),
        Ty::Bytes => format!("__LsValue::Bytes({expression}.as_slice())"),
        Ty::Uuid => format!("__LsValue::Uuid(&{expression})"),
    };
    format!("self.__ls_effect_delete({:?}, {value});", table.name)
}
