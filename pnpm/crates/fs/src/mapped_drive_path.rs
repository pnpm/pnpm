use std::path::{Path, PathBuf};

#[cfg(all(windows, test))]
#[path = "mapped_drive_path/tests.rs"]
mod tests;

/// [`dunce::canonicalize`], except that a path on a mapped Windows network
/// drive keeps its drive letter instead of turning into the share's UNC form.
///
/// Only the drive `path` itself names is restored. A path given in UNC form
/// stays UNC: mapping it onto some drive could cut off ancestors that lie
/// above that drive's root, such as the workspace root.
pub fn canonicalize_preserving_mapped_drive(path: &Path) -> std::io::Result<PathBuf> {
    let absolute =
        if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    let canonical = dunce::canonicalize(&absolute)?;
    #[cfg(windows)]
    if is_unc_path(&canonical)
        && let Some(drive) = drive_letter(&absolute)
        && let Some(remote) = mapped_drive(drive)
        && let Some(mapped) = mapped_drive_path(&canonical, drive, &remote)
    {
        return Ok(mapped);
    }
    Ok(canonical)
}

/// Whether `path` is on a network share: a UNC path, or a path on a drive
/// letter mapped to a share. A relative path is resolved against the
/// working directory first.
#[cfg(windows)]
pub(crate) fn is_on_network_drive(path: &Path) -> bool {
    let absolute = std::path::absolute(path);
    let path = absolute.as_deref().unwrap_or(path);
    is_unc_path(path)
        || drive_letter(path).is_some_and(|drive| is_mapped_drive(drive) == Some(true))
}

/// [`is_on_network_drive`], with whether each drive letter is mapped looked
/// up once per process. An install asks this for every link it writes, and
/// the drive mappings don't change during one. A lookup that fails says
/// nothing about the drive, so it isn't cached.
#[cfg(windows)]
pub(crate) fn is_on_network_drive_cached(path: &Path) -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};

    const UNKNOWN: u8 = 0;
    const LOCAL: u8 = 1;
    const NETWORK: u8 = 2;
    static DRIVES: [AtomicU8; 26] = [const { AtomicU8::new(UNKNOWN) }; 26];

    let absolute = std::path::absolute(path);
    let path = absolute.as_deref().unwrap_or(path);
    if is_unc_path(path) {
        return true;
    }
    let Some(drive) = drive_letter(path).filter(char::is_ascii_uppercase) else {
        return false;
    };
    let slot = &DRIVES[usize::from(drive as u8 - b'A')];
    match slot.load(Ordering::Relaxed) {
        LOCAL => false,
        NETWORK => true,
        _ => match is_mapped_drive(drive) {
            Some(network) => {
                slot.store(if network { NETWORK } else { LOCAL }, Ordering::Relaxed);
                network
            }
            None => false,
        },
    }
}

/// Whether drive `letter` is mapped to a network share, or `None` when
/// Windows can't tell.
#[cfg(windows)]
fn is_mapped_drive(letter: char) -> Option<bool> {
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;

    let drive_name = [letter as u16, u16::from(b':'), 0];
    let mut remote_name = [0u16; 260];
    let mut length = remote_name.len() as u32;
    // SAFETY: The buffers are writable for the lengths passed to WNetGetConnectionW.
    let result = unsafe {
        WNetGetConnectionW(drive_name.as_ptr(), remote_name.as_mut_ptr(), &raw mut length)
    };
    drive_kind_from_connection_result(result)
}

/// What a `WNetGetConnectionW` result says about a drive. A persistent
/// mapping that is disconnected counts as mapped: the first access through
/// it reconnects it. Only `ERROR_NOT_CONNECTED` says the drive isn't
/// redirected. Any other error, such as `ERROR_NO_NETWORK` (no network
/// provider is running) or `ERROR_BAD_DEVICE`, says nothing about the drive.
#[cfg(windows)]
fn drive_kind_from_connection_result(result: u32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{
        ERROR_CONNECTION_UNAVAIL, ERROR_MORE_DATA, ERROR_NOT_CONNECTED, NO_ERROR,
    };

    match result {
        NO_ERROR | ERROR_MORE_DATA | ERROR_CONNECTION_UNAVAIL => Some(true),
        ERROR_NOT_CONNECTED => Some(false),
        _ => None,
    }
}

#[cfg(windows)]
fn is_unc_path(path: &Path) -> bool {
    use std::path::Prefix;

    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)),
    )
}

#[cfg(windows)]
fn drive_letter(path: &Path) -> Option<char> {
    match path.components().next()? {
        std::path::Component::Prefix(prefix) => match prefix.kind() {
            std::path::Prefix::Disk(letter) | std::path::Prefix::VerbatimDisk(letter) => {
                Some((letter as char).to_ascii_uppercase())
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(windows)]
fn mapped_drive(letter: char) -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::ERROR_MORE_DATA, NetworkManagement::WNet::WNetGetConnectionW,
    };

    let drive_name = [letter as u16, u16::from(b':'), 0];
    let mut remote_name = vec![0u16; 512];
    let mut length = remote_name.len() as u32;
    // SAFETY: The buffers are writable for the lengths passed to WNetGetConnectionW.
    let mut result = unsafe {
        WNetGetConnectionW(drive_name.as_ptr(), remote_name.as_mut_ptr(), &raw mut length)
    };
    if result == ERROR_MORE_DATA {
        remote_name.resize(length as usize, 0);
        // SAFETY: The resized buffer and length pointer are writable for this call.
        result = unsafe {
            WNetGetConnectionW(drive_name.as_ptr(), remote_name.as_mut_ptr(), &raw mut length)
        };
    }
    if result != 0 {
        return None;
    }
    let remote_length = remote_name
        .iter()
        .position(|unit| *unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&remote_name[..remote_length])))
}

#[cfg(windows)]
fn mapped_drive_path(path: &Path, drive: char, remote: &Path) -> Option<PathBuf> {
    let suffix = mapped_path_suffix(&remove_verbatim_unc_prefix(path), remote)?;
    Some(PathBuf::from(format!(r"{drive}:\")).join(suffix))
}

#[cfg(windows)]
fn mapped_path_suffix(path: &Path, remote: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let path: Vec<u16> = path.as_os_str().encode_wide().collect();
    let mut remote: Vec<u16> = remote
        .as_os_str()
        .encode_wide()
        .collect();
    while remote
        .last()
        .is_some_and(|unit| matches!(*unit, 0x2f | 0x5c))
    {
        remote.pop();
    }
    if path.len() < remote.len()
        || (path.len() > remote.len() && !matches!(path[remote.len()], 0x2f | 0x5c))
    {
        return None;
    }
    // SAFETY: Both pointers address initialized UTF-16 slices for the lengths passed.
    let equal = unsafe {
        CompareStringOrdinal(
            path.as_ptr(),
            remote.len() as i32,
            remote.as_ptr(),
            remote.len() as i32,
            1,
        )
    };
    if equal != CSTR_EQUAL {
        return None;
    }
    let mut suffix = remote.len();
    while path
        .get(suffix)
        .is_some_and(|unit| matches!(*unit, 0x2f | 0x5c))
    {
        suffix += 1;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&path[suffix..])))
}

#[cfg(windows)]
fn remove_verbatim_unc_prefix(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix = [
        u16::from(b'\\'),
        u16::from(b'\\'),
        u16::from(b'?'),
        u16::from(b'\\'),
        u16::from(b'U'),
        u16::from(b'N'),
        u16::from(b'C'),
        u16::from(b'\\'),
    ];
    if wide.len() < prefix.len() || wide[..prefix.len()] != prefix {
        return path.to_path_buf();
    }
    let mut unc = vec![u16::from(b'\\'), u16::from(b'\\')];
    unc.extend_from_slice(&wide[prefix.len()..]);
    PathBuf::from(std::ffi::OsString::from_wide(&unc))
}
