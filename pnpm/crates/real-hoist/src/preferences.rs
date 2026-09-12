use super::{HashMap, HashSet, HoisterResult, IndexMap, Rc, VecDeque, node_ident};

/// One entry of the preference map: the set of dependent idents
/// (and peer-dependent idents) that pull in a given `(name,
/// ident)` package. Usage count is the sum of the two, matching
/// yarn's `entry.dependents.size + entry.peerDependents.size`.
#[derive(Default)]
struct PreferenceEntry {
    dependents: HashSet<String>,
    peer_dependents: HashSet<String>,
}

impl PreferenceEntry {
    fn usages(&self) -> usize {
        self.dependents.len() + self.peer_dependents.len()
    }
}

/// Port of yarn's `buildPreferenceMap` + `getHoistIdentMap`. For
/// each dependency name reachable from `root`, returns its
/// candidate idents (references) ordered most-preferred first:
///
/// 1. The root's own direct deps are seeded first, so a version the
///    root depends on always wins its name slot.
/// 2. Every other ident follows, ordered by usage (the count of
///    distinct dependents + peer-dependents) descending, stable on
///    ties (preserving depth-first discovery order).
///
/// [`hoist_into_root`](crate::hoist_into_root) consults the front of each list as the
/// currently-preferred ident and shifts it as passes progress.
/// Ports
/// <https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts>.
pub(super) fn build_hoist_ident_map(root: &Rc<HoisterResult>) -> HashMap<String, VecDeque<String>> {
    let root_children: Vec<Rc<HoisterResult>> =
        root.dependencies.borrow().iter().map(|dep| Rc::clone(&dep.0)).collect();
    let hoistable: Vec<&Rc<HoisterResult>> =
        root_children.iter().filter(|dep| !root.peer_names.contains(&dep.name)).collect();

    let preference = collect_preferences(root, &hoistable);
    let mut ident_map = seed_ident_map(root, &hoistable);
    append_preferred_idents(&mut ident_map, root, &preference);
    ident_map.into_iter().collect()
}

fn collect_preferences(
    root: &Rc<HoisterResult>,
    hoistable: &[&Rc<HoisterResult>],
) -> IndexMap<(String, String), PreferenceEntry> {
    let mut preference: IndexMap<(String, String), PreferenceEntry> = IndexMap::new();
    let mut seen: HashSet<*const HoisterResult> = HashSet::new();
    seen.insert(Rc::as_ptr(root));

    let root_ident = node_ident(root);
    for dep in hoistable {
        add_dependent(&root_ident, dep, &mut preference, &mut seen);
    }
    preference
}

/// The root and its direct deps, so their idents always rank first. Mirrors
/// `getHoistIdentMap`'s initial `identMap` construction before the sorted
/// append loop.
fn seed_ident_map(
    root: &Rc<HoisterResult>,
    hoistable: &[&Rc<HoisterResult>],
) -> IndexMap<String, VecDeque<String>> {
    let mut ident_map: IndexMap<String, VecDeque<String>> = IndexMap::new();
    ident_map.insert(root.name.clone(), VecDeque::from([node_ident(root)]));
    for dep in hoistable {
        ident_map.insert(dep.name.clone(), VecDeque::from([node_ident(dep)]));
    }
    ident_map
}

fn append_preferred_idents(
    ident_map: &mut IndexMap<String, VecDeque<String>>,
    root: &Rc<HoisterResult>,
    preference: &IndexMap<(String, String), PreferenceEntry>,
) {
    let mut keys: Vec<(String, String)> = preference.keys().cloned().collect();
    // `hoist_priority` is always 0 in pacquet, so the sort reduces to
    // usage (descending). `sort_by` is stable, so equal-usage keys
    // keep preference-map insertion order (depth-first discovery) —
    // matching yarn's `keyList.sort`, which is likewise stable on
    // equal usage.
    keys.sort_by(|left, right| preference[right].usages().cmp(&preference[left].usages()));
    for (name, ident) in keys {
        if root.peer_names.contains(&name) {
            continue;
        }
        let idents = ident_map.entry(name).or_default();
        if !idents.contains(&ident) {
            idents.push_back(ident);
        }
    }
}

/// Recursive half of [`build_hoist_ident_map`]'s preference pass.
/// Records `dependent_ident` as a dependent of `node`, then (the
/// first time `node` is seen) recurses into its non-peer children
/// and records peer children as peer-dependents. Mirrors yarn's
/// `addDependent`.
fn add_dependent(
    dependent_ident: &str,
    node: &Rc<HoisterResult>,
    preference: &mut IndexMap<(String, String), PreferenceEntry>,
    seen: &mut HashSet<*const HoisterResult>,
) {
    let parent_ident = node_ident(node);
    preference
        .entry((node.name.clone(), parent_ident.clone()))
        .or_default()
        .dependents
        .insert(dependent_ident.to_string());

    if seen.insert(Rc::as_ptr(node)) {
        let children: Vec<Rc<HoisterResult>> =
            node.dependencies.borrow().iter().map(|dep| Rc::clone(&dep.0)).collect();
        for child in children {
            if node.peer_names.contains(&child.name) {
                preference
                    .entry((child.name.clone(), node_ident(&child)))
                    .or_default()
                    .peer_dependents
                    .insert(parent_ident.clone());
            } else {
                add_dependent(&parent_ident, &child, preference, seen);
            }
        }
    }
}

/// Whether `child` carries the ident currently preferred for its
/// name. Names absent from `hoist_ident_map` (none reachable, or a
/// root peer) carry no preference and hoist freely. Ports yarn's
/// `hoistedIdent === node.ident` gate in `getNodeHoistInfo` at
/// [hoist.ts:387](https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L387).
pub(super) fn is_preferred_ident(
    child: &HoisterResult,
    hoist_ident_map: &HashMap<String, VecDeque<String>>,
) -> bool {
    let Some(idents) = hoist_ident_map.get(&child.name) else {
        return true;
    };
    let Some(preferred) = idents.front() else {
        return true;
    };
    child.references.borrow().iter().next().is_some_and(|reference| reference == preferred)
}
