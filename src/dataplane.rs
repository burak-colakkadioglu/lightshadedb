use crate::backend::Backend;
use crate::error::{Corruption, Error, Result, SchemaError};
use crate::table::{Column, FieldType, Table, Value};
use std::io::{Read, Write};
use std::path::Path;

pub const DUMP_MAGIC: &[u8; 8] = b"LIGHTDMP";
pub const DUMP_VERSION: u32 = 1;
const MAX_TEXT_LEN: usize = 16 * 1024 * 1024;
const MAX_ROW_LEN: usize = 512 * 1024 * 1024;

pub struct DumpTable<'a> {
    pub name: &'a str,
    pub columns: &'a [Column],
    pub rows: Vec<Vec<Value>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoredColumn {
    pub name: String,
    pub ty: FieldType,
    pub nullable: bool,
    pub primary_key: bool,
}

#[derive(Clone, Debug)]
pub struct RestoredTable {
    pub name: String,
    pub columns: Vec<RestoredColumn>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Clone, Debug)]
pub struct DumpReport {
    pub tables: usize,
    pub rows: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct RestoreReport {
    pub tables: usize,
    pub rows: u64,
}

#[derive(Clone, Debug)]
pub struct ExportReport {
    pub rows: u64,
}

#[derive(Clone, Debug)]
pub struct ImportReport {
    pub rows: u64,
}

fn invalid(name: impl Into<String>, reason: impl Into<String>) -> Error {
    Error::InvalidArgument {
        name: name.into(),
        reason: reason.into(),
    }
}

fn internal(reason: impl Into<String>) -> Error {
    Error::Internal {
        reason: reason.into(),
    }
}

pub fn write_dump(tables: &[DumpTable], writer: &mut impl Write) -> Result<()> {
    writer
        .write_all(DUMP_MAGIC)
        .map_err(|source| Error::io("write dump", source))?;
    put_u32(writer, DUMP_VERSION)?;
    put_u32(writer, tables.len() as u32)?;
    for table in tables {
        put_text(writer, table.name)?;
        put_u32(writer, table.columns.len() as u32)?;
        for column in table.columns {
            put_text(writer, column.name)?;
            writer
                .write_all(&[
                    column.ty.tag(),
                    column.nullable as u8,
                    column.primary_key as u8,
                ])
                .map_err(|source| Error::io("write dump", source))?;
        }
        put_u64(writer, table.rows.len() as u64)?;
        for row in &table.rows {
            if row.len() != table.columns.len() {
                return Err(internal(format!(
                    "dump table `{}` has {} values for {} columns",
                    table.name,
                    row.len(),
                    table.columns.len()
                )));
            }
            let mut payload = Vec::new();
            for (value, column) in row.iter().zip(table.columns.iter()) {
                match value {
                    Value::Null(ty) if *ty == column.ty => payload.push(0),
                    Value::Null(_) => {
                        return Err(Error::Schema(SchemaError::TypeMismatch {
                            table: table.name.to_string(),
                            field: column.name.to_string(),
                            expected: column.ty,
                            found: value.field_type(),
                        }));
                    }
                    _ => {
                        if value.field_type() != column.ty {
                            return Err(Error::Schema(SchemaError::TypeMismatch {
                                table: table.name.to_string(),
                                field: column.name.to_string(),
                                expected: column.ty,
                                found: value.field_type(),
                            }));
                        }
                        if column.nullable {
                            payload.push(1);
                        }
                        put_payload(&mut payload, value)?;
                    }
                }
            }
            put_u32(writer, payload.len() as u32)?;
            writer
                .write_all(&payload)
                .map_err(|source| Error::io("write dump", source))?;
        }
    }
    writer
        .flush()
        .map_err(|source| Error::io("write dump", source))?;
    Ok(())
}

fn put_payload(buffer: &mut Vec<u8>, value: &Value) -> Result<()> {
    match value {
        Value::I8(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::I16(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::I32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::I64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U8(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U16(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::U64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::F32(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::F64(value) => buffer.extend_from_slice(&value.to_le_bytes()),
        Value::Bool(value) => buffer.push(*value as u8),
        Value::Text(value) => put_text_bytes(buffer, value.as_bytes()),
        Value::Bytes(value) => put_text_bytes(buffer, value),
        Value::Uuid(value) => buffer.extend_from_slice(value),
        Value::Null(_) => {
            return Err(internal("dump encoder reached a NULL payload".to_string()));
        }
    }
    Ok(())
}

fn put_text_bytes(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buffer.extend_from_slice(bytes);
}

fn put_u32(writer: &mut impl Write, value: u32) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|source| Error::io("write dump", source))
}

fn put_u64(writer: &mut impl Write, value: u64) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|source| Error::io("write dump", source))
}

fn put_text(writer: &mut impl Write, value: &str) -> Result<()> {
    put_u32(writer, value.len() as u32)?;
    writer
        .write_all(value.as_bytes())
        .map_err(|source| Error::io("write dump", source))
}

pub fn read_dump(reader: &mut impl Read) -> Result<Vec<RestoredTable>> {
    let mut magic = [0u8; 8];
    read_exact(reader, &mut magic)?;
    if &magic != DUMP_MAGIC {
        return Err(Error::Corrupt(Corruption::BadMagic));
    }
    let version = get_u32(reader)?;
    if version != DUMP_VERSION {
        return Err(Error::Corrupt(Corruption::UnsupportedVersion {
            found: version,
        }));
    }
    let table_count = get_u32(reader)? as usize;
    let mut tables = Vec::with_capacity(table_count.min(1024));
    for _ in 0..table_count {
        let name = get_text(reader)?;
        let column_count = get_u32(reader)? as usize;
        let mut columns = Vec::with_capacity(column_count.min(1024));
        for _ in 0..column_count {
            let name = get_text(reader)?;
            let mut flags = [0u8; 3];
            read_exact(reader, &mut flags)?;
            let ty =
                FieldType::from_tag(flags[0]).ok_or(Error::Corrupt(Corruption::InvalidEncoding))?;
            columns.push(RestoredColumn {
                name,
                ty,
                nullable: flags[1] != 0,
                primary_key: flags[2] != 0,
            });
        }
        let row_count = get_u64(reader)?;
        let mut rows = Vec::new();
        if row_count > 0 {
            rows.reserve(row_count.min(1024) as usize);
        }
        for _ in 0..row_count {
            let len = get_u32(reader)? as usize;
            if len > MAX_ROW_LEN {
                return Err(Error::Corrupt(Corruption::InvalidEncoding));
            }
            let mut payload = vec![0u8; len];
            read_exact(reader, &mut payload)?;
            rows.push(decode_row(&name, &columns, &payload)?);
        }
        tables.push(RestoredTable {
            name,
            columns,
            rows,
        });
    }
    Ok(tables)
}

fn decode_row(table: &str, columns: &[RestoredColumn], payload: &[u8]) -> Result<Vec<Value>> {
    let mut input = std::io::Cursor::new(payload);
    let mut row = Vec::with_capacity(columns.len());
    for column in columns {
        if column.nullable {
            let mut presence = [0u8; 1];
            read_exact(&mut input, &mut presence)?;
            if presence[0] == 0 {
                row.push(Value::Null(column.ty));
                continue;
            }
        }
        row.push(
            get_payload(&mut input, column.ty).map_err(|error| match error {
                Error::Corrupt(_) => Error::Corrupt(Corruption::InvalidEncoding),
                error => invalid(
                    table,
                    format!(
                        "dump table `{table}` has a corrupt value for `{}`: {error}",
                        column.name
                    ),
                ),
            })?,
        );
    }
    if input.position() != payload.len() as u64 {
        return Err(Error::Corrupt(Corruption::InvalidEncoding));
    }
    Ok(row)
}

fn get_payload(input: &mut std::io::Cursor<&[u8]>, ty: FieldType) -> Result<Value> {
    let mut one = [0u8; 1];
    let mut two = [0u8; 2];
    let mut four = [0u8; 4];
    let mut eight = [0u8; 8];
    match ty {
        FieldType::I8 => {
            read_exact(input, &mut one)?;
            Ok(Value::I8(i8::from_le_bytes(one)))
        }
        FieldType::I16 => {
            read_exact(input, &mut two)?;
            Ok(Value::I16(i16::from_le_bytes(two)))
        }
        FieldType::I32 => {
            read_exact(input, &mut four)?;
            Ok(Value::I32(i32::from_le_bytes(four)))
        }
        FieldType::I64 => {
            read_exact(input, &mut eight)?;
            Ok(Value::I64(i64::from_le_bytes(eight)))
        }
        FieldType::U8 => {
            read_exact(input, &mut one)?;
            Ok(Value::U8(u8::from_le_bytes(one)))
        }
        FieldType::U16 => {
            read_exact(input, &mut two)?;
            Ok(Value::U16(u16::from_le_bytes(two)))
        }
        FieldType::U32 => {
            read_exact(input, &mut four)?;
            Ok(Value::U32(u32::from_le_bytes(four)))
        }
        FieldType::U64 => {
            read_exact(input, &mut eight)?;
            Ok(Value::U64(u64::from_le_bytes(eight)))
        }
        FieldType::F32 => {
            read_exact(input, &mut four)?;
            Ok(Value::F32(f32::from_le_bytes(four)))
        }
        FieldType::F64 => {
            read_exact(input, &mut eight)?;
            Ok(Value::F64(f64::from_le_bytes(eight)))
        }
        FieldType::Bool => {
            read_exact(input, &mut one)?;
            match one[0] {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                _ => Err(Error::Corrupt(Corruption::InvalidEncoding)),
            }
        }
        FieldType::Text => Ok(Value::Text(get_text(input)?)),
        FieldType::Bytes => Ok(Value::Bytes(get_bytes(input)?)),
        FieldType::Uuid => {
            let mut id = [0u8; 16];
            read_exact(input, &mut id)?;
            Ok(Value::Uuid(id))
        }
    }
}

fn read_exact(input: &mut impl Read, buf: &mut [u8]) -> Result<()> {
    input.read_exact(buf).map_err(|source| {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::Corrupt(Corruption::Truncated)
        } else {
            Error::io("read dump", source)
        }
    })
}

fn get_u32(input: &mut impl Read) -> Result<u32> {
    let mut bytes = [0u8; 4];
    read_exact(input, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn get_u64(input: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    read_exact(input, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn get_text(input: &mut impl Read) -> Result<String> {
    let len = get_u32(input)? as usize;
    if len > MAX_TEXT_LEN {
        return Err(Error::Corrupt(Corruption::InvalidEncoding));
    }
    let mut bytes = vec![0u8; len];
    read_exact(input, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| Error::Corrupt(Corruption::InvalidEncoding))
}

fn get_bytes(input: &mut impl Read) -> Result<Vec<u8>> {
    let len = get_u32(input)? as usize;
    if len > MAX_TEXT_LEN {
        return Err(Error::Corrupt(Corruption::InvalidEncoding));
    }
    let mut bytes = vec![0u8; len];
    read_exact(input, &mut bytes)?;
    Ok(bytes)
}

pub fn check_tables(expected: &[(&str, &[Column])], found: &[RestoredTable]) -> Result<()> {
    for table in found {
        if !expected.iter().any(|(name, _)| *name == table.name) {
            return Err(Error::Schema(SchemaError::UnknownTable {
                name: table.name.clone(),
            }));
        }
    }
    for (name, columns) in expected {
        let Some(table) = found.iter().find(|table| table.name == *name) else {
            return Err(invalid("dump", format!("dump is missing table `{name}`")));
        };
        if table.columns.len() != columns.len() {
            return Err(invalid(
                *name,
                format!(
                    "dump table `{name}` has {} columns, expected {} columns",
                    table.columns.len(),
                    columns.len()
                ),
            ));
        }
        for (position, (stored, compiled)) in table.columns.iter().zip(columns.iter()).enumerate() {
            if stored.name != compiled.name
                || stored.ty != compiled.ty
                || stored.nullable != compiled.nullable
                || stored.primary_key != compiled.primary_key
            {
                return Err(invalid(
                    *name,
                    format!(
                    "dump table `{name}` column {position} mismatch: stored ({}, type {}, nullable {}, primary key {}), expected {} (type {}, nullable {}, primary key {})",
                    stored.name,
                    stored.ty,
                    stored.nullable,
                    stored.primary_key,
                    compiled.name,
                    compiled.ty,
                    compiled.nullable,
                    compiled.primary_key,
                )));
            }
        }
    }
    Ok(())
}

pub fn write_dump_file(tables: &[DumpTable], dest: impl AsRef<Path>) -> Result<DumpReport> {
    let dest = dest.as_ref();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                invalid(
                    dest.display().to_string(),
                    format!("dump destination `{}` already exists", dest.display()),
                )
            } else {
                Error::io("write dump", source)
            }
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o600))
            .map_err(|source| Error::io("write dump", source))?;
    }
    write_dump(tables, &mut file)?;
    file.sync_all()
        .map_err(|source| Error::io("write dump", source))?;
    let bytes = file
        .metadata()
        .map_err(|source| Error::io("write dump", source))?
        .len();
    Ok(DumpReport {
        tables: tables.len(),
        rows: tables.iter().map(|table| table.rows.len() as u64).sum(),
        bytes,
    })
}

pub fn read_dump_file(src: impl AsRef<Path>) -> Result<Vec<RestoredTable>> {
    let src = src.as_ref();
    let mut file = std::fs::File::open(src).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound {
                path: src.to_path_buf(),
            }
        } else {
            Error::io("read dump", source)
        }
    })?;
    read_dump(&mut file)
}

fn cell_text(value: &Value) -> String {
    match value {
        Value::I8(value) => value.to_string(),
        Value::I16(value) => value.to_string(),
        Value::I32(value) => value.to_string(),
        Value::I64(value) => value.to_string(),
        Value::U8(value) => value.to_string(),
        Value::U16(value) => value.to_string(),
        Value::U32(value) => value.to_string(),
        Value::U64(value) => value.to_string(),
        Value::F32(value) => value.to_string(),
        Value::F64(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Text(value) => escape_cell(value),
        Value::Bytes(value) => hex(value),
        Value::Uuid(value) => hyphenated(value),
        Value::Null(_) => String::new(),
    }
}

fn escape_cell(text: &str) -> String {
    if text
        .bytes()
        .any(|byte| byte == b',' || byte == b'"' || byte == b'\n' || byte == b'\r')
    {
        let mut out = String::with_capacity(text.len() + 2);
        out.push('"');
        out.push_str(&text.replace('"', "\"\""));
        out.push('"');
        out
    } else {
        text.to_string()
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hyphenated(id: &[u8; 16]) -> String {
    let raw = hex(id);
    format!(
        "{}-{}-{}-{}-{}",
        &raw[0..8],
        &raw[8..12],
        &raw[12..16],
        &raw[16..20],
        &raw[20..32]
    )
}

pub fn write_csv<T: Table>(rows: &[T], writer: &mut impl Write) -> Result<()> {
    let header: Vec<&str> = T::COLUMNS.iter().map(|column| column.name).collect();
    writer
        .write_all(header.join(",").as_bytes())
        .map_err(|source| Error::io("write CSV", source))?;
    writer
        .write_all(b"\n")
        .map_err(|source| Error::io("write CSV", source))?;
    for row in rows {
        let cells: Vec<String> = row.to_values().iter().map(cell_text).collect();
        writer
            .write_all(cells.join(",").as_bytes())
            .map_err(|source| Error::io("write CSV", source))?;
        writer
            .write_all(b"\n")
            .map_err(|source| Error::io("write CSV", source))?;
    }
    writer
        .flush()
        .map_err(|source| Error::io("write CSV", source))?;
    Ok(())
}

pub fn read_csv<T: Table>(reader: &mut impl Read) -> Result<Vec<T>> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| Error::io("read CSV", source))?;
    let records = split_records(&bytes)?;
    let mut records = records.into_iter();
    let Some(header) = records.next() else {
        return Err(invalid("csv", "cannot import an empty CSV".to_string()));
    };
    let mut seen = std::collections::HashSet::new();
    for name in &header {
        if !seen.insert(name.clone()) {
            return Err(invalid("csv", format!("duplicate column `{name}`")));
        }
    }
    let mut positions: Vec<Option<usize>> = Vec::with_capacity(T::COLUMNS.len());
    for column in T::COLUMNS {
        positions.push(header.iter().position(|name| name == column.name));
    }
    for name in &header {
        if !T::COLUMNS.iter().any(|column| column.name == name) {
            return Err(invalid(
                T::NAME,
                format!("unknown column `{name}` for table `{}`", T::NAME),
            ));
        }
    }
    for (column, position) in T::COLUMNS.iter().zip(positions.iter()) {
        if !column.nullable && position.is_none() {
            return Err(invalid(
                T::NAME,
                format!("non-nullable column `{}` has no CSV column", column.name),
            ));
        }
    }
    let mut rows = Vec::new();
    for (index, cells) in records.enumerate() {
        let row_no = index + 2;
        if cells.len() != header.len() {
            return Err(invalid(
                T::NAME,
                format!(
                    "row {row_no}: expected {} fields, found {}",
                    T::COLUMNS.len(),
                    cells.len()
                ),
            ));
        }
        let mut values = Vec::with_capacity(T::COLUMNS.len());
        for (column, position) in T::COLUMNS.iter().zip(positions.iter()) {
            let cell = position.map(|position| cells[position].clone());
            values.push(parse_cell::<T>(column, cell, row_no)?);
        }
        rows.push(T::from_values(values)?);
    }
    Ok(rows)
}

fn parse_cell<T: Table>(column: &Column, cell: Option<String>, row_no: usize) -> Result<Value> {
    let table = T::NAME;
    let field = column.name;
    let missing = cell.is_none();
    let text = cell.unwrap_or_default();
    if text.is_empty() {
        if column.nullable {
            return Ok(Value::Null(column.ty));
        }
        if column.ty == FieldType::Text && !missing {
            return Ok(Value::Text(String::new()));
        }
        return Err(invalid(
            table,
            format!("row {row_no}: column `{table}.{field}` needs a value"),
        ));
    }
    let bad = |what: &str| {
        invalid(
            table,
            format!("row {row_no}: column `{table}.{field}` is not a valid {what}: `{text}`"),
        )
    };
    match column.ty {
        FieldType::I8 => text.parse::<i8>().map(Value::I8).map_err(|_| bad("i8")),
        FieldType::I16 => text.parse::<i16>().map(Value::I16).map_err(|_| bad("i16")),
        FieldType::I32 => text.parse::<i32>().map(Value::I32).map_err(|_| bad("i32")),
        FieldType::I64 => text.parse::<i64>().map(Value::I64).map_err(|_| bad("i64")),
        FieldType::U8 => text.parse::<u8>().map(Value::U8).map_err(|_| bad("u8")),
        FieldType::U16 => text.parse::<u16>().map(Value::U16).map_err(|_| bad("u16")),
        FieldType::U32 => text.parse::<u32>().map(Value::U32).map_err(|_| bad("u32")),
        FieldType::U64 => text.parse::<u64>().map(Value::U64).map_err(|_| bad("u64")),
        FieldType::F32 => text.parse::<f32>().map(Value::F32).map_err(|_| bad("f32")),
        FieldType::F64 => text.parse::<f64>().map(Value::F64).map_err(|_| bad("f64")),
        FieldType::Bool => match text.to_lowercase().as_str() {
            "true" | "t" | "1" => Ok(Value::Bool(true)),
            "false" | "f" | "0" => Ok(Value::Bool(false)),
            _ => Err(bad("bool")),
        },
        FieldType::Bytes => unhex(&text).map(Value::Bytes).ok_or_else(|| bad("hex")),
        FieldType::Uuid => unhex(&text.replace('-', ""))
            .ok_or_else(|| bad("UUID"))
            .and_then(|bytes| {
                if bytes.len() == 16 {
                    let mut id = [0u8; 16];
                    id.copy_from_slice(&bytes);
                    Ok(Value::Uuid(id))
                } else {
                    Err(bad("UUID"))
                }
            }),
        FieldType::Text => Ok(Value::Text(text)),
    }
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let high = (bytes[index] as char).to_digit(16)?;
        let low = (bytes[index + 1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
        index += 2;
    }
    Some(out)
}

fn split_records(bytes: &[u8]) -> Result<Vec<Vec<String>>> {
    let mut records = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field: Vec<u8> = Vec::new();
    let mut in_quotes = false;
    let mut saw_data = false;
    let mut row_no = 1;
    let mut index = 0;
    let mut flush = |fields: &mut Vec<String>,
                     field: &mut Vec<u8>,
                     saw_data: &mut bool,
                     row_no: &mut usize|
     -> Result<()> {
        fields.push(decode_field(field, *row_no)?);
        records.push(std::mem::take(fields));
        field.clear();
        *saw_data = false;
        *row_no += 1;
        Ok(())
    };
    while index <= bytes.len() {
        let byte = if index < bytes.len() {
            Some(bytes[index])
        } else {
            None
        };
        match byte {
            None => {
                if in_quotes {
                    return Err(invalid(
                        "csv",
                        format!("row {row_no}: unterminated quoted field"),
                    ));
                }
                if !saw_data && field.is_empty() && fields.is_empty() {
                    break;
                }
                flush(&mut fields, &mut field, &mut saw_data, &mut row_no)?;
                break;
            }
            Some(b'"') => {
                saw_data = true;
                if in_quotes {
                    if bytes.get(index + 1) == Some(&b'"') {
                        field.push(b'"');
                        index += 1;
                    } else {
                        in_quotes = false;
                    }
                } else if field.is_empty() {
                    in_quotes = true;
                } else {
                    return Err(invalid(
                        "csv",
                        format!("row {row_no}: stray quote in unquoted field"),
                    ));
                }
            }
            Some(b',') if !in_quotes => {
                saw_data = true;
                fields.push(decode_field(&field, row_no)?);
                field.clear();
            }
            Some(b'\n') if !in_quotes => {
                flush(&mut fields, &mut field, &mut saw_data, &mut row_no)?;
            }
            Some(b'\r') if !in_quotes => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                flush(&mut fields, &mut field, &mut saw_data, &mut row_no)?;
            }
            Some(byte) => {
                saw_data = true;
                field.push(byte);
            }
        }
        index += 1;
    }
    Ok(records)
}

fn decode_field(field: &[u8], row_no: usize) -> Result<String> {
    String::from_utf8(field.to_vec())
        .map_err(|_| invalid("csv", format!("row {row_no}: field is not valid UTF-8")))
}

pub fn export_csv<T: Table, B: Backend + ?Sized>(
    store: &B,
    dest: impl AsRef<Path>,
) -> Result<ExportReport> {
    let rows: Vec<T> = store.scan()?;
    let dest = dest.as_ref();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                invalid(
                    dest.display().to_string(),
                    format!("export destination `{}` already exists", dest.display()),
                )
            } else {
                Error::io("write CSV", source)
            }
        })?;
    write_csv(&rows, &mut file)?;
    file.sync_all()
        .map_err(|source| Error::io("write CSV", source))?;
    Ok(ExportReport {
        rows: rows.len() as u64,
    })
}

pub fn import_csv<T: Table, B: Backend + ?Sized>(
    store: &mut B,
    src: impl AsRef<Path>,
) -> Result<ImportReport> {
    let src = src.as_ref();
    let mut file = std::fs::File::open(src).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound {
                path: src.to_path_buf(),
            }
        } else {
            Error::io("read CSV", source)
        }
    })?;
    let rows: Vec<T> = read_csv(&mut file)?;
    let count = rows.len() as u64;
    store.insert_many(rows)?;
    Ok(ImportReport { rows: count })
}
