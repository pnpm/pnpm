use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use pipe_trait::Pipe;
use pnpm_network::{AuthHeaders, ThrottledClient};
use serde::{Deserialize, Serialize};

use crate::{
    NetworkError, RegistryError, package_version::PackageVersion, package_versions::PackageVersions,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
    pub versions: PackageVersions,

    /// Per-version publish timestamps as the npm registry reports
    /// them. Each key is either a version string (value: ISO-8601
    /// timestamp) or the reserved `unpublished` key (value: object).
    /// The map is typed as `serde_json::Value` so the reserved key's
    /// object value can round-trip alongside the per-version
    /// timestamps without a custom deserializer.
    ///
    /// This is the input to the `minimumReleaseAge` verifier. Use
    /// [`Self::published_at`] for the typed per-version lookup.
    ///
    /// Optional — abbreviated metadata responses (`application/vnd.npm.install-v1+json`)
    /// omit this field; only the full-metadata fetcher used by the
    /// verifier sees it populated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<HashMap<String, serde_json::Value>>,

    /// Package-level "last modified" timestamp the abbreviated
    /// metadata endpoint sends. The verifier's
    /// `tryAbbreviatedModifiedShortcut` reads this as a conservative
    /// upper bound on every version's publish time — if `modified`
    /// is older than the policy cutoff, every version in this
    /// package was published at least that long ago.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,

    /// Last `ETag` the registry returned when this manifest was
    /// fetched. Threaded into `If-None-Match` on the next
    /// conditional GET by the cached metadata fetcher (Phase 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Package-level `homepage` URL, shown in the `Details` column of
    /// `pacquet outdated --long`.
    ///
    /// Only the full-metadata endpoint (`application/json`) carries this
    /// field; the abbreviated install metadata pacquet fetches by default
    /// (`application/vnd.npm.install-v1+json`) omits it, so it is `None`
    /// unless the registry serves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub mutex: Arc<Mutex<u8>>,

    /// Packuments derived from this one by a policy filter, keyed by
    /// the deriving code's opaque policy key. See
    /// [`DerivedPackuments`].
    #[serde(skip_serializing, skip_deserializing)]
    pub derived: DerivedPackuments,
}

impl Package {
    /// Resolved publish timestamp for `version`, or `None` when the
    /// registry didn't report one for that pin.
    #[must_use]
    pub fn published_at(&self, version: &str) -> Option<&str> {
        self.time.as_ref()?.get(version)?.as_str()
    }

    /// Drop `time` unless it carries a publish timestamp for every
    /// version this packument lists.
    ///
    /// Registries may answer with a partial map: npmmirror adds `time`
    /// to its abbreviated documents but fills it in only for the
    /// versions it has synced since it started recording publish times,
    /// leaving the rest out. A partial map is indistinguishable from a
    /// complete one at the point of use, so the `minimumReleaseAge`
    /// filter reads every absent timestamp as "not mature" and silently
    /// drops the version — resolution then falls back to the lowest
    /// match.
    ///
    /// A map that can't decide maturity is worth nothing to the
    /// resolver, so it is normalized away where the document is parsed.
    /// Every packument past that point carries either a complete `time`
    /// or none at all — the shape the npm registry's own abbreviated
    /// documents have, and the one the rest of the resolver is written
    /// against.
    /// A packument with no versions keeps whatever `time` it has — there
    /// is nothing for the map to be incomplete about — and a version whose
    /// entry is an empty string counts as absent.
    pub fn drop_incomplete_publish_times(&mut self) {
        let Some(time) = self.time.as_ref() else { return };
        let complete = self.versions.keys().all(|version| {
            time.get(version).and_then(serde_json::Value::as_str).is_some_and(|at| !at.is_empty())
        });
        if !complete {
            self.time = None;
        }
    }

    /// Version under `dist-tags.<tag>`, or `None` when the tag is
    /// absent. The picker reads `latest` (for the version-range fast
    /// path) and any user-supplied tag (e.g. `next`, `beta`) through
    /// this accessor.
    pub fn dist_tag(&self, tag: &str) -> Option<&str> {
        self.dist_tags.get(tag).map(String::as_str)
    }

    /// Iterator over all `dist-tags` entries. Used by the picker's
    /// publishedBy filter which rewrites tags after dropping versions
    /// past the cutoff. Iteration order is undefined (`HashMap`), so
    /// callers that need a stable rewrite are expected to sort.
    pub fn dist_tags(&self) -> impl Iterator<Item = (&str, &str)> {
        self.dist_tags.iter().map(|(tag, version)| (tag.as_str(), version.as_str()))
    }
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// Memo of the packuments derived from one document by a policy
/// filter, hung on the document they derive from so the derived copies
/// die with it.
///
/// A pick runs for every dependency edge and re-derives the same view
/// each time, and a derivation allocates a whole versions map, so the
/// per-edge cost is what this removes. The key is opaque here: the
/// deriving code (the resolver's publish-date filter) folds every input
/// its output depends on into the key, because one packument can be
/// served to installs running different policies.
///
/// A single install derives one view per packument, so the memo is
/// capped rather than grown: a long-lived host (the napi bindings, a
/// daemon) runs many installs against one shared meta cache, and an
/// uncapped memo would retain a derived copy from every one of them.
#[derive(Debug, Default, Clone)]
pub struct DerivedPackuments(Arc<Mutex<DerivedMemo>>);

/// Derived packuments in insertion order, so the eviction that keeps
/// [`DerivedPackuments`] bounded drops the oldest policy.
type DerivedMemo = Vec<(String, Arc<Package>)>;

const MAX_DERIVED_PACKUMENTS: usize = 4;

impl DerivedPackuments {
    /// The packument stored under `policy_key`, derived with `derive`
    /// on the first request for that key.
    ///
    /// `derive` runs outside the lock, so two threads racing on the
    /// same key can both run it; the loser drops its copy and takes the
    /// winner's, which keeps every caller of one key on one `Arc`.
    pub fn get_or_derive(
        &self,
        policy_key: &str,
        derive: impl FnOnce() -> Package,
    ) -> Arc<Package> {
        if let Some(derived) = self.get(policy_key) {
            return derived;
        }
        let derived = Arc::new(derive());
        let mut memo = self.lock();
        if let Some(winner) = find(&memo, policy_key) {
            return winner;
        }
        if memo.len() >= MAX_DERIVED_PACKUMENTS {
            memo.remove(0);
        }
        memo.push((policy_key.to_string(), Arc::clone(&derived)));
        derived
    }

    fn get(&self, policy_key: &str) -> Option<Arc<Package>> {
        find(&self.lock(), policy_key)
    }

    /// A poisoned memo is still readable: every entry is a fully-built
    /// packument, and a panic mid-`push` can't leave a half-written one.
    fn lock(&self) -> MutexGuard<'_, DerivedMemo> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn find(memo: &DerivedMemo, policy_key: &str) -> Option<Arc<Package>> {
    memo.iter().find(|(key, _)| key == policy_key).map(|(_, derived)| Arc::clone(derived))
}

impl Package {
    pub async fn fetch_from_registry(
        name: &str,
        http_client: &ThrottledClient,
        registry: &str,
        auth_headers: &AuthHeaders,
    ) -> Result<Self, RegistryError> {
        let encoded_name = pnpm_network::encode_package_name(name);
        let url = format!("{registry}{encoded_name}"); // TODO: use reqwest URL directly
        let network_error = |error| NetworkError { error, url: url.clone() };
        // Hold the semaphore permit across send + body consumption so the
        // socket-bound stays effective under concurrent fan-out. See the
        // doc comment on `ThrottledClientGuard`.
        let guard = http_client.acquire_for_url(&url).await;
        let mut request = guard.get(&url).header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        );
        if let Some(value) = auth_headers.for_url_with_package(&url, Some(name)) {
            request = request.header("authorization", value);
        }
        request
            .send()
            .await
            .map_err(network_error)?
            // An unknown package answers with a JSON error body, which
            // decodes into neither a `Package` nor a useful message.
            .error_for_status()
            .map_err(network_error)?
            .json::<Package>()
            .await
            .map_err(network_error)?
            .pipe(Ok)
    }

    #[must_use]
    pub fn pinned_version(&self, version_range: &str) -> Option<Arc<PackageVersion>> {
        let range: node_semver::Range = version_range.parse().unwrap(); // TODO: this step should have happened in PackageManifest
        // Match on the version *strings* so only winning manifests
        // hydrate from their raw fragments.
        let mut satisfying = self
            .versions
            .keys()
            .filter_map(|key| {
                key.parse::<node_semver::Version>()
                    .ok()
                    .filter(|version| version.satisfies(&range))
                    .map(|version| (version, key))
            })
            .collect::<Vec<_>>();
        satisfying.sort_by(|(left, _), (right, _)| right.partial_cmp(left).unwrap());
        satisfying.into_iter().find_map(|(_, key)| self.versions.get(key))
    }

    /// Manifest under `dist-tags.latest`, or `None` — registry-served
    /// data must not be able to panic the process.
    #[must_use]
    pub fn latest(&self) -> Option<Arc<PackageVersion>> {
        self.versions.get(self.dist_tags.get("latest")?)
    }
}

#[cfg(test)]
mod tests;
