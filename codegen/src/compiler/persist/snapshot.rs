use super::super::common::{
    default_expression, field_conversion, owned_argument, owned_extraction, owned_pattern,
    row_type, stored_field_name, stored_table_name, type_tag,
};
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_snapshot(out: &mut String, workload: &Workload) -> Result<(), String> {
    writeln!(
        out,
        "    fn __ls_write_snapshot<W: Write>(&self, writer: &mut W) -> io::Result<()> {{"
    )
    .unwrap();
    writeln!(out, "        let mut hash = 0xcbf29ce484222325u64;").unwrap();
    writeln!(out, "        let mut header = Vec::new();").unwrap();
    writeln!(out, "        header.extend_from_slice(&__LS_MAGIC);").unwrap();
    writeln!(out, "        __ls_put_u32(&mut header, __LS_VERSION);").unwrap();
    writeln!(out, "        __ls_put_u64(&mut header, __LS_SCHEMA);").unwrap();
    writeln!(out, "        __ls_put_u64(&mut header, self.__ls_lsn);").unwrap();
    writeln!(
        out,
        "        __ls_put_u32(&mut header, {}u32);",
        workload
            .tables
            .iter()
            .filter(|table| !workload.is_projection(&table.name))
            .count()
    )
    .unwrap();
    writeln!(out, "        __ls_emit(writer, &mut hash, &header)?;").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(out, "        let mut meta = Vec::new();").unwrap();
        writeln!(out, "        __ls_put_text(&mut meta, {:?});", table.name).unwrap();
        writeln!(
            out,
            "        __ls_put_u32(&mut meta, {}u32);",
            table.fields.len()
        )
        .unwrap();
        for field in &table.fields {
            writeln!(out, "        __ls_put_text(&mut meta, {:?});", field.name).unwrap();
            writeln!(
                out,
                "        __ls_put_u8(&mut meta, {}u8);",
                type_tag(field.ty)
            )
            .unwrap();
            writeln!(
                out,
                "        __ls_put_u8(&mut meta, {}u8);",
                (field.primary_key as u8) | ((field.nullable as u8) << 1)
            )
            .unwrap();
        }
        if table.ttl_secs.is_some() {
            writeln!(
                out,
                "        let mut __ls_live = 0u64;
        for _ in self.t_{0}.values() {{
            __ls_live += 1;
        }}
        __ls_put_u64(&mut meta, __ls_live);",
                table.name
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "        __ls_put_u64(&mut meta, self.t_{}.len() as u64);",
                table.name
            )
            .unwrap();
        }
        writeln!(out, "        __ls_emit(writer, &mut hash, &meta)?;").unwrap();
        writeln!(
            out,
            "        let mut chunk = Vec::with_capacity(__LS_SNAPSHOT_CHUNK);"
        )
        .unwrap();
        writeln!(out, "        let mut chunk_rows = 0u32;").unwrap();
        writeln!(out, "        for row in self.t_{}.values() {{", table.name).unwrap();
        for field in &table.fields {
            let call = match field.ty {
                Ty::I8 => format!("__ls_put_i8(&mut chunk, row.{})", field.name),
                Ty::I16 => format!("__ls_put_i16(&mut chunk, row.{})", field.name),
                Ty::I32 => format!("__ls_put_i32(&mut chunk, row.{})", field.name),
                Ty::I64 => format!("__ls_put_i64(&mut chunk, row.{})", field.name),
                Ty::U8 => format!("__ls_put_u8(&mut chunk, row.{})", field.name),
                Ty::U16 => format!("__ls_put_u16(&mut chunk, row.{})", field.name),
                Ty::U32 => format!("__ls_put_u32(&mut chunk, row.{})", field.name),
                Ty::U64 => format!("__ls_put_u64(&mut chunk, row.{})", field.name),
                Ty::F32 => format!("__ls_put_f32(&mut chunk, row.{})", field.name),
                Ty::F64 => format!("__ls_put_f64(&mut chunk, row.{})", field.name),
                Ty::Bool => format!("__ls_put_bool(&mut chunk, row.{})", field.name),
                Ty::Text => format!("__ls_put_text(&mut chunk, &row.{})", field.name),
                Ty::Bytes => format!("__ls_put_bytes(&mut chunk, &row.{})", field.name),
                Ty::Uuid => format!("__ls_put_uuid(&mut chunk, &row.{})", field.name),
            };
            if field.nullable {
                let present = call
                    .replace(&format!("&row.{}", field.name), "value")
                    .replace(&format!("row.{}", field.name), "(*value)");
                writeln!(out, "            match &row.{0} {{", field.name).unwrap();
                writeln!(
                    out,
                    "                ::std::option::Option::Some(value) => {{ __ls_put_u8(&mut chunk, 1); {present}; }},"
                )
                .unwrap();
                writeln!(
                    out,
                    "                ::std::option::Option::None => {{ __ls_put_u8(&mut chunk, 0); }},"
                )
                .unwrap();
                writeln!(out, "            }}").unwrap();
            } else {
                writeln!(out, "            {call};").unwrap();
            }
        }
        writeln!(out, "            chunk_rows += 1;").unwrap();
        writeln!(out, "            if chunk.len() >= __LS_SNAPSHOT_CHUNK {{").unwrap();
        writeln!(
            out,
            "                if chunk.len() > __LS_MAX_CHUNK {{ return Err(__ls_invalid(\"snapshot chunk is too large\")); }}"
        )
        .unwrap();
        writeln!(
            out,
            "                __ls_emit_chunk(writer, &mut hash, chunk_rows, &chunk)?;"
        )
        .unwrap();
        writeln!(out, "                chunk.clear();").unwrap();
        writeln!(out, "                chunk_rows = 0;").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        if chunk_rows > 0 {{").unwrap();
        writeln!(
            out,
            "            __ls_emit_chunk(writer, &mut hash, chunk_rows, &chunk)?;"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        writer.write_all(&hash.to_le_bytes())?;").unwrap();
    writeln!(out, "        Ok(())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}

pub(crate) fn emit_from_snapshot(out: &mut String, workload: &Workload) -> Result<(), String> {
    writeln!(
        out,
        "    fn __ls_read_snapshot<R: Read>(reader: R, migrate: bool) -> io::Result<(Db, u64)> {{"
    )
    .unwrap();
    writeln!(out, "        let mut input = __LsInput::new(reader);").unwrap();
    writeln!(
        out,
        "        if input.take(8)?[..] != __LS_MAGIC[..] {{ return Err(__ls_invalid(\"bad snapshot magic\")); }}"
    )
    .unwrap();
    writeln!(out, "        let __ls_version = input.u32()?;").unwrap();
    writeln!(
        out,
        "        if __ls_version != __LS_VERSION && __ls_version != __LS_PREVIOUS_VERSION && __ls_version != __LS_OLD_VERSION && __ls_version != __LS_ANCIENT_VERSION {{ return Err(__ls_invalid(\"unsupported snapshot version\")); }}"
    )
    .unwrap();
    writeln!(out, "        let stored_fingerprint = input.u64()?;").unwrap();
    writeln!(out, "        let lsn = input.u64()?;").unwrap();

    writeln!(
        out,
        "        if __ls_version < 7 && stored_fingerprint == __LS_WORKLOAD {{"
    )
    .unwrap();
    writeln!(
        out,
        "            if input.u32()? as usize != {} {{ return Err(__ls_invalid(\"snapshot table count mismatch\")); }}",
        workload.tables.iter().filter(|table| !workload.is_projection(&table.name)).count()
    )
    .unwrap();
    writeln!(out, "            let mut db = Db::__ls_new_spilled()?;").unwrap();
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        writeln!(
            out,
            "            if input.text()? != {:?} {{ return Err(__ls_invalid(\"snapshot table mismatch\")); }}",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "            if input.u32()? as usize != {} {{ return Err(__ls_invalid(\"snapshot column count mismatch\")); }}",
            table.fields.len()
        )
        .unwrap();
        for field in &table.fields {
            writeln!(
                out,
                "            if input.text()? != {:?} {{ return Err(__ls_invalid(\"snapshot column mismatch\")); }}",
                field.name
            )
            .unwrap();
            writeln!(
                out,
                "            if input.u8()? != {}u8 {{ return Err(__ls_invalid(\"snapshot column type mismatch\")); }}",
                type_tag(field.ty)
            )
            .unwrap();
            writeln!(
                out,
                "            if input.u8()? != {}u8 {{ return Err(__ls_invalid(\"snapshot column flags mismatch\")); }}",
                field.primary_key as u8
            )
            .unwrap();
        }
        writeln!(out, "            let rows = input.u64()?;").unwrap();
        writeln!(out, "            let mut parsed = 0u64;").unwrap();
        writeln!(out, "            while parsed < rows {{").unwrap();
        writeln!(
            out,
            "                let (payload, chunk_rows) = __ls_read_chunk(&mut input, rows, &mut parsed, __ls_version)?;"
        )
        .unwrap();
        writeln!(
            out,
            "                let mut chunk = __LsReader::new(&payload);"
        )
        .unwrap();
        writeln!(out, "                for _ in 0..chunk_rows {{").unwrap();
        writeln!(
            out,
            "                    let row = {} {{",
            row_type(&table.name)
        )
        .unwrap();
        for field in &table.fields {
            let value = match field.ty {
                Ty::I8 => "chunk.i8()?",
                Ty::I16 => "chunk.i16()?",
                Ty::I32 => "chunk.i32()?",
                Ty::I64 => "chunk.i64()?",
                Ty::U8 => "chunk.u8()?",
                Ty::U16 => "chunk.u16()?",
                Ty::U32 => "chunk.u32()?",
                Ty::U64 => "chunk.u64()?",
                Ty::F32 => "chunk.f32()?",
                Ty::F64 => "chunk.f64()?",
                Ty::Bool => "chunk.bool()?",
                Ty::Text => "chunk.text()?",
                Ty::Bytes => "chunk.bytes()?",
                Ty::Uuid => "chunk.uuid()?",
            };
            let value = if field.nullable {
                format!("{{ let value = {value}; ::std::option::Option::Some(value) }}")
            } else {
                value.to_string()
            };
            writeln!(out, "                        {}: {},", field.name, value).unwrap();
        }
        writeln!(out, "                    }};").unwrap();
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        writeln!(
            out,
            "                    let key = row.{}.clone();",
            primary_key.name
        )
        .unwrap();
        writeln!(
            out,
            "                    db.__ls_insert_{}(&row);",
            table.name
        )
        .unwrap();
        writeln!(
            out,
            "                    db.t_{}.__ls_append_loaded(key, row)?;",
            table.name
        )
        .unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(
            out,
            "                if !chunk.is_empty() {{ return Err(__ls_invalid(\"snapshot chunk has trailing bytes\")); }}"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
    }
    writeln!(out, "            input.finish()?;").unwrap();
    writeln!(out, "            return Ok((db, lsn));").unwrap();
    writeln!(out, "        }}").unwrap();

    for line in [
        "        let compatible = if __ls_version >= 7 { stored_fingerprint == __LS_SCHEMA } else { stored_fingerprint == __LS_WORKLOAD };",
        "        if !compatible && !migrate {",
        "            return Err(__ls_invalid(\"snapshot schema mismatch; reopen with open_migrated to upgrade\"));",
        "        }",
        "        let mut raw_tables: Vec<(String, __LsRawTable)> = Vec::new();",
        "        let raw_count = input.u32()? as usize;",
        "        for _ in 0..raw_count {",
        "            let name = input.text()?;",
        "            let column_count = input.u32()? as usize;",
        "            let mut columns = Vec::with_capacity(column_count);",
        "            for _ in 0..column_count {",
        "                let column_name = input.text()?;",
        "                let tag = input.u8()?;",
        "                let flags = input.u8()?;",
        "                if tag > 13 { return Err(__ls_invalid(\"snapshot column has unknown type\")); }",
        "                columns.push((column_name, tag, (flags & 1) == 1, (flags & 2) == 2));",
        "            }",
        "            let row_count = input.u64()?;",
        "            let mut rows = Vec::new();",
        "            let mut parsed = 0u64;",
        "            while parsed < row_count {",
        "                let (payload, chunk_rows) = __ls_read_chunk(&mut input, row_count, &mut parsed, __ls_version)?;",
        "                let mut chunk = __LsReader::new(&payload);",
        "                for _ in 0..chunk_rows {",
        "                    let mut row = Vec::with_capacity(column_count);",
        "                    for (_, tag, _, nullable) in &columns {",
        "                        let present = if *nullable { chunk.u8()? != 0 } else { true };",
        "                        row.push(if present {",
        "                            match *tag {",
        "                            0 => __LsOwnedValue::I64(chunk.i64()?),",
        "                            1 => __LsOwnedValue::Bool(chunk.bool()?),",
        "                            2 => __LsOwnedValue::Text(chunk.text()?),",
        "                            3 => __LsOwnedValue::I8(chunk.i8()?),",
        "                            4 => __LsOwnedValue::I16(chunk.i16()?),",
        "                            5 => __LsOwnedValue::I32(chunk.i32()?),",
        "                            6 => __LsOwnedValue::U8(chunk.u8()?),",
        "                            7 => __LsOwnedValue::U16(chunk.u16()?),",
        "                            8 => __LsOwnedValue::U32(chunk.u32()?),",
        "                            9 => __LsOwnedValue::U64(chunk.u64()?),",
        "                            10 => __LsOwnedValue::Bytes(chunk.bytes()?),",
        "                            11 => __LsOwnedValue::Uuid(chunk.uuid()?),",
        "                            12 => __LsOwnedValue::F32(chunk.f32()?),",
        "                            13 => __LsOwnedValue::F64(chunk.f64()?),",
        "                            _ => return Err(__ls_invalid(\"snapshot column has unknown type\")),",
        "                            }",
        "                        } else {",
        "                            __LsOwnedValue::Null(*tag)",
        "                        });",
        "                    }",
        "                    rows.push(row);",
        "                }",
        "                if !chunk.is_empty() { return Err(__ls_invalid(\"snapshot chunk has trailing bytes\")); }",
        "            }",
        "            raw_tables.push((name, __LsRawTable { columns, rows }));",
        "        }",
        "        input.finish()?;",
        "        let mut db = Db::__ls_new_spilled()?;",
    ] {
        writeln!(out, "{line}").unwrap();
    }
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        writeln!(out, "        {{").unwrap();
        writeln!(
            out,
            "            let empty = __LsRawTable {{ columns: Vec::new(), rows: Vec::new() }};"
        )
        .unwrap();
        let stored_table = stored_table_name(&table.name);
        writeln!(
            out,
            "            let stored_table = if compatible {{ {:?} }} else {{ {:?} }};",
            table.name, stored_table
        )
        .unwrap();
        writeln!(
            out,
            "            let raw = raw_tables.iter().find(|(name, _)| name.as_str() == stored_table).map(|(_, table)| table).unwrap_or(&empty);"
        )
        .unwrap();
        for (field_index, field) in table.fields.iter().enumerate() {
            let stored_field = stored_field_name(&table.name, &field.name);
            let conversion =
                field_conversion(&table.name, &field.name).filter(|(old, _)| *old != field.ty);
            writeln!(
                out,
                "            let stored_field_{field_index} = if compatible {{ {:?} }} else {{ {:?} }};",
                field.name, stored_field
            )
            .unwrap();
            writeln!(
                out,
                "            let map_{field_index} = match raw.columns.iter().position(|(column, _, _, _)| column.as_str() == stored_field_{field_index}) {{"
            )
            .unwrap();
            writeln!(out, "                Some(index) => {{").unwrap();
            if let Some((old, _)) = &conversion {
                writeln!(
                    out,
                    "                    if raw.columns[index].1 != {}u8 && raw.columns[index].1 != {}u8 {{ return Err(__ls_invalid(\"cannot migrate column `{}.{}`: stored type does not match\")); }}",
                    type_tag(field.ty),
                    type_tag(*old),
                    table.name,
                    field.name
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "                    if raw.columns[index].1 != {}u8 {{ return Err(__ls_invalid(\"cannot migrate column `{}.{}`: stored type does not match\")); }}",
                    type_tag(field.ty),
                    table.name,
                    field.name
                )
                .unwrap();
            }
            writeln!(out, "                    Some(index)").unwrap();
            writeln!(out, "                }}").unwrap();
            writeln!(out, "                None => None,").unwrap();
            writeln!(out, "            }};").unwrap();
        }
        writeln!(out, "            for source in &raw.rows {{").unwrap();
        writeln!(
            out,
            "                let row = {} {{",
            row_type(&table.name)
        )
        .unwrap();
        for (field_index, field) in table.fields.iter().enumerate() {
            let conversion =
                field_conversion(&table.name, &field.name).filter(|(old, _)| *old != field.ty);
            writeln!(
                out,
                "                    {}: match map_{field_index} {{",
                field.name
            )
            .unwrap();
            writeln!(
                out,
                "                        Some(index) => match &source[index] {{"
            )
            .unwrap();
            if let Some((old, call)) = &conversion {
                let old_pattern = owned_pattern(*old);
                let old_argument = owned_argument(*old);
                writeln!(
                    out,
                    "                            __LsOwnedValue::{old_pattern} => match {call}({old_argument}) {{"
                )
                .unwrap();
                writeln!(out, "                                Ok(value) => value,").unwrap();
                writeln!(out, "                                Err(error) => return Err(__ls_invalid(&error.to_string())),").unwrap();
                writeln!(out, "                            }},").unwrap();
            }
            if field.nullable {
                writeln!(
                    out,
                    "                            __LsOwnedValue::Null(_) => None,"
                )
                .unwrap();
            }
            let extraction = owned_extraction(field.ty);
            let extraction = if field.nullable {
                let mut parts = extraction.splitn(2, " => ");
                format!(
                    "{} => Some({})",
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or("")
                )
            } else {
                extraction.to_string()
            };
            writeln!(
                out,
                "                            __LsOwnedValue::{extraction},"
            )
            .unwrap();
            writeln!(
                out,
                "                            _ => return Err(__ls_invalid(\"cannot migrate column `{}.{}`: stored type does not match\")),",
                table.name, field.name
            )
            .unwrap();
            writeln!(out, "                        }},").unwrap();
            if field.name == primary_key.name {
                writeln!(
                    out,
                    "                        None => return Err(__ls_invalid(\"cannot migrate table `{}`: primary key column `{}` is missing\")),",
                    table.name, field.name
                )
                .unwrap();
            } else if let Some(default) = &field.default {
                writeln!(
                    out,
                    "                        None => {},",
                    default_expression(default)
                )
                .unwrap();
            } else if field.nullable {
                writeln!(out, "                        None => None,").unwrap();
            } else {
                writeln!(
                    out,
                    "                        None => return Err(__ls_invalid(\"cannot migrate table `{}`: column `{}` is new and has no default\")),",
                    table.name, field.name
                )
                .unwrap();
            }
            writeln!(out, "                    }},").unwrap();
        }
        writeln!(out, "                }};").unwrap();
        writeln!(
            out,
            "                let key = row.{}.clone();",
            primary_key.name
        )
        .unwrap();
        writeln!(out, "                db.__ls_insert_{}(&row);", table.name).unwrap();
        writeln!(
            out,
            "        db.t_{}.__ls_append_loaded(key, row)?;",
            table.name
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        Ok((db, lsn))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}
