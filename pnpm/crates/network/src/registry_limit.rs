//! The `networkConcurrency` a `registries` entry sets: a cap on the requests
//! in flight to that registry's origin, below the global concurrency.

use crate::{
    origin_of_url,
    priority_semaphore::{Permit, PrioritySemaphore},
};
use std::{
    collections::{BTreeMap, HashMap},
    num::NonZeroUsize,
};

/// Keyed by the origin a request targets rather than the proxy it is sent
/// through, because the cap is about the registry server. Each cap grants
/// its slots by the same priority classes as the global semaphore, so
/// queued downloads cannot hold every slot of a small cap ahead of the
/// metadata requests resolution waits on.
#[derive(Debug, Default)]
pub(crate) struct RegistryLimits {
    by_origin: HashMap<String, PrioritySemaphore>,
}

fn origin_of(url: &str) -> Option<String> {
    origin_of_url(&reqwest::Url::parse(url).ok()?)
}

impl RegistryLimits {
    /// Registries that share an origin share the smallest of their caps. A
    /// cap above `global_limit` could never fill, so it is held to it.
    pub(crate) fn new(limits: &BTreeMap<String, NonZeroUsize>, global_limit: usize) -> Self {
        let mut by_origin: HashMap<String, usize> = HashMap::new();
        for (registry, limit) in limits {
            let Some(origin) = origin_of(registry) else {
                continue;
            };
            by_origin
                .entry(origin)
                .and_modify(|smallest| *smallest = (*smallest).min(limit.get()))
                .or_insert_with(|| limit.get().min(global_limit));
        }
        RegistryLimits {
            by_origin: by_origin
                .into_iter()
                .map(|(origin, limit)| (origin, PrioritySemaphore::new(limit)))
                .collect(),
        }
    }

    /// A slot for `url`'s origin at `priority`, or `None` when no registry
    /// caps it.
    pub(crate) async fn acquire(&self, url: &str, priority: u64) -> Option<Permit> {
        if self.by_origin.is_empty() {
            return None;
        }
        let slots = self.by_origin.get(&origin_of(url)?)?;
        Some(slots.acquire(priority).await)
    }

    #[cfg(test)]
    pub(crate) fn slots_for(&self, url: &str) -> Option<&PrioritySemaphore> {
        self.by_origin.get(&origin_of(url)?)
    }
}

#[cfg(test)]
mod tests;
