//! The lockfile's snapshots grouped by package, and the next format's
//! sections built from them.

use super::{
    importers::{importer_dependencies, next_importer, non_empty, without_peer_bindings},
    peers::{
        ProviderFinder, all_groups_named, names_provider, own_peer_names, provider_segments,
        transitive_named_in_key,
    },
    {
        super::{NextFormatError, NextSections, NextSnapshotEntry, NextSnapshots, read},
        Explicit, child, children,
    },
};
use crate::{
    ImporterDepVersion, Lockfile, PackageKey, PackageMetadata, PkgName, ProjectSnapshot,
    SnapshotDepRef, SnapshotEntry,
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// One snapshot of the lockfile, split into the next format's maps.
pub(super) struct Variant<'a> {
    key: &'a PackageKey,
    entry: &'a SnapshotEntry,
    own_peers: HashSet<&'a PkgName>,
    transitive: Vec<(PkgName, SnapshotDepRef)>,
    missing: Vec<String>,
}

impl Variant<'_> {
    fn binding_names(&self) -> impl Iterator<Item = &PkgName> {
        self.own_peers
            .iter()
            .copied()
            .chain(self.transitive.iter().map(|(name, _)| name))
    }

    fn next_entry(&self, explicit: &Explicit) -> NextSnapshotEntry {
        let marked = explicit.snapshot_references.get(self.key);
        let written = |name: &PkgName, reference: &SnapshotDepRef| {
            if marked.is_some_and(|names| names.contains(name)) {
                reference.clone()
            } else {
                without_peer_bindings(reference)
            }
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
            missing_peer_dependencies: (!self.missing.is_empty()).then(|| {
                self.missing.clone()
            }),
            patched: self.entry.patched,
            optional: self.entry.optional,
        }
    }
}

pub(super) struct Variants<'a> {
    /// Every package's variants, in the order the writer lists them.
    by_package: Vec<(PackageKey, Vec<Variant<'a>>)>,
    /// Where each snapshot key is in `by_package`.
    position: HashMap<&'a PackageKey, (usize, usize)>,
}

/// A snapshot's transitive peers: those its key names, those left for
/// [`ProviderFinder`], and those known to be missing.
struct Transitive<'a> {
    named: Vec<(PkgName, SnapshotDepRef)>,
    unnamed: Vec<(PkgName, &'a str)>,
    missing: Vec<String>,
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
        let mut missing = Vec::new();
        if !unnamed.is_empty() && all_groups_named(key, entry, own_peers, named.len()) {
            missing.extend(
                unnamed
                    .drain(..)
                    .map(|(_, name)| name.to_string()),
            );
        }
        Ok(Transitive { named, unnamed, missing })
    }

    /// Finds the providers of the unnamed peers below `key`. A peer whose
    /// provider the key does not name is missing as far as the key goes.
    fn find_unnamed(&mut self, finder: &mut ProviderFinder<'a, '_>, key: &'a PackageKey) {
        if self.unnamed.is_empty() {
            return;
        }
        let segments = provider_segments(&key.suffix);
        for (alias, name) in std::mem::take(&mut self.unnamed) {
            let found = finder
                .provider_below(key, name)
                .filter(|reference| {
                    segments
                        .as_deref()
                        .is_none_or(|segments| names_provider(segments, &alias, reference))
                })
                .cloned();
            match found {
                Some(reference) => self.named.push((alias, reference)),
                None => self.missing.push(name.to_string()),
            }
        }
    }
}

impl<'a> Variants<'a> {
    pub(super) fn of(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    ) -> Result<Self, NextFormatError> {
        let own_peers: HashMap<&PackageKey, HashSet<&PkgName>> = snapshots
            .par_iter()
            .map(|(key, entry)| (key, own_peer_names(key, entry, packages)))
            .collect();
        let prepared = snapshots
            .par_iter()
            .map(|(key, entry)| Ok((key, entry, Transitive::of(key, entry, &own_peers[key])?)))
            .collect::<Result<Vec<_>, NextFormatError>>()?;
        let mut finder = ProviderFinder::new(snapshots, &own_peers);
        let mut grouped: HashMap<PackageKey, Vec<Variant<'a>>> = HashMap::new();
        for (key, entry, mut transitive) in prepared {
            transitive.find_unnamed(&mut finder, key);
            grouped
                .entry(key.without_peer_bindings())
                .or_default()
                .push(Variant {
                    key,
                    entry,
                    own_peers: own_peers[key].clone(),
                    transitive: transitive.named,
                    missing: transitive.missing,
                });
        }
        Ok(Variants::from_groups(grouped))
    }

    fn from_groups(grouped: HashMap<PackageKey, Vec<Variant<'a>>>) -> Self {
        let mut by_package: Vec<(PackageKey, Vec<Variant<'a>>)> = grouped.into_iter().collect();
        by_package
            .par_iter_mut()
            .filter(|(_, variants)| variants.len() > 1)
            .for_each(|(_, variants)| {
                variants.sort_by_cached_key(|variant| variant.key.to_string());
            });
        let position = by_package
            .iter()
            .enumerate()
            .flat_map(|(package, (_, variants))| {
                variants
                    .iter()
                    .enumerate()
                    .map(move |(index, variant)| (variant.key, (package, index)))
            })
            .collect();
        Variants { by_package, position }
    }

    pub(super) fn sections(
        &self,
        lockfile: &Lockfile,
        explicit: &Explicit,
    ) -> Result<NextSections, NextFormatError> {
        let lists: Vec<Vec<NextSnapshotEntry>> = self.by_package
            .par_iter()
            .map(|(_, variants)| {
                variants
                    .iter()
                    .map(|variant| variant.next_entry(explicit))
                    .collect()
            })
            .collect();
        let entry_of = |key: &PackageKey| {
            let &(package, index) = self.position.get(key)?;
            lists[package].get(index)
        };
        let importers = lockfile.importers
            .par_iter()
            .map(|(id, importer)| {
                Ok((id.clone(), next_importer(id, importer, &entry_of, explicit)?))
            })
            .collect::<Result<_, NextFormatError>>()?;
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

    /// Marks every reference that read back to another variant. Returns
    /// whether it marked one that was not marked before.
    pub(super) fn mark_mismatches(
        &self,
        lockfile: &Lockfile,
        restored: &read::Restored,
        explicit: &mut Explicit,
    ) -> bool {
        let mut marked = false;
        for (key, name) in self.snapshot_mismatches(restored) {
            marked |= explicit.mark_snapshot_reference(key, name);
        }
        for (id, name) in importer_mismatches(lockfile, restored) {
            marked |= explicit.mark_importer_reference(id, name);
        }
        marked
    }

    fn snapshot_mismatches(&self, restored: &read::Restored) -> Vec<(&'a PackageKey, &PkgName)> {
        self.by_package
            .par_iter()
            .flat_map_iter(|(package, variants)| {
                variants
                    .iter()
                    .zip(&restored.variant_keys[package])
                    .flat_map(|(variant, restored_key)| variant.mismatches(restored, restored_key))
            })
            .collect()
    }
}

impl<'a> Variant<'a> {
    /// The references of this variant that read back to another variant: all
    /// its bindings when its own key read back wrong, and every child whose
    /// reference changed.
    fn mismatches<'v>(
        &'v self,
        restored: &read::Restored,
        restored_key: &PackageKey,
    ) -> Vec<(&'a PackageKey, &'v PkgName)> {
        let restored_entry = restored.sections.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(restored_key));
        let bindings = self
            .binding_names()
            .filter(|_| restored_key != self.key);
        let changed_children = children(self.entry)
            .filter_map(|(alias, reference)| {
                let restored_reference = restored_entry.and_then(|entry| child(entry, alias));
                (restored_reference != Some(reference)).then_some(alias)
            });
        bindings
            .chain(changed_children)
            .map(|name| (self.key, name))
            .collect()
    }
}

/// The importer references that read back to another variant.
fn importer_mismatches<'l>(
    lockfile: &'l Lockfile,
    restored: &read::Restored,
) -> Vec<(&'l str, &'l PkgName)> {
    lockfile.importers
        .iter()
        .flat_map(|(id, importer)| {
            let restored_importer = restored.sections.importers.get(id);
            importer_dependencies(importer)
                .filter(move |(alias, spec)| {
                    version_of(restored_importer, alias) != Some(&spec.version)
                })
                .map(move |(alias, _)| (id.as_str(), alias))
        })
        .collect()
}

fn version_of<'r>(
    importer: Option<&'r ProjectSnapshot>,
    alias: &PkgName,
) -> Option<&'r ImporterDepVersion> {
    importer_dependencies(importer?)
        .find(|(other, _)| *other == alias)
        .map(|(_, spec)| &spec.version)
}
