//! The next format back to peer-suffixed sections.
//!
//! 1. Every reference is matched to one variant of the package it names. A
//!    package with one variant needs no match. A reference that carries peer
//!    groups names its variant's providers. Any other reference is matched to
//!    the variant whose bindings all agree with the referrer's scope, binding
//!    the most names.
//! 2. Every peer binding is matched to its provider: the variant of the
//!    provider that the scope above the binding snapshot links under the
//!    peer's name. The scope of a variant is that of the referrer that first
//!    reaches it, in a walk from the importers in order.
//! 3. Each variant's key is rebuilt from its providers, with the rules the
//!    resolver uses for the peer suffix: a provider is written as
//!    `name@version` under `dedupePeers`, for a link, and inside a peer cycle,
//!    and as its own key otherwise.
//!
//! The writer checks that this reads its output back to the sections it was
//! given, and writes a reference with its peer groups wherever step 1 or 2
//! would pick another variant.

use super::{
    NextProjectSnapshot, NextSections, NextSnapshotEntry, NextSnapshots, RestoreContext,
    RestoredSections, tarjan, write::importer_dependencies,
};
use crate::{
    DEFAULT_PEERS_SUFFIX_MAX_LENGTH, ImporterDepVersion, PackageKey, PkgName, PkgNameVerPeer,
    PkgVerPeer, ProjectSnapshot, SnapshotDepRef, SnapshotEntry, split_peer_suffix_segments,
};
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, link_path_to_peer_version};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// Rebuild the peer-suffixed `importers` and `snapshots` from the next
/// format's.
#[must_use]
pub fn restore_sections(sections: &NextSections, context: RestoreContext<'_>) -> RestoredSections {
    restore_with_variant_keys(sections, context).sections
}

/// [`restore_sections`], plus the key each variant was given, in the order of
/// [`Graph::index`].
pub(super) struct Restored {
    pub sections: RestoredSections,
    pub variant_keys: HashMap<PackageKey, Vec<PkgNameVerPeer>>,
}

pub(super) fn restore_with_variant_keys(
    sections: &NextSections,
    context: RestoreContext<'_>,
) -> Restored {
    let Some(snapshots) = sections.snapshots.as_ref() else {
        let importers = sections.importers
            .iter()
            .map(|(id, importer)| (id.clone(), importer.snapshot.clone()))
            .collect();
        let sections = RestoredSections { importers, snapshots: None };
        return Restored { sections, variant_keys: HashMap::new() };
    };
    let graph = Graph::new(snapshots, &sections.importers);
    let keys = graph.keys(context);
    let snapshots = graph.snapshot_entries(&keys, context);
    let importers = sections.importers
        .par_iter()
        .map(|(id, importer)| (id.clone(), graph.restore_importer(id, importer, &keys)))
        .collect();
    let variant_keys = graph.variants
        .iter()
        .map(|(key, &(start, count))| ((*key).clone(), keys[start..start + count].to_vec()))
        .collect();
    Restored { sections: RestoredSections { importers, snapshots: Some(snapshots) }, variant_keys }
}

type Vid = usize;

#[derive(Clone, Copy)]
enum Referrer<'a> {
    Importer(&'a str),
    Snapshot(Vid),
}

enum Provider {
    Variant(Vid),
    Link(String),
    /// The lockfile has no snapshot of the provider.
    Missing(String),
}

struct Graph<'a> {
    keys: Vec<&'a PackageKey>,
    entries: Vec<&'a NextSnapshotEntry>,
    variants: HashMap<&'a PackageKey, (Vid, usize)>,
    importers: HashMap<&'a str, &'a NextProjectSnapshot>,
    /// Each variant's chosen children, sorted by name.
    children: Vec<Vec<(&'a PkgName, Vid)>>,
    importer_children: HashMap<&'a str, Vec<(&'a PkgName, Vid)>>,
    /// The referrer that first reaches each variant: the scope its peers are
    /// bound in.
    parent: Vec<Option<Referrer<'a>>>,
    providers: Vec<Vec<(&'a PkgName, Provider)>>,
}

impl<'a> Graph<'a> {
    fn new(
        snapshots: &'a NextSnapshots,
        importers: &'a HashMap<String, NextProjectSnapshot>,
    ) -> Self {
        let mut graph = Graph::index(snapshots, importers);
        graph.children = (0..graph.keys.len())
            .into_par_iter()
            .map(|vid| graph.choose_children(vid))
            .collect();
        graph.importer_children = graph.importers
            .par_iter()
            .map(|(&id, importer)| (id, graph.choose_importer_children(id, importer)))
            .collect();
        graph.parent = graph.parents();
        graph.providers = (0..graph.keys.len())
            .into_par_iter()
            .map(|vid| {
                graph.entries[vid]
                    .bindings()
                    .map(|(name, reference)| (name, graph.provider(vid, name, reference)))
                    .collect()
            })
            .collect();
        graph
    }

    fn index(
        snapshots: &'a NextSnapshots,
        importers: &'a HashMap<String, NextProjectSnapshot>,
    ) -> Self {
        let mut keys = Vec::new();
        let mut entries = Vec::new();
        let mut variants = HashMap::with_capacity(snapshots.len());
        for (key, list) in snapshots {
            variants.insert(key, (keys.len(), list.len()));
            for entry in list {
                keys.push(key);
                entries.push(entry);
            }
        }
        Graph {
            keys,
            entries,
            variants,
            importers: importers
                .iter()
                .map(|(id, importer)| (id.as_str(), importer))
                .collect(),
            children: Vec::new(),
            importer_children: HashMap::new(),
            parent: Vec::new(),
            providers: Vec::new(),
        }
    }

    fn choose_children(&self, vid: Vid) -> Vec<(&'a PkgName, Vid)> {
        let entry = self.entries[vid];
        let mut children: Vec<(&'a PkgName, Vid)> = entry.dependencies
            .iter()
            .chain(entry.optional_dependencies.iter())
            .flatten()
            .filter_map(|(alias, reference)| {
                let key = reference.resolve(alias)?;
                Some((alias, self.choose(&key, Referrer::Snapshot(vid))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    fn choose_importer_children(
        &self,
        id: &'a str,
        importer: &'a NextProjectSnapshot,
    ) -> Vec<(&'a PkgName, Vid)> {
        let mut children: Vec<(&'a PkgName, Vid)> = importer_dependencies(&importer.snapshot)
            .filter_map(|(alias, spec)| {
                let key = spec.version.resolved_key(alias)?;
                Some((alias, self.choose(&key, Referrer::Importer(id))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    /// The first referrer of every variant, in a walk from the importers in
    /// order. A variant no importer reaches starts a walk of its own.
    fn parents(&self) -> Vec<Option<Referrer<'a>>> {
        let mut parent: Vec<Option<Referrer<'a>>> = vec![None; self.keys.len()];
        let mut visited = vec![false; self.keys.len()];
        let mut queue: Vec<Vid> = Vec::new();
        let mut ids: Vec<&str> = self.importer_children
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        let mut walk = |queue: &mut Vec<Vid>, parent: &mut Vec<Option<Referrer<'a>>>| {
            while let Some(vid) = queue.pop() {
                if std::mem::replace(&mut visited[vid], true) {
                    continue;
                }
                for &(_, child) in &self.children[vid] {
                    if !visited[child] {
                        parent[child].get_or_insert(Referrer::Snapshot(vid));
                        queue.push(child);
                    }
                }
            }
        };
        for id in ids {
            for &(_, child) in &self.importer_children[id] {
                parent[child].get_or_insert(Referrer::Importer(id));
                queue.push(child);
            }
            walk(&mut queue, &mut parent);
        }
        let mut unreached: Vec<Vid> = (0..self.keys.len())
            .filter(|&vid| parent[vid].is_none())
            .collect();
        if !unreached.is_empty() {
            // The vids follow the map the snapshots were read into, so the
            // roots are ordered by key, the same in every process.
            unreached.sort_by_cached_key(|&vid| {
                (self.keys[vid].to_string(), vid - self.variants[self.keys[vid]].0)
            });
            for vid in unreached {
                queue.push(vid);
                walk(&mut queue, &mut parent);
            }
        }
        parent
    }

    /// The variant `key` names from `referrer`, or `None` when the lockfile
    /// has no snapshot of the package. A lockfile can omit one, for a skipped
    /// optional dependency, and the reference then stays as it is.
    fn choose(&self, key: &PkgNameVerPeer, referrer: Referrer<'a>) -> Option<Vid> {
        let explicit = has_peer_groups(&key.suffix);
        let &(start, count) = if explicit {
            self.variants.get(&key.without_peer_bindings())?
        } else {
            self.variants.get(key)?
        };
        if count == 1 {
            return Some(start);
        }
        if explicit && let Some(vid) = self.variant_with_providers(start, count, key) {
            return Some(vid);
        }
        Some(self.variant_in_scope(start, count, referrer))
    }

    /// The variant whose providers are the peer groups of `key`.
    fn variant_with_providers(
        &self,
        start: Vid,
        count: usize,
        key: &PkgNameVerPeer,
    ) -> Option<Vid> {
        let wanted: HashSet<String> = split_peer_suffix_segments(key.suffix.peer())?
            .into_iter()
            .filter(|segment| !segment.starts_with("patch_hash="))
            .map(|segment| match segment.find('(') {
                Some(nested) => segment[..nested].to_string(),
                None => segment,
            })
            .collect();
        (start..start + count).find(|&vid| self.provider_pairs(vid) == wanted)
    }

    /// `name@version` of every provider of `vid`, as a collapsed peer suffix
    /// writes it.
    fn provider_pairs(&self, vid: Vid) -> HashSet<String> {
        self.entries[vid]
            .bindings()
            .filter_map(|(name, reference)| match reference.as_link_target() {
                Some(path) => Some(format!("{name}@{}", link_path_to_peer_version(path))),
                None => reference
                    .resolve(name)
                    .map(|key| format!("{}@{}", key.name, key.suffix.without_peer())),
            })
            .collect()
    }

    /// The variant whose bindings all agree with the referrer's scope, binding
    /// the most names. The first variant when none agrees.
    fn variant_in_scope(&self, start: Vid, count: usize, referrer: Referrer<'a>) -> Vid {
        (start..start + count)
            .filter_map(|vid| Some((vid, self.agreeing_bindings(vid, referrer)?)))
            .min_by_key(|&(vid, bound)| (std::cmp::Reverse(bound), vid))
            .map_or(start, |(vid, _)| vid)
    }

    /// How many bindings `vid` has, if every one agrees with the referrer.
    fn agreeing_bindings(&self, vid: Vid, referrer: Referrer<'a>) -> Option<usize> {
        let mut count = 0;
        for (name, reference) in self.entries[vid].bindings() {
            if !self.in_scope(referrer, name, reference) {
                return None;
            }
            count += 1;
        }
        Some(count)
    }

    fn in_scope(&self, referrer: Referrer<'a>, name: &PkgName, reference: &SnapshotDepRef) -> bool {
        match referrer {
            Referrer::Snapshot(vid) => match self.entries[vid].reference_in_scope(name) {
                Some(scoped) => same_target(name, scoped, reference),
                None => self.is_itself(vid, name, reference),
            },
            Referrer::Importer(id) => {
                let importer = self.importers[id];
                let own =
                    importer_dependencies(&importer.snapshot).find(|(alias, _)| *alias == name);
                if let Some((_, spec)) = own {
                    return importer_target_is(name, &spec.version, reference);
                }
                importer.transitive_peer_dependencies
                    .as_ref()
                    .and_then(|transitive| transitive.get(name))
                    .is_some_and(|scoped| same_target(name, scoped, reference))
            }
        }
    }

    /// Whether `reference` names the snapshot `vid` itself: a package is in
    /// the scope of its own dependencies, which can have it as a peer.
    fn is_itself(&self, vid: Vid, name: &PkgName, reference: &SnapshotDepRef) -> bool {
        self.keys[vid].name == *name
            && reference
                .resolve(name)
                .map(|key| key.without_peer_bindings())
                .as_ref()
                == Some(self.keys[vid])
    }

    fn provider(&self, vid: Vid, name: &PkgName, reference: &SnapshotDepRef) -> Provider {
        if let Some(path) = reference.as_link_target() {
            return Provider::Link(path.to_string());
        }
        let Some(key) = reference.resolve(name) else {
            return Provider::Missing(reference.to_string());
        };
        let explicit = has_peer_groups(&key.suffix);
        let package = if explicit {
            self.variants.get(&key.without_peer_bindings())
        } else {
            self.variants.get(&key)
        };
        let Some(&(start, count)) = package else {
            return Provider::Missing(format!("{}@{}", key.name, key.suffix.without_peer()));
        };
        if count == 1 {
            return Provider::Variant(start);
        }
        if explicit && let Some(found) = self.variant_with_providers(start, count, &key) {
            return Provider::Variant(found);
        }
        match self.linked_above(vid, name, start, count) {
            Some(found) => Provider::Variant(found),
            None => Provider::Variant(self.variant_in_scope(start, count, Referrer::Snapshot(vid))),
        }
    }

    /// The variant among `start..start + count` linked as `name` in the scope
    /// `vid` was reached in: under its parent, its parent's parent, and so on
    /// up to an importer.
    fn linked_above(&self, vid: Vid, name: &PkgName, start: Vid, count: usize) -> Option<Vid> {
        let is_candidate = |found: Vid| (start..start + count).contains(&found);
        let mut referrer = self.parent[vid];
        let mut steps = 0;
        while let Some(current) = referrer {
            let linked = match current {
                Referrer::Snapshot(parent) => child_named(&self.children[parent], name)
                    .or_else(|| (self.keys[parent].name == *name).then_some(parent)),
                Referrer::Importer(id) => child_named(&self.importer_children[id], name),
            };
            if let Some(found) = linked.filter(|&found| is_candidate(found)) {
                return Some(found);
            }
            referrer = match current {
                Referrer::Snapshot(parent) => self.parent[parent],
                Referrer::Importer(_) => None,
            };
            steps += 1;
            if steps > self.keys.len() {
                return None;
            }
        }
        None
    }

    fn keys(&self, context: RestoreContext<'_>) -> Vec<PkgNameVerPeer> {
        let rules = SuffixRules::of(context);
        let cyclic_names = if rules.dedupe { HashSet::new() } else { self.cyclic_peer_names() };
        if rules.dedupe {
            // Every provider is written as `name@version`, so no key waits for
            // another.
            return (0..self.keys.len())
                .into_par_iter()
                .map(|vid| self.key_of(vid, &rules, |_, provider| pair_of(self.keys[provider])))
                .collect();
        }
        let order = self.providers_first();
        // A variant without bindings has the package's key, and every other
        // variant may need it.
        let mut keys: Vec<Option<PkgNameVerPeer>> = (0..self.keys.len())
            .map(|vid| {
                self.providers[vid]
                    .is_empty()
                    .then(|| self.keys[vid].clone())
            })
            .collect();
        for level in self.levels(&order) {
            let computed: Vec<(Vid, PkgNameVerPeer)> = level
                .par_iter()
                .flat_map_iter(|&component| order.components[component].iter().copied())
                .filter(|&vid| !self.providers[vid].is_empty())
                .map(|vid| {
                    let key = self.key_of(vid, &rules, |name, provider| {
                        let in_cycle = order.component_of.contains_key(&vid)
                            && order.component_of.get(&vid)
                                == order.component_of.get(&provider);
                        let collapse = in_cycle || cyclic_names.contains(&name.to_string());
                        match keys[provider].as_ref() {
                            Some(provider_key) if !collapse => {
                                PeerId::DepPath(DepPath::from(provider_key.to_string()))
                            }
                            _ => pair_of(self.keys[provider]),
                        }
                    });
                    (vid, key)
                })
                .collect();
            for (vid, key) in computed {
                keys[vid] = Some(key);
            }
        }
        keys.into_iter()
            .map(|key| key.expect("every variant is in one component"))
            .collect()
    }

    /// The key of `vid`, with `segment` giving each provider variant's peer
    /// group.
    fn key_of(
        &self,
        vid: Vid,
        rules: &SuffixRules,
        segment: impl Fn(&PkgName, Vid) -> PeerId,
    ) -> PkgNameVerPeer {
        let base = self.keys[vid];
        if self.providers[vid].is_empty() {
            return base.clone();
        }
        let peer_ids: Vec<PeerId> = self.providers[vid]
            .iter()
            .map(|(name, provider)| match provider {
                Provider::Link(path) => PeerId::Pair {
                    name: name.to_string(),
                    version: link_path_to_peer_version(path),
                },
                Provider::Missing(pair) => PeerId::DepPath(DepPath::from(pair.clone())),
                Provider::Variant(provider) => segment(name, *provider),
            })
            .collect();
        let mut peer = base.suffix.patch_group().to_string();
        peer.push_str(&create_peer_dep_graph_hash(&peer_ids, rules.max_length));
        PkgNameVerPeer::new(base.name.clone(), base.suffix.with_peer(peer))
    }

    /// The components of `order` grouped so that every provider of a group's
    /// members is in an earlier group or in the member's own component.
    fn levels(&self, order: &tarjan::Components) -> Vec<Vec<usize>> {
        let mut level_of = vec![0; order.components.len()];
        let mut levels: Vec<Vec<usize>> = Vec::new();
        for (component, members) in order.components.iter().enumerate() {
            let level = members
                .iter()
                .flat_map(|&vid| self.providers[vid].iter())
                .filter_map(|(_, provider)| match provider {
                    Provider::Variant(provider) => order.component_of.get(provider),
                    Provider::Link(_) | Provider::Missing(_) => None,
                })
                .filter(|&&provider_component| provider_component != component)
                .map(|&provider_component| level_of[provider_component] + 1)
                .max()
                .unwrap_or(0);
            level_of[component] = level;
            if levels.len() <= level {
                levels.resize_with(level + 1, Vec::new);
            }
            levels[level].push(component);
        }
        levels
    }

    /// Variants grouped by peer cycle, each group after the groups of its
    /// providers. Only variants with bindings form cycles.
    fn providers_first(&self) -> tarjan::Components {
        tarjan::components(
            self.keys.len(),
            |vid| {
                self.providers[vid]
                    .iter()
                    .filter_map(|(_, provider)| match provider {
                        Provider::Variant(provider) => Some(*provider),
                        Provider::Link(_) | Provider::Missing(_) => None,
                    })
                    .collect()
            },
            |vid| !self.providers[vid].is_empty(),
        )
    }

    /// Peer names on a cycle of the graph from a package's name to the names
    /// of the peers it binds.
    fn cyclic_peer_names(&self) -> HashSet<String> {
        let mut names: Vec<String> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<HashSet<usize>> = Vec::new();
        let mut intern = |name: String, edges: &mut Vec<HashSet<usize>>| {
            *index
                .entry(name.clone())
                .or_insert_with(|| {
                    names.push(name);
                    edges.push(HashSet::new());
                    names.len() - 1
                })
        };
        for vid in 0..self.keys.len() {
            if self.providers[vid].is_empty() {
                continue;
            }
            let from = intern(self.keys[vid].name.to_string(), &mut edges);
            for (name, _) in &self.providers[vid] {
                let to = intern(name.to_string(), &mut edges);
                edges[from].insert(to);
            }
        }
        let components =
            tarjan::components(edges.len(), |node| edges[node].iter().copied().collect(), |_| true);
        components.components
            .iter()
            .filter(|component| match component.as_slice() {
                [only] => edges[*only].contains(only),
                _ => true,
            })
            .flatten()
            .map(|&node| names[node].clone())
            .collect()
    }

    fn snapshot_entries(
        &self,
        keys: &[PkgNameVerPeer],
        context: RestoreContext<'_>,
    ) -> HashMap<PackageKey, SnapshotEntry> {
        (0..self.keys.len())
            .into_par_iter()
            .map(|vid| (keys[vid].clone(), self.snapshot_entry(vid, keys, context)))
            .collect()
    }

    fn snapshot_entry(
        &self,
        vid: Vid,
        keys: &[PkgNameVerPeer],
        context: RestoreContext<'_>,
    ) -> SnapshotEntry {
        let entry = self.entries[vid];
        let child_ref = |alias: &PkgName, reference: &SnapshotDepRef| match child_named(
            &self.children[vid],
            alias,
        ) {
            Some(child) => with_key(reference, &keys[child]),
            None => reference.clone(),
        };
        let restore_map =
            |map: &Option<HashMap<PkgName, SnapshotDepRef>>| -> HashMap<PkgName, SnapshotDepRef> {
                map.iter()
                    .flatten()
                    .map(|(alias, reference)| (alias.clone(), child_ref(alias, reference)))
                    .collect()
            };
        let mut dependencies = restore_map(&entry.dependencies);
        let mut optional_dependencies = restore_map(&entry.optional_dependencies);
        if let Some(peers) = entry.peer_dependencies.as_ref() {
            let optional_peers = context.packages
                .and_then(|packages| packages.get(&self.keys[vid].without_peer()))
                .and_then(|metadata| metadata.peer_dependencies_meta.as_ref());
            for (name, reference) in peers {
                let provider = self.providers[vid]
                    .iter()
                    .find(|(bound, _)| *bound == name)
                    .map(|(_, provider)| provider);
                let restored = match provider {
                    Some(Provider::Variant(provider)) => with_key(reference, &keys[*provider]),
                    _ => reference.clone(),
                };
                let optional = optional_peers
                    .and_then(|meta| meta.get(&name.to_string()))
                    .is_some_and(|meta| meta.optional);
                let target = if optional { &mut optional_dependencies } else { &mut dependencies };
                target.insert(name.clone(), restored);
            }
        }
        let mut transitive: Vec<String> = entry.transitive_peer_dependencies
            .iter()
            .flatten()
            .map(|(name, _)| name.to_string())
            .chain(
                entry.missing_peer_dependencies
                    .iter()
                    .flatten()
                    .cloned(),
            )
            .collect();
        transitive.sort_unstable();
        SnapshotEntry {
            id: entry.id.clone(),
            dependencies: (!dependencies.is_empty()).then_some(dependencies),
            optional_dependencies: (!optional_dependencies.is_empty()).then_some(
                optional_dependencies,
            ),
            transitive_peer_dependencies: (!transitive.is_empty()).then_some(transitive),
            patched: entry.patched,
            optional: entry.optional,
        }
    }

    fn restore_importer(
        &self,
        id: &str,
        importer: &NextProjectSnapshot,
        keys: &[PkgNameVerPeer],
    ) -> ProjectSnapshot {
        let mut snapshot = importer.snapshot.clone();
        let chosen = &self.importer_children[id];
        for map in [
            &mut snapshot.dependencies,
            &mut snapshot.dev_dependencies,
            &mut snapshot.optional_dependencies,
        ]
        .into_iter()
        .flatten()
        {
            for (alias, spec) in map.iter_mut() {
                if let Some(vid) = child_named(chosen, alias) {
                    spec.version = importer_version_with_key(&spec.version, &keys[vid]);
                }
            }
        }
        snapshot
    }
}

struct SuffixRules {
    dedupe: bool,
    max_length: usize,
}

impl SuffixRules {
    fn of(context: RestoreContext<'_>) -> Self {
        let settings = context.settings;
        let max_length = settings
            .and_then(|settings| settings.peers_suffix_max_length)
            .unwrap_or(DEFAULT_PEERS_SUFFIX_MAX_LENGTH);
        SuffixRules {
            dedupe: settings.is_some_and(|settings| settings.dedupe_peers == Some(true)),
            max_length: usize::try_from(max_length).unwrap_or(usize::MAX),
        }
    }
}

/// An order of package names that every process agrees on.
fn compare_names(left: &PkgName, right: &PkgName) -> std::cmp::Ordering {
    (&left.scope, &left.bare).cmp(&(&right.scope, &right.bare))
}

fn child_named(children: &[(&PkgName, Vid)], name: &PkgName) -> Option<Vid> {
    children
        .binary_search_by(|(alias, _)| compare_names(alias, name))
        .ok()
        .map(|index| children[index].1)
}

/// Whether the reference carries peer groups, not only a patch group.
fn has_peer_groups(version: &PkgVerPeer) -> bool {
    version.peer().len() > version.patch_group().len()
}

fn same_target(name: &PkgName, left: &SnapshotDepRef, right: &SnapshotDepRef) -> bool {
    match (left.as_link_target(), right.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let target = |reference: &SnapshotDepRef| {
                reference.resolve(name).map(|key| key.without_peer_bindings())
            };
            target(left) == target(right)
        }
        _ => false,
    }
}

fn importer_target_is(
    name: &PkgName,
    version: &ImporterDepVersion,
    reference: &SnapshotDepRef,
) -> bool {
    match (version.as_link_target(), reference.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let own = version.resolved_key(name).map(|key| key.without_peer_bindings());
            own.is_some() && own == reference.resolve(name).map(|key| key.without_peer_bindings())
        }
        _ => false,
    }
}

/// `name@version` of a provider, as the resolver writes a collapsed peer.
fn pair_of(key: &PkgNameVerPeer) -> PeerId {
    PeerId::Pair { name: key.name.to_string(), version: key.suffix.without_peer().to_string() }
}

/// `reference` pointing at `key`, in the same shape.
fn with_key(reference: &SnapshotDepRef, key: &PkgNameVerPeer) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(_) => SnapshotDepRef::Plain(key.suffix.clone()),
        SnapshotDepRef::Alias(_) => SnapshotDepRef::Alias(key.clone()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

fn importer_version_with_key(
    version: &ImporterDepVersion,
    key: &PkgNameVerPeer,
) -> ImporterDepVersion {
    match version {
        ImporterDepVersion::Regular(_) => ImporterDepVersion::Regular(key.suffix.clone()),
        ImporterDepVersion::Alias(_) => ImporterDepVersion::Alias(key.clone()),
        ImporterDepVersion::File(payload) => {
            let full = key.suffix.to_string();
            ImporterDepVersion::File(
                full.strip_prefix("file:").map_or_else(|| payload.clone(), str::to_string),
            )
        }
        ImporterDepVersion::Link(path) => ImporterDepVersion::Link(path.clone()),
    }
}
