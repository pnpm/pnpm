pub mod file_mode;
#[cfg(all(windows, feature = "test"))]
pub mod test_support;
pub use background_drop::background_drop;
pub use capabilities::*;
pub use copy_dirent::{copy_dir_contents, copy_dirent, create_symlink};
pub use copy_file_exclusive::{
    CopyPermissions, copy_file_atomic, copy_file_atomic_with_permissions, copy_file_exclusive,
    copy_permissions, read_file_permissions, set_file_permissions,
};
pub use cross_device::is_cross_device;
pub use dir_lock::DirLock;
pub use ensure_file::*;
#[cfg(not(target_os = "wasi"))]
pub use file_lock::{lock_file, try_lock_file};
pub use is_not_found::is_not_found;
pub use is_subdir::is_subdir;
pub use lexical_normalize::{lexical_normalize, lexical_normalize_posix};
#[cfg(unix)]
pub use pending_temp::die_from_signal;
pub use pending_temp::{
    PendingTempFile, install_temp_file_cleanup, remove_pending_temp_files, track_temp_file,
};
pub use private_named_tempfile::private_named_tempfile_in;
pub use read_modules_dir::read_modules_dir;
pub use realpath_missing::realpath_missing;
pub use relative_path::{join_slash_separated_path, push_slash_separated_path, relative_path};
pub use remove_dirent::remove_dirent;
pub use rename_even_across_devices::rename_even_across_devices;
pub use rename_to_free_name::rename_to_free_name;
pub use retry::{
    create_dir_all_with_retry, create_dir_with_retry, is_transient_file_lock_error,
    metadata_with_retry, remove_dir_all_with_retry, remove_dir_with_retry, remove_file_with_retry,
    rename_with_retry, symlink_metadata_with_retry,
};
pub use secure_temp_lock::{
    open_secure_lock_file, secure_temp_lock_dir, secure_user_lock_dir, secure_user_lock_file_path,
};
pub use symlink_dir::*;
#[cfg(target_os = "wasi")]
pub use wasi_fs::{
    check_file_owner, create_inheriting_mode as create_file_inheriting_mode,
    create_new as create_new_with_mode, executable_access, file_link_count, lock_file,
    open_nofollow as open_file_without_following, register_sqlite_permissions, try_lock_file,
};
pub use write_atomic::{write_atomic, write_atomic_private};

mod background_drop;
mod capabilities;
mod copy_dirent;
mod copy_file_exclusive;
mod cross_device;
mod dir_lock;
mod ensure_file;
#[cfg(not(target_os = "wasi"))]
mod file_lock;
mod is_not_found;
mod is_subdir;
mod lexical_normalize;
mod pending_temp;
mod private_named_tempfile;
mod read_modules_dir;
mod realpath_missing;
mod relative_path;
mod remove_dirent;
mod rename_even_across_devices;
mod rename_to_free_name;
mod retry;
mod secure_temp_lock;
mod symlink_dir;
#[cfg(target_os = "wasi")]
mod wasi_fs;
mod write_atomic;

#[cfg(target_os = "wasi")]
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

#[cfg(target_family = "wasm")]
pub use pnpm_wasm_host::process_id;

/// Returns the supervising host's temporary directory.
#[cfg(target_family = "wasm")]
#[must_use]
pub fn temp_dir() -> std::path::PathBuf {
    std::env::var_os("PNPM_WASM_TMPDIR").expect("WASI host must supply PNPM_WASM_TMPDIR").into()
}

#[cfg(any(target_family = "wasm", all(test, unix)))]
mod path_list;
#[cfg(target_family = "wasm")]
pub use path_list::{JoinPathsError, join_paths, split_paths};
#[cfg(not(target_family = "wasm"))]
pub use std::{
    env::{JoinPathsError, join_paths, split_paths, temp_dir},
    process::id as process_id,
};
