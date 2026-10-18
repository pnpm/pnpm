use super::{CanonicalPackageName, VersionTgz};
use pnpr_storage::DocumentStamp;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Versions kept before the cache drops some.
const CAPACITY: usize = 16 * 1024;

/// The version dists the `name@version` route of a tar registry read from
/// cached upstream packuments, so a tar response does not parse the packument
/// again. Each is keyed by the packument's cache namespace and tagged with
/// the [`DocumentStamp`] of the packument it was read from: a refetched
/// packument has another stamp, so its old dists stop answering.
#[derive(Default)]
pub(in super::super) struct VersionTgzCache {
    entries: Mutex<HashMap<(String, String, String), CachedVersionTgz>>,
}

struct CachedVersionTgz {
    stamp: DocumentStamp,
    tgz: Arc<VersionTgz>,
}

impl VersionTgzCache {
    pub(super) fn get(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        version: &str,
        stamp: DocumentStamp,
    ) -> Option<Arc<VersionTgz>> {
        let entries = self.entries.lock().expect("version dist cache lock poisoned");
        let key = (namespace.to_string(), name.as_str().to_string(), version.to_string());
        let cached = entries.get(&key)?;
        (cached.stamp == stamp).then(|| Arc::clone(&cached.tgz))
    }

    pub(super) fn put(
        &self,
        namespace: String,
        name: &CanonicalPackageName,
        version: &str,
        stamp: DocumentStamp,
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
        entries.insert(key, CachedVersionTgz { stamp, tgz });
    }
}

#[cfg(test)]
mod tests;
