use super::{
    DepPath, NodeId, Path, PathBuf, PeerId, Range, ResolvePeersOptions, ResolveResult, Version,
    get_peer_version_range, index_of_dep_path_suffix,
};

pub(super) fn version_gte(left: &str, right: &str) -> bool {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left >= right,
        _ => left >= right,
    }
}

/// Reinterpret a `link:<rel>` [`NodeId`] as a [`DepPath`].
///
/// Linked top-parent `NodeIds` (whether the workspace-link arm or the
/// `excludeLinksFromLockfile` remap) never enter the dependency tree,
/// so [`Walker::node_dep_paths`](crate::resolve_peers::walker::Walker::node_dep_paths) never maps them. The `link:<rel>`
/// `NodeId` is itself a well-formed pnpm `DepPath`, so the snapshot
/// child edge can use it verbatim.
pub(in super::super) fn link_node_id_as_dep_path(node_id: &NodeId) -> Option<DepPath> {
    let NodeId::Leaf(id) = node_id else { return None };
    id.starts_with("link:").then(|| DepPath::from(id.to_string()))
}

pub(in super::super) fn importer_relative_link_dep_path(
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
/// should appear in [`ParentRefs`](super::ParentRefs) when
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
pub(in super::super) fn remap_link_node_id(
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
pub(in super::super) fn pkg_name_version(result: &ResolveResult) -> (String, String) {
    let version = result
        .name_ver
        .as_ref()
        .map_or_else(|| result.id.as_str().to_string(), |name_ver| name_ver.suffix.to_string());
    (pkg_name(result), version)
}

/// The name half of [`fn@pkg_name_version`], for callers that would
/// discard the version. `PkgName` holds scope and bare name separately,
/// so rendering either half allocates.
pub(in super::super) fn pkg_name(result: &ResolveResult) -> String {
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
pub(in super::super) fn peer_id_pair(result: &ResolveResult) -> PeerId {
    let (name, version) = pkg_name_version(result);
    let Some(registry_name) = named_registry_of(result) else {
        return PeerId::Pair { name, version };
    };
    PeerId::Pair { name, version: format!("{registry_name}:{version}") }
}

/// The named-registry alias of a registry-qualified resolution id
/// (`<name>@<registryName>:<version>`), if it is one.
pub(super) fn named_registry_of(result: &ResolveResult) -> Option<&str> {
    let id = result.id.as_str();
    let at = id.get(1..)?.find('@')? + 1;
    let (registry_name, _) = pnpm_deps_path::parse_registry_qualified_version(id.get(at + 1..)?)?;
    Some(registry_name)
}

pub(in super::super) fn peer_segment_names(dep_path: &DepPath) -> Option<Vec<String>> {
    let raw = dep_path.as_str();
    let suffix = index_of_dep_path_suffix(raw);
    let peers_index = suffix.peers_index?;
    let segments = split_peer_suffix_segments(&raw[peers_index..])?;
    segments.iter().map(|segment| peer_segment_name(segment).map(str::to_string)).collect()
}

/// Splits a peer suffix into its segment bodies. `None` when the suffix is
/// anything but a flat run of balanced parenthesised groups.
pub(super) fn split_peer_suffix_segments(suffix: &str) -> Option<Vec<String>> {
    let mut split = PeerSuffixSplit::default();
    for (idx, byte) in suffix.as_bytes().iter().enumerate() {
        split.push_byte(suffix, idx, *byte)?;
    }
    (split.depth == 0).then_some(split.segments)
}

#[derive(Default)]
pub(super) struct PeerSuffixSplit {
    pub(super) segments: Vec<String>,
    pub(super) depth: i32,
    pub(super) start: Option<usize>,
}

impl PeerSuffixSplit {
    pub(super) fn push_byte(&mut self, suffix: &str, idx: usize, byte: u8) -> Option<()> {
        match byte {
            b'(' => {
                if self.depth == 0 {
                    self.start = Some(idx + 1);
                }
                self.depth += 1;
            }
            b')' => {
                self.depth -= 1;
                if self.depth < 0 {
                    return None;
                }
                if self.depth == 0 {
                    let start = self.start.take()?;
                    self.segments.push(suffix[start..idx].to_string());
                }
            }
            _ if self.depth == 0 => return None,
            _ => {}
        }
        Some(())
    }
}

pub(super) fn peer_segment_name(segment: &str) -> Option<&str> {
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
pub(in super::super) fn satisfies_with_prereleases(version: &str, range: &str) -> bool {
    let parsed_range = Range::parse(range).ok();
    satisfies_with_parsed_prereleases(version, range, parsed_range.as_ref())
}

/// [`satisfies_with_prereleases`] against a range that is already
/// parsed. `parsed_range` must be `Range::parse(range).ok()`;
/// [`ComparablePeerRange`] is what keeps the two in step.
pub(super) fn satisfies_with_parsed_prereleases(
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
pub(in super::super) struct ComparablePeerRange {
    /// The comparable range, as peer-dependency issues quote it.
    pub(in super::super) text: String,
    pub(super) parsed: Option<Range>,
}

impl ComparablePeerRange {
    pub(in super::super) fn new(raw_range: &str) -> Self {
        let text = get_peer_version_range(raw_range);
        let parsed = Range::parse(&text).ok();
        ComparablePeerRange { text, parsed }
    }

    /// Whether `version` satisfies this range, by
    /// [`satisfies_with_prereleases`]'s rules.
    pub(in super::super) fn satisfies(&self, version: &str) -> bool {
        satisfies_with_parsed_prereleases(version, &self.text, self.parsed.as_ref())
    }
}
