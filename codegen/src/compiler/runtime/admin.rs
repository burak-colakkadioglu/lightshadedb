pub(crate) const ADMIN_RUNTIME: &str = r#"fn __ls_admin_absolute(path: &Path) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    Ok(fs::canonicalize(parent)?.join(name))
}

fn __ls_admin_alias(source: &Path, destination: &Path) -> io::Result<bool> {
    let destination = __ls_admin_absolute(destination)?;
    for candidate in [
        source.to_path_buf(),
        __ls_sidecar(source, ".wal"),
        __ls_sidecar(source, ".lock"),
    ] {
        if __ls_admin_absolute(&candidate).is_ok_and(|candidate| candidate == destination) {
            return Ok(true);
        }
    }
    Ok(false)
}
"#;
