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
    Lockfile, LockfileResolution, PackageKey, PackageMetadata, PeerSatisfactionEdges,
    PlatformAssetResolution, PlatformSelector, ProjectSnapshot, SnapshotEntry,
    VariationsResolution,
};
use pnpm_package_is_installable::{WantedPlatformRef, inferred_platform, platform_is_supported};
use std::collections::{HashMap, VecDeque};

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
    /// The edges the install's walks leave out, which this walk leaves out
    /// too.
    pub skipped_peer_edges: &'l PeerSatisfactionEdges,
    pub include_optional: bool,
}

/// An archive the store keeps, and whether only optional dependencies
/// reach it, so a failure to fetch it is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptArchive {
    pub key: PackageKey,
    pub archive: StoredArchive,
    pub optional: bool,
}

/// Every archive `platforms` keep in the store that the install does not
/// link, each once, in the order the walk reaches them. An archive any
/// platform reaches through a required dependency is not optional.
#[must_use]
pub fn platform_archives(
    input: &PlatformArchivesInput<'_>,
    platforms: &[PlatformSelector],
) -> Vec<KeptArchive> {
    let mut kept: Vec<KeptArchive> = Vec::new();
    let mut positions: HashMap<(PackageKey, StoredArchive), usize> = HashMap::new();
    for platform in platforms {
        for archive in PlatformWalk::new(input, platform).run() {
            let id = (archive.key.clone(), archive.archive);
            if let Some(&position) = positions.get(&id) {
                kept[position].optional &= archive.optional;
            } else {
                positions.insert(id, kept.len());
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
    /// Each visited package, and whether only optional dependencies reached
    /// it when it was visited.
    visited: HashMap<PackageKey, bool>,
    queue: VecDeque<(PackageKey, bool)>,
    kept: Vec<KeptArchive>,
}

impl<'w> PlatformWalk<'w> {
    fn new(input: &'w PlatformArchivesInput<'w>, platform: &'w PlatformSelector) -> Self {
        let queue = input.roots
            .values()
            .flat_map(root_dependencies)
            .collect();
        PlatformWalk { input, platform, visited: HashMap::new(), queue, kept: Vec::new() }
    }

    fn run(mut self) -> Vec<KeptArchive> {
        while let Some((key, optional)) = self.queue.pop_front() {
            // A package first reached through optional dependencies is
            // visited again once a required one reaches it.
            match self.visited.get(&key) {
                Some(false) => continue,
                Some(true) if optional => continue,
                _ => {}
            }
            self.visited.insert(key.clone(), optional);
            // A required package another platform cannot run is still
            // installed there, with a warning unless `engineStrict` is set,
            // so only an optional one is left out.
            if !optional || self.runs_on_platform(&key, optional) {
                self.visit(&key, optional);
            }
        }
        self.kept
    }

    fn visit(&mut self, key: &PackageKey, optional: bool) {
        let snapshot_keys = if let Some(variations) = self.variations(key) {
            self.keep_variants(key, variations, optional)
        } else {
            if !self.input.materialized.contains_key(key) && self.has_own_archive(key) {
                let archive = StoredArchive::Own;
                self.kept.push(KeptArchive { key: key.clone(), archive, optional });
            }
            vec![key.clone()]
        };
        for snapshot_key in snapshot_keys {
            self.follow(&snapshot_key, optional);
        }
    }

    /// Whether the package of `key` runs on this platform, as the installer
    /// decides it: by the `os`, `cpu`, and `libc` its metadata declares, and
    /// for an optional dependency also by the platform its name states.
    fn runs_on_platform(&self, key: &PackageKey, optional: bool) -> bool {
        let Some(metadata) = self.metadata(key) else { return true };
        let declared = WantedPlatformRef {
            os: metadata.os.as_deref(),
            cpu: metadata.cpu.as_deref(),
            libc: metadata.libc.as_deref(),
        };
        let inferred =
            optional.then(|| inferred_platform(key.name.bare.as_str(), declared)).flatten();
        let wanted = match &inferred {
            Some(inferred) => WantedPlatformRef {
                os: inferred.os.as_deref(),
                cpu: inferred.cpu.as_deref(),
                libc: inferred.libc.as_deref(),
            },
            None => declared,
        };
        let libc = match (&self.platform.libc, self.platform.os.as_str()) {
            (Some(libc), _) => libc.as_str(),
            (None, "linux") => "glibc",
            (None, _) => "unknown",
        };
        platform_is_supported(wanted, None, &self.platform.os, &self.platform.cpu, libc)
    }
    /// Keep the variants this platform fetches, except the one the install
    /// links, and return the snapshots whose dependencies they install.
    fn keep_variants(
        &mut self,
        key: &PackageKey,
        variations: &'w VariationsResolution,
        optional: bool,
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
                let archive = StoredArchive::Variant(position);
                self.kept.push(KeptArchive { key: key.clone(), archive, optional });
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

    fn follow(&mut self, snapshot_key: &PackageKey, optional: bool) {
        let Some(snapshot) = self.snapshot(snapshot_key) else { return };
        let entries = self.input.skipped_peer_edges.followed_entries(
            snapshot_key,
            snapshot,
            self.input.include_optional,
        );
        let optional_aliases = snapshot.optional_dependencies.as_ref();
        for (alias, dep_ref) in entries {
            if dep_ref.as_link_target().is_none()
                && let Some(child) = dep_ref.resolve(alias)
            {
                let through_optional =
                    optional || optional_aliases.is_some_and(|map| map.contains_key(alias));
                self.queue.push_back((child, through_optional));
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

    fn metadata(&self, key: &PackageKey) -> Option<&'w PackageMetadata> {
        self.input.lockfile.packages
            .as_ref()?
            .get(&key.without_peer())
    }

    /// Whether the package of `key` is an archive the store keeps as
    /// downloaded. A git repository, or a tarball a git host serves, is
    /// prepared before it is the package, which may run its scripts, so a
    /// package nothing installs is not prepared for the store.
    fn has_own_archive(&self, key: &PackageKey) -> bool {
        self.metadata(key)
            .is_some_and(|metadata| match &metadata.resolution {
                LockfileResolution::Registry(_) => true,
                LockfileResolution::Tarball(tarball) => !tarball.is_git_hosted(),
                _ => false,
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

/// The snapshot keys a project's installed dependencies resolve to, and
/// whether each is an optional dependency.
fn root_dependencies(importer: &ProjectSnapshot) -> impl Iterator<Item = (PackageKey, bool)> + '_ {
    [
        (&importer.dependencies, false),
        (&importer.dev_dependencies, false),
        (&importer.optional_dependencies, true),
    ]
    .into_iter()
    .filter_map(|(map, optional)| Some((map.as_ref()?, optional)))
    .flat_map(|(map, optional)| {
        map.iter()
            .filter_map(move |(name, spec)| Some((spec.version.resolved_key(name)?, optional)))
    })
}

#[cfg(test)]
mod tests;
