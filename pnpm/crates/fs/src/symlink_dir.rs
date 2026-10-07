pub use absolute::force_absolute_symlink_dir;

use std::{
    borrow::Cow,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::retry::{
    is_transient_file_lock_error, remove_dir_all_with_retry, rename_with_retry,
    retry_transient_file_locks,
};

/// Create a symlink to a directory, matching the on-disk shape pnpm
/// produces.
///
/// On Unix the symlink contents are stored as a path relative to the
/// link's parent directory — `path.relative(dirname(link), target)`.
/// Relative targets keep `node_modules` installs survivable across
/// project-directory moves and match the byte-for-byte symlink
/// contents pnpm writes, so snapshot tooling and lockfile-parity
/// checks stay aligned.
///
/// On Windows the writer tries a true directory symlink first
/// (`std::os::windows::fs::symlink_dir`) and falls back to a junction
/// on a privilege error (symbolic links may require elevated
/// privileges; junctions don't). The first successful branch is cached
/// process-wide so subsequent calls skip the privilege probe.
pub fn symlink_dir(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let rel = relative_target_for(original, link);
        std::os::unix::fs::symlink(&rel, link)
    }
    #[cfg(target_os = "wasi")]
    {
        crate::wasi_fs::symlink(&relative_target_for(original, link), link)
    }
    #[cfg(windows)]
    {
        let result = windows::create(&to_native_separators(original), &to_native_separators(link));
        #[cfg(feature = "test")]
        crate::test_support::notify_attempt(link, &result);
        result
    }
}

/// Create a directory link at `link` holding `contents` as given, which
/// may be relative to the link.
///
/// On Windows a process that may not create symlinks gets a junction
/// instead, which only holds an absolute path, so `original` has to be the
/// absolute path `contents` resolves to from where the link finally lives.
pub fn symlink_dir_with_contents(original: &Path, contents: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = original;
        std::os::unix::fs::symlink(contents, link)
    }
    #[cfg(target_os = "wasi")]
    {
        let _ = original;
        crate::wasi_fs::symlink(contents, link)
    }
    #[cfg(windows)]
    {
        windows::create_with_contents(
            &to_native_separators(original),
            &to_native_separators(contents),
            &to_native_separators(link),
        )
    }
}

/// Rewrite every `/` in `path` to the native `\` on Windows.
///
/// A scoped alias like `@scope/name` is joined into a path as a single
/// segment, and [`Path::join`] appends it verbatim, leaving a forward
/// slash that `CreateSymbolicLinkW` rejects with `ERROR_DIRECTORY`
/// (os error 267) — the long store paths reach it in verbatim `\\?\`
/// form, where `/` is a literal byte rather than a separator.
///
/// Borrows unless a rewrite is needed; a no-op on Unix.
#[cfg(windows)]
#[must_use]
pub fn to_native_separators(path: &Path) -> Cow<'_, Path> {
    // In WTF-8 a 0x2F byte appears iff the path holds a literal `/`, so
    // scanning bytes is a correct, allocation-free check.
    if !path
        .as_os_str()
        .as_encoded_bytes()
        .contains(&b'/')
    {
        return Cow::Borrowed(path);
    }
    // A string replace, not `Path::components`: in a verbatim `\\?\`
    // path `components` treats `/` as a literal byte and leaves it in
    // place. Package paths are valid Unicode, so `to_str` succeeds.
    match path.to_str() {
        Some(s) => Cow::Owned(PathBuf::from(s.replace('/', r"\"))),
        None => Cow::Borrowed(path),
    }
}

#[cfg(not(windows))]
#[must_use]
pub fn to_native_separators(path: &Path) -> Cow<'_, Path> {
    Cow::Borrowed(path)
}

/// Compute the symlink contents for a true symlink: the path from the
/// link's parent directory to `original`, equivalent to
/// `path.relative(path.dirname(dest), src)`.
///
/// Returns an absolute path when no relative form exists between the
/// two arguments.
fn relative_target_for(original: &Path, link: &Path) -> PathBuf {
    let parent = link.parent().unwrap_or_else(|| Path::new(""));
    crate::relative_path(parent, original)
}

/// Whether `link` is a directory symlink or, on Windows, a junction —
/// the two shapes [`symlink_dir`] can produce.
///
/// On Windows the standard library reports both as symlinks, since a
/// junction is a name-surrogate reparse point just like a symlink. There
/// the `lstat` follows the retry policy of [`crate::rename_with_retry`],
/// and a failed one is returned rather than read as `false`.
pub fn is_symlink_or_junction(link: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    return Ok(crate::symlink_metadata_with_retry(link)?.file_type().is_symlink());
    #[cfg(not(windows))]
    Ok(link.is_symlink())
}

/// Remove a symlink (or junction on Windows) previously created with
/// [`symlink_dir`].
///
/// On Unix a directory symlink is a file-shaped entry and removed
/// with `fs::remove_file`. On Windows [`symlink_dir`] may create
/// either a true symlink (directory-shaped, since the target is a
/// directory) or a junction (directory-shaped reparse point); both
/// need `fs::remove_dir` to be unlinked — `remove_file` returns
/// `ERROR_ACCESS_DENIED`. Wrapping the platform split here keeps
/// callers free of `#[cfg]`.
///
/// The unlink follows the retry policy of [`crate::rename_with_retry`].
pub fn remove_symlink_dir(link: &Path) -> io::Result<()> {
    #[cfg(any(unix, target_os = "wasi"))]
    return retry_transient_file_locks(|| std::fs::remove_file(link));
    #[cfg(windows)]
    return retry_transient_file_locks(|| std::fs::remove_dir(link));
}

/// Read the target of a directory symlink (or junction on Windows).
///
/// [`std::fs::read_link`] reads both on Windows. A junction's target is
/// the absolute path it was created with.
///
/// Target inspection follows the retry policy of [`crate::rename_with_retry`].
pub fn read_symlink_dir(link: &Path) -> io::Result<PathBuf> {
    retry_transient_file_locks(|| std::fs::read_link(link))
}

/// Outcome of a [`force_symlink_dir`] call.
///
/// `reused` is `true` when the symlink at `link` already pointed at
/// the requested target, so no on-disk write was needed. `warning`
/// carries the human-readable note emitted when an existing non-symlink
/// occupant had to be moved out of the way or a concurrent junction
/// commit succeeded but left a staging path that could not be cleaned
/// up — surface it to the user if your call site has a reporter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ForceSymlinkOutcome {
    pub reused: bool,
    pub warning: Option<String>,
}

#[derive(Debug)]
struct ConcurrentCleanupWarning(String);

impl fmt::Display for ConcurrentCleanupWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConcurrentCleanupWarning {}

/// Idempotent, overwrite-on-stale symlink creator: an existing occupant
/// at `link` is moved aside.
///
/// A regular file or directory occupying `link` can be gone by the time the
/// rename that moves it aside runs, which is what a second writer racing for
/// the same path looks like. The create is reissued once for that, since the
/// conflict it reported no longer exists. A link that reports a conflict twice
/// over while holding nothing surfaces the create's own error rather than the
/// rename's `NotFound`.
pub fn force_symlink_dir(target: &Path, link: &Path) -> io::Result<ForceSymlinkOutcome> {
    // Normalize up front so every retry-loop fs op on `link` — not just
    // the symlink syscall — sees a native path. See [`to_native_separators`].
    let target = to_native_separators(target);
    let link = to_native_separators(link);
    #[cfg(windows)]
    return force_symlink(&target, &link, windows::create);
    #[cfg(not(windows))]
    force_symlink(&target, &link, symlink_dir)
}

fn force_symlink_inner(
    target: &Path,
    link: &Path,
    tried: TriedOnce,
    create_symlink: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<ForceSymlinkOutcome> {
    let initial_err = match create_symlink(target, link) {
        Ok(()) => return Ok(ForceSymlinkOutcome { reused: false, warning: None }),
        Err(error) => error,
    };
    let reuse_warning = initial_err
        .get_ref()
        .and_then(|error| error.downcast_ref::<ConcurrentCleanupWarning>())
        .map(|warning| warning.0.clone());

    match initial_err.kind() {
        io::ErrorKind::NotFound => {
            create_symlink_parent(target, link)?;
            return force_symlink_inner(target, link, tried, create_symlink);
        }
        io::ErrorKind::AlreadyExists | io::ErrorKind::IsADirectory => {}
        _ => return Err(initial_err),
    }

    let Ok(existing) = read_symlink_dir(link) else {
        return replace_unreadable_occupant(target, link, tried, create_symlink, initial_err);
    };
    if existing_symlink_up_to_date(target, link, &existing) {
        return Ok(ForceSymlinkOutcome { reused: true, warning: reuse_warning });
    }
    // Stale link — unlink and retry. Ignore `NotFound` in case a parallel
    // installer beat us to the unlink.
    match remove_symlink_dir(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    force_symlink_inner(target, link, tried, create_symlink)
}

/// Create the directory the link lives in, wrapping a failure so callers see
/// *which* step tripped.
fn create_symlink_parent(target: &Path, link: &Path) -> io::Result<()> {
    let Some(parent) = link.parent() else {
        return Ok(());
    };
    create_dir_all_healing_reparse(parent)
        .map_err(|mkdir_err| {
            io::Error::new(
                mkdir_err.kind(),
                format!(
                    "Error while trying to symlink {target:?} to {link:?}. \
                 The error happened while trying to create the parent directory \
                 for the symlink target. Details: {mkdir_err}",
                ),
            )
        })
}

/// Like [`std::fs::create_dir_all`], but heals a dangling reparse point
/// squatting on `dir`.
///
/// On Windows a junction whose target is gone — the shape a tar-based CI
/// cache restore leaves behind — keeps the directory attribute, so
/// `create_dir_all` reports `AlreadyExists` yet no child can be created
/// through it. Removing the dangling reparse point (which unlinks only
/// the junction, never a live target) and recreating a real directory
/// clears the way. On Unix this is a plain `create_dir_all`.
fn create_dir_all_healing_reparse(dir: &Path) -> io::Result<()> {
    let error = match fs::create_dir_all(dir) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    #[cfg(windows)]
    {
        if is_reparse_point(dir) {
            remove_symlink_dir(dir)?;
            return fs::create_dir_all(dir);
        }
    }
    Err(error)
}

/// Whether `path` is a reparse point (a symbolic link or a junction).
/// Unlike [`std::fs::FileType::is_symlink`], which is `false` for
/// junctions, this reads the raw reparse-point attribute, so a dangling
/// junction is caught too.
#[cfg(windows)]
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

mod absolute;
mod replace;
mod reuse;
use replace::{TriedOnce, replace_unreadable_occupant};
use reuse::{existing_symlink_up_to_date, force_symlink};
