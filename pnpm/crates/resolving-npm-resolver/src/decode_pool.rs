//! The threads that decode registry metadata responses.
//!
//! glibc gives each thread its own malloc arena, up to eight per CPU, and
//! keeps the memory freed in it. Decoding a packument allocates the whole
//! body and every version's fragment, so each thread that decodes ends up
//! holding an arena of freed packument memory. Decodes run on this pool of
//! a few threads, not on tokio's blocking pool, whose dozens of threads
//! would each hold such an arena (pnpm/pnpm#17020).

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::LazyLock,
};

/// In pnpm/pnpm#17020, four threads came within 0.2 GB of the peak RSS that
/// capping the whole process at four arenas reached. That cap slowed warm
/// installs.
const MAX_DECODE_THREADS: usize = 4;

/// Run `decode` on the decode pool, or on tokio's blocking pool if the
/// decode pool could not be built. A panic in `decode` resumes in the
/// caller.
pub(crate) async fn run_decode<Output: Send + 'static>(
    decode: impl FnOnce() -> Output + Send + 'static,
) -> Result<Output, tokio::task::JoinError> {
    let Some(pool) = decode_pool() else {
        return tokio::task::spawn_blocking(decode).await;
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    pool.spawn(move || {
        let _ = sender.send(catch_unwind(AssertUnwindSafe(decode)));
    });
    match receiver.await.expect("the decode pool runs every task it accepts") {
        Ok(output) => Ok(output),
        Err(panic) => resume_unwind(panic),
    }
}

fn decode_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: LazyLock<Option<rayon::ThreadPool>> = LazyLock::new(|| {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.min(MAX_DECODE_THREADS))
            .thread_name(|index| format!("pnpm-metadata-decode-{index}"))
            .build()
            .map_err(|error| {
                tracing::warn!(
                    target: "pnpm_resolving_npm_resolver",
                    ?error,
                    "failed to build the metadata decode pool; falling back to the blocking pool",
                );
            })
            .ok()
    });
    POOL.as_ref()
}

#[cfg(test)]
mod tests;
