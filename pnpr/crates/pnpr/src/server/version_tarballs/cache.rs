use super::{
    CanonicalPackageName, DistBlock, PackumentDists, Value, tarball_basename_for_package,
    tgz_integrity_key, version_route_tarball_version,
};
use pnpr_storage::DocumentStamp;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) struct VersionTgz {
    pub(super) filename: String,
    pub(super) version_route: Option<String>,
    pub(super) dist: DistBlock,
    pub(super) integrity_key: Option<String>,
}

pub(super) fn version_tgz(
    packument: &[u8],
    name: &CanonicalPackageName,
    version: &str,
) -> Option<VersionTgz> {
    let mut packument: PackumentDists = serde_json::from_slice(packument).ok()?;
    let tgz = dist_tgz(name, packument.versions.shift_remove(version)?.dist?)?;
    let shared_filename = packument.versions
        .values()
        .filter_map(|manifest| manifest.dist.as_ref()?.tarball.as_deref())
        .filter_map(|url| tarball_basename_for_package(url, name))
        .any(|basename| basename == tgz.filename);
    (!shared_filename).then_some(tgz)
}

#[derive(serde::Deserialize)]
struct VersionManifest {
    version: String,
    #[serde(default)]
    dist: Option<DistBlock>,
}

/// The dist of a version manifest for `version`. An upstream resolves a
/// dist-tag at the same URL, so a manifest of another version is refused. A
/// dist without a valid integrity or with a non-canonical filename is left to
/// the packument, which carries the integrity pnpr pins for it and checks that
/// no other version claims the same filename.
pub(super) fn version_manifest_tgz(
    manifest: &[u8],
    name: &CanonicalPackageName,
    version: &str,
) -> Option<VersionTgz> {
    let manifest: VersionManifest = serde_json::from_slice(manifest).ok()?;
    let dist = manifest.dist.filter(|dist| dist.integrity.is_some())?;
    let tgz = (manifest.version == version).then(|| dist_tgz(name, dist))??;
    (tgz.integrity_key.is_some() && tgz.filename == name.tarball_name_for_version(version))
        .then_some(tgz)
}

fn dist_tgz(name: &CanonicalPackageName, dist: DistBlock) -> Option<VersionTgz> {
    let url = dist.tarball.as_deref()?;
    let filename = tarball_basename_for_package(url, name)?.into_owned();
    let version_route = version_route_tarball_version(url, name).map(str::to_string);
    let mut fields = serde_json::Map::new();
    if let Some(integrity) = &dist.integrity {
        fields.insert("integrity".to_string(), Value::String(integrity.clone()));
    }
    if let Some(shasum) = &dist.shasum {
        fields.insert("shasum".to_string(), Value::String(shasum.clone()));
    }
    Some(VersionTgz { filename, version_route, integrity_key: tgz_integrity_key(&fields), dist })
}

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
    pub(super) fn get_bound(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        version: &str,
    ) -> Option<Arc<VersionTgz>> {
        let entries = self.entries.lock().expect("version dist cache lock poisoned");
        let key = (namespace.to_string(), name.as_str().to_string(), version.to_string());
        entries.get(&key).map(|cached| Arc::clone(&cached.tgz))
    }

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
