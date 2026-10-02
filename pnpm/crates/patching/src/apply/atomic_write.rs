#[cfg(target_os = "wasi")]
use pnpm_fs::CopyPermissions as Permissions;
#[cfg(not(target_os = "wasi"))]
use std::{
    fs::{self, OpenOptions, Permissions},
    sync::atomic::{AtomicU64, Ordering},
};

use std::{
    io::{self, Write},
    path::Path,
};

/// Atomic write: stage `content` in a sibling temp file (with
/// `permissions` applied before the rename so the final file has the
/// right mode atomically), then `rename` over `target`. Mirrors the
/// pattern in
/// [`pnpm_lockfile::save_lockfile::write_atomic`](../../lockfile/src/save_lockfile.rs):
/// `create_new(true)` rather than `create + truncate` so we never
/// follow a symlink or truncate a file an attacker (or a crashed prior
/// install) pre-seeded at our predicted temp path; on `AlreadyExists`
/// the counter advances and we retry up to `MAX_TEMP_ATTEMPTS` times.
///
/// `rename` is atomic on Unix and replaces in-place on Windows, so an
/// IO failure mid-write leaves either the original file or the
/// rewritten one — never an empty dirent. **This is atomic against IO
/// errors, not against power loss**: we don't `fsync` the temp file
/// or the parent directory, so a host crash between rename and the
/// kernel's writeback flush can lose the rename. This matches Node's
/// `fs.writeFileSync` semantics — it doesn't fsync either, and a
/// partially-written patched install is recoverable by re-running
/// `pnpm install` anyway.
///
/// As a side effect, `rename` creates a fresh inode at `target`,
/// breaking any hardlink the path previously shared with the content-
/// addressable store; the store inode (and every other hardlink to it)
/// stays untouched.
#[cfg(not(target_os = "wasi"))]
pub(super) fn write_atomic_with_mode(
    target: &Path,
    content: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    /// Sixteen fresh counter values is plenty — under benign
    /// conditions we never collide; under shared-store-across-
    /// containers the chance of 16 consecutive same-pid same-counter
    /// collisions is negligible. Matches the constant in
    /// `pnpm_lockfile::save_lockfile::write_atomic`.
    const MAX_TEMP_ATTEMPTS: usize = 16;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = crate::process::id();
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .map_or_else(|| String::from("patched"), |name| name.to_string_lossy().into_owned());

    let mut last_already_exists: Option<io::Error> = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{file_name}.{pid}.{counter}.pacquet-tmp"));

        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_already_exists = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        if let Err(error) = file.write_all(content) {
            drop(file);
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        // Close before chmod / rename. Required on Windows: `MoveFileEx`
        // over a still-open source handle fails with a sharing
        // violation. Not strictly required on Unix but matches the
        // pattern in `save_lockfile::write_atomic`. No `sync_all`: this
        // routine is atomic against IO errors, not power loss — see
        // the `fn` doc above.
        drop(file);

        return replace_with_permissions(&tmp, target, permissions);
    }

    Err(last_already_exists.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exhausted temp-path attempts for atomic patch write",
        )
    }))
}

#[cfg(not(target_os = "wasi"))]
fn replace_with_permissions(
    tmp: &Path,
    target: &Path,
    permissions: &Permissions,
) -> io::Result<()> {
    if let Err(error) = fs::set_permissions(tmp, permissions.clone()) {
        let _ = fs::remove_file(tmp);
        return Err(error);
    }

    fs::rename(tmp, target)
        .inspect_err(|_| {
            let _ = fs::remove_file(tmp);
        })
}

#[cfg(target_os = "wasi")]
#[allow(clippy::trivially_copy_pass_by_ref, reason = "Shares the native permissions signature")]
pub(super) fn write_atomic_with_mode(
    target: &Path,
    content: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .make_in(parent, |path| pnpm_fs::create_new_with_mode(path, *permissions))?;
    temporary.write_all(content)?;
    pnpm_fs::set_file_permissions(temporary.as_file(), permissions)?;
    temporary.persist(target).map_err(|error| error.error)?;
    Ok(())
}
