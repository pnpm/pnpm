use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

/// The directory a spawned lifecycle script runs in.
///
/// A store path can reach the executor in Windows' verbatim `\\?\`
/// form, which many Windows programs, including the lifecycle shell,
/// do not accept. [`dunce::simplified`] hands over the plain form and
/// is a no-op on every other platform.
pub(crate) fn script_working_dir(pkg_root: &Path) -> &Path {
    dunce::simplified(pkg_root)
}

/// The shortest verified spelling to hand to the shell emulator.
///
/// The emulator hides external-process spawn errors behind their exit code, so
/// it cannot retry after Windows refuses a long working directory.
#[cfg(windows)]
pub(crate) fn emulator_working_dir(pkg_root: &Path) -> Cow<'_, Path> {
    let pkg_root = script_working_dir(pkg_root);
    const MAX_WORKING_DIR_WITHOUT_TRAILING_SEPARATOR: usize = 258;
    if windows_path_len(pkg_root) <= MAX_WORKING_DIR_WITHOUT_TRAILING_SEPARATOR {
        return Cow::Borrowed(pkg_root);
    }
    shorter_working_dirs(pkg_root)
        .into_iter()
        .min_by_key(|spelling| windows_path_len(spelling))
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed(pkg_root))
}

#[cfg(not(windows))]
pub(crate) fn emulator_working_dir(pkg_root: &Path) -> Cow<'_, Path> {
    Cow::Borrowed(pkg_root)
}

pub(crate) fn is_refused_directory(error: &io::Error) -> bool {
    const ERROR_DIRECTORY: i32 = 267;
    cfg!(windows) && error.raw_os_error() == Some(ERROR_DIRECTORY)
}

/// Other spellings of `pkg_root` to try after the operating system refuses it.
///
/// Windows bounds a process working directory below `MAX_PATH`, and a global
/// virtual store slot readily reaches that length. The error also covers a
/// missing directory, so retries are offered only after a refusal and only
/// when the filesystem confirms that a shorter spelling names the same place.
pub(crate) fn shorter_working_dirs(pkg_root: &Path) -> Vec<PathBuf> {
    let mut spellings = Vec::with_capacity(2);
    let normalized = pnpm_fs::lexical_normalize(pkg_root);
    if normalized != pkg_root {
        spellings.push(normalized.clone());
    }
    if let Some(short) = short_path(&normalized)
        && short != normalized
    {
        spellings.push(short);
    }
    spellings.retain(|spelling| names_the_same_dir(spelling, pkg_root));
    spellings
}

fn names_the_same_dir(spelling: &Path, pkg_root: &Path) -> bool {
    match (std::fs::canonicalize(spelling), std::fs::canonicalize(pkg_root)) {
        (Ok(spelled), Ok(root)) => spelled == root,
        _ => false,
    }
}

#[cfg(windows)]
fn windows_path_len(path: &Path) -> usize {
    path.as_os_str().encode_wide().count()
}

/// The 8.3 short form of an existing drive-letter path, when the volume
/// generates one. The verbatim prefix lets the API receive paths over
/// `MAX_PATH`; only a strictly shorter, non-verbatim answer is useful.
#[cfg(windows)]
fn short_path(path: &Path) -> Option<PathBuf> {
    use std::{
        ffi::{OsStr, OsString},
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Component, Prefix},
        ptr,
    };
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let Some(Component::Prefix(prefix)) = path.components().next() else { return None };
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0));
    let verbatim: Vec<u16> = match prefix.kind() {
        Prefix::Disk(_) => OsStr::new(r"\\?\")
            .encode_wide()
            .chain(wide)
            .collect(),
        Prefix::VerbatimDisk(_) => wide.collect(),
        _ => return None,
    };

    // SAFETY: the first call obtains the required UTF-16 buffer size from a
    // null-terminated input. The second writes into that sized buffer, and its
    // returned length is checked before constructing the path.
    let short = unsafe {
        let required = GetShortPathNameW(verbatim.as_ptr(), ptr::null_mut(), 0);
        if required == 0 {
            return None;
        }
        let mut buffer = vec![0_u16; required as usize];
        let length = GetShortPathNameW(verbatim.as_ptr(), buffer.as_mut_ptr(), required);
        if length == 0 || length >= required {
            return None;
        }
        PathBuf::from(OsString::from_wide(&buffer[..length as usize]))
    };
    let short = dunce::simplified(&short);
    let still_verbatim = match short.components().next() {
        Some(Component::Prefix(prefix)) => matches!(
            prefix.kind(),
            Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..),
        ),
        _ => false,
    };
    (!still_verbatim && windows_path_len(short) < windows_path_len(path)).then(|| {
        short.to_path_buf()
    })
}

#[cfg(not(windows))]
fn short_path(_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests;
