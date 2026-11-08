//! Rebuilding each snapshot's peer-suffixed key from its providers.

use super::{
    super::{RestoreContext, tarjan},
    Graph, Provider, Sid,
};
use crate::{
    DEFAULT_PEERS_SUFFIX_MAX_LENGTH, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer,
    name_version_from_package_key, patch_groups_from_hashes,
};
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, link_path_to_peer_version};
use pnpm_patching_config::{PatchGroupRecord, get_patch_info};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

impl Graph<'_> {
    pub(super) fn keys(&self, context: RestoreContext<'_>) -> Vec<PkgNameVerPeer> {
        let rules = SuffixRules::of(context);
        let patches = Patches::of(context);
        if rules.dedupe {
            // Every provider is written as `name@version`, so no key waits for
            // another.
            return (0..self.keys.len())
                .into_par_iter()
                .map(|sid| {
                    self.key_of(sid, &rules, &patches, |_, provider| pair_of(self.keys[provider]))
                })
                .collect();
        }
        self.nested_keys(&rules, &patches)
    }

    /// `name@version` of `sid`, with its patch group when it is patched: the
    /// key of a snapshot that binds no peers.
    fn base_key(&self, sid: Sid, patches: &Patches<'_>) -> PkgNameVerPeer {
        let key = self.keys[sid];
        PkgNameVerPeer::new(key.name.clone(), key.suffix.with_peer(patches.group(self, sid)))
    }

    /// The keys when a provider's group nests its own key, computed level by
    /// level so that every provider's key is ready before its dependents'.
    fn nested_keys(&self, rules: &SuffixRules, patches: &Patches<'_>) -> Vec<PkgNameVerPeer> {
        let nesting =
            Nesting { order: self.providers_first(), cyclic_names: self.cyclic_peer_names() };
        // A snapshot without bindings has the package's key, and every other
        // snapshot may need it.
        let mut keys: Vec<Option<PkgNameVerPeer>> = (0..self.keys.len())
            .map(|sid| {
                self.providers[sid]
                    .is_empty()
                    .then(|| self.base_key(sid, patches))
            })
            .collect();
        for level in self.levels(&nesting.order) {
            for (sid, key) in self.level_keys(&level, &nesting, &keys, rules, patches) {
                keys[sid] = Some(key);
            }
        }
        keys.into_iter()
            .map(|key| key.expect("every snapshot is in one component"))
            .collect()
    }

    fn level_keys(
        &self,
        level: &[usize],
        nesting: &Nesting,
        keys: &[Option<PkgNameVerPeer>],
        rules: &SuffixRules,
        patches: &Patches<'_>,
    ) -> Vec<(Sid, PkgNameVerPeer)> {
        level
            .par_iter()
            .flat_map_iter(|&component| nesting.order.components[component].iter().copied())
            .filter(|&sid| !self.providers[sid].is_empty())
            .map(|sid| {
                let key = self.key_of(sid, rules, patches, |name, provider| {
                    match keys[provider].as_ref() {
                        Some(provider_key) if !nesting.collapses(sid, provider, name) => {
                            PeerId::DepPath(DepPath::from(provider_key.to_string()))
                        }
                        _ => pair_of(self.keys[provider]),
                    }
                });
                (sid, key)
            })
            .collect()
    }

    /// The key of `sid`, with `segment` giving each provider snapshot's peer
    /// group.
    fn key_of(
        &self,
        sid: Sid,
        rules: &SuffixRules,
        patches: &Patches<'_>,
        segment: impl Fn(&PkgName, Sid) -> PeerId,
    ) -> PkgNameVerPeer {
        let base = self.keys[sid];
        if self.providers[sid].is_empty() {
            return self.base_key(sid, patches);
        }
        let peer_ids: Vec<PeerId> = self.providers[sid]
            .iter()
            .map(|(name, provider)| match provider {
                Provider::Link(path) => PeerId::Pair {
                    name: name.to_string(),
                    version: link_path_to_peer_version(path),
                },
                Provider::Missing(pair) => PeerId::DepPath(DepPath::from(pair.clone())),
                Provider::Snapshot(provider) => segment(name, *provider),
            })
            .collect();
        let mut peer = patches.group(self, sid);
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
                .flat_map(|&sid| self.providers[sid].iter())
                .filter_map(|(_, provider)| match provider {
                    Provider::Snapshot(provider) => order.component_of.get(provider),
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

    /// Snapshots grouped by peer cycle, each group after the groups of its
    /// providers. Only snapshots with bindings form cycles.
    fn providers_first(&self) -> tarjan::Components {
        tarjan::components(
            self.keys.len(),
            |sid| {
                self.providers[sid]
                    .iter()
                    .filter_map(|(_, provider)| match provider {
                        Provider::Snapshot(provider) => Some(*provider),
                        Provider::Link(_) | Provider::Missing(_) => None,
                    })
                    .collect()
            },
            |sid| !self.providers[sid].is_empty(),
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
        for sid in 0..self.keys.len() {
            if self.providers[sid].is_empty() {
                continue;
            }
            let from = intern(self.keys[sid].name.to_string(), &mut edges);
            for (name, _) in &self.providers[sid] {
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
}

/// What decides whether a provider's group nests its key.
struct Nesting {
    order: tarjan::Components,
    cyclic_names: HashSet<String>,
}

impl Nesting {
    /// Whether the provider `sid` binds as `name` is written as
    /// `name@version`: the two are in one peer cycle, or `name` is on a cycle
    /// of peer names.
    fn collapses(&self, sid: Sid, provider: Sid, name: &PkgName) -> bool {
        let component = self.order.component_of.get(&sid);
        let in_cycle =
            component.is_some() && component == self.order.component_of.get(&provider);
        in_cycle || self.cyclic_names.contains(&name.to_string())
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

/// `name@version` of a provider, as the resolver writes a collapsed peer.
fn pair_of(key: &PkgNameVerPeer) -> PeerId {
    PeerId::Pair { name: key.name.to_string(), version: key.suffix.without_peer().to_string() }
}

/// The patch group each package's key gets, from `patchedDependencies`,
/// matched the way the resolver matches its keys to packages.
struct Patches<'a> {
    groups: Option<PatchGroupRecord>,
    packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
}

impl<'a> Patches<'a> {
    fn of(context: RestoreContext<'a>) -> Self {
        Patches {
            groups: context.patched_dependencies.and_then(patch_groups_from_hashes),
            packages: context.packages,
        }
    }

    /// `(patch_hash=…)` for a patched package, or nothing.
    fn group(&self, graph: &Graph<'_>, sid: Sid) -> String {
        let Some(groups) = self.groups.as_ref() else { return String::new() };
        let (name, version) = name_version_from_package_key(graph.keys[sid], self.packages);
        match get_patch_info(Some(groups), &name, &version) {
            Ok(Some(patch)) => format!("(patch_hash={})", patch.hash),
            _ => String::new(),
        }
    }
}
