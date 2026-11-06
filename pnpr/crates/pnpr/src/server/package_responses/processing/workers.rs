use super::RegistryError;
use std::sync::Arc;

pub(crate) struct Workers {
    permits: Arc<tokio::sync::Semaphore>,
}

impl Workers {
    pub(super) fn new(count: usize) -> Self {
        Self { permits: Arc::new(tokio::sync::Semaphore::new(count)) }
    }

    /// The permit stays with the blocking work even if its request is cancelled.
    pub(crate) async fn run<Output, Work>(&self, work: Work) -> Result<Output, RegistryError>
    where
        Output: Send + 'static,
        Work: FnOnce() -> Output + Send + 'static,
    {
        let permit =
            Arc::clone(&self.permits).acquire_owned().await.expect("document workers never close");
        Ok(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await?)
    }

    #[cfg(test)]
    pub(super) fn available(&self) -> usize {
        self.permits.available_permits()
    }
}
