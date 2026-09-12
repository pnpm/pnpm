use super::{Arc, DashMap, Package, Semaphore};

/// A cached packument together with its registry-verification state.
/// The two travel as one value so a reader can never pair a packument
/// with the verification state of a concurrent overwrite.
#[derive(Debug, Clone)]
pub struct CachedPackument {
    pub meta: Arc<Package>,
    /// `false` when the packument was parsed straight from the
    /// on-disk mirror without a validating registry round-trip (see
    /// [`PackageMetaCache::set_unverified`]).
    pub registry_verified: bool,
}

/// In-memory packument cache the orchestrator consults before any
/// disk read. A thin map abstraction so a long-lived install can
/// share one cache across many [`fn@crate::pick_package`] calls.
///
/// Implementations must be safe to call concurrently from multiple
/// resolve tasks. The default [`InMemoryPackageMetaCache`] uses a
/// std `Mutex`; a tokio-aware variant can land later if the
/// contention shows up in benchmarks.
pub trait PackageMetaCache: Send + Sync {
    /// Shared handle to the cached packument for `key`, or `None`
    /// when the cache hasn't seen it. The packument is carried as
    /// [`Arc<Package>`] so cross-resolve sharing of a popular
    /// packument (`react`, `lodash`, ...) doesn't deep-clone the
    /// full versions map on every consumer's hit. Returns shared
    /// references, not copies.
    fn get(&self, key: &str) -> Option<CachedPackument>;
    /// Insert/overwrite `meta` under `key`. The orchestrator inserts
    /// after a fresh fetch and after any disk-fast-path that returns
    /// successfully — populating the cache from the disk read avoids
    /// re-paying the `spawn_blocking` + `serde_json::from_str` for
    /// every later resolve of the same `(registry, name)` within the
    /// install. The cache is install-scoped, so a disk-loaded entry
    /// can't outlive the freshness window the disk read already
    /// accepted; the next install starts a fresh cache. Takes
    /// [`Arc<Package>`] so callers can share the same handle they
    /// hand back to [`crate::PickPackageResult`] without an extra clone.
    ///
    /// The caller vouches that `meta` came from (or was revalidated
    /// by) the registry: the entry replaces any registry-unverified
    /// one a previous [`PackageMetaCache::set_unverified`] stored
    /// under `key`.
    fn set(&self, key: String, meta: Arc<Package>);

    /// Like [`PackageMetaCache::set`], but stores the entry as
    /// registry-unverified: it was parsed straight from the on-disk
    /// mirror without a validating registry round-trip, so it may
    /// predate versions the registry has. [`fn@crate::pick_package`] uses the
    /// state to fall through to a conditional registry request —
    /// instead of failing the pick — when a cache hit on such an entry
    /// can't satisfy the requested spec and the resolver isn't offline.
    /// The verified [`PackageMetaCache::set`] the fetch then performs
    /// replaces the entry, so each package revalidates at most once.
    ///
    /// Required (no default) on purpose: an implementation that stored
    /// these entries as verified would silently turn a recoverable
    /// stale-mirror miss into a terminal "no matching version".
    fn set_unverified(&self, key: String, meta: Arc<Package>);
}

/// The packument-fetching state one install owns, shared across every
/// [`PickPackageContext`](super::PickPackageContext) in it so the npm and named-registry resolvers
/// coalesce against the same in-flight set.
///
/// Both maps key on the string [`PackageMetaCache`] uses
/// (`{registry}\x00{name}` for abbreviated, `{registry}\x00{name}:full` for
/// full), so two callers asking for different forms of the same packument
/// don't accidentally serialize — the `metaDir` differentiator is embedded
/// in the key.
///
/// The state is install-scoped rather than hung on [`Package`] because a
/// caller may hand the same [`PackageMetaCache`] to install after install,
/// and each of those installs has to make its own decisions.
#[derive(Debug, Default)]
pub struct PackumentFetchState {
    /// One single-permit [`tokio::sync::Semaphore`] per in-memory cache key:
    /// the first caller acquires it and runs the disk-then-network flow,
    /// later ones wait, then re-check [`PackageMetaCache`] and short-circuit
    /// on the hit the winner just wrote, so only the first hits the network.
    pub(super) limits: DashMap<String, Arc<Semaphore>>,
    /// The packument each cache key's release-age upgrade last got a `304`
    /// for. Holding the document itself (compared by [`Arc::ptr_eq`]) rather
    /// than a bare flag keeps the answer tied to what was revalidated, so a
    /// newer response under the same key is checked on its own.
    pub(super) release_age_upgrade_checked: DashMap<String, Arc<Package>>,
}

impl PackumentFetchState {
    pub(super) fn release_age_upgrade_was_checked(
        &self,
        cache_key: &str,
        meta: &Arc<Package>,
    ) -> bool {
        self.release_age_upgrade_checked
            .get(cache_key)
            .is_some_and(|checked| Arc::ptr_eq(checked.value(), meta))
    }

    pub(super) fn mark_release_age_upgrade_checked(&self, cache_key: &str, meta: &Arc<Package>) {
        self.release_age_upgrade_checked.insert(cache_key.to_string(), Arc::clone(meta));
    }
}

pub type PackumentFetchLocker = Arc<PackumentFetchState>;

/// Construct a fresh [`PackumentFetchLocker`] for a new install.
/// Equivalent to `Default::default()`; named for symmetry with
/// [`shared_in_memory_cache`].
#[must_use]
pub fn shared_packument_fetch_locker() -> PackumentFetchLocker {
    Arc::new(PackumentFetchState::default())
}

/// Per-`(registry, pkg_name, version)` cache for the resolver's
/// serialized `manifest` JSON. The npm resolver builds
/// [`pnpm_resolving_resolver_base::ResolveResult`]'s `manifest`
/// field via `serde_json::to_value(picked)`; when many resolves
/// pick the same version of the same package (the common case for
/// shared deps like `react`, `lodash`, ...) every duplicate would
/// otherwise re-walk and re-allocate the same JSON tree. Cache the
/// `Arc<Value>` once per `(registry, pkg_name, version)` triple so
/// the second pick onwards is an `Arc::clone` instead of a full
/// reserialise.
///
/// Shared across [`crate::NpmResolver`] (default + JSR registries)
/// and [`crate::NamedRegistryResolver`] (`<alias>:` specifiers).
/// The key includes `registry` because two registries can serve
/// different artifacts under the same `name@version` — a public
/// `lodash@4.17.21` and a privately-hosted package of the same
/// name-version pair are not interchangeable, and a registry-
/// agnostic key would hand one resolver the other's manifest,
/// breaking the downstream dependency graph / peer extraction /
/// lockfile metadata. Same `{registry}\x00…` scoping shape as
/// [`PackageMetaCache`].
pub type PickedManifestCache = Arc<DashMap<String, Arc<serde_json::Value>>>;

/// Construct a fresh [`PickedManifestCache`] for a new install.
#[must_use]
pub fn shared_picked_manifest_cache() -> PickedManifestCache {
    Arc::new(DashMap::new())
}

/// Default thread-safe [`PackageMetaCache`] backed by a sharded
/// [`DashMap`]. A consumer that already has its own shared map can
/// implement the trait directly instead of using this.
///
/// Every resolve edge consults the cache before anything else, so on
/// a large graph the map takes tens of thousands of lookups from all
/// runtime workers at once — a single `Mutex<HashMap>` here was the
/// top contention point of a warm-resolve time profile.
#[derive(Debug, Default)]
pub struct InMemoryPackageMetaCache {
    pub(super) inner: DashMap<String, CachedPackument>,
}

impl PackageMetaCache for InMemoryPackageMetaCache {
    fn get(&self, key: &str) -> Option<CachedPackument> {
        self.inner.get(key).map(|entry| entry.value().clone())
    }

    fn set(&self, key: String, meta: Arc<Package>) {
        self.inner.insert(key, CachedPackument { meta, registry_verified: true });
    }

    fn set_unverified(&self, key: String, meta: Arc<Package>) {
        self.inner.insert(key, CachedPackument { meta, registry_verified: false });
    }
}

/// Shared-state helper that lets a long-running install build one
/// [`PackageMetaCache`] and pass it (by [`Arc`]) to every
/// [`fn@crate::pick_package`] call.
#[must_use]
pub fn shared_in_memory_cache() -> Arc<InMemoryPackageMetaCache> {
    Arc::new(InMemoryPackageMetaCache::default())
}
