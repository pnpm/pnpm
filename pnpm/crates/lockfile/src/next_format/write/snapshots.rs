//! The lockfile's snapshots grouped by package, and the next format's
//! sections built from them.

use super::{
    importers::{importer_dependencies, next_importer, non_empty},
    peers::{
        ProviderConstraint, ProviderFinder, all_groups_named, own_peer_names, provider_segments,
        transitive_named_in_key,
    },
    {
        super::{
            NamedBindings, NextFormatError, NextRef, NextSections, NextSnapshotEntry,
            NextSnapshots, read, without_groups,
        },
        Explicit, children,
    },
};
use crate::{
    BoundProviders, Lockfile, LockfileEntries, PackageKey, PackageMetadata, PkgName,
    SnapshotDepRef, SnapshotEntry,
};
use naming::{BindingVariation, NamingBudget, binding_components, varying_bindings};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

mod naming;

/// One snapshot of the lockfile, split into the next format's maps.
pub(super) struct Snapshot<'a> {
    key: &'a PackageKey,
    entry: &'a SnapshotEntry,
    own_peers: HashSet<&'a PkgName>,
    transitive: Vec<(PkgName, SnapshotDepRef)>,
}

impl Snapshot<'_> {
    fn binding_names(&self) -> impl Iterator<Item = &PkgName> {
        self.own_peers
            .iter()
            .copied()
            .chain(self.transitive.iter().map(|(name, _)| name))
    }

    /// The peer bindings of this snapshot: its own peers, then those bound
    /// below it.
    fn bindings(&self) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
        children(self.entry)
            .filter(|(alias, _)| self.own_peers.contains(alias))
            .chain(
                self.transitive
                    .iter()
                    .map(|(name, reference)| (name, reference)),
            )
    }

    /// `reference` as the next format writes it: its version, naming the
    /// snapshot by its bindings when the reference is marked explicit.
    fn written(
        &self,
        name: &PkgName,
        reference: &SnapshotDepRef,
        explicit: &Explicit,
        snapshots: &Snapshots<'_>,
    ) -> NextRef {
        let version = without_groups(reference);
        let named = explicit
            .has_snapshot_reference(self.key, name)
            .then(|| reference.resolve(name))
            .flatten()
            .and_then(|target| snapshots.naming_bindings(&target));
        match named {
            Some(peers) => NextRef::Named { version, peers },
            None => NextRef::Plain(version),
        }
    }

    fn next_entry(&self, explicit: &Explicit, snapshots: &Snapshots<'_>) -> NextSnapshotEntry {
        let written = |name: &PkgName, reference: &SnapshotDepRef| {
            self.written(name, reference, explicit, snapshots)
        };
        let mut dependencies = HashMap::new();
        let mut optional_dependencies = HashMap::new();
        let mut peer_dependencies = HashMap::new();
        for (map, target) in [
            (&self.entry.dependencies, &mut dependencies),
            (&self.entry.optional_dependencies, &mut optional_dependencies),
        ] {
            for (alias, reference) in map.iter().flatten() {
                let target = if self.own_peers.contains(alias) {
                    &mut peer_dependencies
                } else {
                    &mut *target
                };
                target.insert(alias.clone(), written(alias, reference));
            }
        }
        let transitive = self.transitive
            .iter()
            .map(|(name, reference)| (name.clone(), written(name, reference)))
            .collect();
        NextSnapshotEntry {
            id: self.entry.id.clone(),
            dependencies: non_empty(dependencies),
            optional_dependencies: non_empty(optional_dependencies),
            peer_dependencies: non_empty(peer_dependencies),
            transitive_peer_dependencies: non_empty(transitive),
            unbound_peer_dependencies: explicit.unbound_peer_names
                .contains(self.key)
                .then(|| self.unbound_peer_names()),
            optional: self.entry.optional,
        }
    }
    fn unbound_peer_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.entry.transitive_peer_dependencies
            .iter()
            .flatten()
            .filter(|name| {
                !self.transitive
                    .iter()
                    .any(|(bound, _)| bound.to_string() == **name)
            })
            .cloned()
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

pub(super) struct Snapshots<'a> {
    /// Every package's snapshots, in the order the writer lists them.
    by_package: Vec<(PackageKey, Vec<Snapshot<'a>>)>,
    /// Where each of today's snapshot keys is in `by_package`.
    index: HashMap<&'a PackageKey, (usize, usize)>,
    /// Per package, the peers its snapshots bind differently: the bindings
    /// that name one of them.
    varying: Vec<BindingVariation>,
    named: HashMap<&'a PackageKey, Option<NamedBindings>>,
    components: HashMap<&'a PackageKey, usize>,
}

/// A snapshot's transitive peers: those its key names, and those left for
/// [`ProviderFinder`].
struct Transitive<'a> {
    named: Vec<(PkgName, SnapshotDepRef)>,
    unnamed: Vec<(PkgName, &'a str)>,
}

impl<'a> Transitive<'a> {
    fn of(
        key: &PackageKey,
        entry: &'a SnapshotEntry,
        own_peers: &HashSet<&PkgName>,
    ) -> Result<Self, NextFormatError> {
        let (named, mut unnamed) = match entry.transitive_peer_dependencies.as_ref() {
            Some(_) => transitive_named_in_key(key, entry)?,
            None => (Vec::new(), Vec::new()),
        };
        if !unnamed.is_empty() && all_groups_named(key, entry, own_peers, named.len()) {
            // The key names every provider, so the rest have none.
            unnamed.clear();
        }
        Ok(Transitive { named, unnamed })
    }

    /// Finds the providers of the unnamed peers below `key`. A peer whose
    /// provider the key does not name is missing as far as the key goes.
    fn find_unnamed(
        &mut self,
        finder: &mut ProviderFinder<'a, '_>,
        key: &'a PackageKey,
        providers: Option<&BoundProviders>,
    ) {
        if self.unnamed.is_empty() {
            return;
        }
        let segments = provider_segments(&key.suffix);
        for (alias, name) in std::mem::take(&mut self.unnamed) {
            let constraint = ProviderConstraint::new(&alias, segments.as_deref(), providers);
            let found = finder.provider_below(key, name, &constraint).cloned();
            if let Some(reference) = found {
                self.named.push((alias, reference));
            }
        }
    }
}

impl<'a> Snapshots<'a> {
    pub(super) fn of(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    ) -> Result<Self, NextFormatError> {
        let entries = LockfileEntries { packages, snapshots: Some(snapshots) };
        let providers: HashMap<_, _> = snapshots
            .par_iter()
            .filter(|(key, _)| provider_segments(&key.suffix).is_none())
            .map(|(key, _)| (key, BoundProviders::of_snapshot(entries, key)))
            .collect();
        let own_peers: HashMap<&PackageKey, HashSet<&PkgName>> = snapshots
            .par_iter()
            .map(|(key, entry)| (key, own_peer_names(key, entry, packages, providers.get(key))))
            .collect();
        let prepared = snapshots
            .par_iter()
            .map(|(key, entry)| Ok((key, entry, Transitive::of(key, entry, &own_peers[key])?)))
            .collect::<Result<Vec<_>, NextFormatError>>()?;
        let mut finder = ProviderFinder::new(snapshots, &own_peers);
        let mut grouped: HashMap<PackageKey, Vec<Snapshot<'a>>> = HashMap::new();
        for (key, entry, mut transitive) in prepared {
            transitive.find_unnamed(&mut finder, key, providers.get(key));
            grouped
                .entry(key.without_peer())
                .or_default()
                .push(Snapshot {
                    key,
                    entry,
                    own_peers: own_peers[key].clone(),
                    transitive: transitive.named,
                });
        }
        Snapshots::from_groups(grouped)
    }

    fn from_groups(
        grouped: HashMap<PackageKey, Vec<Snapshot<'a>>>,
    ) -> Result<Self, NextFormatError> {
        let mut by_package: Vec<(PackageKey, Vec<Snapshot<'a>>)> = grouped.into_iter().collect();
        by_package
            .par_iter_mut()
            .filter(|(_, snapshots)| snapshots.len() > 1)
            .for_each(|(_, snapshots)| {
                snapshots.sort_by_cached_key(|snapshot| snapshot.key.to_string());
            });
        let index = by_package
            .iter()
            .enumerate()
            .flat_map(|(package, (_, snapshots))| {
                snapshots
                    .iter()
                    .enumerate()
                    .map(move |(index, snapshot)| (snapshot.key, (package, index)))
            })
            .collect();
        let varying = by_package
            .par_iter()
            .map(|(_, snapshots)| varying_bindings(snapshots))
            .collect();
        let components = binding_components(&by_package);
        let mut snapshots =
            Snapshots { by_package, index, varying, named: HashMap::new(), components };
        for key in snapshots.index.keys().copied() {
            let named = snapshots.naming_bindings_in(key, &mut NamingBudget::default())?;
            snapshots.named.insert(key, named);
        }
        Ok(snapshots)
    }

    /// The bindings that name the snapshot `target` among its package's: the
    /// peers the package's snapshots bind differently, as `target` binds
    /// them. `None` when nothing tells the snapshot apart.
    pub(super) fn naming_bindings(&self, target: &PackageKey) -> Option<NamedBindings> {
        self.named
            .get(target)
            .cloned()
            .flatten()
    }

    /// Nested discriminators must also be written on the bindings they identify.
    pub(super) fn mark_nested_bindings(&self, explicit: &mut Explicit) {
        for snapshot in self.by_package.iter().flat_map(|(_, snapshots)| snapshots) {
            let Some(bindings) = self.naming_bindings(snapshot.key) else { continue };
            for name in snapshot
                .binding_names()
                .filter(|name| {
                    bindings
                        .get(&name.to_string())
                        .is_some_and(|reference| reference.peers().is_some())
                })
            {
                explicit.mark_snapshot_reference(snapshot.key, name);
            }
        }
    }

    pub(super) fn sections(
        &self,
        lockfile: &Lockfile,
        explicit: &Explicit,
    ) -> Result<NextSections, NextFormatError> {
        let lists: Vec<Vec<NextSnapshotEntry>> = self.by_package
            .par_iter()
            .map(|(_, snapshots)| {
                snapshots
                    .iter()
                    .map(|snapshot| snapshot.next_entry(explicit, self))
                    .collect()
            })
            .collect();
        let importers = lockfile.importers
            .par_iter()
            .map(|(id, importer)| (id.clone(), next_importer(id, importer, explicit, self)))
            .collect();
        let snapshots = lockfile.snapshots
            .as_ref()
            .map(|_| {
                self.by_package
                    .iter()
                    .map(|(package, _)| package.clone())
                    .zip(lists)
                    .collect::<NextSnapshots>()
            });
        Ok(NextSections { importers, snapshots })
    }

    /// Marks every reference that read back to another snapshot. Returns
    /// whether it marked one that was not marked before.
    pub(super) fn mark_mismatches(
        &self,
        lockfile: &Lockfile,
        rebuilt: &read::Rebuilt<'_>,
        explicit: &mut Explicit,
    ) -> bool {
        let mut marked = false;
        for (key, name) in self.snapshot_mismatches(rebuilt) {
            marked |= explicit.mark_snapshot_reference(key, name);
        }
        for (id, name) in importer_mismatches(lockfile, rebuilt) {
            marked |= explicit.mark_importer_reference(id, name);
        }
        marked
    }

    fn snapshot_mismatches(&self, rebuilt: &read::Rebuilt<'_>) -> Vec<(&'a PackageKey, &PkgName)> {
        self.by_package
            .par_iter()
            .flat_map_iter(|(package, snapshots)| {
                snapshots
                    .iter()
                    .enumerate()
                    .flat_map(move |(index, snapshot)| snapshot.mismatches(rebuilt, package, index))
            })
            .collect()
    }
}

impl<'a> Snapshot<'a> {
    /// The references of this snapshot that read back to another snapshot: all
    /// its bindings when its own key read back wrong, and every child that
    /// read back to another key.
    fn mismatches<'v>(
        &'v self,
        rebuilt: &read::Rebuilt<'_>,
        package: &PackageKey,
        index: usize,
    ) -> Vec<(&'a PackageKey, &'v PkgName)> {
        let wrong_key = rebuilt.snapshot_key(package, index) != Some(self.key);
        let bindings = self.binding_names().filter(|_| wrong_key);
        let changed_children = children(self.entry)
            .filter_map(|(alias, reference)| {
                let expected = reference.resolve(alias)?;
                (rebuilt.child_key(package, index, alias) != Some(&expected)).then_some(alias)
            });
        bindings
            .chain(changed_children)
            .map(|name| (self.key, name))
            .collect()
    }
}

/// The importer references that read back to another snapshot.
fn importer_mismatches<'l>(
    lockfile: &'l Lockfile,
    rebuilt: &read::Rebuilt<'_>,
) -> Vec<(&'l str, &'l PkgName)> {
    lockfile.importers
        .iter()
        .flat_map(|(id, importer)| {
            importer_dependencies(importer)
                .filter(move |(alias, spec)| {
                    spec.version
                        .resolved_key(alias)
                        .is_some_and(|expected| {
                            rebuilt.importer_child_key(id, alias) != Some(&expected)
                        })
                })
                .map(move |(alias, _)| (id.as_str(), alias))
        })
        .collect()
}
