use super::{CanonicalPackageName, VersionTgz};
use pnpr_storage::DocumentStamp;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Versions kept before the cache drops some.
const CAPACITY: usize = 16 * 1024;

/// The version dists the `name@version` route read from an upstream, so a
/// repeated request reads no metadata. Each is keyed by the upstream's cache
/// namespace and answers while its [`DistOrigin`] is fresh.
#[derive(Default)]
pub(in super::super) struct VersionTgzCache {
    entries: Mutex<HashMap<(String, String, String), CachedVersionTgz>>,
}

/// Where a cached dist was read from.
#[derive(Clone, Copy)]
pub(super) enum DistOrigin {
    /// The cached packument with this stamp. A refetched packument has
    /// another stamp, so its old dists stop answering.
    Packument(DocumentStamp),
    /// The version manifest, fetched at this instant. It answers for the
    /// packument TTL, until a fresh packument is cached.
    VersionManifest(Instant),
}

struct CachedVersionTgz {
    origin: DistOrigin,
    tgz: Arc<VersionTgz>,
}

impl VersionTgzCache {
    pub(super) fn get(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        version: &str,
        packument: Option<DocumentStamp>,
        ttl: Duration,
    ) -> Option<Arc<VersionTgz>> {
        let entries = self.entries.lock().expect("version dist cache lock poisoned");
        let key = (namespace.to_string(), name.as_str().to_string(), version.to_string());
        let cached = entries.get(&key)?;
        let fresh = match cached.origin {
            DistOrigin::Packument(stamp) => packument == Some(stamp),
            DistOrigin::VersionManifest(fetched) => packument.is_none() && fetched.elapsed() <= ttl,
        };
        fresh.then(|| Arc::clone(&cached.tgz))
    }

    pub(super) fn put(
        &self,
        namespace: String,
        name: &CanonicalPackageName,
        version: &str,
        origin: DistOrigin,
        tgz: Arc<VersionTgz>,
    ) {
        let mut entries = self.entries.lock().expect("version dist cache lock poisoned");
        if entries.len() >= CAPACITY {
            // Drop a quarter of the entries, whichever the map yields first,
            // rather than every hot one.
            let evicted: Vec<_> = entries
                .keys()
                .take(CAPACITY / 4)
                .cloned()
                .collect();
            for key in evicted {
                entries.remove(&key);
            }
        }
        let key = (namespace, name.as_str().to_string(), version.to_string());
        entries.insert(key, CachedVersionTgz { origin, tgz });
    }
}

#[cfg(test)]
mod tests;
