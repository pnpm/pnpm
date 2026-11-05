//! The archives `supportedArchitectures` keeps in the store for its
//! platforms beside the ones the install links, and the packages those
//! archives depend on
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)).
//!
//! For each platform, the walk starts at the installed projects and keeps,
//! for a package with variants, every archive the platform fetches: each
//! variant that matches it, and the archive that targets `any` when no
//! variant can be linked there. It follows the dependencies of each kept
//! archive, from the variant's own snapshot when it has one, so a package
//! reachable only through an archive no listed platform keeps is left out.

use super::variant_snapshots::variant_snapshot_key;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PlatformAssetResolution, PlatformSelector,
    ProjectSnapshot, SnapshotEntry, VariationsResolution,
};
use std::collections::{HashMap, HashSet, VecDeque};

/// Which archive of a package the store keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoredArchive {
    /// The variant at this position of the version's `variations`.
    Variant(usize),
    /// The version's one archive, for a package without variants.
    Own,
}

/// What [`platform_archives`] walks.
pub struct PlatformArchivesInput<'l> {
    /// The whole wanted lockfile, with every variant's own snapshot.
    pub lockfile: &'l Lockfile,
    /// The projects the install materializes, with the dependencies it
    /// installs for them.
    pub roots: &'l HashMap<String, ProjectSnapshot>,
    /// The snapshots the install materializes: their linked archive is
    /// already in the store.
    pub materialized: &'l HashMap<PackageKey, SnapshotEntry>,
    /// The platform the install links variants for.
    pub linked: &'l PlatformSelector,
    pub include_optional: bool,
}

/// Every archive `platforms` keep in the store that the install does not
/// link, each once, in the order the walk reaches them.
#[must_use]
pub fn platform_archives(
    input: &PlatformArchivesInput<'_>,
    platforms: &[PlatformSelector],
) -> Vec<(PackageKey, StoredArchive)> {
    let mut kept = Vec::new();
    let mut seen = HashSet::new();
    for platform in platforms {
        for archive in PlatformWalk::new(input, platform).run() {
            if seen.insert(archive.clone()) {
                kept.push(archive);
            }
        }
    }
    kept
}

/// One platform's walk over the lockfile.
struct PlatformWalk<'w> {
    input: &'w PlatformArchivesInput<'w>,
    platform: &'w PlatformSelector,
    visited: HashSet<PackageKey>,
    queue: VecDeque<PackageKey>,
    kept: Vec<(PackageKey, StoredArchive)>,
}

impl<'w> PlatformWalk<'w> {
    fn new(input: &'w PlatformArchivesInput<'w>, platform: &'w PlatformSelector) -> Self {
        let queue = input.roots
            .values()
            .flat_map(root_dependencies)
            .collect();
        PlatformWalk { input, platform, visited: HashSet::new(), queue, kept: Vec::new() }
    }

    fn run(mut self) -> Vec<(PackageKey, StoredArchive)> {
        while let Some(key) = self.queue.pop_front() {
            if self.visited.insert(key.clone()) {
                self.visit(&key);
            }
        }
        self.kept
    }

    fn visit(&mut self, key: &PackageKey) {
        let snapshot_keys = if let Some(variations) = self.variations(key) {
            self.keep_variants(key, variations)
        } else {
            if !self.input.materialized.contains_key(key) && self.has_own_archive(key) {
                self.kept.push((key.clone(), StoredArchive::Own));
            }
            vec![key.clone()]
        };
        for snapshot_key in snapshot_keys {
            self.follow(&snapshot_key);
        }
    }

    /// Keep the variants this platform fetches, except the one the install
    /// links, and return the snapshots whose dependencies they install.
    fn keep_variants(
        &mut self,
        key: &PackageKey,
        variations: &'w VariationsResolution,
    ) -> Vec<PackageKey> {
        let linked = self.input.materialized
            .contains_key(key)
            .then(|| {
                variations
                    .select(self.input.linked)
                    .ok()
                    .flatten()
            })
            .flatten();
        for archive in stored_archives(variations, std::slice::from_ref(self.platform), linked) {
            if let Some(position) =
                variations.variants.iter().position(|variant| std::ptr::eq(variant, archive))
            {
                self.kept.push((key.clone(), StoredArchive::Variant(position)));
            }
        }
        let mut snapshot_keys = Vec::new();
        for archive in variations.fetched_for(self.platform) {
            let own = archive
                .variant_tag()
                .and_then(|tag| variant_snapshot_key(key, &tag))
                .filter(|variant_key| self.snapshot(variant_key).is_some());
            let snapshot_key = own.unwrap_or_else(|| key.clone());
            if !snapshot_keys.contains(&snapshot_key) {
                snapshot_keys.push(snapshot_key);
            }
        }
        snapshot_keys
    }

    fn follow(&mut self, snapshot_key: &PackageKey) {
        let Some(snapshot) = self.snapshot(snapshot_key) else { return };
        let optional = self.input.include_optional
            .then_some(snapshot.optional_dependencies.as_ref())
            .flatten();
        for (alias, dep_ref) in snapshot.dependencies
            .iter()
            .chain(optional)
            .flatten()
        {
            if dep_ref.as_link_target().is_none()
                && let Some(child) = dep_ref.resolve(alias)
            {
                self.queue.push_back(child);
            }
        }
    }

    fn snapshot(&self, key: &PackageKey) -> Option<&'w SnapshotEntry> {
        self.input.lockfile.snapshots.as_ref()?.get(key)
    }

    /// A registry package's variants. A runtime's archives follow its own
    /// selection, so the store keeps none of them here.
    fn variations(&self, key: &PackageKey) -> Option<&'w VariationsResolution> {
        match &self.input.lockfile.packages
            .as_ref()?
            .get(&key.without_peer())?
            .resolution
        {
            LockfileResolution::Variations(variations) if !variations.is_runtime() => {
                Some(variations)
            }
            _ => None,
        }
    }

    fn has_own_archive(&self, key: &PackageKey) -> bool {
        self.input.lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&key.without_peer()))
            .is_some_and(|metadata| {
                matches!(
                    metadata.resolution,
                    LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
                )
            })
    }
}

/// The archives of a registry package's `variations` that `platforms` keep
/// in the store, except `linked`, each once.
pub(crate) fn stored_archives<'a>(
    variations: &'a VariationsResolution,
    platforms: &[PlatformSelector],
    linked: Option<&PlatformAssetResolution>,
) -> Vec<&'a PlatformAssetResolution> {
    let mut stored: Vec<&PlatformAssetResolution> = Vec::new();
    for archive in platforms
        .iter()
        .flat_map(|platform| variations.fetched_for(platform))
    {
        let known = linked.is_some_and(|linked| std::ptr::eq(archive, linked))
            || stored.iter().any(|kept| std::ptr::eq(*kept, archive));
        if !known {
            stored.push(archive);
        }
    }
    stored
}

/// The snapshot keys a project's installed dependencies resolve to.
fn root_dependencies(importer: &ProjectSnapshot) -> impl Iterator<Item = PackageKey> + '_ {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(|map| map.iter())
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
}

#[cfg(test)]
mod tests;
