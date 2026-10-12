//! Rayon thread pool sizing policy and configuration.
//!
//! Sizes rayon's global pool for installation workloads across the CLI
//! and the Node.js addon.

use std::sync::Once;

/// Size rayon's global pool with [`rayon_pool_size`].
///
/// Must run before anything touches rayon. The first parallel iterator
/// builds the global pool at rayon's default size, after which this
/// call can no longer size it, so debug builds assert that it did. The
/// repeat-install fast path uses rayon for workspace discovery.
///
/// Deliberately NOT communicated via the `RAYON_NUM_THREADS`
/// environment variable: a process-env write would leak into every
/// child the install spawns (lifecycle scripts, `node --version`,
/// git), and pnpm exposes no such variable to scripts. An explicit
/// `RAYON_NUM_THREADS` from the caller is honoured by skipping the
/// override.
///
/// Use [`std::thread::available_parallelism`] rather than the
/// workspace's existing `num_cpus::get()` so cgroup / CPU-quota
/// limits in containers and CI runners are respected — `num_cpus`
/// reports the host's logical CPU count, which on a quota-limited
/// runner can spin up far more rayon threads than the kernel will
/// actually schedule onto our cores.
pub fn configure_rayon_pool() {
    static INITIALIZE: Once = Once::new();
    INITIALIZE.call_once(|| {
        if std::env::var_os("RAYON_NUM_THREADS").is_some() {
            return;
        }
        let parallelism =
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        let built = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon_pool_size(parallelism, RAYON_THREADS_PER_CORE))
            .build_global();
        let configured = built.is_ok();
        debug_assert!(
            configured,
            "rayon's global pool was built before it was configured: {built:?}",
        );
    });
}

/// `threads_per_core × parallelism`, kept between [`MIN_RAYON_THREADS`]
/// and [`MAX_RAYON_THREADS`]. See [`RAYON_THREADS_PER_CORE`] for the
/// multiplier. The link phase is dominated by clonefile /
/// hardlink syscalls that block the calling thread on the kernel's
/// metadata journal, not by CPU work, so oversubscribing CPUs gives
/// more in-flight syscalls and a higher effective throughput.
/// Empirically sweeping 4-200 threads on a 1352-package warm install
/// on macOS APFS, 2× was the knee — fewer threads underutilize the
/// journal, way more (100+) loses to context switching and per-thread
/// fixed costs (`user` time scales linearly past 50 without any
/// wall-time payoff). That knee holds up to the ceiling below. Past 16
/// threads, 2× no longer pays off.
///
/// **Floor of 4 threads is intentional.** A 1-2-CPU CI runner left
/// at `2 × parallelism` would be capped to 2-4 rayon threads, and
/// at that point we go back to the original "one rayon thread is
/// blocked on a `clonefile` while the next fully-ready snapshot
/// can't even start" pattern that the 2× tuning is trying to
/// avoid. The kernel metadata journal is the bottleneck even on
/// small hosts, so a small intentional oversubscription
/// (`max(4, 2 × parallelism)`) is a better trade than respecting the
/// quota literally.
///
/// **Ceiling of 16 threads.** Past 16, the extra workers add system
/// time without shortening the install. A warm-install sweep (nuxt, next, nitro
/// fixtures) found no machine where 2× beat 16 threads: a 32-vCPU
/// Linux runner kept its wall time and halved its system time, a
/// 16-vCPU Windows runner got 10% faster, and a 10-core M1 Max, the
/// one host where 2× beat 8 threads, was unchanged at 16
/// (pnpm/tasks#51). A ceiling of 8 cut Linux system time further and
/// sped up Windows, but cost that Mac 19%.
#[must_use]
pub fn rayon_pool_size(parallelism: usize, threads_per_core: usize) -> usize {
    parallelism.saturating_mul(threads_per_core).clamp(MIN_RAYON_THREADS, MAX_RAYON_THREADS)
}

/// Two threads per core, except on Windows, where the sweeps mostly
/// favoured one (pnpm/tasks#52). Warm frozen installs were 4-5% faster
/// at 1× on 4- and 8-vCPU runners. A fresh install of the 1352-package
/// benchmark fixture took 3.9 s at 1× and 4.5 s at 2× on a 4-vCPU
/// runner, and was even on an 8-vCPU one. A 16-vCPU runner was fastest
/// at 4 threads, below what this multiplier gives it.
pub const RAYON_THREADS_PER_CORE: usize = if cfg!(windows) { 1 } else { 2 };
pub const MIN_RAYON_THREADS: usize = 4;
pub const MAX_RAYON_THREADS: usize = 16;

#[cfg(test)]
mod tests;
