//! Safe abstractions over the filesystem foreign calls pnpm makes.
//!
//! One of the FFI abstraction crates the
//! `unsafe` policy in `UNSAFE_POLICY.md` defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own. Every
//! descriptor and handle a foreign call produces is adopted by a
//! [`std::fs::File`] before it crosses back out, and every status code
//! becomes an [`std::io::Error`].
//!
//! What is *not* here is the policy a caller wraps around these calls:
//! which fallback to take, how long to retry under descriptor pressure,
//! which errors to treat as "already done". Those decisions would still be
//! the caller's if the foreign calls were safe, so they stay with the
//! caller.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

#[cfg(target_os = "android")]
pub use advisory_lock::{AdvisoryLock, Blocking, lock_file};
pub use confined_open::open_directory_beneath;
#[cfg(unix)]
pub use dir_entries::directory_entry_names;
#[cfg(all(unix, not(target_os = "linux")))]
pub use dir_handle::set_mode_at;
#[cfg(unix)]
pub use dir_handle::{
    DirectoryAccess, create_directory_at, create_file_at, open_directory_at,
    open_directory_for_traversal, open_file_at, open_file_at_without_blocking,
    open_search_directory, read_link_at, remove_directory_at, rename_at, symlink_at, unlink_at,
};
pub use exchange::exchange_dirents;
#[cfg(unix)]
pub use fifo::create_fifo;
#[cfg(unix)]
pub use file_mode::FileMode;
#[cfg(target_vendor = "apple")]
pub use sync::fsync;
#[cfg(unix)]
pub use unlink::unlink_without_allocating;
#[cfg(target_os = "wasi")]
pub use wasi::{
    FileIdentity, file_identity, file_link_count, open_for_overwrite, path_identity, symlink,
};
#[cfg(windows)]
pub use windows_file::{
    FileAttributes, FileInformation, change_time_ticks, file_attributes, file_information,
    local_app_data_directory, rename_over, short_path, system_directory,
};

#[cfg(target_os = "android")]
mod advisory_lock;
mod confined_open;
#[cfg(unix)]
mod dir_entries;
#[cfg(unix)]
mod dir_handle;
mod exchange;
#[cfg(unix)]
mod fifo;
#[cfg(unix)]
mod file_mode;
#[cfg(target_vendor = "apple")]
mod sync;
#[cfg(unix)]
mod unlink;
#[cfg(target_os = "wasi")]
mod wasi;
#[cfg(windows)]
mod windows_file;
