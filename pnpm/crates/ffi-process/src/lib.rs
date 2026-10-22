//! Safe abstractions over the process-control foreign calls pnpm makes.
//!
//! One of the FFI abstraction crates the
//! [`unsafe` policy](../../../UNSAFE_POLICY.md) defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own. Nothing in
//! the public API hands out a raw descriptor, handle, pointer or `errno`.
//!
//! The process environment is here too. `std::env::set_var` is a wrapper
//! over the same foreign table, and it is unsound for the same reason a
//! raw call would be, so [`SingleThreaded`] carries the proof that makes
//! it sound.

#[cfg(unix)]
pub use core_dump::suppress_core_dump;
#[cfg(unix)]
pub use credentials::effective_user_id;
#[cfg(all(unix, not(target_os = "cygwin")))]
pub use credentials::real_user_id;
#[cfg(target_vendor = "apple")]
pub use descriptors::inherit_only_stdio;
pub use environment::SingleThreaded;
#[cfg(windows)]
pub use handle::{ProcessHandle, Wait};
#[cfg(unix)]
pub use inheritance::allow_inheritance;
#[cfg(windows)]
pub use job::{Job, enclosing_job_releases_children};
#[cfg(target_os = "macos")]
pub use kernel::darwin_major_version;
#[cfg(windows)]
pub use machine::native_architecture;
#[cfg(unix)]
pub use open_file_limit::{raise_open_file_limit, soft_open_file_limit};
#[cfg(windows)]
pub use process_list::{has_children, runs_image};
#[cfg(unix)]
pub use umask::current_umask;

#[cfg(unix)]
mod core_dump;
#[cfg(unix)]
mod credentials;
#[cfg(target_vendor = "apple")]
mod descriptors;
mod environment;
#[cfg(windows)]
mod handle;
#[cfg(unix)]
mod inheritance;
#[cfg(windows)]
mod job;
#[cfg(target_os = "macos")]
mod kernel;
#[cfg(windows)]
mod machine;
#[cfg(unix)]
mod open_file_limit;
#[cfg(windows)]
mod process_list;
#[cfg(unix)]
mod umask;
