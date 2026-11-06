//! The rendered-view caches, bounded by the bytes they hold.

use super::PackageKey;
use crate::BoundedCache;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Bytes of rendered version fragments kept in memory.
const FRAGMENT_BUDGET: usize = 64 * 1024 * 1024;
/// Bytes of assembled package views kept in memory.
const VIEW_BUDGET: usize = 64 * 1024 * 1024;

/// Bumped whenever a rendered fragment would change for the same rows, so no
/// fragment from an earlier renderer is reused.
pub(super) const RENDERER_VERSION: u32 = 1;

/// The response representation a fragment or view renders. The abbreviated
/// representation is derived from the full one by the response layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Representation {
    Full,
    /// One version's accepted manifest and its interpretation, without its
    /// README and annotations: what a release record is made of. It embeds
    /// no mutable state, so a deprecation never re-renders it.
    Accepted,
}

/// Everything a rendered version fragment depends on. A version and revision
/// always name the same bytes, since the store keeps the identity of a removed
/// version and never accepts it again, so the key needs no generation of its
/// own. A tag change touches none of it; a deprecation changes
/// `annotation_generation` of one version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FragmentKey {
    pub(super) package: PackageKey,
    pub(super) version: String,
    pub(super) revision_key: String,
    /// The interpretation profile the version was accepted under.
    pub(super) profile: String,
    pub(super) representation: Representation,
    pub(super) renderer: u32,
    /// The generation of the version's annotations, for a representation
    /// that embeds them, as the full packument embeds a deprecation.
    pub(super) annotation_generation: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ViewKey {
    pub(super) package: PackageKey,
    pub(super) representation: Representation,
}

/// Counts of rendering work since the database was opened.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RenderStats {
    /// Version fragments rendered from rows.
    pub fragments_rendered: u64,
    /// Version fragments taken from the cache.
    pub fragments_reused: u64,
    /// Package views assembled from fragments and package state.
    pub views_assembled: u64,
    /// Package views served from the cache at their current generation.
    pub views_reused: u64,
}

/// The caches every read connection shares. Each call holds the lock only
/// for the lookup or insert, never while rows are read or rendered.
#[derive(Debug, Default)]
pub(super) struct RenderCache {
    caches: Mutex<Caches>,
}

#[derive(Debug, Default)]
struct Caches {
    fragments: BoundedCache<FragmentKey, Arc<str>>,
    views: BoundedCache<ViewKey, (u64, Arc<[u8]>)>,
    stats: RenderStats,
}

impl RenderCache {
    fn lock(&self) -> MutexGuard<'_, Caches> {
        self.caches.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn stats(&self) -> RenderStats {
        self.lock().stats
    }

    pub(super) fn fragment(&self, key: &FragmentKey) -> Option<Arc<str>> {
        let mut caches = self.lock();
        let found = caches.fragments.get(key).cloned();
        if found.is_some() {
            caches.stats.fragments_reused += 1;
        }
        found
    }

    pub(super) fn store_fragment(&self, key: FragmentKey, fragment: Arc<str>) {
        let mut caches = self.lock();
        caches.stats.fragments_rendered += 1;
        let weight = fragment.len();
        caches.fragments.insert(key, fragment, weight, FRAGMENT_BUDGET);
    }

    /// The generation of the view of `key` the cache holds, if any.
    pub(super) fn view_generation(&self, key: &ViewKey) -> Option<u64> {
        self.lock().views
            .get(key)
            .map(|(generation, _)| *generation)
    }

    /// The view of `key` when the cache holds it at `generation`.
    pub(super) fn view(&self, key: &ViewKey, generation: u64) -> Option<Arc<[u8]>> {
        let mut caches = self.lock();
        let (cached, bytes) = caches.views.get(key)?;
        if *cached != generation {
            return None;
        }
        let bytes = Arc::clone(bytes);
        caches.stats.views_reused += 1;
        Some(bytes)
    }

    /// Cache the view of `key` rendered at `generation`, unless a read of a
    /// later snapshot already cached a newer one.
    pub(super) fn store_view(&self, key: ViewKey, generation: u64, bytes: Arc<[u8]>) {
        let mut caches = self.lock();
        caches.stats.views_assembled += 1;
        if caches.views
            .get(&key)
            .is_some_and(|(cached, _)| *cached > generation)
        {
            return;
        }
        let weight = bytes.len();
        caches.views.insert(key, (generation, bytes), weight, VIEW_BUDGET);
    }
}
