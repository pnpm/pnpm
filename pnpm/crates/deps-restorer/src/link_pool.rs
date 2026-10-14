//! The thread pool that materializes package files.

/// How many workers materialize package files at once on macOS, unless
/// [`MACOS_LINK_THREADS_ENV`] says otherwise.
///
/// APFS serializes metadata writes volume-wide (see
/// [`crate::dir_clone_cache`]), so the limit is the volume's, not the
/// CPU count's: workers past it only queue inside the kernel. Cloning
/// 40k files into 2k directories on a 12-core Mac ran at 12k files/s
/// on 1 thread, 21k on 4, 8k on 8 and 5k on 16.
#[cfg(target_os = "macos")]
const MACOS_LINK_THREADS: usize = 4;

/// The environment variable that overrides [`MACOS_LINK_THREADS`], for
/// volumes whose limit differs from the measured one.
#[cfg(target_os = "macos")]
const MACOS_LINK_THREADS_ENV: &str = "PNPM_MACOS_LINK_THREADS";

/// The thread count `configured` asks for, or [`MACOS_LINK_THREADS`] when
/// it is unset or not a positive number.
#[cfg(target_os = "macos")]
fn link_threads(configured: Option<&str>) -> usize {
    configured
        .and_then(|threads| threads.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or(MACOS_LINK_THREADS)
}

/// Run `link`, and every rayon task it spawns, on the link pool.
#[cfg(target_os = "macos")]
pub(crate) fn in_link_pool<Output: Send>(link: impl FnOnce() -> Output + Send) -> Output {
    use std::sync::LazyLock;

    static POOL: LazyLock<Option<rayon::ThreadPool>> = LazyLock::new(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(link_threads(std::env::var(MACOS_LINK_THREADS_ENV).ok().as_deref()))
            .thread_name(|index| format!("pnpm-link-{index}"))
            .build()
            .ok()
    });
    match POOL.as_ref() {
        Some(pool) => pool.install(link),
        None => link(),
    }
}

/// Run `link` on the pool of the calling thread.
///
/// Only macOS has a link pool. ext4 links in parallel: a warm install of
/// 1352 packages on 8 vCPUs took 0.44 s on 8 or 16 threads and 0.54 s on 4.
#[cfg(not(target_os = "macos"))]
pub(crate) fn in_link_pool<Output: Send>(link: impl FnOnce() -> Output + Send) -> Output {
    link()
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
