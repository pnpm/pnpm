//! Rebuilding each variant's peer-suffixed key from its providers.

use super::{
    super::{RestoreContext, tarjan},
    Graph, Provider, Vid,
};
use crate::{DEFAULT_PEERS_SUFFIX_MAX_LENGTH, PkgName, PkgNameVerPeer};
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, link_path_to_peer_version};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

impl Graph<'_> {
    pub(super) fn keys(&self, context: RestoreContext<'_>) -> Vec<PkgNameVerPeer> {
        let rules = SuffixRules::of(context);
        if rules.dedupe {
            // Every provider is written as `name@version`, so no key waits for
            // another.
            return (0..self.keys.len())
                .into_par_iter()
                .map(|vid| self.key_of(vid, &rules, |_, provider| pair_of(self.keys[provider])))
                .collect();
        }
        self.nested_keys(&rules)
    }

    /// The keys when a provider's group nests its own key, computed level by
    /// level so that every provider's key is ready before its dependents'.
    fn nested_keys(&self, rules: &SuffixRules) -> Vec<PkgNameVerPeer> {
        let nesting =
            Nesting { order: self.providers_first(), cyclic_names: self.cyclic_peer_names() };
        // A variant without bindings has the package's key, and every other
        // variant may need it.
        let mut keys: Vec<Option<PkgNameVerPeer>> = (0..self.keys.len())
            .map(|vid| {
                self.providers[vid]
                    .is_empty()
                    .then(|| self.keys[vid].clone())
            })
            .collect();
        for level in self.levels(&nesting.order) {
            for (vid, key) in self.level_keys(&level, &nesting, &keys, rules) {
                keys[vid] = Some(key);
            }
        }
        keys.into_iter()
            .map(|key| key.expect("every variant is in one component"))
            .collect()
    }

    fn level_keys(
        &self,
        level: &[usize],
        nesting: &Nesting,
        keys: &[Option<PkgNameVerPeer>],
        rules: &SuffixRules,
    ) -> Vec<(Vid, PkgNameVerPeer)> {
        level
            .par_iter()
            .flat_map_iter(|&component| nesting.order.components[component].iter().copied())
            .filter(|&vid| !self.providers[vid].is_empty())
            .map(|vid| {
                let key = self.key_of(vid, rules, |name, provider| match keys[provider].as_ref() {
                    Some(provider_key) if !nesting.collapses(vid, provider, name) => {
                        PeerId::DepPath(DepPath::from(provider_key.to_string()))
                    }
                    _ => pair_of(self.keys[provider]),
                });
                (vid, key)
            })
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
}

/// What decides whether a provider's group nests its key.
struct Nesting {
    order: tarjan::Components,
    cyclic_names: HashSet<String>,
}

impl Nesting {
    /// Whether the provider `vid` binds as `name` is written as
    /// `name@version`: the two are in one peer cycle, or `name` is on a cycle
    /// of peer names.
    fn collapses(&self, vid: Vid, provider: Vid, name: &PkgName) -> bool {
        let component = self.order.component_of.get(&vid);
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
