use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
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
/// on `PermissionDenied` (symbolic links may require elevated
/// privileges; junctions don't). The first successful branch is cached
/// process-wide so subsequent calls skip the EPERM probe.
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
fn to_native_separators(path: &Path) -> Cow<'_, Path> {
    // In WTF-8 a 0x2F byte appears iff the path holds a literal `/`, so
    // scanning bytes is a correct, allocation-free check.
    if !path.as_os_str().as_encoded_bytes().contains(&b'/') {
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
fn to_native_separators(path: &Path) -> Cow<'_, Path> {
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
pub fn remove_symlink_dir(link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::fs::remove_file(link);
    #[cfg(windows)]
    return std::fs::remove_dir(link);
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
/// occupant had to be moved out of the way to install the symlink —
/// surface it to the user if your call site has a reporter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ForceSymlinkOutcome {
    pub reused: bool,
    pub warning: Option<String>,
}

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
        Ok(()) => return Ok(ForceSymlinkOutcome { reused: false, warning: None }),
        Err(error) => error,
    };

    match initial_err.kind() {
        io::ErrorKind::NotFound => {
            // Wrap the mkdir failure so callers see *which* step
            // tripped.
            if let Some(parent) = link.parent() {
                create_dir_all_healing_reparse(parent).map_err(|mkdir_err| {
                    io::Error::new(
                        mkdir_err.kind(),
                        format!(
                            "Error while trying to symlink {target:?} to {link:?}. \
                             The error happened while trying to create the parent directory \
                             for the symlink target. Details: {mkdir_err}",
                        ),
                    )
                })?;
            }
            return force_symlink_inner(target, link, rename_tried, create_symlink);
        }
        io::ErrorKind::AlreadyExists | io::ErrorKind::IsADirectory => {}
        _ => return Err(initial_err),
    }

    if let Ok(existing) = read_symlink_dir(link) {
        if existing_symlink_up_to_date(target, link, &existing) {
            return Ok(ForceSymlinkOutcome { reused: true, warning: None });
        }
        // Stale link — unlink and retry. Ignore `NotFound` in
        // case a parallel installer beat us to the unlink.
        match remove_symlink_dir(link) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        force_symlink_inner(target, link, rename_tried, create_symlink)
    } else {
        // `link` is occupied by a regular file or directory.
        // Move it out of the way, then retry. On the second
        // attempt (`rename_tried`) drop down to a plain unlink
        // as a fallback for an intermittent macOS bug, see
        // <https://github.com/pnpm/pnpm/issues/5909#issuecomment-1400066890>.
        let parent = link.parent().unwrap_or_else(|| Path::new(""));
        let basename = link.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let warning = if rename_tried {
            remove_occupant(link)?;
            format!(
                "Symlink wanted name was occupied by directory or file. \
                 Old entity removed: {parent:?}{sep}{basename}",
                sep = std::path::MAIN_SEPARATOR,
            )
        } else {
            let ignore_name = format!(".ignored_{basename}");
            let ignore_path = parent.join(&ignore_name);
            if let Err(rename_err) = rename_overwrite(link, &ignore_path) {
                if rename_err.kind() == io::ErrorKind::NotFound {
                    return Err(initial_err);
                }
                return Err(rename_err);
            }
            format!(
                "Symlink wanted name was occupied by directory or file. \
                 Old entity moved: {parent:?}{sep}{basename} => {ignore_name}",
                sep = std::path::MAIN_SEPARATOR,
            )
        };
        let mut outcome = force_symlink_inner(target, link, true, create_symlink)?;
        outcome.warning = Some(warning);
        Ok(outcome)
    }
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
        link.parent().unwrap_or_else(|| Path::new("")).join(existing_link_string)
    };
    crate::lexical_normalize(&existing_absolute) == crate::lexical_normalize(wanted)
}

/// Remove a regular file or directory that's occupying a symlink
/// slot. Tries `remove_dir_all` first; if the target isn't a
/// directory, falls back to `remove_file`.
fn remove_occupant(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => fs::remove_file(path),
    }
}

/// `fs::rename` that overwrites the destination when it exists.
/// Follows the
/// [`rename-overwrite`](https://github.com/zkochan/packages/tree/main/rename-overwrite)
/// package's approach: if the rename fails because the destination
/// is occupied (`AlreadyExists` for files, `DirectoryNotEmpty` for
/// dirs, `PermissionDenied` on Windows when something holds a handle
/// to the dest), remove the destination and retry once.
fn rename_overwrite(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(error) => {
            let occupied = matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists
                    | io::ErrorKind::DirectoryNotEmpty
                    | io::ErrorKind::PermissionDenied,
            );
            if !occupied {
                return Err(error);
            }
            remove_occupant(dst)?;
            fs::rename(src, dst)
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        fs, io,
        path::{Path, PathBuf},
        sync::{
            Mutex, PoisonError,
            atomic::{AtomicU8, AtomicU64, Ordering},
        },
    };

    /// Cached choice of writer. `UNDECIDED` until the first successful
    /// call resolves the EPERM probe; afterward `USE_SYMLINK` or
    /// `USE_JUNCTION`. Caching the winning branch after the first call
    /// avoids re-probing on every subsequent symlink.
    const UNDECIDED: u8 = 0;
    const USE_SYMLINK: u8 = 1;
    const USE_JUNCTION: u8 = 2;
    static MODE: AtomicU8 = AtomicU8::new(UNDECIDED);
    static JUNCTION_STAGING_ID: AtomicU64 = AtomicU64::new(0);
    static JUNCTION_COMMIT_LOCK: Mutex<()> = Mutex::new(());

    pub fn create(original: &Path, link: &Path) -> io::Result<()> {
        match MODE.load(Ordering::Relaxed) {
            USE_SYMLINK => match create_true_symlink(original, link) {
                Err(error) if should_fallback_to_junction(&error) => {
                    create_and_cache_junction(original, link)
                }
                result => result,
            },
            USE_JUNCTION => create_junction(original, link),
            _ => probe_and_cache(original, link),
        }
    }

    /// True symlinks on Windows take a relative target —
    /// `path.relative(dirname(dest), src)`, the same form used on Unix.
    /// Junctions take the absolute path with a trailing backslash, but
    /// the `junction` crate handles that internally so we pass
    /// `original` through unchanged for the junction branch.
    fn create_true_symlink(original: &Path, link: &Path) -> io::Result<()> {
        let rel = super::relative_target_for(original, link);
        std::os::windows::fs::symlink_dir(&rel, link)
    }

    fn probe_and_cache(original: &Path, link: &Path) -> io::Result<()> {
        // Try the true directory symlink first — that's what users
        // running in Developer Mode (or as Administrator) get, and
        // true symlinks are preferred over junctions when allowed.
        // `CreateSymbolicLinkW` returns
        // `ERROR_PRIVILEGE_NOT_HELD` (`PermissionDenied`) when the
        // process can't create symlinks; junctions don't carry that
        // constraint, so fall back to those.
        match create_true_symlink(original, link) {
            Ok(()) => {
                MODE.store(USE_SYMLINK, Ordering::Relaxed);
                Ok(())
            }
            Err(error) if should_fallback_to_junction(&error) => {
                create_and_cache_junction(original, link)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn should_fallback_to_junction(error: &io::Error) -> bool {
        const ERROR_DIRECTORY: i32 = 267;
        error.kind() == io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(ERROR_DIRECTORY)
    }

    fn create_and_cache_junction(original: &Path, link: &Path) -> io::Result<()> {
        let result = create_junction(original, link);
        if result.is_ok() {
            MODE.store(USE_JUNCTION, Ordering::Relaxed);
        }
        result
    }

    pub(super) fn create_junction(original: &Path, link: &Path) -> io::Result<()> {
        let staging = loop {
            let candidate = junction_staging_path(link);
            match junction::create(original, &candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };

        let _commit_guard = JUNCTION_COMMIT_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        match fs::symlink_metadata(link) {
            Ok(_) => {
                fs::remove_dir(&staging).map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!(
                            "junction path already exists at {link:?}, but staged junction \
                             cleanup failed at {staging:?}: {error}",
                        ),
                    )
                })?;
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("junction path already exists: {link:?}"),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let cleanup_error = fs::remove_dir(&staging).err();
                let cleanup_context = cleanup_error
                    .map(|cleanup| format!("; staged-junction cleanup failed: {cleanup}"))
                    .unwrap_or_default();
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "failed to inspect junction destination {link:?}: \
                         {error}{cleanup_context}",
                    ),
                ));
            }
        }

        if let Err(rename_error) = fs::rename(&staging, link) {
            let cleanup_error = fs::remove_dir(&staging).err();
            match fs::symlink_metadata(link) {
                Ok(_) => {
                    if let Some(cleanup_error) = cleanup_error {
                        return Err(io::Error::new(
                            rename_error.kind(),
                            format!(
                                "failed to rename staged junction {staging:?} to {link:?}: \
                                 {rename_error}; destination now exists, but cleanup failed: \
                                 {cleanup_error}",
                            ),
                        ));
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "junction path already exists after rename failed: {link:?}; \
                             rename error: {rename_error}",
                        ),
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    let cleanup_context = cleanup_error
                        .map(|cleanup| format!("; staged-junction cleanup failed: {cleanup}"))
                        .unwrap_or_default();
                    return Err(io::Error::new(
                        rename_error.kind(),
                        format!(
                            "failed to inspect junction destination {link:?} after rename \
                             failed: {rename_error}; inspection error: {error}{cleanup_context}",
                        ),
                    ));
                }
            }
            if let Some(cleanup_error) = cleanup_error {
                return Err(io::Error::new(
                    rename_error.kind(),
                    format!(
                        "failed to rename staged junction {staging:?} to {link:?}: \
                         {rename_error}; cleanup failed: {cleanup_error}",
                    ),
                ));
            }
            return Err(rename_error);
        }

        Ok(())
    }

    fn junction_staging_path(link: &Path) -> PathBuf {
        let id = JUNCTION_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!(".pnpm-junction-{}-{id}", std::process::id());
        link.with_file_name(name)
    }
}

#[cfg(test)]
mod tests;
