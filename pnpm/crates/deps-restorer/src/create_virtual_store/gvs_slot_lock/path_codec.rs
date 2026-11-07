use base64::Engine as _;
use std::{fs, io, path::PathBuf};

pub const GVS_LOCKED_SLOT_PATHS_ENV: &str = "PNPM_GVS_LOCKED_SLOT_PATHS";

pub(super) fn parse_slot_lock_paths(value: &str) -> io::Result<Vec<PathBuf>> {
    let encoded = serde_json::from_str::<Vec<String>>(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    encoded
        .into_iter()
        .map(decode_slot_path)
        .collect()
}

pub(crate) fn inherited_slot_locks() -> io::Result<Vec<PathBuf>> {
    let Some(value) = std::env::var_os(GVS_LOCKED_SLOT_PATHS_ENV) else {
        return Ok(Vec::new());
    };
    let value = value
        .into_string()
        .map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "slot lock paths are not valid UTF-8")
        })?;
    parse_slot_lock_paths(&value)
}

pub(crate) fn encode_slot_lock_paths(paths: &[PathBuf]) -> io::Result<String> {
    let encoded = paths
        .iter()
        .map(|path| encode_slot_path(path))
        .collect::<io::Result<Vec<_>>>()?;
    serde_json::to_string(&encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(unix)]
fn encode_slot_path(path: &std::path::Path) -> io::Result<String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(base64::engine::general_purpose::STANDARD_NO_PAD.encode(path.as_os_str().as_bytes()))
}

#[cfg(windows)]
fn encode_slot_path(path: &std::path::Path) -> io::Result<String> {
    use std::os::windows::ffi::OsStrExt;
    let bytes = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    Ok(base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes))
}

#[cfg(not(any(unix, windows)))]
fn encode_slot_path(path: &std::path::Path) -> io::Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "slot path is not valid UTF-8")
        })?;
    Ok(base64::engine::general_purpose::STANDARD_NO_PAD.encode(path.as_bytes()))
}

#[cfg(unix)]
fn decode_slot_path(encoded: String) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

pub(super) fn slot_lock_is_inherited(
    path: &std::path::Path,
    inherited_paths: &[PathBuf],
) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let path_identity = fs::canonicalize(path)?;
    for inherited_path in inherited_paths {
        match fs::canonicalize(inherited_path) {
            Ok(inherited_identity) if inherited_identity == path_identity => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn decode_slot_path(encoded: String) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() % 2 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid encoded slot path"));
    }
    let wide = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

#[cfg(not(any(unix, windows)))]
fn decode_slot_path(encoded: String) -> io::Result<PathBuf> {
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
