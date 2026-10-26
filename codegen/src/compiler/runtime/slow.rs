pub(crate) const SLOW_RUNTIME: &str = r#"struct __LsSlowSample {
    __ls_op: &'static str,
    __ls_strategy: &'static str,
    __ls_start: std::time::Instant,
    __ls_rows: std::cell::Cell<std::option::Option<u64>>,
}

impl __LsSlowSample {
    fn __ls_new(__ls_op: &'static str, __ls_strategy: &'static str) -> Self {
        Self {
            __ls_op,
            __ls_strategy,
            __ls_start: std::time::Instant::now(),
            __ls_rows: std::cell::Cell::new(None),
        }
    }

    fn __ls_set_rows(&self, __ls_rows: u64) {
        self.__ls_rows.set(std::option::Option::Some(__ls_rows));
    }
}

fn __ls_slow_config() -> std::option::Option<&'static (std::path::PathBuf, u64)> {
    static __LS_SLOW: std::sync::OnceLock<std::option::Option<(std::path::PathBuf, u64)>> =
        std::sync::OnceLock::new();
    __LS_SLOW
        .get_or_init(|| {
            let __ls_path = match std::env::var_os("LIGHTSHADE_SLOW_LOG") {
                std::option::Option::Some(__ls_path) => std::path::PathBuf::from(__ls_path),
                std::option::Option::None => return None,
            };
            let __ls_threshold = std::env::var("LIGHTSHADE_SLOW_MS")
                .ok()
                .and_then(|__ls_value| __ls_value.parse::<u64>().ok())
                .unwrap_or(100);
            std::option::Option::Some((__ls_path, __ls_threshold))
        })
        .as_ref()
}

fn __ls_slow_escape(__ls_text: &str) -> String {
    use std::fmt::Write as _;
    let mut __ls_out = String::with_capacity(__ls_text.len());
    for __ls_char in __ls_text.chars() {
        match __ls_char {
            '"' => __ls_out.push_str("\\\""),
            '\\' => __ls_out.push_str("\\\\"),
            __ls_char if (__ls_char as u32) < 0x20 => {
                let _ = write!(__ls_out, "\\u{:04x}", __ls_char as u32);
            }
            _ => __ls_out.push(__ls_char),
        }
    }
    __ls_out
}

impl Drop for __LsSlowSample {
    fn drop(&mut self) {
        let (__ls_path, __ls_threshold) = match __ls_slow_config() {
            std::option::Option::Some(__ls_config) => __ls_config,
            std::option::Option::None => return,
        };
        let __ls_elapsed = self.__ls_start.elapsed();
        if __ls_elapsed.as_millis() < *__ls_threshold as u128 {
            return;
        }
        let __ls_rows = match self.__ls_rows.get() {
            std::option::Option::Some(__ls_rows) => __ls_rows.to_string(),
            std::option::Option::None => "null".to_string(),
        };
        let __ls_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|__ls_age| __ls_age.as_millis())
            .unwrap_or(0);
        let mut __ls_line = String::new();
        __ls_line.push_str("{\"ts_ms\":");
        __ls_line.push_str(&__ls_timestamp.to_string());
        __ls_line.push_str(",\"op\":\"");
        __ls_line.push_str(&__ls_slow_escape(self.__ls_op));
        __ls_line.push_str("\",\"strategy\":\"");
        __ls_line.push_str(&__ls_slow_escape(self.__ls_strategy));
        __ls_line.push_str("\",\"elapsed_ms\":");
        __ls_line.push_str(&__ls_elapsed.as_millis().to_string());
        __ls_line.push_str(",\"rows\":");
        __ls_line.push_str(&__ls_rows);
        __ls_line.push_str("}\n");
        if let Ok(mut __ls_file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(__ls_path)
        {
            use std::io::Write as _;
            let _ = __ls_file.write_all(__ls_line.as_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(
                    __ls_path,
                    std::fs::Permissions::from_mode(__LS_FILE_MODE),
                );
            }
        }
    }
}
"#;
