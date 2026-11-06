use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
pub(crate) enum DocumentSource<'a> {
    Stored,
    Upstream(&'a str),
}

pub(crate) struct Admissions {
    pub(super) stored: Arc<Semaphore>,
    upstreams: Mutex<HashMap<String, Arc<Semaphore>>>,
    pub(super) capacity: usize,
}

impl Admissions {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            stored: Arc::new(Semaphore::new(capacity)),
            upstreams: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    pub(super) fn upstream(&self, namespace: &str) -> Arc<Semaphore> {
        let mut upstreams = self.upstreams.lock().expect("document admission poisoned");
        Arc::clone(
            upstreams
                .entry(namespace.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.capacity))),
        )
    }

    /// Poll the source-loading future only after its origin has capacity.
    pub(crate) async fn serve<Output>(
        &self,
        source: DocumentSource<'_>,
        response: impl Future<Output = Output>,
    ) -> Output {
        let admission = match source {
            DocumentSource::Stored => Arc::clone(&self.stored),
            DocumentSource::Upstream(namespace) => self.upstream(namespace),
        };
        let _permit = admission.acquire_owned().await.expect("document admission never closes");
        response.await
    }
}
