//! Minimal flat-key INI read/modify/write for `.npmrc` and `auth.ini`.
//!
//! `pnpm config` reads and writes these files through the `ini` npm package
//! (`read-ini-file` / `write-ini-file`). The config keys it touches are flat
//! `key=value` pairs (`registry`, `@scope:registry`, `//host/:_authToken`,
//! `cafile`, ...) — no sections, no arrays — so this reproduces just that
//! subset: a missing file reads as empty, comments (`;` / `#`) and blank lines
//! are ignored, and the map round-trips as `key=value` lines.

use indexmap::IndexMap;
use std::{
    fs,
    io::{self, Write as _},
    path::Path,
};

/// Read `path` into an ordered key→value map. A missing file is an empty map;
/// any other read error propagates.
pub fn read(path: &Path) -> io::Result<IndexMap<String, String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(IndexMap::new()),
        Err(err) => Err(err),
    }
}

fn parse(text: &str) -> IndexMap<String, String> {
    let mut map = IndexMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

/// Write `settings` to `path` as `key=value` lines, creating parent
/// directories and replacing the file atomically (temp file + rename).
pub fn write(path: &Path, settings: &IndexMap<String, String>) -> io::Result<()> {
    let mut contents = String::new();
    for (key, value) in settings {
        contents.push_str(key);
        contents.push('=');
        contents.push_str(value);
        contents.push('\n');
    }
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    // `NamedTempFile` creates with mode 0600 on Unix; persisting it over an
    // existing file would silently tighten that file's permissions. Carry the
    // target's existing mode across the rename so rewriting a project `.npmrc`
    // preserves it (pnpm's `write-ini-file` keeps the target's mode too). New
    // files keep the conservative 0600 default — they may hold credentials.
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode();
        tmp.as_file().set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}
