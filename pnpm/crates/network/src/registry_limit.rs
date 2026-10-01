//! The `networkConcurrency` a `registries` entry sets: a cap on the requests
//! in flight to that registry's origin, below the global concurrency.

use crate::origin_of_url;
use std::{
    collections::{BTreeMap, HashMap},
    num::NonZeroUsize,
    sync::Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Keyed by the origin a request targets rather than the proxy it is sent
/// through, because the cap is about the registry server.
#[derive(Debug, Default)]
pub(crate) struct RegistryLimits {
    by_origin: HashMap<String, Arc<Semaphore>>,
}

impl RegistryLimits {
    /// A slot for `url`'s origin, or `None` when no registry caps it.
    pub(crate) async fn acquire(&self, url: &str) -> Option<OwnedSemaphorePermit> {
        if self.by_origin.is_empty() {
            return None;
        }
        let slots = Arc::clone(self.by_origin.get(&origin_of(url)?)?);
        Some(slots.acquire_owned().await.expect("registry limit semaphore is never closed"))
    }
}

fn origin_of(url: &str) -> Option<String> {
    origin_of_url(&reqwest::Url::parse(url).ok()?)
}

impl RegistryLimits {
    /// Registries that share an origin share the smallest of their caps.
    pub(crate) fn new(limits: &BTreeMap<String, NonZeroUsize>) -> Self {
        let mut by_origin: HashMap<String, usize> = HashMap::new();
        for (registry, limit) in limits {
            let Some(origin) = origin_of(registry) else {
                continue;
            };
            by_origin
                .entry(origin)
                .and_modify(|smallest| *smallest = (*smallest).min(limit.get()))
                .or_insert_with(|| limit.get());
        }
        RegistryLimits {
            by_origin: by_origin
                .into_iter()
                .map(|(origin, limit)| (origin, Arc::new(Semaphore::new(limit))))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
