//! The directory packages a package that is not a directory depends on, such
//! as a directory an override or a package extension gives a registry
//! package. An importer's walk descends through directory packages only, so
//! it never reaches them.

use super::{
    FreshnessCheckError, ImporterSatisfactionCheck, LocalDepContext, is_deploy_file_url,
    package::check_single_directory_dep_freshness,
    scope::{AliasScope, Visited, importer_aliases, snapshot_aliases},
    spec_dirs,
};
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PkgName, SnapshotDepRef, SnapshotEntry,
};
use pnpm_package_manifest::DependencyGroup;
use std::collections::{HashMap, HashSet};

/// A directory package beneath a package that is not one.
pub(crate) struct NestedDirectoryPackage {
    /// The alias a parent depends on it by.
    pub(in crate::install::lockfile_freshness) alias: String,
    pub(in crate::install::lockfile_freshness) key: PackageKey,
    pub(in crate::install::lockfile_freshness) directory: String,
    /// The aliases of every package above it, which supply its peers with
    /// the aliases of the importer checking it. Ancestors on different paths
    /// add up, which only widens the peers the check takes as supplied.
    ancestor_aliases: AliasScope,
    /// The importers that reach it.
    pub(in crate::install::lockfile_freshness) importers: HashSet<String>,
}

/// Which dependency edges a walk of the lockfile graph follows. An install
/// without optional dependencies leaves the `optionalDependencies` edges out.
#[derive(Clone, Copy)]
pub(crate) enum GraphEdges {
    All,
    Required,
}

impl GraphEdges {
    pub(crate) fn installed(optional_included: bool) -> Self {
        if optional_included { GraphEdges::All } else { GraphEdges::Required }
    }
}

/// Every [`NestedDirectoryPackage`] in `lockfile` along `edges`.
pub(crate) fn nested_directory_packages(
    lockfile: &Lockfile,
    edges: GraphEdges,
) -> Vec<NestedDirectoryPackage> {
    let nested = nested_directory_entries(lockfile, edges);
    let Some(snapshots) = lockfile.snapshots
        .as_ref()
        .filter(|_| !nested.is_empty())
    else {
        return Vec::new();
    };
    let graph = ReverseGraph::of(lockfile, snapshots, edges);
    nested
        .into_iter()
        .map(|(key, entry)| {
            let (ancestor_aliases, importers) = graph.ancestors(&key);
            NestedDirectoryPackage {
                alias: entry.alias,
                key,
                directory: entry.directory.to_string(),
                ancestor_aliases,
                importers,
            }
        })
        .collect()
}

/// A nested directory package as the lockfile's edges reach it.
struct NestedEntry<'a> {
    /// The alias a parent depends on it by.
    alias: String,
    directory: &'a str,
}

/// Every nested directory package in `lockfile` along `edges`. Empty,
/// without reading any snapshot, when the lockfile has no directory package.
fn nested_directory_entries(
    lockfile: &Lockfile,
    edges: GraphEdges,
) -> HashMap<PackageKey, NestedEntry<'_>> {
    let (Some(packages), Some(snapshots)) = (&lockfile.packages, &lockfile.snapshots) else {
        return HashMap::new();
    };
    let directories = DirectoryPackages::of(packages);
    if directories.by_key.is_empty() {
        return HashMap::new();
    }
    snapshots
        .iter()
        .filter(|(parent, _)| directories.directory(parent).is_none())
        .flat_map(|(_, snapshot)| snapshot_dependencies(snapshot, edges))
        .filter_map(|(alias, child)| {
            let (key, directory) = directories.nested_child(alias, child)?;
            Some((key, NestedEntry { alias: alias.to_string(), directory }))
        })
        .collect()
}

/// The nested directory packages each importer's check walks.
pub(crate) struct NestedByImporter<'n>(HashMap<&'n str, Vec<&'n NestedDirectoryPackage>>);

impl<'n> NestedByImporter<'n> {
    /// Assigns each of `nested` to the importer with the lowest id among
    /// those that reach it and that `is_checked` accepts. One that no such
    /// importer reaches is left out, as its importers are.
    pub(crate) fn assign(
        nested: &'n [NestedDirectoryPackage],
        is_checked: impl Fn(&str) -> bool,
    ) -> Self {
        let mut by_importer: HashMap<&str, Vec<_>> = HashMap::new();
        for package in nested {
            if let Some(importer_id) = package.importers
                .iter()
                .filter(|importer_id| is_checked(importer_id))
                .min()
            {
                by_importer
                    .entry(importer_id.as_str())
                    .or_default()
                    .push(package);
            }
        }
        NestedByImporter(by_importer)
    }

    pub(crate) fn packages(&self) -> impl Iterator<Item = &'n NestedDirectoryPackage> + '_ {
        self.0.values().flatten().copied()
    }

    pub(crate) fn of(&self, importer_id: &str) -> &[&'n NestedDirectoryPackage] {
        self.0
            .get(importer_id)
            .map_or(&[], Vec::as_slice)
    }
}

/// The directory packages of a lockfile, by their `packages:` key.
struct DirectoryPackages<'a> {
    by_key: HashMap<&'a PackageKey, &'a str>,
    names: HashSet<&'a PkgName>,
}

impl<'a> DirectoryPackages<'a> {
    fn of(packages: &'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>) -> Self {
        let by_key: HashMap<&PackageKey, &str> = packages
            .iter()
            .filter_map(|(key, metadata)| match &metadata.resolution {
                LockfileResolution::Directory(resolution) => {
                    Some((key, resolution.directory.as_str()))
                }
                _ => None,
            })
            .collect();
        let names = by_key
            .keys()
            .map(|key| &key.name)
            .collect();
        DirectoryPackages { by_key, names }
    }

    /// The directory of `key`'s package. The name is checked first, so a
    /// key of another package is not stripped of its peers.
    fn directory(&self, key: &PackageKey) -> Option<&'a str> {
        if !self.names.contains(&key.name) {
            return None;
        }
        self.by_key
            .get(&key.without_peer())
            .copied()
    }

    /// The entry of a dependency of a package that is not a directory, when
    /// it is a directory package other than a `pnpm deploy` one. A deploy
    /// entry is recognized as [`super::is_deploy_file_url_key`] recognizes
    /// one: an aliased reference to a `file://` key.
    fn nested_child(
        &self,
        alias: &PkgName,
        child: &SnapshotDepRef,
    ) -> Option<(PackageKey, &'a str)> {
        if !self.names.contains(target_name(alias, child)) {
            return None;
        }
        let key = child.resolve(alias)?;
        let directory = self
            .directory(&key)
            .filter(|directory| {
                !(matches!(child, SnapshotDepRef::Alias(_)) && is_deploy_file_url(&key, directory))
            })?;
        Some((key, directory))
    }
}

/// Checks `nested` as the importer walk checks a directory package. The
/// `file:` and `workspace:` declarations can read different manifests (see
/// [`super::unhooked_manifest`]), and the lockfile does not record which one
/// the parent used, so the snapshot must match both. Where they differ, a
/// lockfile the resolver just wrote fails the check: the price of never
/// accepting a stale one until the lockfile records the declaration
/// (pnpm/tasks#228).
pub(crate) fn check_nested_directory_package(
    check: &ImporterSatisfactionCheck<'_>,
    nested: &NestedDirectoryPackage,
) -> Result<(), FreshnessCheckError> {
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&nested.key.without_peer()))
    else {
        return Ok(());
    };
    let snapshot = check.lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&nested.key));
    let dir = check.lockfile_dir.join(&nested.directory);
    let mut scope = nested.ancestor_aliases.clone();
    scope.extend(
        check.lockfile.importers
            .get(check.importer_id)
            .into_iter()
            .flat_map(importer_aliases),
    );
    let visited = Visited::new(check.lockfile);
    let check_declared = |declared_as_file| {
        let dep = LocalDepContext {
            name: &nested.alias,
            declared_as_file,
            rel_path: &nested.directory,
            dir: &dir,
            dirs: spec_dirs(check),
            catalogs: check.workspace.catalogs,
        };
        check_single_directory_dep_freshness(
            check,
            &scope,
            &dep,
            (&nested.key, snapshot, pkg_meta),
            &mut visited.clone(),
        )
    };
    let declared_as_workspace = false;
    let declared_as_file = true;
    check_declared(declared_as_workspace).and_then(|()| check_declared(declared_as_file))
}

fn snapshot_dependencies(
    snapshot: &SnapshotEntry,
    edges: GraphEdges,
) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    let optional = match edges {
        GraphEdges::All => snapshot.optional_dependencies.as_ref(),
        GraphEdges::Required => None,
    };
    [snapshot.dependencies.as_ref(), optional]
        .into_iter()
        .flatten()
        .flatten()
}

/// The name of the package `child` resolves to, without building its key.
fn target_name<'a>(alias: &'a PkgName, child: &'a SnapshotDepRef) -> &'a PkgName {
    match child {
        SnapshotDepRef::Alias(key) => &key.name,
        SnapshotDepRef::Plain(_) | SnapshotDepRef::Link(_) => alias,
    }
}

/// Who depends on each snapshot.
struct ReverseGraph<'a> {
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    parents: HashMap<PackageKey, Vec<&'a PackageKey>>,
    importers: HashMap<PackageKey, Vec<&'a str>>,
}

impl<'a> ReverseGraph<'a> {
    fn of(
        lockfile: &'a Lockfile,
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        edges: GraphEdges,
    ) -> Self {
        ReverseGraph {
            snapshots,
            parents: Self::parents(snapshots, edges),
            importers: Self::importer_roots(lockfile, edges),
        }
    }

    fn parents(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        edges: GraphEdges,
    ) -> HashMap<PackageKey, Vec<&'a PackageKey>> {
        let mut parents: HashMap<PackageKey, Vec<&PackageKey>> = HashMap::new();
        for (parent, snapshot) in snapshots {
            for (alias, child) in snapshot_dependencies(snapshot, edges) {
                if let Some(key) = child.resolve(alias) {
                    parents
                        .entry(key)
                        .or_default()
                        .push(parent);
                }
            }
        }
        parents
    }

    /// The importers that depend on each package directly.
    fn importer_roots(
        lockfile: &'a Lockfile,
        edges: GraphEdges,
    ) -> HashMap<PackageKey, Vec<&'a str>> {
        let mut importers: HashMap<PackageKey, Vec<_>> = HashMap::new();
        let groups = match edges {
            GraphEdges::All => {
                &[DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional][..]
            }
            GraphEdges::Required => &[DependencyGroup::Prod, DependencyGroup::Dev][..],
        };
        for (importer_id, importer) in &lockfile.importers {
            for (alias, spec) in importer.dependencies_by_groups(groups.iter().copied()) {
                if let Some(key) = spec.version.resolved_key(alias) {
                    importers
                        .entry(key)
                        .or_default()
                        .push(importer_id.as_str());
                }
            }
        }
        importers
    }

    /// The aliases of every package above `key`, and the importers above it.
    fn ancestors(&self, key: &PackageKey) -> (AliasScope, HashSet<String>) {
        let mut aliases = AliasScope::new();
        let mut importers = HashSet::new();
        let mut seen: HashSet<&PackageKey> = HashSet::from([key]);
        let mut queue = vec![key];
        while let Some(current) = queue.pop() {
            importers.extend(
                self.importers
                    .get(current)
                    .into_iter()
                    .flatten()
                    .map(|importer_id| (*importer_id).to_string()),
            );
            for parent in self.parents
                .get(current)
                .into_iter()
                .flatten()
            {
                if seen.insert(parent) {
                    aliases.extend(
                        self.snapshots
                            .get(*parent)
                            .into_iter()
                            .flat_map(snapshot_aliases),
                    );
                    queue.push(parent);
                }
            }
        }
        (aliases, importers)
    }
}
