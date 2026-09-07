//! Peer-resolution context helpers: the propagating parent-reference
//! map, the shared ancestor chains threaded down the walk, `link:`
//! node-id remapping, peer-suffix parsing, and range matching.

use crate::{
    node_id::NodeId,
    resolve_peers::{ResolvePeersOptions, walker::Walker},
    resolved_tree::{ResolvedPackage, TreeChildren},
};
use node_semver::{Range, Version};
use pnpm_deps_path::{DepPath, PeerId, index_of_dep_path_suffix};
use pnpm_resolving_resolver_base::{ResolveResult, get_peer_version_range};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Per-name entry in the propagating [`ParentRefs`] map.
#[derive(Debug, Clone)]
pub(super) struct ParentRef {
    pub(super) version: String,
    /// `None` for top-level deps that were already installed. Pacquet
    /// doesn't surface those yet — `None` only appears on the
    /// importer-level cycle-break fallback, where the `name@version`
    /// form of the peer-id is the only useful representation.
    pub(super) node_id: Option<NodeId>,
    /// Local install name in `node_modules`. May differ from the
    /// package's real name for npm-alias entries.
    pub(super) alias: Option<String>,
    /// Depth at which this parent was added. Threaded into
    /// [`ParentPkgInfo`] so [`Walker::parent_packages_match`] can
    /// apply the depth-equality fallback when peer dependencies are
    /// shadowed across occurrences.
    pub(super) depth: i32,
    /// Per-name shadowing counter. Incremented when a same-name
    /// parent is added at a deeper walk that doesn't match the
    /// existing entry. Used by [`Walker::parent_packages_match`] to
    /// detect shadowed peers.
    pub(super) occurrence: u32,
}

/// `name → ParentRef` map propagated down the walk. Entries are indexed
/// by both the package's real name and its alias when the two differ —
/// `react-dom@npm:next` resolves a `peerDependencies.react-dom`
/// requirement against the alias and `peerDependencies.next` against
/// the real name.
pub(super) type ParentRefs = HashMap<String, ParentRef>;

/// Per-peer-name snapshot stored on [`Walker::parent_pkgs_of_node`].
///
/// `pkg_id` is `None` for parents that came in without a real
/// `NodeId` (the importer-level `topParents` path); those
/// fall back to a pure `version` comparison.
#[derive(Debug, Clone)]
pub(super) struct ParentPkgInfo {
    pub(super) pkg_id: Option<Arc<str>>,
    pub(super) version: Option<String>,
    pub(super) depth: i32,
    pub(super) occurrence: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct SharedChain<Element>(Option<Arc<SharedChainLink<Element>>>);

#[derive(Debug)]
struct SharedChainLink<Element> {
    value: Element,
    parent: Option<Arc<SharedChainLink<Element>>>,
}

impl<Element> Default for SharedChain<Element> {
    fn default() -> Self {
        SharedChain(None)
    }
}

impl<Element> SharedChain<Element> {
    pub(crate) fn pushed(&self, value: Element) -> Self {
        SharedChain(Some(Arc::new(SharedChainLink { value, parent: self.0.clone() })))
    }

    pub(crate) fn iter(&self) -> SharedChainIter<'_, Element> {
        SharedChainIter { next: self.0.as_deref() }
    }
}

impl<Element: PartialEq> SharedChain<Element> {
    /// Takes `&str` rather than `&Element` so a caller holding a
    /// shared package id can test membership without allocating a
    /// `String` to compare against.
    pub(super) fn contains_str(&self, value: &str) -> bool
    where
        Element: AsRef<str>,
    {
        self.iter().any(|item| item.as_ref() == value)
    }
}

/// Memo for [`SharedChain::any_memoized`], keyed by link address: the
/// answer for a link covers that link and everything above it, so it
/// holds for every chain that shares the suffix. One memo is valid for
/// one predicate.
///
/// Each entry keeps the link it is keyed on alive. Addresses are only
/// unique among live allocations, so a dropped link could otherwise
/// hand its address — and its answer — to whatever was allocated there
/// next.
pub(super) struct ChainSuffixMemo<Element> {
    answers: HashMap<usize, (Arc<SharedChainLink<Element>>, bool)>,
}

impl<Element> Default for ChainSuffixMemo<Element> {
    fn default() -> Self {
        ChainSuffixMemo { answers: HashMap::default() }
    }
}

impl<Element> SharedChain<Element> {
    /// Whether any element from here to the root satisfies `predicate`,
    /// answering from `memo` for suffixes already evaluated. Chains built
    /// by pushing onto a common ancestor share those suffixes, so a
    /// repeated query over a family of chains costs each link once
    /// instead of once per chain.
    ///
    /// `predicate` must be pure: it is called on the links this call is
    /// first to reach, in root-to-tip order, and skipped entirely for
    /// suffixes the memo already answers.
    pub(super) fn any_memoized(
        &self,
        memo: &mut ChainSuffixMemo<Element>,
        mut predicate: impl FnMut(&Element) -> bool,
    ) -> bool {
        let mut unmemoized = Vec::new();
        let mut cursor = self.0.as_ref();
        let mut satisfied = false;
        while let Some(link) = cursor {
            if let Some(&(_, answer)) = memo.answers.get(&link_key(link)) {
                satisfied = answer;
                break;
            }
            unmemoized.push(link);
            cursor = link.parent.as_ref();
        }
        for link in unmemoized.into_iter().rev() {
            satisfied = satisfied || predicate(&link.value);
            memo.answers.insert(link_key(link), (Arc::clone(link), satisfied));
        }
        satisfied
    }
}

fn link_key<Element>(link: &Arc<SharedChainLink<Element>>) -> usize {
    Arc::as_ptr(link) as usize
}

impl<Element: Clone> SharedChain<Element> {
    pub(crate) fn to_root_vec(&self) -> Vec<Element> {
        let mut values: Vec<Element> = self.iter().cloned().collect();
        values.reverse();
        values
    }
}

pub(crate) struct SharedChainIter<'a, Element> {
    next: Option<&'a SharedChainLink<Element>>,
}

impl<'a, Element> Iterator for SharedChainIter<'a, Element> {
    type Item = &'a Element;

    fn next(&mut self) -> Option<Self::Item> {
        let link = self.next?;
        self.next = link.parent.as_deref();
        Some(&link.value)
    }
}

/// One importer whose direct dependencies count as "current" peer
/// providers for the must-win guard of locked-peer-provider reuse.
pub(super) struct CurrentProviderSource {
    pub(super) direct_node_ids_by_alias: HashMap<String, NodeId>,
    pub(super) declared_direct_dependencies: HashSet<String>,
    pub(super) explicitly_requested_direct_dependencies: HashSet<String>,
}

impl Walker<'_> {
    /// Build the `(peer_name → ParentPkgInfo)` snapshot that gets
    /// stored on [`Self::parent_pkgs_of_node`] for each child the
    /// caller is about to descend into.
    ///
    /// `link:` parents don't have a real tree entry; pacquet's
    /// [`ParentRef`] keeps the `NodeId` but the tree-lookup falls back
    /// to a pure `version` comparison.
    pub(super) fn parent_dep_paths_from_refs(
        &self,
        parent_refs: &ParentRefs,
    ) -> Arc<HashMap<String, ParentPkgInfo>> {
        let mut out = HashMap::default();
        for (name, parent_ref) in parent_refs {
            if !self.tree.all_peer_dep_names.contains(name) {
                continue;
            }
            let pkg_id = parent_ref
                .node_id
                .as_ref()
                .and_then(|nid| self.tree.dependencies_tree.get(nid))
                .map(|tn| std::sync::Arc::<str>::clone(&tn.resolved_package_id));
            let version = pkg_id.is_none().then(|| parent_ref.version.clone());
            out.insert(
                name.clone(),
                ParentPkgInfo {
                    pkg_id,
                    version,
                    depth: parent_ref.depth,
                    occurrence: parent_ref.occurrence,
                },
            );
        }
        // Shared, not cloned: the same snapshot is recorded for every
        // child of a node, and these maps dominated the walker's
        // allocation churn when cloned per child.
        Arc::new(out)
    }

    pub(super) fn parent_refs_match(&self, current: &ParentRef, new: &ParentRef) -> bool {
        if current.version != new.version || current.alias != new.alias {
            return false;
        }
        let Some(current_name) = self.parent_ref_package_name(current) else {
            return true;
        };
        let Some(new_name) = self.parent_ref_package_name(new) else {
            return true;
        };
        current_name == new_name
    }

    fn parent_ref_package_name(&self, parent_ref: &ParentRef) -> Option<String> {
        let node_id = parent_ref.node_id.as_ref()?;
        let tree_node = self.tree.dependencies_tree.get(node_id)?;
        let pkg = self.tree.packages.get(&tree_node.resolved_package_id)?;
        Some(pkg_name_version(&pkg.result).0)
    }

    pub(super) fn inherited_parent_pkg_breaks_peer_diamond(
        &self,
        parent_refs: &ParentRefs,
        inherited_parent_pkg: &ParentRef,
        own_child_parent_pkg: &ParentRef,
        node_id: &NodeId,
    ) -> bool {
        let (Some(inherited_node_id), Some(own_child_node_id)) =
            (inherited_parent_pkg.node_id.as_ref(), own_child_parent_pkg.node_id.as_ref())
        else {
            return false;
        };
        if inherited_node_id == own_child_node_id {
            return false;
        }
        let Some(inherited_context) = self.parent_pkgs_of_node.get(inherited_node_id) else {
            return false;
        };
        let Some(parent_pkg) = self
            .tree
            .dependencies_tree
            .get(own_child_node_id)
            .and_then(|node| self.tree.packages.get(&node.resolved_package_id))
        else {
            return false;
        };
        let (parent_pkg_name, _) = pkg_name_version(&parent_pkg.result);

        let mut conflicting_peers = HashSet::default();
        for peer_name in parent_pkg.peer_dependencies.keys() {
            if !self.tree.all_peer_dep_names.contains(peer_name) {
                continue;
            }
            let Some(inherited_peer) = inherited_context.get(peer_name) else { continue };
            let Some(current_peer) = parent_refs.get(peer_name) else { continue };
            if self.parent_peer_differs(current_peer, inherited_peer) {
                conflicting_peers.insert(peer_name.clone());
            }
        }
        if conflicting_peers.is_empty() {
            return false;
        }

        let Some(node) = self.tree.dependencies_tree.get(node_id) else { return false };
        let child_pkg_ids: Vec<&str> = match &node.children {
            TreeChildren::Realized(children) => children
                .values()
                .filter_map(|child_node_id| self.tree.dependencies_tree.get(child_node_id))
                .map(|child| &*child.resolved_package_id)
                .collect(),
            TreeChildren::Lazy { .. } => self
                .tree
                .children_by_id
                .get(&node.resolved_package_id)
                .into_iter()
                .flat_map(|children| children.iter())
                .map(|child| &*child.pkg_id)
                .collect(),
        };
        for child_pkg_id in child_pkg_ids {
            let Some(child_pkg) = self.tree.packages.get(child_pkg_id) else { continue };
            if !child_pkg.peer_dependencies.contains_key(&parent_pkg_name) {
                continue;
            }
            if conflicting_peers.iter().any(|peer| child_pkg.peer_dependencies.contains_key(peer)) {
                return true;
            }
        }
        false
    }

    fn parent_peer_differs(
        &self,
        current_peer: &ParentRef,
        inherited_peer: &ParentPkgInfo,
    ) -> bool {
        if let Some(inherited_pkg_id) = inherited_peer.pkg_id.as_ref() {
            let Some(current_node_id) = current_peer.node_id.as_ref() else {
                return true;
            };
            return self
                .tree
                .dependencies_tree
                .get(current_node_id)
                .is_none_or(|node| *node.resolved_package_id != **inherited_pkg_id);
        }
        inherited_peer.version.as_ref() != Some(&current_peer.version)
    }
}

/// Record a parent ref under both keys: each parent is recorded by its
/// install alias *and* its real name when the two differ.
///
/// `parent_node_id` is the [`NodeId`] the parent should appear under
/// in the [`ParentRefs`] map. For most parents this is just
/// `direct.node_id`, but `link:` parents may carry the remapped
/// node id produced by [`remap_link_node_id`] when
/// `excludeLinksFromLockfile` is on.
pub(super) fn insert_parent_ref(
    refs: &mut ParentRefs,
    direct_alias: &str,
    parent_node_id: NodeId,
    pkg: &ResolvedPackage,
    depth: i32,
) {
    let (real_name, version) = pkg_name_version(&pkg.result);
    let parent_ref = ParentRef {
        version,
        node_id: Some(parent_node_id),
        alias: (direct_alias != real_name).then(|| direct_alias.to_string()),
        depth,
        occurrence: 0,
    };
    update_parent_refs(refs, direct_alias, &parent_ref);
    if direct_alias != real_name {
        update_parent_refs(refs, &real_name, &parent_ref);
    }
}

fn update_parent_refs(refs: &mut ParentRefs, new_alias: &str, parent_ref: &ParentRef) {
    if let Some(existing) = refs.get(new_alias) {
        let existing_has_alias = existing.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if !existing_has_alias {
            return;
        }
        let new_has_alias = parent_ref.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if new_has_alias && version_gte(&existing.version, &parent_ref.version) {
            return;
        }
    }
    refs.insert(new_alias.to_string(), parent_ref.clone());
}

fn version_gte(left: &str, right: &str) -> bool {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left >= right,
        _ => left >= right,
    }
}

/// Reinterpret a `link:<rel>` [`NodeId`] as a [`DepPath`].
///
/// Linked top-parent `NodeIds` (whether the workspace-link arm or the
/// `excludeLinksFromLockfile` remap) never enter the dependency tree,
/// so [`Walker::node_dep_paths`] never maps them. The `link:<rel>`
/// `NodeId` is itself a well-formed pnpm `DepPath`, so the snapshot
/// child edge can use it verbatim.
pub(super) fn link_node_id_as_dep_path(node_id: &NodeId) -> Option<DepPath> {
    let NodeId::Leaf(id) = node_id else { return None };
    id.starts_with("link:").then(|| DepPath::from(id.to_string()))
}

pub(super) fn importer_relative_link_dep_path(
    dep_path: &DepPath,
    anchor: &crate::link_target::ImporterAnchor,
    lockfile_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> DepPath {
    let Some(target) = dep_path.as_str().strip_prefix("link:") else {
        return dep_path.clone();
    };
    let (Some(lockfile_dir), Some(project_dir)) = (lockfile_dir, project_dir) else {
        return dep_path.clone();
    };
    let relative_target = anchor.target_relative_to_importer(target).unwrap_or_else(|| {
        let target = Path::new(target);
        let absolute_target = if target.is_absolute() {
            pnpm_fs::lexical_normalize(target)
        } else {
            pnpm_fs::lexical_normalize(&lockfile_dir.join(target))
        };
        // `diff_paths` walks both paths component-wise, so a base still
        // carrying `.` / `..` segments would consume them as real directories
        // and count the wrong number of `..` hops back out.
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        pathdiff::diff_paths(&absolute_target, project_dir)
            .unwrap_or(absolute_target)
            .display()
            .to_string()
            .replace('\\', "/")
    });
    DepPath::from(format!("link:{relative_target}"))
}

/// Compute the `link:` [`NodeId`] under which a workspace-link parent
/// should appear in [`ParentRefs`] when
/// [`ResolvePeersOptions::exclude_links_from_lockfile`] is on.
///
/// Returns `None` when:
///
/// - the dep isn't a `link:` directory resolution — an injected
///   (`file:`) one is a real package in the graph and keeps its own
///   node id;
/// - the setting is off or the lockfile / modules dirs are missing;
/// - the link target lives under `lockfile_dir` (workspace-internal
///   link — already stable across machines, no remap needed).
///
/// On `Some`, the remap encodes `<modules_dir>/<alias>` as a path
/// relative to `lockfile_dir`, prefixed with `link:`.
pub(super) fn remap_link_node_id(
    opts: &ResolvePeersOptions,
    alias: &str,
    result: &ResolveResult,
) -> Option<NodeId> {
    if !opts.exclude_links_from_lockfile {
        return None;
    }
    let lockfile_dir = opts.lockfile_dir.as_ref()?;
    let modules_dir = opts.modules_dir.as_ref()?;
    let directory = match &result.resolution {
        pnpm_lockfile::LockfileResolution::Directory(dir) => &dir.directory,
        _ => return None,
    };
    if !result.id.as_str().starts_with("link:") {
        return None;
    }
    // A workspace link records its directory relative to the importer
    // (the local resolver's is absolute), so it has to be resolved
    // against `project_dir` before the lexical containment check.
    let link_target = match opts.project_dir.as_deref() {
        Some(project_dir) => project_dir.join(directory),
        None => PathBuf::from(directory),
    };
    if pnpm_fs::is_subdir(lockfile_dir, &link_target) {
        return None;
    }
    let target = modules_dir.join(alias);
    let rel = pathdiff::diff_paths(&target, lockfile_dir)?;
    let rel = rel.display().to_string().replace('\\', "/");
    Some(NodeId::leaf(&format!("link:{rel}")))
}

/// Pull `(name, version)` out of a `ResolveResult` the peer-resolution
/// stage can hash and compare on.
///
/// The npm-registry resolver always fills [`ResolveResult::name_ver`],
/// so the fast path lifts it straight out. The git / tarball / local
/// resolvers leave it `None` (their canonical name lives in the
/// fetched manifest, which the resolver doesn't read at resolve
/// time); for those, fall back to `(alias, id-as-string)`. The peer
/// graph machinery only ever looks the name up in
/// [`crate::ResolvedTree::all_peer_dep_names`] — a set built by parsing the
/// peer dependencies of npm-shaped packages — so the fallback's
/// "name" will simply miss every lookup, naturally short-circuiting
/// peer propagation for non-npm packages without panicking on
/// `name_ver = None`.
pub(super) fn pkg_name_version(result: &ResolveResult) -> (String, String) {
    let version = result
        .name_ver
        .as_ref()
        .map_or_else(|| result.id.as_str().to_string(), |name_ver| name_ver.suffix.to_string());
    (pkg_name(result), version)
}

/// The name half of [`fn@pkg_name_version`], for callers that would
/// discard the version. `PkgName` holds scope and bare name separately,
/// so rendering either half allocates.
pub(super) fn pkg_name(result: &ResolveResult) -> String {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return name_ver.name.to_string();
    }
    result.alias.clone().unwrap_or_else(|| result.id.as_str().to_string())
}

/// The `name@version` identity a peer contributes to a depPath's peer suffix.
///
/// A package resolved from a named registry keeps its `<registryName>:` in the
/// version slot. Dropping it would let the same name and version served by two
/// registries render one suffix, so two variants of the dependent, each bound
/// to a different peer artifact, would collapse onto a single depPath.
///
/// Separate from [`pkg_name_version`] on purpose: that version also feeds
/// semver comparisons, which a qualified string would break.
pub(super) fn peer_id_pair(result: &ResolveResult) -> PeerId {
    let (name, version) = pkg_name_version(result);
    let Some(registry_name) = named_registry_of(result) else {
        return PeerId::Pair { name, version };
    };
    PeerId::Pair { name, version: format!("{registry_name}:{version}") }
}

/// The named-registry alias of a registry-qualified resolution id
/// (`<name>@<registryName>:<version>`), if it is one.
fn named_registry_of(result: &ResolveResult) -> Option<&str> {
    let id = result.id.as_str();
    let at = id.get(1..)?.find('@')? + 1;
    let (registry_name, _) = pnpm_deps_path::parse_registry_qualified_version(id.get(at + 1..)?)?;
    Some(registry_name)
}

pub(super) fn peer_segment_names(dep_path: &DepPath) -> Option<Vec<String>> {
    let raw = dep_path.as_str();
    let suffix = index_of_dep_path_suffix(raw);
    let peers_index = suffix.peers_index?;
    let segments = split_peer_suffix_segments(&raw[peers_index..])?;
    segments.iter().map(|segment| peer_segment_name(segment).map(str::to_string)).collect()
}

fn split_peer_suffix_segments(suffix: &str) -> Option<Vec<String>> {
    let bytes = suffix.as_bytes();
    let mut segments = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (idx, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => {
                if depth == 0 {
                    start = Some(idx + 1);
                }
                depth += 1;
            }
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    let start = start.take()?;
                    segments.push(suffix[start..idx].to_string());
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    (depth == 0).then_some(segments)
}

fn peer_segment_name(segment: &str) -> Option<&str> {
    let version_separator = if let Some(unscoped) = segment.strip_prefix('@') {
        1 + unscoped.find('@')?
    } else {
        segment.find('@')?
    };
    Some(&segment[..version_separator])
}

/// Range-satisfaction check that tolerates prereleases the way
/// `@yarnpkg/core/semverUtils.satisfiesWithPrereleases` does — falls
/// back to a literal-equality check when the range can't be parsed,
/// which lets non-semver peer ranges (`*`, git refs, etc.) still
/// match.
///
/// **Prerelease tolerance.** `node-semver`'s [`Range::satisfies`]
/// rejects prerelease versions against non-prerelease comparators.
/// Yarn's `satisfiesWithPrereleases` explicitly allows that pairing.
/// We approximate it by retrying with the prerelease tag stripped: if
/// `version` is a prerelease and the straight check fails, see whether
/// the base `MAJOR.MINOR.PATCH` satisfies the range — without pulling
/// in Yarn's full per-comparator algorithm.
pub(super) fn satisfies_with_prereleases(version: &str, range: &str) -> bool {
    let parsed_range = Range::parse(range).ok();
    satisfies_with_parsed_prereleases(version, range, parsed_range.as_ref())
}

/// [`satisfies_with_prereleases`] against a range that is already
/// parsed. `parsed_range` must be `Range::parse(range).ok()`;
/// [`ComparablePeerRange`] is what keeps the two in step.
fn satisfies_with_parsed_prereleases(
    version: &str,
    range: &str,
    parsed_range: Option<&Range>,
) -> bool {
    if range == "*" {
        return true;
    }
    let Ok(parsed_version) = Version::parse(version) else {
        return version == range;
    };
    let Some(parsed_range) = parsed_range else {
        return version == range;
    };
    if parsed_version.satisfies(parsed_range) {
        return true;
    }
    if !parsed_version.is_prerelease() {
        return false;
    }
    let base = Version {
        major: parsed_version.major,
        minor: parsed_version.minor,
        patch: parsed_version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    base.satisfies(parsed_range)
}

/// A peer range in the comparable form [`get_peer_version_range`]
/// yields, paired with its parsed form so a range that many nodes
/// declare is parsed once. Holding the two together is what keeps the
/// parsed range in step with the text it came from.
pub(super) struct ComparablePeerRange {
    /// The comparable range, as peer-dependency issues quote it.
    pub(super) text: String,
    parsed: Option<Range>,
}

impl ComparablePeerRange {
    pub(super) fn new(raw_range: &str) -> Self {
        let text = get_peer_version_range(raw_range);
        let parsed = Range::parse(&text).ok();
        ComparablePeerRange { text, parsed }
    }

    /// Whether `version` satisfies this range, by
    /// [`satisfies_with_prereleases`]'s rules.
    pub(super) fn satisfies(&self, version: &str) -> bool {
        satisfies_with_parsed_prereleases(version, &self.text, self.parsed.as_ref())
    }
}

#[cfg(test)]
mod tests;
