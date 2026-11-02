//! The rendered-view caches, bounded by the bytes they hold.

use super::PackageKey;
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::Arc,
};

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
}

/// Everything a rendered version fragment depends on. A tag change touches
/// none of it; a deprecation changes `annotation_generation` of one version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FragmentKey {
    pub(super) package: PackageKey,
    pub(super) version: String,
    pub(super) revision_key: String,
    /// Tells a version apart from an earlier one removed and accepted again.
    pub(super) accepted_generation: u64,
    pub(super) profile: &'static str,
    pub(super) representation: Representation,
    pub(super) renderer: u32,
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

#[derive(Debug, Default)]
pub(super) struct RenderCache {
    fragments: BoundedCache<FragmentKey, Arc<str>>,
    views: BoundedCache<ViewKey, (u64, Arc<[u8]>)>,
    stats: RenderStats,
}

impl RenderCache {
    pub(super) fn stats(&self) -> RenderStats {
        self.stats
    }

    pub(super) fn fragment(&mut self, key: &FragmentKey) -> Option<Arc<str>> {
        let found = self.fragments.get(key).cloned();
        if found.is_some() {
            self.stats.fragments_reused += 1;
        }
        found
    }

    pub(super) fn store_fragment(&mut self, key: FragmentKey, fragment: Arc<str>) {
        self.stats.fragments_rendered += 1;
        let weight = fragment.len();
        self.fragments.insert(key, fragment, weight, FRAGMENT_BUDGET);
    }

    /// The view of `key` when the cache holds it at `generation`.
    pub(super) fn view(&mut self, key: &ViewKey, generation: u64) -> Option<Arc<[u8]>> {
        let (cached, bytes) = self.views.get(key)?;
        if *cached != generation {
            return None;
        }
        let bytes = Arc::clone(bytes);
        self.stats.views_reused += 1;
        Some(bytes)
    }

    pub(super) fn store_view(&mut self, key: ViewKey, generation: u64, bytes: Arc<[u8]>) {
        self.stats.views_assembled += 1;
        let weight = bytes.len();
        self.views.insert(key, (generation, bytes), weight, VIEW_BUDGET);
    }
}

/// A map that drops its oldest entries once their weights exceed a budget.
#[derive(Debug)]
struct BoundedCache<Key, Value> {
    entries: HashMap<Key, Entry<Value>>,
    /// Keys in insertion order, with the insertion that put them there.
    order: VecDeque<(Key, u64)>,
    weight: usize,
    insertions: u64,
}

#[derive(Debug)]
struct Entry<Value> {
    value: Value,
    weight: usize,
    insertion: u64,
}

impl<Key, Value> Default for BoundedCache<Key, Value> {
    fn default() -> Self {
        Self { entries: HashMap::new(), order: VecDeque::new(), weight: 0, insertions: 0 }
    }
}

impl<Key: Clone + Eq + Hash, Value> BoundedCache<Key, Value> {
    fn get(&self, key: &Key) -> Option<&Value> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    fn insert(&mut self, key: Key, value: Value, weight: usize, budget: usize) {
        self.insertions += 1;
        let entry = Entry { value, weight, insertion: self.insertions };
        if let Some(replaced) = self.entries.insert(key.clone(), entry) {
            self.weight -= replaced.weight;
        }
        self.weight += weight;
        self.order.push_back((key, self.insertions));
        while self.weight > budget {
            let Some((oldest, insertion)) = self.order.pop_front() else { break };
            if self.entries
                .get(&oldest)
                .is_some_and(|entry| entry.insertion == insertion)
                && let Some(evicted) = self.entries.remove(&oldest)
            {
                self.weight -= evicted.weight;
            }
        }
        // A replaced entry leaves its old position behind; drop those before
        // they outnumber the live ones.
        if self.order.len() > 2 * self.entries.len() + 64 {
            let entries = &self.entries;
            self.order.retain(|(key, insertion)| {
                entries
                    .get(key)
                    .is_some_and(|entry| entry.insertion == *insertion)
            });
        }
    }
}
