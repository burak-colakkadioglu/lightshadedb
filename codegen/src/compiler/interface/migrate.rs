use super::super::common::type_tag;
use super::line;
use crate::ir::Workload;

pub(crate) fn emit_migrate_plan(out: &mut String, workload: &Workload) -> Result<(), String> {
    let mut compiled = String::from(
        "    let mut __ls_compiled: Vec<(&str, Vec<(&str, u8, bool, bool, bool)>)> = Vec::new();\n",
    );
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        compiled.push_str(&format!("    __ls_compiled.push(({:?}, vec![", table.name));
        for field in &table.fields {
            compiled.push_str(&format!(
                "({:?}, {}u8, {}, {}, {}), ",
                field.name,
                type_tag(field.ty),
                field.primary_key,
                field.default.is_some(),
                field.nullable,
            ));
        }
        compiled.push_str("]));\n");
    }
    line(
        out,
        "fn __ls_cli_migrate_plan(__ls_path: &str) -> Result<(), String> {",
    );
    line(out, "    __ls_cli_require_existing(__ls_path)?;");
    line(
        out,
        "    let __ls_file = File::open(__ls_path).map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    let mut __ls_input = __LsInput::new(io::BufReader::new(__ls_file));",
    );
    line(
        out,
        "    if __ls_input.take(8).map_err(|error| error.to_string())?[..] != __LS_MAGIC[..] {",
    );
    line(
        out,
        "        return Err(\"bad snapshot magic\".to_string());",
    );
    line(out, "    }");
    line(
        out,
        "    let __ls_version = __ls_input.u32().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    if __ls_version != __LS_VERSION && __ls_version != __LS_PREVIOUS_VERSION && __ls_version != __LS_OLD_VERSION {",
    );
    line(
        out,
        "        return Err(\"unsupported snapshot version\".to_string());",
    );
    line(out, "    }");
    line(
        out,
        "    let _ls_stored_fingerprint = __ls_input.u64().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    let _ls_stored_lsn = __ls_input.u64().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "    let __ls_table_count = __ls_input.u32().map_err(|error| error.to_string())? as usize;",
    );
    line(
        out,
        "    let mut __ls_stored: Vec<(String, Vec<(String, u8, bool, bool)>)> = Vec::new();",
    );
    line(out, "    for _ in 0..__ls_table_count {");
    line(
        out,
        "        let __ls_name = __ls_input.text().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "        let __ls_field_count = __ls_input.u32().map_err(|error| error.to_string())? as usize;",
    );
    line(
        out,
        "        let mut __ls_columns = Vec::with_capacity(__ls_field_count);",
    );
    line(out, "        for _ in 0..__ls_field_count {");
    line(
        out,
        "            let __ls_field = __ls_input.text().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "            let __ls_tag = __ls_input.u8().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "            let __ls_flags = __ls_input.u8().map_err(|error| error.to_string())?;",
    );
    line(
        out,
        "            __ls_columns.push((__ls_field, __ls_tag, (__ls_flags & 1) == 1, (__ls_flags & 2) == 2));",
    );
    line(out, "        }");
    line(
        out,
        "        let __ls_rows = __ls_input.u64().map_err(|error| error.to_string())?;",
    );
    line(out, "        let mut __ls_parsed = 0u64;");
    line(out, "        while __ls_parsed < __ls_rows {");
    line(
        out,
        "            __ls_read_chunk(&mut __ls_input, __ls_rows, &mut __ls_parsed).map_err(|error| error.to_string())?;",
    );
    line(out, "        }");
    line(out, "        __ls_stored.push((__ls_name, __ls_columns));");
    line(out, "    }");
    line(
        out,
        "    __ls_input.finish().map_err(|error| error.to_string())?;",
    );
    out.push_str(&compiled);
    line(out, "    let mut __ls_steps: Vec<String> = Vec::new();");
    line(out, "    let mut __ls_destructive = false;");
    line(out, "    for (__ls_name, _) in &__ls_stored {");
    line(
        out,
        "        if !__ls_compiled.iter().any(|(name, _)| name == __ls_name) {",
    );
    line(
        out,
        "            __ls_steps.push(format!(\"remove table `{__ls_name}` (destructive)\"));",
    );
    line(out, "            __ls_destructive = true;");
    line(out, "        }");
    line(out, "    }");
    line(out, "    for (__ls_name, __ls_fields) in &__ls_compiled {");
    line(
        out,
        "        let Some((_, __ls_stored_fields)) = __ls_stored.iter().find(|(name, _)| name == __ls_name) else {",
    );
    line(
        out,
        "            __ls_steps.push(format!(\"add table `{__ls_name}`\"));",
    );
    line(out, "            continue;");
    line(out, "        };");
    line(
        out,
        "        for (__ls_field, __ls_tag, __ls_primary, __ls_default, __ls_nullable) in __ls_fields {",
    );
    line(
        out,
        "            match __ls_stored_fields.iter().find(|(name, _, _, _)| name == __ls_field) {",
    );
    line(
        out,
        "                None if *__ls_primary => __ls_steps.push(format!(\"add field `{__ls_name}.{__ls_field}` (blocked: primary key is missing)\")),",
    );
    line(
        out,
        "                None if *__ls_default => __ls_steps.push(format!(\"add field `{__ls_name}.{__ls_field}`\")),",
    );
    line(
        out,
        "                None if *__ls_nullable => __ls_steps.push(format!(\"add field `{__ls_name}.{__ls_field}`\")),",
    );
    line(
        out,
        "                None => __ls_steps.push(format!(\"add field `{__ls_name}.{__ls_field}` (blocked: no default)\")),",
    );
    line(
        out,
        "                Some((_, __ls_stored_tag, _, _)) if *__ls_stored_tag != *__ls_tag => __ls_steps.push(format!(\"convert field `{__ls_name}.{__ls_field}`: {} -> {} (blocked: type mismatch)\", __ls_tag_name(*__ls_stored_tag), __ls_tag_name(*__ls_tag))),",
    );
    line(
        out,
        "                Some((_, _, _, __ls_stored_nullable)) if *__ls_stored_nullable && !__ls_nullable => __ls_steps.push(format!(\"convert field `{__ls_name}.{__ls_field}` (blocked: cannot become non-nullable)\")),",
    );
    line(out, "                _ => {}");
    line(out, "            }");
    line(out, "        }");
    line(
        out,
        "        for (__ls_field, _, _, _) in __ls_stored_fields {",
    );
    line(
        out,
        "            if !__ls_fields.iter().any(|(name, _, _, _, _)| name == __ls_field) {",
    );
    line(
        out,
        "                __ls_steps.push(format!(\"remove field `{__ls_name}.{__ls_field}` (destructive)\"));",
    );
    line(out, "                __ls_destructive = true;");
    line(out, "            }");
    line(out, "        }");
    line(out, "    }");
    line(out, "    if __ls_steps.is_empty() {");
    line(out, "        println!(\"no migration needed\");");
    line(out, "    } else {");
    line(
        out,
        "        println!(\"plan: {} step(s)\", __ls_steps.len());",
    );
    line(out, "        for __ls_step in &__ls_steps {");
    line(out, "            println!(\"{__ls_step}\");");
    line(out, "        }");
    line(out, "        if __ls_destructive {");
    line(out, "            println!(\"destructive: yes\");");
    line(out, "        }");
    line(out, "    }");
    line(out, "    Ok(())");
    line(out, "}");
    line(out, "");
    line(out, "fn __ls_tag_name(__ls_tag: u8) -> &'static str {");
    line(out, "    match __ls_tag {");
    for (tag, name) in [
        (0u8, "i64"),
        (1, "bool"),
        (2, "text"),
        (3, "i8"),
        (4, "i16"),
        (5, "i32"),
        (6, "u8"),
        (7, "u16"),
        (8, "u32"),
        (9, "u64"),
        (10, "bytes"),
        (11, "uuid"),
        (12, "f32"),
        (13, "f64"),
    ] {
        line(out, &format!("        {tag} => \"{name}\","));
    }
    line(out, "        _ => \"unknown\",");
    line(out, "    }");
    line(out, "}");
    line(out, "");
    Ok(())
}
