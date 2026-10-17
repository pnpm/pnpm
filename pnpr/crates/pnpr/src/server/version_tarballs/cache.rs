use super::{CanonicalPackageName, VersionTgz};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Versions kept before the cache starts over.
const CAPACITY: usize = 16 * 1024;

/// The version dists the `name@version` route read recently, keyed by the
/// packument scope they were read from, so a tar response does not parse the
/// packument again. The scope keeps a dist read for one caller's hosted
/// namespace from answering another caller's.
#[derive(Default)]
pub(in super::super) struct VersionTgzCache {
    entries: Mutex<HashMap<(String, String, String), CachedVersionTgz>>,
}

struct CachedVersionTgz {
    expires_at: Instant,
    tgz: Arc<VersionTgz>,
}

impl VersionTgzCache {
    pub(super) fn get(
        &self,
        scope: &str,
        name: &CanonicalPackageName,
        version: &str,
    ) -> Option<Arc<VersionTgz>> {
        let entries = self.entries.lock().expect("version dist cache lock poisoned");
        let key = (scope.to_string(), name.as_str().to_string(), version.to_string());
        let cached = entries.get(&key)?;
        (Instant::now() < cached.expires_at).then(|| Arc::clone(&cached.tgz))
    }

    pub(super) fn put(
        &self,
        scope: String,
        name: &CanonicalPackageName,
        version: &str,
        tgz: Arc<VersionTgz>,
        ttl: Duration,
    ) {
        if ttl.is_zero() {
            return;
        }
        let mut entries = self.entries.lock().expect("version dist cache lock poisoned");
        if entries.len() >= CAPACITY {
            entries.clear();
        }
        let key = (scope, name.as_str().to_string(), version.to_string());
        entries.insert(key, CachedVersionTgz { expires_at: Instant::now() + ttl, tgz });
    }
}

#[cfg(test)]
mod tests;
