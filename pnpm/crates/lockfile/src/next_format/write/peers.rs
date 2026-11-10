//! Which children of a snapshot are its own peers, and which packages
//! provide the peers bound below it.

use super::{super::NextFormatError, children};
use crate::{
    PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, PkgVerPeer, SnapshotDepRef,
    SnapshotEntry, split_peer_suffix_segments,
};
use std::collections::{HashMap, HashSet};

/// The transitive peers of `key` whose providers its peer groups name, and the
/// names left for [`ProviderFinder`].
///
/// The groups name providers under their package names. A hashed key names
/// none, and an aliased peer is under another name. A peer bound below from
/// anywhere in the graph, rather than from the scope above, is not in the
/// key, and is missing as far as the key goes.
#[expect(clippy::type_complexity, reason = "a private pair of lists")]
pub(super) fn transitive_named_in_key<'a>(
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

/// Whether every peer group of the key names one of the snapshot's own peers
/// or one of its `named_transitive` peers, so the key names no other provider.
/// A hashed key names none of them.
pub(super) fn all_groups_named(
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
pub(super) struct ProviderFinder<'a, 'own> {
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    own_peers: &'own HashMap<&'a PackageKey, HashSet<&'a PkgName>>,
    /// `None` while a lookup is in progress, which ends a dependency cycle.
    found: HashMap<(&'a PackageKey, &'a str), Option<&'a SnapshotDepRef>>,
}

impl<'a, 'own> ProviderFinder<'a, 'own> {
    pub(super) fn new(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        own_peers: &'own HashMap<&'a PackageKey, HashSet<&'a PkgName>>,
    ) -> Self {
        ProviderFinder { snapshots, own_peers, found: HashMap::new() }
    }

    pub(super) fn provider_below(
        &mut self,
        key: &'a PackageKey,
        name: &'a str,
    ) -> Option<&'a SnapshotDepRef> {
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
pub(super) fn own_peer_names<'a>(
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
pub(super) fn provider_segments(version: &PkgVerPeer) -> Option<Vec<String>> {
    let mut segments = split_peer_suffix_segments(version.peer()).unwrap_or_default();
    segments.retain(|segment| !segment.starts_with("patch_hash="));
    let hashed = matches!(
        segments.as_slice(),
        [only] if only.len() == 32 && only.bytes().all(|byte| byte.is_ascii_hexdigit()),
    );
    (!hashed).then_some(segments)
}

pub(super) fn names_provider(
    segments: &[String],
    alias: &PkgName,
    reference: &SnapshotDepRef,
) -> bool {
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

pub(super) fn parse_peer_name(key: &PackageKey, name: &str) -> Result<PkgName, NextFormatError> {
    PkgName::parse(name)
        .map_err(|_| NextFormatError::InvalidPeerName {
            referrer: key.to_string(),
            name: name.to_string(),
        })
}
