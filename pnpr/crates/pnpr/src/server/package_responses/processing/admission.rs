use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy)]
pub(crate) enum DocumentSource<'a> {
    Stored,
    Upstream(&'a str),
}

pub(crate) struct Admissions {
    pub(super) stored: Arc<Semaphore>,
    upstream_total: Arc<Semaphore>,
    upstreams: Mutex<HashMap<String, Arc<Semaphore>>>,
    pub(super) capacity: usize,
}

pub(crate) struct SourceAdmission {
    _source: OwnedSemaphorePermit,
    _upstream: Option<OwnedSemaphorePermit>,
}

impl Admissions {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            stored: Arc::new(Semaphore::new(capacity)),
            upstream_total: Arc::new(Semaphore::new(capacity)),
            upstreams: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    pub(super) fn upstream(&self, namespace: &str) -> Arc<Semaphore> {
        let mut upstreams = self.upstreams.lock().expect("document admission poisoned");
        Arc::clone(
            upstreams
                .entry(namespace.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.capacity.div_ceil(2)))),
        )
    }

    pub(crate) async fn acquire(&self, source: DocumentSource<'_>) -> SourceAdmission {
        let admission = match source {
            DocumentSource::Stored => Arc::clone(&self.stored),
            DocumentSource::Upstream(namespace) => self.upstream(namespace),
        };
        let source_permit =
            admission.acquire_owned().await.expect("document admission never closes");
        let upstream_permit = match source {
            DocumentSource::Stored => None,
            DocumentSource::Upstream(_) => Some(
                Arc::clone(&self.upstream_total)
                    .acquire_owned()
                    .await
                    .expect("upstream admission never closes"),
            ),
        };
        SourceAdmission { _source: source_permit, _upstream: upstream_permit }
    }

    /// Poll the source-loading future only after its origin has capacity.
    pub(crate) async fn serve<Output>(
        &self,
        source: DocumentSource<'_>,
        response: impl Future<Output = Output>,
    ) -> Output {
        let _admission = self.acquire(source).await;
        response.await
    }
}
