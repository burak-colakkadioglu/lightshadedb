use super::super::common::{row_type, type_tag};
use super::line;
use crate::ir::{Ty, Workload};

pub(crate) fn emit_data_plane(out: &mut String, workload: &Workload) -> Result<(), String> {
    emit_csv_helpers(out);
    emit_export(out, workload)?;
    emit_import(out, workload)?;
    emit_dump(out, workload)?;
    emit_restore(out, workload)?;
    Ok(())
}

pub(crate) fn emit_csv_helpers(out: &mut String) {
    line(out, "#[allow(dead_code)]");
    line(out, "fn __ls_csv_escape_cell(__ls_cell: &str) -> String {");
    line(out, "    if __ls_cell.bytes().any(|__ls_b| __ls_b == b',' || __ls_b == b'\"' || __ls_b == b'\\n' || __ls_b == b'\\r') {");
    line(
        out,
        "        let mut __ls_out = String::with_capacity(__ls_cell.len() + 2);",
    );
    line(out, "        __ls_out.push('\"');");
    line(
        out,
        "        __ls_out.push_str(&__ls_cell.replace('\"', \"\\\"\\\"\"));",
    );
    line(out, "        __ls_out.push('\"');");
    line(out, "        __ls_out");
    line(out, "    } else {");
    line(out, "        __ls_cell.to_string()");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_decode_field(__ls_bytes: Vec<u8>, __ls_row: usize) -> Result<String, String> {",
    );
    line(
        out,
        "    String::from_utf8(__ls_bytes).map_err(|_| format!(\"row {__ls_row}: field is not valid UTF-8\"))",
    );
    line(out, "}");
    line(out, "");
    line(out, "struct __LsCsvReader<R: std::io::Read> {");
    line(
        out,
        "    __ls_bytes: std::iter::Peekable<std::io::Bytes<std::io::BufReader<R>>>,",
    );
    line(out, "    __ls_row: usize,");
    line(out, "    __ls_exhausted: bool,");
    line(out, "}");
    line(out, "");
    line(out, "impl<R: std::io::Read> __LsCsvReader<R> {");
    line(out, "    fn __ls_new(__ls_inner: R) -> Self {");
    line(
        out,
        "        Self { __ls_bytes: std::io::BufReader::new(__ls_inner).bytes().peekable(), __ls_row: 1, __ls_exhausted: false }",
    );
    line(out, "    }");
    line(
        out,
        "    fn __ls_next(&mut self) -> Result<Option<Vec<String>>, String> {",
    );
    line(out, "        if self.__ls_exhausted {");
    line(out, "            return Ok(None);");
    line(out, "        }");
    line(
        out,
        "        let mut __ls_fields: Vec<String> = Vec::new();",
    );
    line(out, "        let mut __ls_field: Vec<u8> = Vec::new();");
    line(out, "        let mut __ls_in_quotes = false;");
    line(out, "        let mut __ls_saw_data = false;");
    line(out, "        loop {");
    line(
        out,
        "            let __ls_byte = match self.__ls_bytes.next() {",
    );
    line(
        out,
        "                Some(Ok(__ls_byte)) => Some(__ls_byte),",
    );
    line(
        out,
        "                Some(Err(__ls_error)) => return Err(format!(\"row {}: cannot read CSV: {__ls_error}\", self.__ls_row)),",
    );
    line(out, "                None => None,");
    line(out, "            };");
    line(out, "            match __ls_byte {");
    line(out, "                None => {");
    line(out, "                    if __ls_in_quotes {");
    line(
        out,
        "                        return Err(format!(\"row {}: unterminated quoted field\", self.__ls_row));",
    );
    line(out, "                    }");
    line(out, "                    if !__ls_saw_data && __ls_field.is_empty() && __ls_fields.is_empty() {");
    line(out, "                        self.__ls_exhausted = true;");
    line(out, "                        return Ok(None);");
    line(out, "                    }");
    line(
        out,
        "                    __ls_fields.push(__ls_decode_field(__ls_field, self.__ls_row)?);",
    );
    line(out, "                    self.__ls_row += 1;");
    line(out, "                    return Ok(Some(__ls_fields));");
    line(out, "                }");
    line(out, "                Some(b'\"') => {");
    line(out, "                    __ls_saw_data = true;");
    line(out, "                    if __ls_in_quotes {");
    line(
        out,
        "                        match self.__ls_bytes.peek() {",
    );
    line(
        out,
        "                            Some(Ok(__ls_next)) if *__ls_next == b'\"' => {",
    );
    line(
        out,
        "                                self.__ls_bytes.next();",
    );
    line(
        out,
        "                                __ls_field.push(b'\"');",
    );
    line(out, "                            }");
    line(
        out,
        "                            _ => { __ls_in_quotes = false; }",
    );
    line(out, "                        }");
    line(out, "                    } else if __ls_field.is_empty() {");
    line(out, "                        __ls_in_quotes = true;");
    line(out, "                    } else {");
    line(
        out,
        "                        return Err(format!(\"row {}: stray quote in unquoted field\", self.__ls_row));",
    );
    line(out, "                    }");
    line(out, "                }");
    line(out, "                Some(b',') if !__ls_in_quotes => {");
    line(out, "                    __ls_saw_data = true;");
    line(
        out,
        "                    __ls_fields.push(__ls_decode_field(std::mem::take(&mut __ls_field), self.__ls_row)?);",
    );
    line(out, "                }");
    line(out, "                Some(b'\\n') if !__ls_in_quotes => {");
    line(
        out,
        "                    __ls_fields.push(__ls_decode_field(std::mem::take(&mut __ls_field), self.__ls_row)?);",
    );
    line(out, "                    self.__ls_row += 1;");
    line(out, "                    return Ok(Some(__ls_fields));");
    line(out, "                }");
    line(out, "                Some(b'\\r') if !__ls_in_quotes => {");
    line(out, "                    match self.__ls_bytes.peek() {");
    line(out, "                        Some(Ok(__ls_next)) if *__ls_next == b'\\n' => { self.__ls_bytes.next(); }");
    line(out, "                        _ => {}");
    line(out, "                    }");
    line(
        out,
        "                    __ls_fields.push(__ls_decode_field(std::mem::take(&mut __ls_field), self.__ls_row)?);",
    );
    line(out, "                    self.__ls_row += 1;");
    line(out, "                    return Ok(Some(__ls_fields));");
    line(out, "                }");
    line(out, "                Some(__ls_byte) => {");
    line(out, "                    __ls_saw_data = true;");
    line(out, "                    __ls_field.push(__ls_byte);");
    line(out, "                }");
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    line(out, "#[allow(dead_code)]");
    line(
        out,
        "fn __ls_csv_unhex(__ls_text: &str, __ls_table: &str, __ls_column: &str, __ls_row: usize) -> Result<Vec<u8>, String> {",
    );
    line(
        out,
        "    let __ls_err = || format!(\"row {__ls_row}: column `{__ls_table}.{__ls_column}` is not valid hex: `{__ls_text}`\");",
    );
    line(
        out,
        "    if __ls_text.len() % 2 != 0 { return Err(__ls_err()); }",
    );
    line(
        out,
        "    let mut __ls_out = Vec::with_capacity(__ls_text.len() / 2);",
    );
    line(out, "    let __ls_bytes = __ls_text.as_bytes();");
    line(out, "    let mut __ls_index = 0;");
    line(out, "    while __ls_index < __ls_bytes.len() {");
    line(
        out,
        "        let __ls_hi = (__ls_bytes[__ls_index] as char).to_digit(16).ok_or_else(__ls_err)?;",
    );
    line(
        out,
        "        let __ls_lo = (__ls_bytes[__ls_index + 1] as char).to_digit(16).ok_or_else(__ls_err)?;",
    );
    line(
        out,
        "        __ls_out.push((__ls_hi * 16 + __ls_lo) as u8);",
    );
    line(out, "        __ls_index += 2;");
    line(out, "    }");
    line(out, "    Ok(__ls_out)");
    line(out, "}");
    line(out, "");
    line(out, "#[allow(dead_code)]");
    line(
        out,
        "fn __ls_csv_uuid(__ls_text: &str, __ls_table: &str, __ls_column: &str, __ls_row: usize) -> Result<[u8; 16], String> {",
    );
    line(
        out,
        "    let __ls_err = || format!(\"row {__ls_row}: column `{__ls_table}.{__ls_column}` is not a valid UUID: `{__ls_text}`\");",
    );
    line(
        out,
        "    let __ls_hex: String = __ls_text.chars().filter(|__ls_c| *__ls_c != '-').collect();",
    );
    line(
        out,
        "    if __ls_hex.len() != 32 { return Err(__ls_err()); }",
    );
    line(
        out,
        "    let __ls_bytes = __ls_csv_unhex(&__ls_hex, __ls_table, __ls_column, __ls_row)?;",
    );
    line(out, "    let mut __ls_uuid = [0u8; 16];");
    line(out, "    __ls_uuid.copy_from_slice(&__ls_bytes);");
    line(out, "    Ok(__ls_uuid)");
    line(out, "}");
    line(out, "");
    line(out, "#[allow(dead_code)]");
    line(
        out,
        "fn __ls_csv_bool(__ls_text: &str, __ls_table: &str, __ls_column: &str, __ls_row: usize) -> Result<bool, String> {",
    );
    line(out, "    match __ls_text.to_lowercase().as_str() {");
    line(out, "        \"true\" | \"t\" | \"1\" => Ok(true),");
    line(out, "        \"false\" | \"f\" | \"0\" => Ok(false),");
    line(
        out,
        "        _ => Err(format!(\"row {__ls_row}: column `{__ls_table}.{__ls_column}` is not a valid bool: `{__ls_text}`\")),",
    );
    line(out, "    }");
    line(out, "}");
    line(out, "");
}

pub(crate) fn csv_cell(ty: &Ty, value: &str) -> String {
    match ty {
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
        | Ty::Bool => format!("({value}).to_string()"),
        Ty::Text => format!("__ls_csv_escape_cell(&({value}))"),
        Ty::Bytes => format!("__ls_cli_hex(&({value}))"),
        Ty::Uuid => format!("__ls_cli_uuid_text(&({value}))"),
    }
}

pub(crate) fn csv_parse(ty: &Ty, cell: &str, table: &str, field: &str) -> String {
    let err = |what: &str| {
        format!(
            "format!(\"row {{__ls_row_no}}: column `{table}.{field}` is not a valid {what}: `{{{cell}}}`\")",
        )
    };
    match ty {
        Ty::I8 => format!("{cell}.parse::<i8>().map_err(|_| {err})?", err = err("i8")),
        Ty::I16 => format!(
            "{cell}.parse::<i16>().map_err(|_| {err})?",
            err = err("i16")
        ),
        Ty::I32 => format!(
            "{cell}.parse::<i32>().map_err(|_| {err})?",
            err = err("i32")
        ),
        Ty::I64 => format!(
            "{cell}.parse::<i64>().map_err(|_| {err})?",
            err = err("i64")
        ),
        Ty::U8 => format!("{cell}.parse::<u8>().map_err(|_| {err})?", err = err("u8")),
        Ty::U16 => format!(
            "{cell}.parse::<u16>().map_err(|_| {err})?",
            err = err("u16")
        ),
        Ty::U32 => format!(
            "{cell}.parse::<u32>().map_err(|_| {err})?",
            err = err("u32")
        ),
        Ty::U64 => format!(
            "{cell}.parse::<u64>().map_err(|_| {err})?",
            err = err("u64")
        ),
        Ty::F32 => format!(
            "{cell}.parse::<f32>().map_err(|_| {err})?",
            err = err("f32")
        ),
        Ty::F64 => format!(
            "{cell}.parse::<f64>().map_err(|_| {err})?",
            err = err("f64")
        ),
        Ty::Bool => format!("__ls_csv_bool(&{cell}, {table:?}, {field:?}, __ls_row_no)?",),
        Ty::Bytes => format!("__ls_csv_unhex(&{cell}, {table:?}, {field:?}, __ls_row_no)?",),
        Ty::Uuid => format!("__ls_csv_uuid(&{cell}, {table:?}, {field:?}, __ls_row_no)?",),
        Ty::Text => format!("{cell}.to_string()"),
    }
}

pub(crate) fn emit_export(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(
        out,
        "fn __ls_cli_export(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(
        out,
        "    let (__ls_table, __ls_destination) = match __ls_args {",
    );
    line(
        out,
        "        [__ls_table, __ls_destination] => (__ls_table.as_str(), __ls_destination.as_str()),",
    );
    line(
        out,
        "        _ => return Err(\"export expects exactly a table and a destination path\".to_string()),",
    );
    line(out, "    };");
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_destination_path = std::path::Path::new(__ls_destination);",
    );
    line(out, "    if __ls_destination_path.exists() {");
    line(
        out,
        "        return Err(format!(\"export destination `{__ls_destination}` already exists\"));",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let __ls_file = std::fs::OpenOptions::new()");
    line(out, "        .write(true)");
    line(out, "        .create_new(true)");
    line(out, "        .open(__ls_destination_path)");
    line(
        out,
        "        .map_err(|error| format!(\"cannot write `{__ls_destination}`: {error}\"))?;",
    );
    line(out, "    #[cfg(unix)]");
    line(out, "    {");
    line(out, "        use std::os::unix::fs::PermissionsExt;");
    line(
        out,
        "        std::fs::set_permissions(__ls_destination_path, std::fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())?;",
    );
    line(out, "    }");
    line(
        out,
        "    let mut __ls_out = std::io::BufWriter::new(__ls_file);",
    );
    line(out, "    use std::io::Write as _;");
    let tables = workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
        .collect::<Vec<_>>();
    line(out, "    let mut __ls_rows = 0usize;");
    line(out, "    match __ls_table {");
    for table in &tables {
        line(out, &format!("        {:?} => {{", table.name));
        let header = table
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        line(
            out,
            &format!(
                "            writeln!(__ls_out, {header:?}).map_err(|error| error.to_string())?;"
            ),
        );
        line(
            out,
            &format!(
                "            for __ls_row in __ls_db.t_{}.values() {{",
                table.name
            ),
        );
        line(
            out,
            "                let mut __ls_cells: Vec<String> = Vec::new();",
        );
        for field in &table.fields {
            let cell = if field.nullable {
                let some = csv_cell(&field.ty, "__ls_v");
                format!(
                    "match &__ls_row.{0} {{ Some(__ls_v) => {some}, None => String::new() }}",
                    field.name
                )
            } else {
                csv_cell(&field.ty, &format!("__ls_row.{}", field.name))
            };
            line(out, &format!("                __ls_cells.push({cell});"));
        }
        line(out, "                writeln!(__ls_out, \"{}\", __ls_cells.join(\",\")).map_err(|error| error.to_string())?;");
        line(out, "                __ls_rows += 1;");
        line(out, "            }");
        line(out, "        }");
    }
    let available = tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    line(
        out,
        &format!("        _ => return Err(format!(\"unknown table `{{}}` (available: {available})\", __ls_table)),"),
    );
    line(out, "    }");
    line(
        out,
        "    __ls_out.flush().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_out.get_ref().sync_all().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"exported: {__ls_rows} rows from `{__ls_table}` -> `{__ls_destination}`\");",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    Ok(())
}

pub(crate) fn emit_import(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(
        out,
        "fn __ls_cli_import(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    let (__ls_table, __ls_source) = match __ls_args {");
    line(
        out,
        "        [__ls_table, __ls_source] => (__ls_table.as_str(), __ls_source.as_str()),",
    );
    line(
        out,
        "        _ => return Err(\"import expects exactly a table and a source CSV path\".to_string()),",
    );
    line(out, "    };");
    line(
        out,
        "    // Import creates the database when missing, mirroring `run`.",
    );
    line(
        out,
        "    let __ls_file = std::fs::File::open(__ls_source).map_err(|error| format!(\"cannot read `{__ls_source}`: {error}\"))?;",
    );
    line(
        out,
        "    let mut __ls_reader = __LsCsvReader::__ls_new(__ls_file);",
    );
    line(
        out,
        "    let __ls_header = match __ls_reader.__ls_next()? {",
    );
    line(out, "        Some(__ls_header) => __ls_header,");
    line(
        out,
        "        None => return Err(format!(\"cannot import `{__ls_source}`: CSV is empty\")),",
    );
    line(out, "    };");
    line(
        out,
        "    let mut __ls_seen = std::collections::HashSet::new();",
    );
    line(out, "    for __ls_name in &__ls_header {");
    line(out, "        if !__ls_seen.insert(__ls_name.as_str()) {");
    line(
        out,
        "            return Err(format!(\"cannot import `{__ls_source}`: duplicate column `{__ls_name}`\"));",
    );
    line(out, "        }");
    line(out, "    }");
    line(
        out,
        "    let mut __ls_db = Db::open(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let mut __ls_rows = 0usize;");
    line(out, "    match __ls_table {");
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        line(out, &format!("        {:?} => {{", table.name));
        line(
            out,
            "            let mut __ls_positions: Vec<Option<usize>> = Vec::new();",
        );
        for field in &table.fields {
            line(
                out,
                &format!(
                    "            __ls_positions.push(__ls_header.iter().position(|__ls_name| __ls_name == {:?}));",
                    field.name
                ),
            );
        }
        line(out, "            for __ls_name in &__ls_header {");
        line(
            out,
            &format!(
                "                if ![{}].contains(&__ls_name.as_str()) {{",
                table
                    .fields
                    .iter()
                    .map(|field| format!("{:?}", field.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        line(
            out,
            "                    return Err(format!(\"cannot import `{__ls_source}`: unknown column `{__ls_name}` for table `{__ls_table}`\"));",
        );
        line(out, "                }");
        line(out, "            }");
        for (index, field) in table.fields.iter().enumerate() {
            if !field.nullable {
                line(
                    out,
                    &format!(
                        "            if __ls_positions[{index}].is_none() {{ return Err(format!(\"cannot import `{{__ls_source}}`: non-nullable column `{{}}` has no CSV column\", {:?})); }}",
                        field.name
                    ),
                );
            }
        }
        line(
            out,
            "            while let Some(__ls_cells) = __ls_reader.__ls_next()? {",
        );
        line(out, "                __ls_rows += 1;");
        line(out, "                let __ls_row_no = __ls_rows + 1;");
        line(
            out,
            "                if __ls_cells.len() != __ls_header.len() {",
        );
        line(
            out,
            &format!(
                "                    return Err(format!(\"row {{__ls_row_no}}: expected {} fields, found {{}}\", __ls_cells.len()));",
                table.fields.len()
            ),
        );
        line(out, "                }");
        line(out, "                let mut __ls_cells = __ls_cells;");
        for (index, field) in table.fields.iter().enumerate() {
            let parse = csv_parse(
                &field.ty,
                &format!("__ls_raw_{index}"),
                &table.name,
                &field.name,
            );
            if field.nullable {
                line(out, &format!("                let __ls_v{index}: Option<{ty}> = match __ls_positions[{index}] {{", ty = field.ty.rust()));
                line(out, "                    None => None,");
                line(out, "                    Some(__ls_position) => {");
                line(out, &format!("                        let __ls_raw_{index} = std::mem::replace(&mut __ls_cells[__ls_position], String::new());"));
                line(out, &format!("                        if __ls_raw_{index}.is_empty() {{ None }} else {{ Some({parse}) }}"));
                line(out, "                    }");
                line(out, "                };");
            } else {
                line(out, &format!("                let __ls_raw_{index} = std::mem::take(&mut __ls_cells[__ls_positions[{index}].unwrap()]);"));
                if field.ty == Ty::Text {
                    line(
                        out,
                        &format!("                let __ls_v{index}: String = __ls_raw_{index};"),
                    );
                } else {
                    line(out, &format!("                if __ls_raw_{index}.is_empty() {{ return Err(format!(\"row {{__ls_row_no}}: column `{table}.{field}` needs a value\")); }}", table = table.name, field = field.name, index = index));
                    line(
                        out,
                        &format!(
                            "                let __ls_v{index}: {ty} = {parse};",
                            ty = field.ty.rust()
                        ),
                    );
                }
            }
        }
        let row_type = row_type(&table.name);
        let assignments = table
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| format!("{}: __ls_v{index}", field.name))
            .collect::<Vec<_>>()
            .join(", ");
        line(
            out,
            &format!("                let __ls_row = {row_type} {{ {assignments} }};"),
        );
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        line(
            out,
            &format!(
                "                let __ls_key = __ls_row.{}.clone();",
                primary_key.name
            ),
        );
        line(
            out,
            &format!(
                "                if __ls_db.t_{}.contains_key(&__ls_key) {{ return Err(format!(\"row {{__ls_row_no}}: duplicate key for table `{{}}`\", {:?})); }}",
                table.name, table.name
            ),
        );
        line(
            out,
            &format!(
                "                __ls_db.__ls_insert_{}(&__ls_row);",
                table.name
            ),
        );
        line(
            out,
            &format!(
                "                __ls_db.t_{}.__ls_append_loaded(__ls_key, __ls_row).map_err(|error| error.to_string())?;",
                table.name
            ),
        );
        line(out, "            }");
        line(out, "        }");
    }
    let available = workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    line(
        out,
        &format!("        _ => return Err(format!(\"unknown table `{{}}` (available: {available})\", __ls_table)),"),
    );
    line(out, "    }");
    line(
        out,
        "    __ls_db.close().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"imported: {__ls_rows} rows into `{__ls_table}` from `{__ls_source}`\");",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    Ok(())
}

pub(crate) fn dump_put(ty: &Ty, buffer: &str, place: &str) -> String {
    match ty {
        Ty::I8 => format!("__ls_put_i8({buffer}, {place})"),
        Ty::I16 => format!("__ls_put_i16({buffer}, {place})"),
        Ty::I32 => format!("__ls_put_i32({buffer}, {place})"),
        Ty::I64 => format!("__ls_put_i64({buffer}, {place})"),
        Ty::U8 => format!("__ls_put_u8({buffer}, {place})"),
        Ty::U16 => format!("__ls_put_u16({buffer}, {place})"),
        Ty::U32 => format!("__ls_put_u32({buffer}, {place})"),
        Ty::U64 => format!("__ls_put_u64({buffer}, {place})"),
        Ty::F32 => format!("__ls_put_f32({buffer}, {place})"),
        Ty::F64 => format!("__ls_put_f64({buffer}, {place})"),
        Ty::Bool => format!("__ls_put_bool({buffer}, {place})"),
        Ty::Text => format!("__ls_put_text({buffer}, &({place}))"),
        Ty::Bytes => format!("__ls_put_bytes({buffer}, &({place}))"),
        Ty::Uuid => format!("__ls_put_uuid({buffer}, &({place}))"),
    }
}

pub(crate) fn dump_read(ty: &Ty) -> &'static str {
    match ty {
        Ty::I8 => "chunk.i8()",
        Ty::I16 => "chunk.i16()",
        Ty::I32 => "chunk.i32()",
        Ty::I64 => "chunk.i64()",
        Ty::U8 => "chunk.u8()",
        Ty::U16 => "chunk.u16()",
        Ty::U32 => "chunk.u32()",
        Ty::U64 => "chunk.u64()",
        Ty::F32 => "chunk.f32()",
        Ty::F64 => "chunk.f64()",
        Ty::Bool => "chunk.bool()",
        Ty::Text => "chunk.text()",
        Ty::Bytes => "chunk.bytes()",
        Ty::Uuid => "chunk.uuid()",
    }
}

pub(crate) fn emit_dump(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(
        out,
        "fn __ls_cli_dump(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    let __ls_destination = match __ls_args {");
    line(
        out,
        "        [__ls_destination] => __ls_destination.as_str(),",
    );
    line(
        out,
        "        _ => return Err(\"dump expects exactly one destination path\".to_string()),",
    );
    line(out, "    };");
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_destination_path = std::path::Path::new(__ls_destination);",
    );
    line(out, "    if __ls_destination_path.exists() {");
    line(
        out,
        "        return Err(format!(\"dump destination `{__ls_destination}` already exists\"));",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_db = Db::open_read_only(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let __ls_file = std::fs::OpenOptions::new()");
    line(out, "        .write(true)");
    line(out, "        .create_new(true)");
    line(out, "        .open(__ls_destination_path)");
    line(
        out,
        "        .map_err(|error| format!(\"cannot write `{__ls_destination}`: {error}\"))?;",
    );
    line(out, "    #[cfg(unix)]");
    line(out, "    {");
    line(out, "        use std::os::unix::fs::PermissionsExt;");
    line(
        out,
        "        std::fs::set_permissions(__ls_destination_path, std::fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())?;",
    );
    line(out, "    }");
    line(
        out,
        "    let mut __ls_out = std::io::BufWriter::new(__ls_file);",
    );
    line(out, "    use std::io::Write as _;");
    line(
        out,
        "    __ls_out.write_all(b\"LIGHTDMP\").map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_out.write_all(&1u32.to_le_bytes()).map_err(|error| error.to_string())?;",
    );
    let tables = workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
        .collect::<Vec<_>>();
    line(
        out,
        &format!(
            "    __ls_out.write_all(&({}u32).to_le_bytes()).map_err(|error| error.to_string())?;",
            tables.len()
        ),
    );
    line(out, "    let mut __ls_rows = 0usize;");
    line(out, "    let mut __ls_buf = Vec::new();");
    for table in &tables {
        line(
            out,
            &format!("    __ls_put_text(&mut __ls_buf, {:?});", table.name),
        );
        line(
            out,
            "    __ls_out.write_all(&__ls_buf).map_err(|error| error.to_string())?;",
        );
        line(out, "    __ls_buf.clear();");
        line(
            out,
            &format!(
                "    __ls_out.write_all(&({}u32).to_le_bytes()).map_err(|error| error.to_string())?;",
                table.fields.len()
            ),
        );
        for field in &table.fields {
            line(
                out,
                &format!("    __ls_put_text(&mut __ls_buf, {:?});", field.name),
            );
            line(
                out,
                "    __ls_out.write_all(&__ls_buf).map_err(|error| error.to_string())?;",
            );
            line(out, "    __ls_buf.clear();");
            line(
                out,
                &format!(
                    "    __ls_out.write_all(&[{}u8, {}u8, {}u8]).map_err(|error| error.to_string())?;",
                    type_tag(field.ty),
                    field.nullable as u8,
                    field.primary_key as u8,
                ),
            );
        }
        line(
            out,
            &format!(
                "    __ls_out.write_all(&(__ls_db.t_{}.len() as u64).to_le_bytes()).map_err(|error| error.to_string())?;",
                table.name
            ),
        );
        line(
            out,
            &format!("    for __ls_row in __ls_db.t_{}.values() {{", table.name),
        );
        line(out, "        __ls_buf.clear();");
        for field in &table.fields {
            let put = dump_put(
                &field.ty,
                "&mut __ls_buf",
                &format!("__ls_row.{}", field.name),
            );
            if field.nullable {
                let present = put
                    .replace(&format!("&__ls_row.{}", field.name), "__ls_v")
                    .replace(&format!("__ls_row.{}", field.name), "(*__ls_v)");
                line(out, &format!("        match &__ls_row.{0} {{", field.name));
                line(out, "            ::std::option::Option::Some(__ls_v) => {");
                line(out, "                __ls_put_u8(&mut __ls_buf, 1);");
                line(out, &format!("                {present};"));
                line(out, "            }");
                line(out, "            ::std::option::Option::None => {");
                line(out, "                __ls_put_u8(&mut __ls_buf, 0);");
                line(out, "            }");
                line(out, "        }");
            } else {
                line(out, &format!("        {put};"));
            }
        }
        line(out, "        __ls_rows += 1;");
        line(
            out,
            "        __ls_out.write_all(&(__ls_buf.len() as u32).to_le_bytes()).map_err(|error| error.to_string())?;",
        );
        line(
            out,
            "        __ls_out.write_all(&__ls_buf).map_err(|error| error.to_string())?;",
        );
        line(out, "    }");
    }
    line(
        out,
        "    __ls_out.flush().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    __ls_out.get_ref().sync_all().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"dumped: {__ls_rows} rows -> `{__ls_destination}`\");",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    Ok(())
}

pub(crate) fn emit_restore(out: &mut String, workload: &Workload) -> Result<(), String> {
    line(
        out,
        "fn __ls_dump_u8(__ls_file: &mut std::fs::File) -> Result<u8, String> {",
    );
    line(out, "    use std::io::Read as _;");
    line(out, "    let mut __ls_byte = [0u8; 1];");
    line(
        out,
        "    __ls_file.read_exact(&mut __ls_byte).map_err(|error| format!(\"dump is truncated: {error}\"))?;",
    );
    line(out, "    Ok(__ls_byte[0])");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_dump_u32(__ls_file: &mut std::fs::File) -> Result<u32, String> {",
    );
    line(out, "    use std::io::Read as _;");
    line(out, "    let mut __ls_bytes = [0u8; 4];");
    line(
        out,
        "    __ls_file.read_exact(&mut __ls_bytes).map_err(|error| format!(\"dump is truncated: {error}\"))?;",
    );
    line(out, "    Ok(u32::from_le_bytes(__ls_bytes))");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_dump_u64(__ls_file: &mut std::fs::File) -> Result<u64, String> {",
    );
    line(out, "    use std::io::Read as _;");
    line(out, "    let mut __ls_bytes = [0u8; 8];");
    line(
        out,
        "    __ls_file.read_exact(&mut __ls_bytes).map_err(|error| format!(\"dump is truncated: {error}\"))?;",
    );
    line(out, "    Ok(u64::from_le_bytes(__ls_bytes))");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_dump_text(__ls_file: &mut std::fs::File) -> Result<String, String> {",
    );
    line(out, "    use std::io::Read as _;");
    line(
        out,
        "    let __ls_len = __ls_dump_u32(__ls_file)? as usize;",
    );
    line(
        out,
        "    if __ls_len > 16 * 1024 * 1024 { return Err(\"dump has an oversized text value\".to_string()); }",
    );
    line(out, "    let mut __ls_bytes = vec![0u8; __ls_len];");
    line(
        out,
        "    __ls_file.read_exact(&mut __ls_bytes).map_err(|error| format!(\"dump is truncated: {error}\"))?;",
    );
    line(
        out,
        "    String::from_utf8(__ls_bytes).map_err(|_| \"dump has invalid UTF-8\".to_string())",
    );
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_cli_restore(__ls_path: &str, __ls_args: &[String]) -> Result<(), String> {",
    );
    line(out, "    let __ls_source = match __ls_args {");
    line(out, "        [__ls_source] => __ls_source.as_str(),");
    line(
        out,
        "        _ => return Err(\"restore expects exactly one dump path\".to_string()),",
    );
    line(out, "    };");
    line(
        out,
        "    let mut __ls_file = std::fs::File::open(__ls_source).map_err(|error| format!(\"cannot read `{__ls_source}`: {error}\"))?;",
    );
    line(out, "    let mut __ls_magic = [0u8; 8];");
    line(
        out,
        "    { use std::io::Read as _; __ls_file.read_exact(&mut __ls_magic).map_err(|error| format!(\"cannot read `{__ls_source}`: {error}\"))?; }",
    );
    line(
        out,
        "    if __ls_magic != *b\"LIGHTDMP\" { return Err(format!(\"`{__ls_source}` is not a Lightshade DB dump\")); }",
    );
    line(
        out,
        "    let __ls_version = __ls_dump_u32(&mut __ls_file)?;",
    );
    line(out, "    if __ls_version != 1 {");
    line(
        out,
        "        return Err(format!(\"unsupported dump version {__ls_version} (expected 1)\"));",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_table_count = __ls_dump_u32(&mut __ls_file)? as usize;",
    );
    line(
        out,
        "    let mut __ls_db = Db::open(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(out, "    let mut __ls_restored = 0usize;");
    line(out, "    let mut __ls_seen: Vec<String> = Vec::new();");
    line(out, "    for _ in 0..__ls_table_count {");
    line(
        out,
        "        let __ls_table = __ls_dump_text(&mut __ls_file)?;",
    );
    line(
        out,
        "        let __ls_column_count = __ls_dump_u32(&mut __ls_file)? as usize;",
    );
    line(
        out,
        "        let mut __ls_columns: Vec<(String, u8, bool, bool)> = Vec::new();",
    );
    line(out, "        for _ in 0..__ls_column_count {");
    line(
        out,
        "            let __ls_name = __ls_dump_text(&mut __ls_file)?;",
    );
    line(
        out,
        "            let __ls_tag = __ls_dump_u8(&mut __ls_file)?;",
    );
    line(
        out,
        "            let __ls_nullable = __ls_dump_u8(&mut __ls_file)? != 0;",
    );
    line(
        out,
        "            let __ls_primary = __ls_dump_u8(&mut __ls_file)? != 0;",
    );
    line(
        out,
        "            if __ls_tag > 13 { return Err(format!(\"dump table `{__ls_table}` has unknown type tag {__ls_tag}\")); }",
    );
    line(
        out,
        "            __ls_columns.push((__ls_name, __ls_tag, __ls_nullable, __ls_primary));",
    );
    line(out, "        }");
    line(
        out,
        "        let __ls_row_count = __ls_dump_u64(&mut __ls_file)?;",
    );
    line(out, "        __ls_restore_table(&mut __ls_db, &__ls_source, &__ls_table, &__ls_columns, __ls_row_count, &mut __ls_file)?;");
    line(
        out,
        "        if !__ls_seen.iter().any(|__ls_name| __ls_name == &__ls_table) {",
    );
    line(out, "            __ls_seen.push(__ls_table.clone());");
    line(out, "        }");
    line(out, "        __ls_restored += 1;");
    line(out, "    }");
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        line(
            out,
            &format!(
                "    if !__ls_seen.iter().any(|__ls_name| __ls_name == {:?}) {{",
                table.name
            ),
        );
        line(
            out,
            &format!(
                "        return Err(\"dump is missing table `{}`\".to_string());",
                table.name
            ),
        );
        line(out, "    }");
    }
    line(
        out,
        "    __ls_db.close().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    println!(\"restored: {__ls_restored} tables -> `{__ls_path}`\");",
    );
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(
        out,
        "fn __ls_restore_table(__ls_db: &mut Db, __ls_source: &str, __ls_table: &str, __ls_columns: &[(String, u8, bool, bool)], __ls_row_count: u64, __ls_file: &mut std::fs::File) -> Result<(), String> {",
    );
    line(out, "    use std::io::Read as _;");
    line(out, "    match __ls_table {");
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        line(out, &format!("        {:?} => {{", table.name));
        line(
            out,
            &format!(
                "            if __ls_columns.len() != {} {{",
                table.fields.len()
            ),
        );
        line(
            out,
            &format!(
                "                return Err(format!(\"dump table `{{__ls_table}}` has {{}} columns, expected {} columns\", __ls_columns.len()));",
                table.fields.len()
            ),
        );
        line(out, "            }");
        for (position, field) in table.fields.iter().enumerate() {
            line(
                out,
                &format!(
                    "            if __ls_columns[{position}] != ({:?}.to_string(), {}u8, {}, {}) {{",
                    field.name,
                    type_tag(field.ty),
                    field.nullable,
                    field.primary_key,
                ),
            );
            line(
                out,
                &format!(
                    "                return Err(format!(\"dump table `{{__ls_table}}` column {position} mismatch: stored {{:?}}, expected {} (type {}, nullable {}, primary key {})\", __ls_columns[{position}]));",
                    field.name,
                    type_tag(field.ty),
                    field.nullable,
                    field.primary_key,
                ),
            );
            line(out, "            }");
        }
        let row_type = row_type(&table.name);
        line(out, "            for _ in 0..__ls_row_count {");
        line(
            out,
            "                let __ls_len = { let mut __ls_n = [0u8; 4]; { use std::io::Read as _; __ls_file.read_exact(&mut __ls_n).map_err(|error| format!(\"dump is truncated: {error}\"))?; } u32::from_le_bytes(__ls_n) as usize };",
        );
        line(out, "                if __ls_len > 512 * 1024 * 1024 {");
        line(
            out,
            "                    return Err(format!(\"dump table `{__ls_table}` has an oversized row ({__ls_len} bytes)\"));",
        );
        line(out, "                }");
        line(
            out,
            "                let mut __ls_payload = vec![0u8; __ls_len];",
        );
        line(
            out,
            "                { use std::io::Read as _; __ls_file.read_exact(&mut __ls_payload).map_err(|error| format!(\"dump is truncated: {error}\"))?; }",
        );
        line(
            out,
            "                let mut chunk = __LsReader::new(&__ls_payload);",
        );
        for field in &table.fields {
            let read = dump_read(&field.ty);
            let map_err = format!(
                ".map_err(|error| format!(\"dump table `{{__ls_table}}` has a corrupt value for `{{}}`: {{error}}\", {:?}))?",
                field.name
            );
            if field.nullable {
                line(
                    out,
                    &format!(
                        "                let __ls_f_{f} = if chunk.u8(){e} != 0 {{ Some({read}{e}) }} else {{ None }};",
                        f = field.name,
                        e = map_err,
                    ),
                );
            } else {
                line(
                    out,
                    &format!(
                        "                let __ls_f_{f} = {read}{e};",
                        f = field.name,
                        e = map_err,
                    ),
                );
            }
        }
        line(
            out,
            "                if !chunk.is_empty() { return Err(format!(\"dump table `{__ls_table}` has trailing row bytes\")); }",
        );
        let assignments = table
            .fields
            .iter()
            .map(|field| format!("{}: __ls_f_{}", field.name, field.name))
            .collect::<Vec<_>>()
            .join(", ");
        line(
            out,
            &format!("                let __ls_row = {row_type} {{ {assignments} }};"),
        );
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        line(
            out,
            &format!(
                "                let __ls_key = __ls_row.{}.clone();",
                primary_key.name
            ),
        );
        line(
            out,
            &format!(
                "                if __ls_db.t_{}.contains_key(&__ls_key) {{ return Err(format!(\"dump table `{{__ls_table}}` has a duplicate key\")); }}",
                table.name
            ),
        );
        line(
            out,
            &format!(
                "                __ls_db.__ls_insert_{}(&__ls_row);",
                table.name
            ),
        );
        line(
            out,
            &format!(
                "                __ls_db.t_{}.__ls_append_loaded(__ls_key, __ls_row).map_err(|error| error.to_string())?;",
                table.name
            ),
        );
        line(out, "            }");
        line(out, "        }");
    }
    line(out, "        _ => return Err(format!(\"dump table `{__ls_table}` is not in the workload schema\")),");
    line(out, "    }");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    Ok(())
}
