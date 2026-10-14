//! Gates the metadata-write syscalls that materialize `node_modules`:
//! `clonefile`, `link`, `mkdir`, `symlink` and the copy fallback.
//!
//! APFS serializes these volume-wide (see [`crate::dir_clone_cache`]),
//! so the throughput of the link phase rises with the number of calls
//! in flight only up to a knee and falls past it, as the extra callers
//! contend inside the kernel. The knee differs between Macs: a 12-core
//! machine peaked at 4 concurrent calls and a 10-core M1 Max at 16, and
//! each is 35-50% slower at the other's optimum. No fixed count fits,
//! so macOS measures the knee while linking: the gate admits `permits`
//! calls at once, counts completions, and a hill-climb moves `permits`
//! toward the width that completes the most calls per second. The
//! width the climb settles on is saved per volume and seeds the next
//! install.
//!
//! Only macOS gates. ext4 links in parallel: a warm install of 1352
//! packages on 8 vCPUs took 0.44 s on 8 or 16 threads and 0.54 s on 4.
//! There [`gated`] runs the call directly.

#[cfg(target_os = "macos")]
mod climb;
#[cfg(target_os = "macos")]
mod gate;

#[cfg(target_os = "macos")]
pub(crate) use gate::{gated, load, save};

/// Run `syscall` on the calling thread.
#[cfg(not(target_os = "macos"))]
pub(crate) fn gated<Output>(syscall: impl FnOnce() -> Output) -> Output {
    syscall()
}

/// No gate to seed.
#[cfg(not(target_os = "macos"))]
pub(crate) fn load(_store_dir: &pnpm_store_dir::StoreDir, _destination: &std::path::Path) {}

/// No gate to record.
#[cfg(not(target_os = "macos"))]
pub(crate) fn save(_config: &pnpm_config::Config, _destination: &std::path::Path) {}
