//! Peer-suffixed sections to the next format.
//!
//! A reference is written without peer groups, unless reading the result back
//! would select another variant for it. The writer reads its own output back
//! and writes those references with their peer groups, until nothing changes.

use super::{
    NextFormatError, NextProjectSnapshot, NextSections, NextSnapshotEntry, NextSnapshots,
    RestoreContext, RestoredSections, read,
};
use crate::{
    ImporterDepVersion, Lockfile, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    split_peer_suffix_segments,
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// Reading back can need a reference with peer groups only where an earlier
/// round wrote another reference with them, so the rounds are few. This
/// bounds them for a lockfile that cannot be represented.
const MAX_ROUNDS: usize = 8;

/// The next format's sections, read back and checked against the lockfile.
pub(super) fn next_sections(lockfile: &Lockfile) -> Result<NextSections, NextFormatError> {
    let empty = HashMap::new();
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty);
    let variants = Variants::of(snapshots, lockfile.packages.as_ref())?;
    let context = RestoreContext {
        packages: lockfile.packages.as_ref(),
        settings: lockfile.settings.as_ref(),
    };
    let mut explicit = Explicit::default();
    for _ in 0..MAX_ROUNDS {
        let sections = variants.sections(lockfile, &explicit)?;
        let restored = read::restore_with_variant_keys(&sections, context);
        if !variants.mark_mismatches(lockfile, &restored, &mut explicit) {
            check_round_trip(lockfile, &restored.sections)?;
            return Ok(sections);
        }
    }
    Err(NextFormatError::NotRepresentable { detail: "the references did not settle".to_string() })
}

/// The references written with their peer groups.
#[derive(Default)]
struct Explicit {
    snapshot_references: HashMap<PackageKey, HashSet<PkgName>>,
    importer_references: HashMap<String, HashSet<PkgName>>,
}

impl Explicit {
    fn has_snapshot_reference(&self, key: &PackageKey, name: &PkgName) -> bool {
        self.snapshot_references
            .get(key)
            .is_some_and(|names| names.contains(name))
    }

    fn has_importer_reference(&self, id: &str, name: &PkgName) -> bool {
        self.importer_references
            .get(id)
            .is_some_and(|names| names.contains(name))
    }

    fn mark_snapshot_reference(&mut self, key: &PackageKey, name: &PkgName) -> bool {
        if self.has_snapshot_reference(key, name) {
            return false;
        }
        self.snapshot_references
            .entry(key.clone())
            .or_default()
            .insert(name.clone())
    }

    fn mark_importer_reference(&mut self, id: &str, name: &PkgName) -> bool {
        if self.has_importer_reference(id, name) {
            return false;
        }
        self.importer_references
            .entry(id.to_string())
            .or_default()
            .insert(name.clone())
    }
}

/// One snapshot of the lockfile, split into the next format's maps.
struct Variant<'a> {
    key: &'a PackageKey,
    entry: &'a SnapshotEntry,
    own_peers: HashSet<&'a PkgName>,
    transitive: Vec<(PkgName, SnapshotDepRef)>,
    missing: Vec<String>,
}

struct Variants<'a> {
    /// Every package's variants, in the order the writer lists them.
    by_package: Vec<(PackageKey, Vec<Variant<'a>>)>,
    /// Where each snapshot key is in `by_package`.
    position: HashMap<&'a PackageKey, (usize, usize)>,
}

impl<'a> Variants<'a> {
    fn of(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    ) -> Result<Self, NextFormatError> {
        let own_peers: HashMap<&PackageKey, HashSet<&PkgName>> = snapshots
            .par_iter()
            .map(|(key, entry)| (key, own_peer_names(key, entry, packages)))
            .collect();
        let mut prepared = snapshots
            .par_iter()
            .map(|(key, entry)| {
                let (transitive, mut unnamed) =
                    match entry.transitive_peer_dependencies.as_ref() {
                        Some(_) => transitive_named_in_key(key, entry)?,
                        None => (Vec::new(), Vec::new()),
                    };
                let mut missing = Vec::new();
                if !unnamed.is_empty()
                    && all_groups_named(key, entry, &own_peers[key], transitive.len())
                {
                    missing.extend(
                        unnamed
                            .drain(..)
                            .map(|(_, name)| name.to_string()),
                    );
                }
                Ok((key, entry, transitive, unnamed, missing))
            })
            .collect::<Result<Vec<_>, NextFormatError>>()?;
        let mut finder = ProviderFinder { snapshots, own_peers: &own_peers, found: HashMap::new() };
        let mut grouped: HashMap<PackageKey, Vec<Variant<'a>>> = HashMap::new();
        for (key, entry, transitive, unnamed, missing) in &mut prepared {
            let segments = (!unnamed.is_empty()).then(|| provider_segments(&key.suffix)).flatten();
            for (alias, name) in unnamed.drain(..) {
                let found = finder
                    .provider_below(key, name)
                    .filter(|reference| {
                        segments
                            .as_deref()
                            .is_none_or(|segments| names_provider(segments, &alias, reference))
                    })
                    .cloned();
                match found {
                    Some(reference) => transitive.push((alias, reference)),
                    None => missing.push(name.to_string()),
                }
            }
            grouped
                .entry(key.without_peer_bindings())
                .or_default()
                .push(Variant {
                    key,
                    entry,
                    own_peers: own_peers[key].clone(),
                    transitive: std::mem::take(transitive),
                    missing: std::mem::take(missing),
                });
        }
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
        Ok(Variants { by_package, position })
    }

    fn sections(
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
    fn mark_mismatches(
        &self,
        lockfile: &Lockfile,
        restored: &read::Restored,
        explicit: &mut Explicit,
    ) -> bool {
        let restored_snapshots = restored.sections.snapshots.as_ref();
        let marks: Vec<(&PackageKey, &PkgName)> = self.by_package
            .par_iter()
            .flat_map_iter(|(package, variants)| {
                let restored_keys = &restored.variant_keys[package];
                variants
                    .iter()
                    .zip(restored_keys)
                    .flat_map(move |(variant, restored_key)| {
                        let wrong_key = restored_key != variant.key;
                        let restored_entry =
                            restored_snapshots.and_then(|snapshots| snapshots.get(restored_key));
                        let bindings = variant.binding_names().filter(move |_| wrong_key);
                        let children = children(variant.entry)
                            .filter_map(move |(alias, reference)| {
                                let restored_reference =
                                    restored_entry.and_then(|entry| child(entry, alias));
                                (restored_reference != Some(reference)).then_some(alias)
                            });
                        bindings
                            .chain(children)
                            .map(|name| (variant.key, name))
                            .collect::<Vec<_>>()
                    })
            })
            .collect();
        let mut marked = false;
        for (key, name) in marks {
            marked |= explicit.mark_snapshot_reference(key, name);
        }
        for (id, importer) in &lockfile.importers {
            let restored_importer = restored.sections.importers.get(id);
            for (alias, spec) in importer_dependencies(importer) {
                let restored_spec = restored_importer.and_then(|restored| {
                    importer_dependencies(restored).find(|(other, _)| *other == alias)
                });
                if restored_spec.map(|(_, restored)| &restored.version) != Some(&spec.version) {
                    marked |= explicit.mark_importer_reference(id, alias);
                }
            }
        }
        marked
    }
}

/// The transitive peers of `key` whose providers its peer groups name, and the
/// names left for [`ProviderFinder`].
///
/// The groups name providers under their package names. A hashed key names
/// none, and an aliased peer is under another name. A peer bound below from
/// anywhere in the graph, rather than from the scope above, is not in the
/// key, and is missing as far as the key goes.
#[expect(clippy::type_complexity, reason = "a private pair of lists")]
fn transitive_named_in_key<'a>(
    key: &PackageKey,
    entry: &'a SnapshotEntry,
) -> Result<(Vec<(PkgName, SnapshotDepRef)>, Vec<(PkgName, &'a str)>), NextFormatError> {
    let providers: Option<Vec<PkgNameVerPeer>> = provider_segments(&key.suffix)
        .map(|segments| {
            segments
                .iter()
                .filter_map(|segment| segment.parse::<PkgNameVerPeer>().ok())
                .collect()
        });
    let mut named = Vec::new();
    let mut unnamed = Vec::new();
    for name in entry.transitive_peer_dependencies.iter().flatten() {
        let alias = parse_peer_name(key, name)?;
        let provider = providers
            .iter()
            .flatten()
            .find(|provider| provider.name == alias);
        match provider {
            Some(provider) => named.push((alias, SnapshotDepRef::Plain(provider.suffix.clone()))),
            None => unnamed.push((alias, name.as_str())),
        }
    }
    Ok((named, unnamed))
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

/// Fails unless `restored` equals the lockfile's own sections.
pub(super) fn check_round_trip(
    lockfile: &Lockfile,
    restored: &RestoredSections,
) -> Result<(), NextFormatError> {
    if let Some((id, _)) = lockfile.importers
        .iter()
        .find(|(id, importer)| restored.importers.get(*id) != Some(importer))
    {
        return Err(NextFormatError::NotRepresentable { detail: format!("importer {id}") });
    }
    let empty = HashMap::new();
    let original = lockfile.snapshots.as_ref().unwrap_or(&empty);
    let restored = restored.snapshots.as_ref().unwrap_or(&empty);
    if let Some(key) = original
        .par_iter()
        .find_any(|(key, entry)| !restored.get(*key).is_some_and(|other| same_entry(entry, other)))
        .map(|(key, _)| key)
        .or_else(|| {
            restored
                .keys()
                .find(|key| !original.contains_key(*key))
        })
    {
        return Err(NextFormatError::NotRepresentable { detail: format!("snapshot {key}") });
    }
    Ok(())
}

/// Equal, with `transitivePeerDependencies` compared as a set.
fn same_entry(left: &SnapshotEntry, right: &SnapshotEntry) -> bool {
    let sorted = |entry: &SnapshotEntry| {
        let mut names = entry.transitive_peer_dependencies.clone().unwrap_or_default();
        names.sort();
        names
    };
    left.id == right.id
        && left.dependencies == right.dependencies
        && left.optional_dependencies == right.optional_dependencies
        && left.patched == right.patched
        && left.optional == right.optional
        && sorted(left) == sorted(right)
}

fn children(entry: &SnapshotEntry) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    entry.dependencies
        .iter()
        .chain(entry.optional_dependencies.iter())
        .flatten()
}

fn child<'e>(entry: &'e SnapshotEntry, alias: &PkgName) -> Option<&'e SnapshotDepRef> {
    [&entry.dependencies, &entry.optional_dependencies]
        .into_iter()
        .find_map(|map| map.as_ref()?.get(alias))
}

/// Whether every peer group of the key names one of the snapshot's own peers
/// or one of its `named_transitive` peers, so the key names no other provider.
/// A hashed key names none of them.
fn all_groups_named(
    key: &PackageKey,
    entry: &SnapshotEntry,
    own_peers: &HashSet<&PkgName>,
    named_transitive: usize,
) -> bool {
    let Some(segments) = provider_segments(&key.suffix) else { return false };
    let own_children = children(entry)
        .filter(|(alias, _)| own_peers.contains(alias))
        .count();
    segments.len() <= own_children + named_transitive
}

/// Finds which package below a snapshot provides a peer it binds from
/// outside, by following the children that need the peer from outside.
struct ProviderFinder<'a, 'own> {
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    own_peers: &'own HashMap<&'a PackageKey, HashSet<&'a PkgName>>,
    /// `None` while a lookup is in progress, which ends a dependency cycle.
    found: HashMap<(&'a PackageKey, &'a str), Option<&'a SnapshotDepRef>>,
}

impl<'a> ProviderFinder<'a, '_> {
    fn provider_below(&mut self, key: &'a PackageKey, name: &'a str) -> Option<&'a SnapshotDepRef> {
        if let Some(found) = self.found.get(&(key, name)) {
            return *found;
        }
        self.found.insert((key, name), None);
        let found = self.search(key, name);
        self.found.insert((key, name), found);
        found
    }

    fn search(&mut self, key: &'a PackageKey, name: &'a str) -> Option<&'a SnapshotDepRef> {
        let entry = self.snapshots.get(key)?;
        let own = &self.own_peers[key];
        if let Some((_, reference)) =
            children(entry).find(|(alias, _)| own.contains(alias) && alias.to_string() == name)
        {
            return Some(reference);
        }
        #[expect(clippy::needless_collect, reason = "the search borrows `self` mutably")]
        let candidates: Vec<&'a PackageKey> = children(entry)
            .filter(|(alias, _)| !own.contains(alias))
            .filter_map(|(alias, reference)| {
                let (child_key, _) = self.snapshots.get_key_value(&reference.resolve(alias)?)?;
                self.needs_from_outside(child_key, name).then_some(child_key)
            })
            .collect();
        candidates
            .into_iter()
            .find_map(|child_key| self.provider_below(child_key, name))
    }

    fn needs_from_outside(&self, key: &PackageKey, name: &str) -> bool {
        self.snapshots[key].transitive_peer_dependencies
            .iter()
            .flatten()
            .any(|transitive| transitive == name)
            || self.own_peers[key]
                .iter()
                .any(|own| own.to_string() == name)
    }
}

/// The children of `key` that are bound as its own peers: those whose
/// provider is in the key's peer groups. A dependency's provider never is,
/// because a peer below it would bind to the dependency itself. A hashed
/// suffix names no providers, so there every child whose name the package
/// declares as a peer counts.
fn own_peer_names<'a>(
    key: &PackageKey,
    entry: &'a SnapshotEntry,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> HashSet<&'a PkgName> {
    if key.suffix.peer().len() == key.suffix.patch_group().len() {
        return HashSet::new();
    }
    let segments = provider_segments(&key.suffix);
    let declared = packages
        .and_then(|packages| packages.get(&key.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref());
    children(entry)
        .filter(|(alias, reference)| match &segments {
            Some(segments) => names_provider(segments, alias, reference),
            None => declared.is_some_and(|declared| declared.contains_key(&alias.to_string())),
        })
        .map(|(alias, _)| alias)
        .collect()
}

/// The provider groups of a peer suffix, or `None` when it is hashed.
fn provider_segments(version: &PkgVerPeer) -> Option<Vec<String>> {
    let mut segments = split_peer_suffix_segments(version.peer()).unwrap_or_default();
    segments.retain(|segment| !segment.starts_with("patch_hash="));
    let hashed = matches!(
        segments.as_slice(),
        [only] if only.len() == 32 && only.bytes().all(|byte| byte.is_ascii_hexdigit())
    );
    (!hashed).then_some(segments)
}

fn names_provider(segments: &[String], alias: &PkgName, reference: &SnapshotDepRef) -> bool {
    let named = |name: &str| {
        segments
            .iter()
            .any(|segment| {
                segment
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('@'))
            })
    };
    if reference.as_link_target().is_some() {
        return named(&alias.to_string());
    }
    let Some(provider) = reference.resolve(alias) else { return false };
    if !named(&provider.name.to_string()) {
        return false;
    }
    let full = provider.to_string();
    let pair = format!("{}@{}", provider.name, provider.suffix.without_peer());
    segments
        .iter()
        .any(|segment| *segment == full || *segment == pair)
}

fn next_importer<'e>(
    id: &str,
    importer: &ProjectSnapshot,
    entry_of: &impl Fn(&PackageKey) -> Option<&'e NextSnapshotEntry>,
    explicit: &Explicit,
) -> Result<NextProjectSnapshot, NextFormatError> {
    let mut snapshot = importer.clone();
    for map in [
        &mut snapshot.dependencies,
        &mut snapshot.dev_dependencies,
        &mut snapshot.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    {
        for (alias, spec) in map.iter_mut() {
            if !explicit.has_importer_reference(id, alias) {
                spec.version = importer_version_without_peer_bindings(&spec.version);
            }
        }
    }
    let transitive = importer_transitive_peers(id, importer, entry_of)?;
    Ok(NextProjectSnapshot { snapshot, transitive_peer_dependencies: non_empty(transitive) })
}

/// The peers the importer's dependencies bind to something other than the
/// importer's own dependency of that name.
fn importer_transitive_peers<'e>(
    id: &str,
    importer: &ProjectSnapshot,
    entry_of: &impl Fn(&PackageKey) -> Option<&'e NextSnapshotEntry>,
) -> Result<HashMap<PkgName, SnapshotDepRef>, NextFormatError> {
    let mut transitive: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (alias, spec) in importer_dependencies(importer) {
        let Some(entry) = spec.version.resolved_key(alias).and_then(|key| entry_of(&key)) else {
            continue;
        };
        for (name, reference) in entry.bindings() {
            if same_importer_target(importer, name, reference) {
                continue;
            }
            let reference = without_peer_bindings(reference);
            if let Some(previous) = transitive.insert(name.clone(), reference.clone())
                && previous != reference
            {
                return Err(NextFormatError::ConflictingBindings {
                    referrer: format!("importer {id}"),
                    name: name.to_string(),
                });
            }
        }
    }
    Ok(transitive)
}

pub(super) fn importer_dependencies(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (&PkgName, &ResolvedDependencySpec)> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(ResolvedDependencyMap::iter)
}

fn same_importer_target(
    importer: &ProjectSnapshot,
    name: &PkgName,
    reference: &SnapshotDepRef,
) -> bool {
    importer_dependencies(importer)
        .find(|(alias, _)| *alias == name)
        .is_some_and(|(_, spec)| {
            let own = spec.version.resolved_key(name).map(|key| key.without_peer_bindings());
            own.is_some() && own == reference.resolve(name).map(|key| key.without_peer_bindings())
        })
}

fn parse_peer_name(key: &PackageKey, name: &str) -> Result<PkgName, NextFormatError> {
    PkgName::parse(name)
        .map_err(|_| NextFormatError::InvalidPeerName {
            referrer: key.to_string(),
            name: name.to_string(),
        })
}

pub(super) fn without_peer_bindings(reference: &SnapshotDepRef) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(version) => SnapshotDepRef::Plain(version.without_peer_bindings()),
        SnapshotDepRef::Alias(key) => SnapshotDepRef::Alias(key.without_peer_bindings()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

fn importer_version_without_peer_bindings(version: &ImporterDepVersion) -> ImporterDepVersion {
    match version {
        ImporterDepVersion::Regular(version) => {
            ImporterDepVersion::Regular(version.without_peer_bindings())
        }
        ImporterDepVersion::Alias(key) => ImporterDepVersion::Alias(key.without_peer_bindings()),
        ImporterDepVersion::File(payload) => {
            ImporterDepVersion::File(match format!("file:{payload}").parse::<PkgVerPeer>() {
                Ok(version) => version
                    .without_peer_bindings()
                    .to_string()
                    .strip_prefix("file:")
                    .map_or_else(|| payload.clone(), str::to_string),
                Err(_) => payload.clone(),
            })
        }
        ImporterDepVersion::Link(path) => ImporterDepVersion::Link(path.clone()),
    }
}

fn non_empty<Key, Value>(map: HashMap<Key, Value>) -> Option<HashMap<Key, Value>> {
    (!map.is_empty()).then_some(map)
}

impl PkgNameVerPeer {
    /// This key with its peer groups dropped and its patch group kept: the
    /// key of the next format.
    #[must_use]
    pub fn without_peer_bindings(&self) -> PkgNameVerPeer {
        PkgNameVerPeer::new(self.name.clone(), self.suffix.without_peer_bindings())
    }
}
