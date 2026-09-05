use tokio::{sync::SemaphorePermit, task::JoinHandle};

/// Keep extraction capacity occupied until the blocking task exits, even if its
/// caller stops awaiting it.
pub(crate) fn spawn_extraction<Output: Send + 'static>(
    permit: SemaphorePermit<'static>,
    extract: impl FnOnce() -> Output + Send + 'static,
) -> JoinHandle<Output> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        extract()
    })
}

#[cfg(test)]
mod tests;
