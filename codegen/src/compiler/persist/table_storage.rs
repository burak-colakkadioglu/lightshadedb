use super::super::common::{key_rust, row_type};
use super::super::plan::KeyField;
use crate::ir::*;
use std::fmt::Write;

pub(crate) fn emit_table_storage(out: &mut String, workload: &Workload) -> Result<(), String> {
    for table in workload
        .tables
        .iter()
        .filter(|table| !workload.is_projection(&table.name))
    {
        let primary_key = table
            .primary_key()
            .ok_or_else(|| format!("table `{}` has no primary key", table.name))?;
        let key = primary_key.ty.rust();
        let storage_key = key_rust(primary_key.ty);
        let slot_key = storage_key.clone();
        let dense_slots = primary_key.ty == Ty::I64;
        let slot_type = if dense_slots {
            "__LsSlotsI64".to_string()
        } else {
            format!("__LsSlots<{storage_key}>")
        };
        let slot_new = if dense_slots {
            "__LsSlotsI64::new()"
        } else {
            "__LsSlots::new()"
        };
        let slot_iter_type = if dense_slots {
            "__LsSlotsI64Iter<'a>".to_string()
        } else {
            format!("__LsSlotsIter<'a, {storage_key}>")
        };
        let storage_from_ref = match primary_key.ty {
            Ty::Text => "__LsText::from(key.as_str())".to_string(),
            Ty::F32 | Ty::F64 => format!("{storage_key}::from(*key)"),
            _ => "key.clone()".to_string(),
        };
        let storage_from_start = match primary_key.ty {
            Ty::Text => "__LsText::from(start.as_str())".to_string(),
            Ty::F32 | Ty::F64 => format!("{storage_key}::from(*start)"),
            _ => "start.clone()".to_string(),
        };
        let storage_from_owned = match primary_key.ty {
            Ty::Text => "__LsText::from(key.as_str())".to_string(),
            Ty::F32 | Ty::F64 => format!("{storage_key}::from(key)"),
            _ => "key".to_string(),
        };
        let storage_to_raw = match primary_key.ty {
            Ty::Text => "key.to_string()".to_string(),
            Ty::F32 | Ty::F64 => "key.0".to_string(),
            _ => "key.clone()".to_string(),
        };
        let row = row_type(&table.name);
        let store = format!("Rows_{}", table.name);
        let slot_get = "self.slots.get(&__ls_key)?";
        let slot_contains = "self.slots.contains_key(&__ls_key)";
        let iter = format!("{}Iter", store);
        let values = format!("{}Values", store);
        let is_ttl = table.ttl_secs.is_some();

        writeln!(out, "struct {store} {{").unwrap();
        writeln!(out, "    slots: {slot_type},").unwrap();
        writeln!(out, "    spill: std::sync::RwLock<Option<File>>,").unwrap();
        writeln!(out, "    spill_len: std::sync::atomic::AtomicU64,").unwrap();
        writeln!(out, "    staging: std::sync::RwLock<__LsStaging>,").unwrap();
        writeln!(out, "    spill_path: PathBuf,").unwrap();
        writeln!(out, "    direct: bool,").unwrap();
        writeln!(out, "    arena: Vec<Vec<u8>>,").unwrap();
        writeln!(
            out,
            "    overflow: std::collections::HashMap<{storage_key}, {row}, std::hash::BuildHasherDefault<__LsFx>>,"
        )
        .unwrap();
        writeln!(
            out,
            "    cache: [std::sync::RwLock<std::collections::HashMap<{storage_key}, ({row}, u64, u64), std::hash::BuildHasherDefault<__LsFx>>>; 16],"
        )
        .unwrap();
        writeln!(
            out,
            "    order: [std::sync::Mutex<std::collections::VecDeque<({storage_key}, u64)>>; 16],"
        )
        .unwrap();
        writeln!(out, "    cached_bytes: std::sync::atomic::AtomicU64,").unwrap();
        writeln!(out, "    dead_rows: std::sync::atomic::AtomicU64,").unwrap();
        writeln!(out, "    shard_bytes: [std::sync::atomic::AtomicU64; 16],").unwrap();
        writeln!(out, "    stamp: std::sync::atomic::AtomicU64,").unwrap();
        writeln!(out, "    budget: u64,").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "impl {store} {{").unwrap();
        writeln!(
            out,
            "    fn __ls_create(budget: u64) -> io::Result<{store}> {{"
        )
        .unwrap();
        writeln!(out, "        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);").unwrap();
        writeln!(
            out,
            "        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(
            out,
            "        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0);"
        )
        .unwrap();
        writeln!(
            out,
            "        let path = __ls_spill_dir()?.join(format!(\"lightshade-{{}}-{{}}-{{}}-{}.spill\", std::process::id(), stamp, sequence));",
            table.name
        )
        .unwrap();
        writeln!(out, "        let mut options = OpenOptions::new();").unwrap();
        writeln!(
            out,
            "        options.read(true).write(true).create_new(true);"
        )
        .unwrap();
        writeln!(out, "        #[cfg(unix)]").unwrap();
        writeln!(out, "        {{").unwrap();
        writeln!(out, "            use std::os::unix::fs::OpenOptionsExt;").unwrap();
        writeln!(out, "            options.mode(__LS_FILE_MODE);").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        let spill = options.open(&path)?;").unwrap();
        writeln!(
            out,
            "        Ok({store} {{ slots: {slot_new}, spill: std::sync::RwLock::new(Some(spill)), spill_len: std::sync::atomic::AtomicU64::new(0), staging: std::sync::RwLock::new(__LsStaging {{ base: 0, buf: Vec::new() }}), spill_path: path, direct: false, arena: Vec::new(), overflow: std::collections::HashMap::with_hasher(Default::default()), cache: std::array::from_fn(|_| std::sync::RwLock::new(std::collections::HashMap::with_hasher(Default::default()))), order: std::array::from_fn(|_| std::sync::Mutex::new(std::collections::VecDeque::new())), cached_bytes: std::sync::atomic::AtomicU64::new(0), dead_rows: std::sync::atomic::AtomicU64::new(0), shard_bytes: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)), stamp: std::sync::atomic::AtomicU64::new(0), budget }})"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    fn __ls_create_direct() -> io::Result<{store}> {{").unwrap();
        writeln!(
            out,
            "        Ok({store} {{ slots: {slot_new}, spill: std::sync::RwLock::new(None), spill_len: std::sync::atomic::AtomicU64::new(0), staging: std::sync::RwLock::new(__LsStaging {{ base: 0, buf: Vec::new() }}), spill_path: PathBuf::new(), direct: true, arena: Vec::new(), overflow: std::collections::HashMap::with_hasher(Default::default()), cache: std::array::from_fn(|_| std::sync::RwLock::new(std::collections::HashMap::with_hasher(Default::default()))), order: std::array::from_fn(|_| std::sync::Mutex::new(std::collections::VecDeque::new())), cached_bytes: std::sync::atomic::AtomicU64::new(0), dead_rows: std::sync::atomic::AtomicU64::new(0), shard_bytes: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)), stamp: std::sync::atomic::AtomicU64::new(0), budget: 0 }})"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    fn __ls_encode(row: &{row}) -> Vec<u8> {{").unwrap();
        writeln!(out, "        let mut bytes = Vec::new();").unwrap();
        for field in &table.fields {
            if field.name == primary_key.name {
                continue;
            }
            let call = match field.ty {
                Ty::I8 => format!("__ls_put_var_i64(&mut bytes, row.{} as i64)", field.name),
                Ty::I16 => format!("__ls_put_var_i64(&mut bytes, row.{} as i64)", field.name),
                Ty::I32 => format!("__ls_put_var_i64(&mut bytes, row.{} as i64)", field.name),
                Ty::I64 => format!("__ls_put_var_i64(&mut bytes, row.{})", field.name),
                Ty::U8 => format!("__ls_put_var_u64(&mut bytes, row.{} as u64)", field.name),
                Ty::U16 => format!("__ls_put_var_u64(&mut bytes, row.{} as u64)", field.name),
                Ty::U32 => format!("__ls_put_var_u64(&mut bytes, row.{} as u64)", field.name),
                Ty::U64 => format!("__ls_put_var_u64(&mut bytes, row.{})", field.name),
                Ty::F32 => format!("__ls_put_f32(&mut bytes, row.{})", field.name),
                Ty::F64 => format!("__ls_put_f64(&mut bytes, row.{})", field.name),
                Ty::Bool => format!("__ls_put_bool(&mut bytes, row.{})", field.name),
                Ty::Text => {
                    format!("__ls_put_var_text(&mut bytes, &row.{})", field.name)
                }
                Ty::Bytes => {
                    format!("__ls_put_var_bytes(&mut bytes, &row.{})", field.name)
                }
                Ty::Uuid => format!("__ls_put_uuid(&mut bytes, &row.{})", field.name),
            };
            if field.nullable {
                let present = call
                    .replace(&format!("&row.{}", field.name), "value")
                    .replace(&format!("row.{}", field.name), "(*value)");
                writeln!(out, "        match &row.{0} {{", field.name).unwrap();
                writeln!(
                    out,
                    "            ::std::option::Option::Some(value) => {{ __ls_put_u8(&mut bytes, 1); {present}; }},"
                )
                .unwrap();
                writeln!(
                    out,
                    "            ::std::option::Option::None => {{ __ls_put_u8(&mut bytes, 0); }},"
                )
                .unwrap();
                writeln!(out, "        }}").unwrap();
            } else {
                writeln!(out, "        {call};").unwrap();
            }
        }
        writeln!(out, "        if bytes.is_empty() {{ bytes.push(0); }}").unwrap();
        writeln!(out, "        bytes").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_encode_into(row: &{row}, bytes: &mut Vec<u8>) {{"
        )
        .unwrap();
        writeln!(out, "        let __ls_mark = bytes.len();").unwrap();
        for field in &table.fields {
            if field.name == primary_key.name {
                continue;
            }
            let call = match field.ty {
                Ty::I8 => format!("__ls_put_var_i64(bytes, row.{} as i64)", field.name),
                Ty::I16 => format!("__ls_put_var_i64(bytes, row.{} as i64)", field.name),
                Ty::I32 => format!("__ls_put_var_i64(bytes, row.{} as i64)", field.name),
                Ty::I64 => format!("__ls_put_var_i64(bytes, row.{})", field.name),
                Ty::U8 => format!("__ls_put_var_u64(bytes, row.{} as u64)", field.name),
                Ty::U16 => format!("__ls_put_var_u64(bytes, row.{} as u64)", field.name),
                Ty::U32 => format!("__ls_put_var_u64(bytes, row.{} as u64)", field.name),
                Ty::U64 => format!("__ls_put_var_u64(bytes, row.{})", field.name),
                Ty::F32 => format!("__ls_put_f32(bytes, row.{})", field.name),
                Ty::F64 => format!("__ls_put_f64(bytes, row.{})", field.name),
                Ty::Bool => format!("__ls_put_bool(bytes, row.{})", field.name),
                Ty::Text => {
                    format!("__ls_put_var_text(bytes, &row.{})", field.name)
                }
                Ty::Bytes => {
                    format!("__ls_put_var_bytes(bytes, &row.{})", field.name)
                }
                Ty::Uuid => format!("__ls_put_uuid(bytes, &row.{})", field.name),
            };
            if field.nullable {
                let present = call
                    .replace(&format!("&row.{}", field.name), "value")
                    .replace(&format!("row.{}", field.name), "(*value)");
                writeln!(out, "        match &row.{0} {{", field.name).unwrap();
                writeln!(
                    out,
                    "            ::std::option::Option::Some(value) => {{ __ls_put_u8(bytes, 1); {present}; }},"
                )
                .unwrap();
                writeln!(
                    out,
                    "            ::std::option::Option::None => {{ __ls_put_u8(bytes, 0); }},"
                )
                .unwrap();
                writeln!(out, "        }}").unwrap();
            } else {
                writeln!(out, "        {call};").unwrap();
            }
        }
        writeln!(
            out,
            "        if bytes.len() == __ls_mark {{ bytes.push(0); }}"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_decode(bytes: &[u8], key: &{slot_key}) -> {row} {{"
        )
        .unwrap();
        writeln!(out, "        let mut input = __LsReader::new(bytes);").unwrap();
        writeln!(out, "        {row} {{").unwrap();
        for field in &table.fields {
            if field.name == primary_key.name {
                let value = storage_to_raw.as_str();
                writeln!(out, "            {}: {value},", field.name).unwrap();
                continue;
            }
            let read = match field.ty {
                Ty::I8 => "input.var_i64().map(|value| value as i8)",
                Ty::I16 => "input.var_i64().map(|value| value as i16)",
                Ty::I32 => "input.var_i64().map(|value| value as i32)",
                Ty::I64 => "input.var_i64()",
                Ty::U8 => "input.var_u64().map(|value| value as u8)",
                Ty::U16 => "input.var_u64().map(|value| value as u16)",
                Ty::U32 => "input.var_u64().map(|value| value as u32)",
                Ty::U64 => "input.var_u64()",
                Ty::F32 => "input.f32()",
                Ty::F64 => "input.f64()",
                Ty::Bool => "input.bool()",
                Ty::Text => "input.var_text()",
                Ty::Bytes => "input.var_bytes()",
                Ty::Uuid => "input.uuid()",
            };
            if field.nullable {
                writeln!(
                    out,
                    "            {}: match input.u8().expect(\"invalid spill row\") {{",
                    field.name
                )
                .unwrap();
                writeln!(out, "                0 => ::std::option::Option::None,").unwrap();
                writeln!(
                    out,
                    "                _ => ::std::option::Option::Some({read}.expect(\"invalid spill row\")),"
                )
                .unwrap();
                writeln!(out, "            }},").unwrap();
            } else {
                writeln!(
                    out,
                    "            {}: {read}.expect(\"invalid spill row\"),",
                    field.name
                )
                .unwrap();
            }
        }
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_read_slot(&self, key: &{slot_key}, slot: (u64, u64)) -> {row} {{"
        )
        .unwrap();
        writeln!(out, "        if self.direct {{").unwrap();
        writeln!(out, "            let chunk = (slot.0 >> 32) as usize;").unwrap();
        writeln!(
            out,
            "            let start = (slot.0 & 0xffff_ffff) as usize;"
        )
        .unwrap();
        writeln!(out, "            let end = start + slot.1 as usize;").unwrap();
        writeln!(
            out,
            "            return Self::__ls_decode(&self.arena[chunk][start..end], key);"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        {{").unwrap();
        writeln!(
            out,
            "            let __ls_staging = self.staging.read().unwrap();"
        )
        .unwrap();
        writeln!(out, "            if slot.0 >= __ls_staging.base {{").unwrap();
        writeln!(
            out,
            "                if let Some(__ls_end) = slot.0.checked_add(slot.1) {{"
        )
        .unwrap();
        writeln!(out, "                    if __ls_end <= __ls_staging.base + __ls_staging.buf.len() as u64 {{").unwrap();
        writeln!(
            out,
            "                        let __ls_off = (slot.0 - __ls_staging.base) as usize;"
        )
        .unwrap();
        writeln!(out, "                        let bytes = __ls_staging.buf[__ls_off..__ls_off + slot.1 as usize].to_vec();").unwrap();
        writeln!(out, "                        drop(__ls_staging);").unwrap();
        writeln!(
            out,
            "                        return Self::__ls_decode(&bytes, key);"
        )
        .unwrap();
        writeln!(out, "                    }}").unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        let mut bytes = vec![0u8; slot.1 as usize];").unwrap();
        writeln!(out, "        {{").unwrap();
        writeln!(out, "            let spill = self.spill.read().unwrap();").unwrap();
        writeln!(
            out,
            "            let file = spill.as_ref().expect(\"spill is closed\");"
        )
        .unwrap();
        writeln!(
            out,
            "            __ls_spill_read(file, slot.0, &mut bytes).expect(\"spill read failed\");"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        Self::__ls_decode(&bytes, key)").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_read_packed(&self, key: &{slot_key}, packed: u64) -> {row} {{"
        )
        .unwrap();
        writeln!(out, "        if packed == __LS_SLOT_LARGE {{").unwrap();
        writeln!(out, "            return self.__ls_read_slot(key, self.slots.large_get(key).expect(\"missing large slot\"));").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        self.__ls_read_slot(key, __ls_slot_unpack(packed))"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_append(&mut self, key: {key}, row: &{row}) -> io::Result<u64> {{"
        )
        .unwrap();
        writeln!(out, "        if self.direct {{").unwrap();
        writeln!(out, "            let bytes = Self::__ls_encode(row);").unwrap();
        writeln!(out, "            let length = bytes.len() as u64;").unwrap();
        writeln!(out, "            if self.arena.last().is_none_or(|chunk| chunk.len() + bytes.len() > __LS_ARENA_CHUNK) {{").unwrap();
        writeln!(out, "                self.arena.push(Vec::with_capacity(bytes.len().max(__LS_ARENA_CHUNK)));").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "            let chunk = self.arena.len() - 1;").unwrap();
        writeln!(out, "            let position = self.arena[chunk].len();").unwrap();
        writeln!(
            out,
            "            self.arena[chunk].extend_from_slice(&bytes);"
        )
        .unwrap();
        writeln!(
            out,
            "            let offset = ((chunk as u64) << 32) | position as u64;"
        )
        .unwrap();
        writeln!(
            out,
            "            self.slots.insert({}, offset, length);",
            storage_from_owned
        )
        .unwrap();
        writeln!(out, "            return Ok(length);").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        loop {{").unwrap();
        writeln!(
            out,
            "            let mut __ls_staging = self.staging.write().unwrap();"
        )
        .unwrap();
        writeln!(out, "            if __ls_staging.buf.is_empty() || __ls_staging.buf.len() < __LS_STAGING_MAX {{").unwrap();
        writeln!(
            out,
            "                let offset = __ls_staging.base + __ls_staging.buf.len() as u64;"
        )
        .unwrap();
        writeln!(
            out,
            "                let __ls_mark = __ls_staging.buf.len();"
        )
        .unwrap();
        writeln!(
            out,
            "                Self::__ls_encode_into(row, &mut __ls_staging.buf);"
        )
        .unwrap();
        writeln!(
            out,
            "                let length = (__ls_staging.buf.len() - __ls_mark) as u64;"
        )
        .unwrap();
        writeln!(out, "                drop(__ls_staging);").unwrap();
        writeln!(out, "                self.spill_len.fetch_add(length, std::sync::atomic::Ordering::Relaxed);").unwrap();
        writeln!(
            out,
            "                self.slots.insert({}, offset, length);",
            storage_from_owned
        )
        .unwrap();
        writeln!(out, "                return Ok(length);").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "            drop(__ls_staging);").unwrap();
        writeln!(out, "            self.__ls_staging_flush()?;").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "    fn __ls_staging_flush(&self) -> io::Result<()> {{").unwrap();
        writeln!(
            out,
            "        let mut __ls_staging = self.staging.write().unwrap();"
        )
        .unwrap();
        writeln!(
            out,
            "        if __ls_staging.buf.is_empty() {{ return Ok(()); }}"
        )
        .unwrap();
        writeln!(out, "        let base = __ls_staging.base;").unwrap();
        writeln!(out, "        let spill = self.spill.read().map_err(|_| io::Error::other(\"spill lock poisoned\"))?;").unwrap();
        writeln!(out, "        let file = spill.as_ref().ok_or_else(|| io::Error::other(\"spill is closed\"))?;").unwrap();
        writeln!(
            out,
            "        __ls_spill_write(file, base, &__ls_staging.buf)?;"
        )
        .unwrap();
        writeln!(out, "        drop(spill);").unwrap();
        writeln!(
            out,
            "        __ls_staging.base += __ls_staging.buf.len() as u64;"
        )
        .unwrap();
        writeln!(out, "        __ls_staging.buf.clear();").unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    fn __ls_vacuum(&mut self) -> io::Result<()> {{").unwrap();
        writeln!(out, "        if self.direct {{ return Ok(()); }}").unwrap();
        writeln!(out, "        self.__ls_staging_flush()?;").unwrap();
        writeln!(
            out,
            "        let __ls_dead = self.dead_rows.load(std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "        if __ls_dead == 0 {{ return Ok(()); }}").unwrap();
        writeln!(
            out,
            "        let __ls_live_count = (self.slots.len() + self.overflow.len()) as u64;"
        )
        .unwrap();
        writeln!(out, "        if __ls_dead < 64 && __ls_dead.saturating_mul(16) < __ls_live_count.max(1) {{ return Ok(()); }}").unwrap();
        writeln!(
            out,
            "        if self.slots.len() == 0 && self.overflow.is_empty() {{"
        )
        .unwrap();
        writeln!(out, "            if let Ok(spill) = self.spill.read() {{").unwrap();
        writeln!(
            out,
            "                if let Some(file) = spill.as_ref() {{ let _ = file.set_len(0); }}"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            self.spill_len.store(0, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "            if let Ok(mut staging) = self.staging.write() {{ staging.base = 0; staging.buf.clear(); }}").unwrap();
        writeln!(
            out,
            "            self.dead_rows.store(0, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "            return Ok(());").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        let mut __ls_live: Vec<({storage_key}, Vec<u8>)> = Vec::new();"
        )
        .unwrap();
        writeln!(out, "        __ls_live.reserve(self.slots.len());").unwrap();
        writeln!(
            out,
            "        for (__ls_key, __ls_packed) in self.slots.iter() {{"
        )
        .unwrap();
        writeln!(
            out,
            "            let __ls_row = self.__ls_read_packed(&__ls_key, __ls_packed);"
        )
        .unwrap();
        writeln!(
            out,
            "            __ls_live.push((__ls_key, Self::__ls_encode(&__ls_row)));"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        let mut spill = self.spill.write().map_err(|_| io::Error::other(\"spill lock poisoned\"))?;").unwrap();
        writeln!(out, "        let file = spill.as_mut().ok_or_else(|| io::Error::other(\"spill is closed\"))?;").unwrap();
        writeln!(out, "        file.set_len(0)?;").unwrap();
        writeln!(out, "        let mut __ls_offset = 0u64;").unwrap();
        writeln!(out, "        for (__ls_key, __ls_bytes) in &__ls_live {{").unwrap();
        writeln!(
            out,
            "            __ls_spill_write(file, __ls_offset, __ls_bytes)?;"
        )
        .unwrap();
        writeln!(out, "            self.slots.insert(__ls_key.clone(), __ls_offset, __ls_bytes.len() as u64);").unwrap();
        writeln!(out, "            __ls_offset += __ls_bytes.len() as u64;").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        self.spill_len.store(__ls_offset, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "        if let Ok(mut staging) = self.staging.write() {{ staging.base = __ls_offset; staging.buf.clear(); }}").unwrap();
        writeln!(
            out,
            "        self.dead_rows.store(0, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_append_loaded(&mut self, key: {key}, row: {row}) -> io::Result<()> {{"
        )
        .unwrap();
        writeln!(out, "        self.__ls_append(key, &row)?;").unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(
            out,
            "    fn __ls_clone_from(&mut self, source: &{store}) -> io::Result<()> {{"
        )
        .unwrap();
        writeln!(out, "        if source.direct || self.direct {{").unwrap();
        writeln!(out, "            for (key, row) in source.iter() {{").unwrap();
        writeln!(out, "                self.__ls_append_loaded(key, row)?;").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "            return Ok(());").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        source.__ls_staging_flush()?;").unwrap();
        writeln!(out, "        self.__ls_staging_flush()?;").unwrap();
        writeln!(out, "        {{").unwrap();
        writeln!(
            out,
            "            let source_spill = source.spill.read().map_err(|_| io::Error::other(\"spill lock poisoned\"))?;"
        )
        .unwrap();
        writeln!(out, "            let source_file = source_spill.as_ref().ok_or_else(|| io::Error::other(\"spill is closed\"))?;").unwrap();
        writeln!(out, "            let source_len = source.spill_len.load(std::sync::atomic::Ordering::Relaxed);").unwrap();
        writeln!(
            out,
            "            let mut spill = self.spill.write().map_err(|_| io::Error::other(\"spill lock poisoned\"))?;"
        )
        .unwrap();
        writeln!(out, "            let file = spill.as_mut().ok_or_else(|| io::Error::other(\"spill is closed\"))?;").unwrap();
        writeln!(out, "            file.set_len(0)?;").unwrap();
        writeln!(out, "            let mut __ls_copied = 0u64;").unwrap();
        writeln!(out, "            let mut __ls_buf = vec![0u8; 256 * 1024];").unwrap();
        writeln!(out, "            while __ls_copied < source_len {{").unwrap();
        writeln!(out, "                let __ls_want = ((source_len - __ls_copied).min(__ls_buf.len() as u64)) as usize;").unwrap();
        writeln!(out, "                __ls_spill_read(source_file, __ls_copied, &mut __ls_buf[..__ls_want])?;").unwrap();
        writeln!(
            out,
            "                __ls_spill_write(file, __ls_copied, &__ls_buf[..__ls_want])?;"
        )
        .unwrap();
        writeln!(out, "                __ls_copied += __ls_want as u64;").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            self.spill_len.store(source_len, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        self.slots = source.slots.clone();").unwrap();
        writeln!(out, "        self.overflow = source.overflow.clone();").unwrap();
        writeln!(out, "        Ok(())").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    const __LS_CACHE_SHARDS: usize = 16;").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "    fn __ls_cache_shard(key: &{storage_key}) -> usize {{"
        )
        .unwrap();
        writeln!(
            out,
            "        __ls_shard16(key) & (Self::__LS_CACHE_SHARDS - 1)"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "    fn __ls_cache_put(&self, key: {key}, row: {row}, size: u64) {{"
        )
        .unwrap();
        writeln!(out, "        if self.budget == 0 {{ return; }}").unwrap();
        writeln!(out, "        let __ls_key = {storage_from_owned};").unwrap();
        writeln!(
            out,
            "        let __ls_shard = Self::__ls_cache_shard(&__ls_key);"
        )
        .unwrap();
        writeln!(
            out,
            "        let mut cache = self.cache[__ls_shard].write().unwrap();"
        )
        .unwrap();
        writeln!(out, "        let stamp = self.stamp.fetch_add(1, std::sync::atomic::Ordering::Relaxed).wrapping_add(1);").unwrap();
        writeln!(
            out,
            "        if let Some((_, previous, _)) = cache.insert(__ls_key.clone(), (row, size, stamp)) {{"
        )
        .unwrap();
        writeln!(
            out,
            "            let _ = self.cached_bytes.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(previous)));"
        )
        .unwrap();
        writeln!(
            out,
            "            let _ = self.shard_bytes[__ls_shard].fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(previous)));"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        self.cached_bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(
            out,
            "        self.shard_bytes[__ls_shard].fetch_add(size, std::sync::atomic::Ordering::Relaxed);"
        )
        .unwrap();
        writeln!(
            out,
            "        let __ls_shard_budget = (self.budget / 16).max(1);"
        )
        .unwrap();
        writeln!(out, "        drop(cache);").unwrap();
        writeln!(
            out,
            "        let order_len = {{ let mut order = self.order[__ls_shard].lock().unwrap(); order.push_back((__ls_key, stamp)); order.len() }};"
        )
        .unwrap();
        writeln!(out, "        if order_len > 4096 {{").unwrap();
        writeln!(
            out,
            "            let cache_len = self.cache[__ls_shard].read().unwrap().len();"
        )
        .unwrap();
        writeln!(
            out,
            "            if order_len > cache_len.saturating_mul(4).saturating_add(64) {{"
        )
        .unwrap();
        writeln!(
            out,
            "            let mut live: Vec<(u64, {storage_key})> = self.cache[__ls_shard].read().unwrap().iter().map(|(key, (_, _, stamp))| (*stamp, key.clone())).collect();"
        )
        .unwrap();
        writeln!(out, "            live.sort_by_key(|(stamp, _)| *stamp);").unwrap();
        writeln!(
            out,
            "            let mut order = self.order[__ls_shard].lock().unwrap();"
        )
        .unwrap();
        writeln!(out, "            order.clear();").unwrap();
        writeln!(
            out,
            "            for (stamp, key) in live {{ order.push_back((key, stamp)); }}"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        let mut __ls_evictions = 0u32;").unwrap();
        writeln!(
            out,
            "        while self.shard_bytes[__ls_shard].load(std::sync::atomic::Ordering::Relaxed) > __ls_shard_budget {{"
        )
        .unwrap();
        writeln!(out, "            __ls_evictions += 1;").unwrap();
        writeln!(out, "            if __ls_evictions > 64 {{ break; }}").unwrap();
        writeln!(out, "            let Some((candidate, candidate_stamp)) = self.order[__ls_shard].lock().unwrap().pop_front() else {{ break; }};").unwrap();
        writeln!(out, "            let removed = {{").unwrap();
        writeln!(
            out,
            "                let mut cache = self.cache[__ls_shard].write().unwrap();"
        )
        .unwrap();
        writeln!(out, "                match cache.get(&candidate) {{").unwrap();
        writeln!(
            out,
            "                    Some((_, size, stamp)) if *stamp == candidate_stamp => {{"
        )
        .unwrap();
        writeln!(out, "                        let size = *size;").unwrap();
        writeln!(out, "                        cache.remove(&candidate);").unwrap();
        writeln!(out, "                        Some(size)").unwrap();
        writeln!(out, "                    }}").unwrap();
        writeln!(out, "                    _ => None,").unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(out, "            }};").unwrap();
        writeln!(out, "            if let Some(size) = removed {{").unwrap();
        writeln!(
            out,
            "                let _ = self.cached_bytes.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(size)));"
        )
        .unwrap();
        writeln!(
            out,
            "                let _ = self.shard_bytes[__ls_shard].fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(size)));"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(
            out,
            "    fn __ls_get(&self, key: &{key}) -> Option<{row}> {{"
        )
        .unwrap();
        writeln!(out, "        let __ls_key = {storage_from_ref};").unwrap();
        writeln!(
            out,
            "        let __ls_shard = Self::__ls_cache_shard(&__ls_key);"
        )
        .unwrap();
        if is_ttl {
            writeln!(out, "        if let Some((row, _, _)) = self.cache[__ls_shard].read().unwrap().get(&__ls_key) {{").unwrap();
            writeln!(out, "            if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        if !self.overflow.is_empty() {{").unwrap();
            writeln!(
                out,
                "        if let Some(row) = self.overflow.get(&__ls_key) {{"
            )
            .unwrap();
            writeln!(out, "            if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        let slot = {slot_get};").unwrap();
            writeln!(
                out,
                "        let row = self.__ls_read_slot(&__ls_key, slot);"
            )
            .unwrap();
            writeln!(out, "        if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
            writeln!(out, "        self.__ls_cache_put(key.clone(), row.clone(), slot.1.saturating_add(128));").unwrap();
            writeln!(out, "        Some(row)").unwrap();
        } else {
            writeln!(out, "        if let Some((row, _, _)) = self.cache[__ls_shard].read().unwrap().get(&__ls_key) {{").unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        if !self.overflow.is_empty() {{").unwrap();
            writeln!(
                out,
                "        if let Some(row) = self.overflow.get(&__ls_key) {{"
            )
            .unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        let slot = {slot_get};").unwrap();
            writeln!(
                out,
                "        let row = self.__ls_read_slot(&__ls_key, slot);"
            )
            .unwrap();
            writeln!(out, "        self.__ls_cache_put(key.clone(), row.clone(), slot.1.saturating_add(128));").unwrap();
            writeln!(out, "        Some(row)").unwrap();
        }
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        if is_ttl {
            writeln!(
                out,
                "    fn __ls_get_including_expired(&self, key: &{key}) -> Option<{row}> {{"
            )
            .unwrap();
            writeln!(out, "        let __ls_key = {storage_from_ref};").unwrap();
            writeln!(
                out,
                "        let __ls_shard = Self::__ls_cache_shard(&__ls_key);"
            )
            .unwrap();
            writeln!(out, "        if let Some((row, _, _)) = self.cache[__ls_shard].read().unwrap().get(&__ls_key) {{").unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        if !self.overflow.is_empty() {{").unwrap();
            writeln!(
                out,
                "        if let Some(row) = self.overflow.get(&__ls_key) {{"
            )
            .unwrap();
            writeln!(out, "            return Some(row.clone());").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        let slot = {slot_get};").unwrap();
            writeln!(
                out,
                "        let row = self.__ls_read_slot(&__ls_key, slot);"
            )
            .unwrap();
            writeln!(out, "        self.__ls_cache_put(key.clone(), row.clone(), slot.1.saturating_add(128));").unwrap();
            writeln!(out, "        Some(row)").unwrap();
            writeln!(out, "    }}").unwrap();
            writeln!(out).unwrap();
        }

        if primary_key.ty == Ty::Text {
            writeln!(
                out,
                "    fn __ls_get_str(&self, key: &str) -> Option<{row}> {{"
            )
            .unwrap();
            writeln!(out, "        let __ls_key = __LsText::from(key);").unwrap();
            writeln!(out, "        if !self.direct {{").unwrap();
            if is_ttl {
                writeln!(out, "            if let Some((row, _, _)) = self.cache[Self::__ls_cache_shard(&__ls_key)].read().unwrap().get(key) {{").unwrap();
                writeln!(out, "                if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
                writeln!(out, "                return Some(row.clone());").unwrap();
                writeln!(out, "            }}").unwrap();
            } else {
                writeln!(out, "            if let Some((row, _, _)) = self.cache[Self::__ls_cache_shard(&__ls_key)].read().unwrap().get(key) {{").unwrap();
                writeln!(out, "                return Some(row.clone());").unwrap();
                writeln!(out, "            }}").unwrap();
            }
            writeln!(out, "        }}").unwrap();
            if is_ttl {
                writeln!(out, "        if !self.overflow.is_empty() {{").unwrap();
                writeln!(
                    out,
                    "        if let Some(row) = self.overflow.get(&__ls_key) {{"
                )
                .unwrap();
                writeln!(out, "            if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
                writeln!(out, "            return Some(row.clone());").unwrap();
                writeln!(out, "        }}").unwrap();
                writeln!(out, "        }}").unwrap();
                writeln!(out, "        let slot = self.slots.get(&__ls_key)?;").unwrap();
                writeln!(
                    out,
                    "        let row = self.__ls_read_slot(&__ls_key, slot);"
                )
                .unwrap();
                writeln!(out, "        if row.expires_at != 0 && row.expires_at <= __ls_now_millis() as i64 {{ return None; }}").unwrap();
                writeln!(out, "        self.__ls_cache_put(key.to_string(), row.clone(), slot.1.saturating_add(128));").unwrap();
                writeln!(out, "        Some(row)").unwrap();
            } else {
                writeln!(out, "        if !self.overflow.is_empty() {{").unwrap();
                writeln!(
                    out,
                    "        if let Some(row) = self.overflow.get(&__ls_key) {{"
                )
                .unwrap();
                writeln!(out, "            return Some(row.clone());").unwrap();
                writeln!(out, "        }}").unwrap();
                writeln!(out, "        }}").unwrap();
                writeln!(out, "        let slot = self.slots.get(&__ls_key)?;").unwrap();
                writeln!(
                    out,
                    "        let row = self.__ls_read_slot(&__ls_key, slot);"
                )
                .unwrap();
                writeln!(out, "        self.__ls_cache_put(key.to_string(), row.clone(), slot.1.saturating_add(128));").unwrap();
                writeln!(out, "        Some(row)").unwrap();
            }
            writeln!(out, "    }}").unwrap();
            writeln!(out).unwrap();
        }

        writeln!(
            out,
            "    fn __ls_insert(&mut self, key: {key}, row: {row}) {{"
        )
        .unwrap();
        writeln!(out, "        let __ls_key = {storage_from_owned};").unwrap();
        writeln!(out, "        match self.__ls_append(key.clone(), &row) {{").unwrap();
        writeln!(out, "            Ok(size) => {{").unwrap();
        writeln!(out, "                self.overflow.remove(&__ls_key);").unwrap();
        writeln!(
            out,
            "                self.__ls_cache_put(key, row, size.saturating_add(128));"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "            Err(_) => {{").unwrap();
        writeln!(out, "                self.slots.remove(&__ls_key);").unwrap();
        writeln!(out, "                let __ls_shard = Self::__ls_cache_shard(&__ls_key);
                let removed = self.cache[__ls_shard].write().unwrap().remove(&__ls_key).map(|(_, size, _)| size).unwrap_or(0);").unwrap();
        writeln!(out, "                let _ = self.cached_bytes.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(removed)));").unwrap();
        writeln!(out, "                let _ = self.shard_bytes[__ls_shard].fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(removed)));").unwrap();
        writeln!(out, "                self.overflow.insert(__ls_key, row);").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(
            out,
            "    fn __ls_remove(&mut self, key: &{key}) -> Option<{row}> {{"
        )
        .unwrap();
        writeln!(out, "        let __ls_key = {storage_from_ref};").unwrap();
        if is_ttl {
            writeln!(
                out,
                "        let row = self.__ls_get_including_expired(key)?;"
            )
            .unwrap();
        } else {
            writeln!(out, "        let row = self.__ls_get(key)?;").unwrap();
        }
        writeln!(out, "        self.slots.remove(&__ls_key);").unwrap();
        writeln!(out, "        self.overflow.remove(&__ls_key);").unwrap();
        writeln!(
            out,
            "        let __ls_shard = Self::__ls_cache_shard(&__ls_key);"
        )
        .unwrap();
        writeln!(out, "        let removed = self.cache[__ls_shard].write().unwrap().remove(&__ls_key).map(|(_, size, _)| size).unwrap_or(0);").unwrap();
        writeln!(
            out,
            "        let _ = self.cached_bytes.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(removed)));"
        )
        .unwrap();
        writeln!(
            out,
            "        let _ = self.shard_bytes[__ls_shard].fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |__ls_bytes| Some(__ls_bytes.saturating_sub(removed)));"
        )
        .unwrap();
        writeln!(out, "        if !self.direct {{ self.dead_rows.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }}").unwrap();
        writeln!(out, "        Some(row)").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(
            out,
            "    fn get(&self, key: &{key}) -> Option<{row}> {{ self.__ls_get(key) }}"
        )
        .unwrap();
        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(
            out,
            "    fn remove(&mut self, key: &{key}) -> Option<{row}> {{ self.__ls_remove(key) }}"
        )
        .unwrap();
        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(out, "    fn contains_key(&self, key: &{key}) -> bool {{").unwrap();
        if is_ttl {
            writeln!(out, "        self.__ls_get(key).is_some()").unwrap();
        } else {
            writeln!(out, "        let __ls_key = {storage_from_ref};").unwrap();
            writeln!(
                out,
                "        {slot_contains} || self.overflow.contains_key(&__ls_key)"
            )
            .unwrap();
        }
        writeln!(out, "    }}").unwrap();
        writeln!(
            out,
            "    fn len(&self) -> usize {{ self.slots.len() + self.overflow.len() }}"
        )
        .unwrap();
        writeln!(out, "    fn keys(&self) -> std::vec::IntoIter<{key}> {{").unwrap();
        writeln!(out, "        let mut keys: Vec<{key}> = self.slots.keys().map(|key| {storage_to_raw}).collect();").unwrap();
        writeln!(
            out,
            "        keys.extend(self.overflow.keys().map(|key| {storage_to_raw}));"
        )
        .unwrap();
        writeln!(out, "        keys.into_iter()").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    fn values(&self) -> {values}<'_> {{ {values} {{ slots: self.slots.iter(), table: self, overflow: self.overflow.values() }} }}").unwrap();
        writeln!(
            out,
            "    fn for_each_value(&self, mut visit: impl FnMut(&{row})) {{"
        )
        .unwrap();
        writeln!(out, "        if !self.direct {{").unwrap();
        writeln!(out, "            let mut __ls_cached = 0usize;").unwrap();
        writeln!(out, "            for __ls_shard in self.cache.iter() {{").unwrap();
        writeln!(
            out,
            "                __ls_cached += __ls_shard.read().unwrap().len();"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            if __ls_cached == self.slots.len() + self.overflow.len() {{"
        )
        .unwrap();
        writeln!(
            out,
            "                for __ls_shard in self.cache.iter() {{"
        )
        .unwrap();
        writeln!(
            out,
            "                    for (row, _, _) in __ls_shard.read().unwrap().values() {{ visit(row); }}"
        )
        .unwrap();
        writeln!(out, "                }}").unwrap();
        writeln!(out, "                return;").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        for (key, packed) in self.slots.iter() {{").unwrap();
        writeln!(out, "            if !self.direct {{").unwrap();
        writeln!(
            out,
            "                if let Some((row, _, _)) = self.cache[Self::__ls_cache_shard(&key)].read().unwrap().get(&key) {{ visit(row); continue; }}"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            let row = self.__ls_read_packed(&key, packed);"
        )
        .unwrap();
        writeln!(out, "            visit(&row);").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(
            out,
            "        for row in self.overflow.values() {{ visit(row); }}"
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    fn iter(&self) -> {iter}<'_> {{ {iter} {{ slots: self.slots.iter(), table: self, overflow: self.overflow.iter() }} }}").unwrap();
        writeln!(out, "    #[allow(clippy::ptr_arg)]").unwrap();
        writeln!(
            out,
            "    fn values_from(&self, start: &{key}) -> impl Iterator<Item = {row}> + '_ {{"
        )
        .unwrap();
        writeln!(out, "        let __ls_start = {storage_from_start};").unwrap();
        if is_ttl {
            writeln!(out, "        let __ls_now = __ls_now_millis() as i64;").unwrap();
            writeln!(out, "        let mut __ls_overflow: Vec<({storage_key}, &{row})> = self.overflow.iter().filter(|(key, row)| *key >= &__ls_start && (row.expires_at == 0 || row.expires_at > __ls_now)).map(|(key, row)| (key.clone(), row)).collect();").unwrap();
        } else {
            writeln!(out, "        let mut __ls_overflow: Vec<({storage_key}, &{row})> = self.overflow.iter().filter(|(key, _)| *key >= &__ls_start).map(|(key, row)| (key.clone(), row)).collect();").unwrap();
        }
        writeln!(
            out,
            "        __ls_overflow.sort_by(|left, right| left.0.cmp(&right.0));"
        )
        .unwrap();
        if is_ttl {
            writeln!(
                out,
                "        self.slots.iter_from(&__ls_start).filter_map(move |(key, packed)| {{"
            )
            .unwrap();
            writeln!(out, "            if !self.direct {{").unwrap();
            writeln!(out, "                if let Some((row, _, _)) = self.cache[Self::__ls_cache_shard(&key)].read().unwrap().get(&key) {{").unwrap();
            writeln!(out, "                    if row.expires_at == 0 || row.expires_at > __ls_now {{ return Some(row.clone()); }} else {{ return None; }}").unwrap();
            writeln!(out, "                }}").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(
                out,
                "            let __ls_row = self.__ls_read_packed(&key, packed);"
            )
            .unwrap();
            writeln!(out, "            if __ls_row.expires_at == 0 || __ls_row.expires_at > __ls_now {{ Some(__ls_row) }} else {{ None }}").unwrap();
            writeln!(
                out,
                "        }}).chain(__ls_overflow.into_iter().map(|(_, row)| row.clone()))"
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "        self.slots.iter_from(&__ls_start).map(move |(key, packed)| {{"
            )
            .unwrap();
            writeln!(out, "            if !self.direct {{").unwrap();
            writeln!(out, "                if let Some((row, _, _)) = self.cache[Self::__ls_cache_shard(&key)].read().unwrap().get(&key) {{").unwrap();
            writeln!(out, "                    return row.clone();").unwrap();
            writeln!(out, "                }}").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "            self.__ls_read_packed(&key, packed)").unwrap();
            writeln!(
                out,
                "        }}).chain(__ls_overflow.into_iter().map(|(_, row)| row.clone()))"
            )
            .unwrap();
        }
        writeln!(out, "    }}").unwrap();
        writeln!(
            out,
            "    fn __ls_cached_bytes(&self) -> u64 {{ self.cached_bytes.load(std::sync::atomic::Ordering::Relaxed) }}"
        )
        .unwrap();
        writeln!(
            out,
            "    fn __ls_cached_rows(&self) -> usize {{ self.cache.iter().map(|__ls_shard| __ls_shard.read().unwrap().len()).sum() }}"
        )
        .unwrap();
        writeln!(out, "    fn __ls_spill_bytes(&self) -> u64 {{ self.spill_len.load(std::sync::atomic::Ordering::Relaxed) }}").unwrap();
        if is_ttl {
            writeln!(
                out,
                "    fn __ls_expired_keys(&self, __ls_now: u64) -> Vec<{key}> {{"
            )
            .unwrap();
            writeln!(out, "        let __ls_cutoff = __ls_now as i64;").unwrap();
            writeln!(out, "        let mut __ls_out: Vec<{key}> = Vec::new();").unwrap();
            writeln!(out, "        for (key, packed) in self.slots.iter() {{").unwrap();
            writeln!(
                out,
                "            let __ls_row = self.__ls_read_packed(&key, packed);"
            )
            .unwrap();
            writeln!(
                out,
                "            if __ls_row.expires_at != 0 && __ls_row.expires_at <= __ls_cutoff {{"
            )
            .unwrap();
            writeln!(out, "                let key = {storage_to_raw};").unwrap();
            writeln!(out, "                __ls_out.push(key);").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        for (key, row) in self.overflow.iter() {{").unwrap();
            writeln!(
                out,
                "            if row.expires_at != 0 && row.expires_at <= __ls_cutoff {{"
            )
            .unwrap();
            writeln!(out, "                let key = {storage_to_raw};").unwrap();
            writeln!(out, "                __ls_out.push(key);").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        __ls_out").unwrap();
            writeln!(out, "    }}").unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "impl Drop for {store} {{").unwrap();
        writeln!(out, "    fn drop(&mut self) {{").unwrap();
        writeln!(out, "        self.spill.write().unwrap().take();").unwrap();
        writeln!(out, "        if !self.spill_path.as_os_str().is_empty() {{").unwrap();
        writeln!(
            out,
            "            let _ = fs::remove_file(&self.spill_path);"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "struct {iter}<'a> {{").unwrap();
        writeln!(out, "    slots: {slot_iter_type},").unwrap();
        writeln!(out, "    table: &'a {store},").unwrap();
        writeln!(
            out,
            "    overflow: std::collections::hash_map::Iter<'a, {storage_key}, {row}>,"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "impl<'a> Iterator for {iter}<'a> {{").unwrap();
        writeln!(out, "    type Item = ({key}, {row});").unwrap();
        writeln!(out, "    fn next(&mut self) -> Option<Self::Item> {{").unwrap();
        writeln!(out, "        match self.slots.next() {{").unwrap();
        writeln!(out, "            Some((key, packed)) => {{").unwrap();
        writeln!(out, "                let __ls_raw = {storage_to_raw};").unwrap();
        writeln!(
            out,
            "                let __ls_row = if !self.table.direct {{"
        )
        .unwrap();
        writeln!(out, "                    self.table.cache[{store}::__ls_cache_shard(&key)].read().unwrap().get(&key).map(|(row, _, _)| row.clone())").unwrap();
        writeln!(out, "                }} else {{").unwrap();
        writeln!(out, "                    None").unwrap();
        writeln!(out, "                }};").unwrap();
        writeln!(out, "                Some((__ls_raw, __ls_row.unwrap_or_else(|| self.table.__ls_read_packed(&key, packed))))").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "            None => self.overflow.next().map(|(key, row)| ({storage_to_raw}, row.clone())),").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "impl<'a> IntoIterator for &'a {store} {{").unwrap();
        writeln!(out, "    type Item = ({key}, {row});").unwrap();
        writeln!(out, "    type IntoIter = {iter}<'a>;").unwrap();
        writeln!(
            out,
            "    fn into_iter(self) -> Self::IntoIter {{ self.iter() }}"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();

        writeln!(out, "struct {values}<'a> {{").unwrap();
        writeln!(out, "    slots: {slot_iter_type},").unwrap();
        writeln!(out, "    table: &'a {store},").unwrap();
        writeln!(
            out,
            "    overflow: std::collections::hash_map::Values<'a, {storage_key}, {row}>,"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out, "impl<'a> Iterator for {values}<'a> {{").unwrap();
        writeln!(out, "    type Item = {row};").unwrap();
        writeln!(out, "    fn next(&mut self) -> Option<Self::Item> {{").unwrap();
        if is_ttl {
            writeln!(out, "        let __ls_now = __ls_now_millis() as i64;").unwrap();
            writeln!(out, "        loop {{").unwrap();
            writeln!(out, "            match self.slots.next() {{").unwrap();
            writeln!(out, "                Some((key, packed)) => {{").unwrap();
            writeln!(
                out,
                "                    let __ls_cached = if !self.table.direct {{"
            )
            .unwrap();
            writeln!(out, "                        self.table.cache[{store}::__ls_cache_shard(&key)].read().unwrap().get(&key).map(|(row, _, _)| row.clone())").unwrap();
            writeln!(out, "                    }} else {{ None }};").unwrap();
            writeln!(out, "                    let __ls_row = __ls_cached.unwrap_or_else(|| self.table.__ls_read_packed(&key, packed));").unwrap();
            writeln!(out, "                    if __ls_row.expires_at != 0 && __ls_row.expires_at <= __ls_now {{ continue; }}").unwrap();
            writeln!(out, "                    return Some(__ls_row);").unwrap();
            writeln!(out, "                }}").unwrap();
            writeln!(out, "                None => break,").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "        loop {{").unwrap();
            writeln!(out, "            match self.overflow.next() {{").unwrap();
            writeln!(out, "                Some(row) => {{").unwrap();
            writeln!(out, "                    if row.expires_at != 0 && row.expires_at <= __ls_now {{ continue; }}").unwrap();
            writeln!(out, "                    return Some(row.clone());").unwrap();
            writeln!(out, "                }}").unwrap();
            writeln!(out, "                None => return None,").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }}").unwrap();
        } else {
            writeln!(out, "        match self.slots.next() {{").unwrap();
            writeln!(out, "            Some((key, packed)) => {{").unwrap();
            writeln!(
                out,
                "                let __ls_row = if !self.table.direct {{"
            )
            .unwrap();
            writeln!(out, "                    self.table.cache[{store}::__ls_cache_shard(&key)].read().unwrap().get(&key).map(|(row, _, _)| row.clone())").unwrap();
            writeln!(out, "                }} else {{ None }};").unwrap();
            writeln!(out, "                Some(__ls_row.unwrap_or_else(|| self.table.__ls_read_packed(&key, packed)))").unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "            None => self.overflow.next().cloned(),").unwrap();
            writeln!(out, "        }}").unwrap();
        }
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
    Ok(())
}

pub(crate) fn emit_key_struct(out: &mut String, name: &str, fields: &[KeyField], prefix: &str) {
    writeln!(out, "#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]").unwrap();
    if fields.is_empty() {
        writeln!(out, "struct {name};").unwrap();
    } else {
        writeln!(out, "struct {name} {{").unwrap();
        for (index, field) in fields.iter().enumerate() {
            let base = key_rust(field.ty);
            let ty = if field.reverse {
                format!("Reverse<{base}>")
            } else {
                base
            };
            writeln!(out, "    {prefix}{index}: {ty},").unwrap();
        }
        writeln!(out, "}}").unwrap();
    }
    writeln!(out).unwrap();
}

pub(crate) fn row_eq_ctor(name: &str, eq: &[String], row: &str) -> String {
    if eq.is_empty() {
        return name.to_string();
    }
    let fields = eq
        .iter()
        .enumerate()
        .map(|(index, field)| format!("e{index}: {row}.{field}.clone().into()"))
        .collect::<Vec<_>>();
    format!("{name} {{ {} }}", fields.join(", "))
}

pub(crate) fn row_key_ctor(name: &str, fields: &[KeyField], row: &str) -> String {
    let fields = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value = format!("{row}.{}.clone().into()", field.name);
            let value = if field.reverse {
                format!("Reverse({value})")
            } else {
                value
            };
            format!("f{index}: {value}")
        })
        .collect::<Vec<_>>();
    format!("{name} {{ {} }}", fields.join(", "))
}
