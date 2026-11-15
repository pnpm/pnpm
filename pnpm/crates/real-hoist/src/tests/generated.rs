//! Random peer-dependency graphs shaped like pnpm lockfiles, hoisted
//! under [`assert_resolves_as_declared`](super::resolution::assert_resolves_as_declared).

use super::{HoistOpts, HoisterTree, Rc, nm_hoist, tree_node, tree_root};
use std::{
    collections::{BTreeMap, HashMap},
    panic::{AssertUnwindSafe, catch_unwind},
};

const NAMES: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
const VERSIONS: u8 = 2;
const SEEDS: u64 = 20_000;

/// A linear congruential generator, so a failing seed reproduces.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }

    fn version(&mut self) -> u8 {
        1 + u8::try_from(self.below(u64::from(VERSIONS))).unwrap()
    }
}

/// The manifests of every package version: regular dependencies and an
/// optional peer. A package only depends on names after its own, so the
/// graph has no cycles.
struct Registry {
    dependencies: HashMap<(usize, u8), Vec<(usize, u8)>>,
    peer: HashMap<(usize, u8), usize>,
}

fn registry(rng: &mut Rng) -> Registry {
    let mut dependencies = HashMap::new();
    let mut peer = HashMap::new();
    for name in 0..NAMES.len() {
        for version in 1..=VERSIONS {
            let (deps, peer_name) = manifest(rng, name);
            dependencies.insert((name, version), deps);
            if let Some(peer_name) = peer_name {
                peer.insert((name, version), peer_name);
            }
        }
    }
    Registry { dependencies, peer }
}

/// Random dependencies and peer of one version of `NAMES[name]`.
fn manifest(rng: &mut Rng, name: usize) -> (Vec<(usize, u8)>, Option<usize>) {
    let later = name + 1..NAMES.len();
    let mut deps: Vec<(usize, u8)> = Vec::new();
    for dep in later.clone() {
        if rng.below(3) == 0 {
            deps.push((dep, rng.version()));
        }
    }
    let peer_name = (!later.is_empty() && rng.below(2) == 0).then(|| {
        name + 1 + usize::try_from(rng.below(later.len() as u64)).unwrap()
    });
    deps.retain(|(dep, _)| Some(*dep) != peer_name);
    // A peer points at a later name, so building the later names first
    // lets a sibling's peer resolve to it.
    deps.reverse();
    (deps, peer_name)
}

/// The peers a node resolves from outside its own dependencies, by name.
type ExternalPeers = BTreeMap<String, Rc<HoisterTree>>;

/// Build the snapshot of `package`, resolving peers the way pnpm does:
/// from the parent's dependencies first, then each ancestor's. `scopes`
/// lists those dependency sets, nearest first. A peer resolved from
/// outside the node joins its peer set and its reference suffix, as
/// `transitivePeerDependencies` does in a lockfile.
fn build(
    registry: &Registry,
    package: (usize, u8),
    scopes: &[Vec<Rc<HoisterTree>>],
    snapshots: &mut HashMap<String, Rc<HoisterTree>>,
) -> (Rc<HoisterTree>, ExternalPeers) {
    let mut external = ExternalPeers::new();
    let mut children: Vec<Rc<HoisterTree>> = Vec::new();
    let peer_provider = registry.peer
        .get(&package)
        .and_then(|peer| {
            scopes
                .iter()
                .find_map(|scope| {
                    scope
                        .iter()
                        .find(|node| node.name == NAMES[*peer])
                        .cloned()
                })
        });
    if let Some(provider) = peer_provider {
        external.insert(provider.name.clone(), Rc::clone(&provider));
        children.push(provider);
    }
    for &dependency in &registry.dependencies[&package] {
        let child_scopes: Vec<Vec<Rc<HoisterTree>>> = std::iter::once(children.clone())
            .chain(scopes.iter().cloned())
            .collect();
        let (child, child_external) = build(registry, dependency, &child_scopes, snapshots);
        for (name, provider) in child_external {
            if !children.iter().any(|sibling| Rc::ptr_eq(sibling, &provider)) {
                external.insert(name, provider);
            }
        }
        children.push(child);
    }

    let name = NAMES[package.0];
    let suffix: String = external
        .values()
        .map(|provider| ["(", provider.reference.as_str(), ")"].concat())
        .collect();
    let reference = format!("{name}@{}{suffix}", package.1);
    let peer_names: Vec<&str> = external
        .keys()
        .map(String::as_str)
        .collect();
    let node = snapshots
        .entry(reference.clone())
        .or_insert_with(|| tree_node(name, &reference, &peer_names, children));
    (Rc::clone(node), external)
}

fn importer(seed: u64) -> Rc<HoisterTree> {
    let mut rng = Rng(seed);
    let registry = registry(&mut rng);
    let mut snapshots = HashMap::new();
    let mut root_deps: Vec<Rc<HoisterTree>> = Vec::new();
    for name in (0..NAMES.len()).rev() {
        if rng.below(2) == 0 {
            let package = (name, rng.version());
            let (node, _) = build(&registry, package, &[root_deps.clone()], &mut snapshots);
            root_deps.push(node);
        }
    }
    tree_root(root_deps)
}

#[test]
fn generated_peer_graphs_resolve_as_declared() {
    let failures: Vec<(u64, String)> = (0..SEEDS)
        .filter_map(|seed| {
            let root = importer(seed);
            let error =
                catch_unwind(AssertUnwindSafe(|| nm_hoist(&root, &HoistOpts::default()))).err()?;
            let message = error
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_default();
            Some((seed, message))
        })
        .collect();
    assert!(failures.is_empty(), "{} graphs break, first: {:?}", failures.len(), failures.first());
}
