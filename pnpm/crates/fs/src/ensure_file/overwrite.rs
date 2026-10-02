use super::retry_on_fd_pressure;
#[cfg(not(target_os = "wasi"))]
use crate::retry::retry_transient_file_locks;
#[cfg(not(target_os = "wasi"))]
use std::fs::OpenOptions;
use std::{
    fs::{self, File},
    io::{self, Seek},
    path::Path,
};

/// Overwrite the regular file at `file_path` in place with bytes from
/// `reader`, keeping the inode so hard-linked copies of the file — other
/// projects' `node_modules` entries importing the same CAS blob — are
/// healed by the same write (pnpm/pnpm#3445).
///
/// The open does not follow a symlink at `file_path` (`O_NOFOLLOW` on
/// Unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows), and the opened
/// handle is compared against the identity of the file seen before the
/// open, so a dirent swapped in between is left untouched. Nothing is
/// truncated before that check passes. `O_NONBLOCK` keeps a FIFO from
/// holding the open.
///
/// Returns `false` when in-place overwrite is refused and the caller
/// should fall back to an atomic temp+rename: the target is not a
/// regular file, it refuses the write open (write protection, a running
/// executable's `ETXTBSY`, another owner's file), or the write failed.
/// Every such state is one the rename handles correctly, and a
/// persistent failure (e.g. `ENOSPC`) re-surfaces with proper context
/// when the fallback attempts its own write, so no error detail is lost
/// by collapsing these into `false`.
///
/// In-place overwrite is not atomic: a concurrent reader can observe
/// torn content for the duration of the write. The file was already
/// corrupt, and a failed integrity check re-triggers this repair, so
/// the trade is a brief torn-read window for healing every hard-linked
/// copy at once.
pub fn overwrite_file_in_place(file_path: &Path, reader: &mut dyn io::Read) -> bool {
    // A write-protected file refuses the write open, and on Windows that
    // refusal would first spend the transient-lock retry budget.
    #[cfg_attr(
        any(windows, target_os = "wasi"),
        expect(unused_variables, reason = "these targets compare file identities separately")
    )]
    let meta = match fs::symlink_metadata(file_path) {
        Ok(meta)
            if meta.file_type().is_file()
                && (cfg!(target_os = "wasi") || !meta.permissions().readonly()) =>
        {
            meta
        }
        _ => return false,
    };
    #[cfg(target_os = "wasi")]
    if !crate::wasi_fs::path_mode(file_path).is_ok_and(|mode| mode & 0o222 != 0) {
        return false;
    }
    #[cfg(unix)]
    let expected = meta;
    #[cfg(windows)]
    let Ok(expected) = same_file::Handle::from_path(file_path) else {
        return false;
    };
    #[cfg(target_os = "wasi")]
    let Ok(expected) = crate::wasi_fs::path_identity(file_path) else {
        return false;
    };
    let Ok(mut file) = open_for_overwrite(file_path) else {
        return false;
    };
    same_file(&file, &expected)
        && file.set_len(0).is_ok()
        && file.rewind().is_ok()
        && io::copy(reader, &mut file).is_ok()
}

/// Whether the opened handle is the same regular file `expected`
/// describes — the guard against a dirent swapped into the path between
/// the metadata check and the open.
#[cfg(unix)]
fn same_file(file: &File, expected: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata()
        .is_ok_and(|handle_meta| {
            handle_meta.file_type().is_file()
                && handle_meta.dev() == expected.dev()
                && handle_meta.ino() == expected.ino()
        })
}

#[cfg(target_os = "wasi")]
fn same_file(file: &File, expected: &crate::wasi_fs::FileIdentity) -> bool {
    file.metadata().is_ok_and(|metadata| metadata.is_file())
        && crate::wasi_fs::file_identity(file).is_ok_and(|identity| &identity == expected)
}

#[cfg(windows)]
fn same_file(file: &File, expected: &same_file::Handle) -> bool {
    file.metadata()
        .is_ok_and(|handle_meta| handle_meta.file_type().is_file())
        && file
            .try_clone()
            .and_then(same_file::Handle::from_file)
            .is_ok_and(|handle| &handle == expected)
}

fn open_for_overwrite(file_path: &Path) -> io::Result<File> {
    #[cfg(not(target_os = "wasi"))]
    let mut options = OpenOptions::new();
    #[cfg(not(target_os = "wasi"))]
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    // Antivirus and indexer scans briefly hold just-written Windows
    // paths open, failing an unlucky open with an access-denied error
    // that clears moments later.
    #[cfg(not(target_os = "wasi"))]
    let open = || retry_transient_file_locks(|| retry_on_fd_pressure(|| options.open(file_path)));
    #[cfg(target_os = "wasi")]
    let open = || retry_on_fd_pressure(|| crate::wasi_fs::open_for_overwrite(file_path));
    open()
}
