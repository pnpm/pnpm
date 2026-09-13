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
    #[cfg(windows)]
    {
        let original = to_native_separators(original);
        let link = to_native_separators(link);
        windows::create(&original, &link)
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
        Some(s) => Cow::Owned(PathBuf::from(s.replace('/', "\\"))),
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
    let parent = link
        .parent()
        .unwrap_or_else(|| Path::new(""));
    crate::relative_path(parent, original)
}

/// Whether `link` is a directory symlink or, on Windows, a junction —
/// the two shapes [`symlink_dir`] can produce.
///
/// [`Path::is_symlink`] only reports the `IO_REPARSE_TAG_SYMLINK` shape,
/// so a caller that cleans up after [`symlink_dir`] and tests with it
/// alone misses every junction [`symlink_dir`] fell back to.
pub fn is_symlink_or_junction(link: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        // Check the symlink case first so a true symlink never reaches
        // `junction::exists`.
        if link.is_symlink() {
            return Ok(true);
        }
        // `junction::exists` reports a path that is not a reparse point
        // at all (a plain directory) as `ERROR_NOT_A_REPARSE_POINT`
        // rather than `Ok(false)`; for this question that is a plain
        // "no".
        const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
        match junction::exists(link) {
            Ok(is_junction) => Ok(is_junction),
            Err(error) if error.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT) => Ok(false),
            Err(error) => Err(error),
        }
    }
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
/// On Windows the unlink follows the retry policy of
/// [`crate::rename_with_retry`].
pub fn remove_symlink_dir(link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::fs::remove_file(link);
    #[cfg(windows)]
    return retry_transient_file_locks(|| std::fs::remove_dir(link));
}

/// Read the target of a directory symlink (or junction on Windows).
///
/// On Unix this is just [`std::fs::read_link`]. On Windows the
/// stdlib's `read_link` only handles
/// [`IO_REPARSE_TAG_SYMLINK`](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-tags)
/// reparse points and returns `ERROR_NOT_A_REPARSE_POINT`
/// (`InvalidInput`) for `IO_REPARSE_TAG_MOUNT_POINT` junctions —
/// see [`rust-lang/rust#28528`](https://github.com/rust-lang/rust/issues/28528),
/// which has been open since 2015. Since [`symlink_dir`] may create
/// junctions on Windows, fall back to `junction::get_target` on
/// `InvalidInput` to handle the junction case while keeping
/// `fs::read_link` as the fast path for true symlinks. (Plain
/// backticks rather than an intra-doc link because the `junction`
/// crate is only in scope on Windows targets — a link would
/// break the Linux doc build.)
pub fn read_symlink_dir(link: &Path) -> io::Result<PathBuf> {
    #[cfg(unix)]
    return std::fs::read_link(link);
    #[cfg(windows)]
    {
        match std::fs::read_link(link) {
            Ok(target) => Ok(target),
            // EINVAL on Windows from `read_link` means the reparse
            // point isn't a symbolic link tag — almost certainly a
            // junction, the only other kind of reparse point
            // pacquet's writer produces.
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => junction::get_target(link),
            Err(error) => Err(error),
        }
    }
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

/// Idempotent, overwrite-on-stale symlink creator with overwrite-on
/// semantics: an existing occupant at `link` is moved aside.
///
/// When a regular file or directory occupies `link` and the rename
/// that moves it aside fails because the source disappeared between
/// the `AlreadyExists` and the rename, the initial `AlreadyExists`
/// error is surfaced rather than the rename's `NotFound`.
pub fn force_symlink_dir(target: &Path, link: &Path) -> io::Result<ForceSymlinkOutcome> {
    // Normalize up front so every retry-loop fs op on `link` — not just
    // the symlink syscall — sees a native path. See [`to_native_separators`].
    let target = to_native_separators(target);
    let link = to_native_separators(link);
    #[cfg(windows)]
    return force_symlink_inner(&target, &link, false, windows::create);
    #[cfg(not(windows))]
    force_symlink_inner(&target, &link, false, symlink_dir)
}

fn force_symlink_inner(
    target: &Path,
    link: &Path,
    rename_tried: bool,
    create_symlink: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<ForceSymlinkOutcome> {
    let initial_err = match create_symlink(target, link) {
        Ok(()) => {
            return Ok(ForceSymlinkOutcome {
                reused: false,
                warning: None,
            });
        }
        Err(error) => error,
    };
    let reuse_warning = initial_err
        .get_ref()
        .and_then(|error| error.downcast_ref::<ConcurrentCleanupWarning>())
        .map(|warning| warning.0.clone());

    match initial_err.kind() {
        io::ErrorKind::NotFound => {
            create_symlink_parent(target, link)?;
            return force_symlink_inner(target, link, rename_tried, create_symlink);
        }
        io::ErrorKind::AlreadyExists | io::ErrorKind::IsADirectory => {}
        _ => return Err(initial_err),
    }

    let Ok(existing) = read_symlink_dir(link) else {
        // A vanished occupant means the path was cleared under us, so the
        // original symlink failure is the one worth reporting.
        let Some(warning) = clear_symlink_occupant(link, rename_tried)? else {
            return Err(initial_err);
        };
        let mut outcome = force_symlink_inner(target, link, true, create_symlink)?;
        outcome.warning = Some(warning);
        return Ok(outcome);
    };
    if existing_symlink_up_to_date(target, link, &existing) {
        return Ok(ForceSymlinkOutcome {
            reused: true,
            warning: reuse_warning,
        });
    }
    replace::remove_stale_link(link)?;
    force_symlink_inner(target, link, rename_tried, create_symlink)
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

/// Move whatever regular file or directory occupies the link path out of the
/// way, and describe what was done with it. `None` means the occupant was
/// already gone.
///
/// On the second attempt (`rename_tried`) this drops down to a plain unlink,
/// as a fallback for an intermittent macOS bug — see
/// [pnpm/pnpm#5909](https://github.com/pnpm/pnpm/issues/5909#issuecomment-1400066890).
fn clear_symlink_occupant(link: &Path, rename_tried: bool) -> io::Result<Option<String>> {
    let parent = link
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let basename = link
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if rename_tried {
        remove_occupant(link)?;
        return Ok(Some(format!(
            "Symlink wanted name was occupied by directory or file. \
             Old entity removed: {parent:?}{sep}{basename}",
            sep = std::path::MAIN_SEPARATOR,
        )));
    }
    let ignore_name = format!(".ignored_{basename}");
    if let Err(rename_err) = rename_overwrite(link, &parent.join(&ignore_name)) {
        if rename_err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(rename_err);
    }
    Ok(Some(format!(
        "Symlink wanted name was occupied by directory or file. \
         Old entity moved: {parent:?}{sep}{basename} => {ignore_name}",
        sep = std::path::MAIN_SEPARATOR,
    )))
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

/// Lexical "does the existing link resolve to the wanted target?"
/// check: resolve the existing link's contents to an absolute path
/// (using `link`'s parent dir when the contents are relative), then
/// compare lexically against `wanted`. Single-level — does not follow
/// chained symlinks.
///
/// Both sides pass through [`fn@crate::lexical_normalize`] before
/// comparing. The `..` segments in the relative link contents
/// [`symlink_dir`] writes must collapse before the comparison;
/// without that, every up-to-date relative symlink reads as stale and
/// pays an unlink + recreate.
fn existing_symlink_up_to_date(wanted: &Path, link: &Path, existing_link_string: &Path) -> bool {
    let existing_absolute = if existing_link_string.is_absolute() {
        existing_link_string.to_path_buf()
    } else {
        link
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(existing_link_string)
    };
    crate::lexical_normalize(&existing_absolute) == crate::lexical_normalize(wanted)
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

mod replace;
use replace::{remove_occupant, rename_overwrite};
