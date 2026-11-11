//! Flushing a file to the storage device on Apple platforms.

use std::{fs::File, io, os::fd::AsRawFd as _};

/// Flush `file`'s data to the storage device with `fsync`.
///
/// Apple's [`File::sync_all`] is `fcntl(F_FULLFSYNC)`, which virtiofs and
/// SMB shares reject outright. `fsync` is what `SQLite` and Node.js fall
/// back to there, so it is offered separately rather than as a stronger
/// sync's implementation detail.
pub fn fsync(file: &File) -> io::Result<()> {
    // SAFETY: the `&File` keeps the descriptor open for the whole call, and
    // `fsync` takes it by value and writes no memory.
    let synced = unsafe { libc::fsync(file.as_raw_fd()) };
    if synced == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
