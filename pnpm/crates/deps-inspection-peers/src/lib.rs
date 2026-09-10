//! Peer-dependency issue inspection, shared by every command that
//! reports unmet peers.
//!
//! The verdict is read off a lockfile rather than off a live
//! resolution: a lockfile records each snapshot's `peerDependencies`
//! next to the dependency refs the resolver settled on, which is all a
//! verdict needs. `pnpm peers check` reads one off disk; an install
//! passes the one it just resolved. Sharing the source is what keeps
//! the two commands from disagreeing about the same tree.
//!
//! Mirrors pnpm's `@pnpm/deps.inspection.peers-checker` (the walk and
//! the `peerDependencyRules` filter) and
//! `@pnpm/deps.inspection.peers-issues-renderer` (the terminal
//! rendering). pnpm's own install derives its issues from the
//! resolution instead, which is the one place the two stacks compute
//! the same verdict by different routes.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use node_semver::{Range, Version};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;

use pnpm_catalogs_resolver::{
    CatalogResolutionError, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::PeerDependencyRules;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageMetadata, PkgName, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_resolving_resolver_base::get_peer_version_range;
use pnpm_text_sanitize::sanitize;

#[derive(Debug, Default, Clone, Serialize)]
pub struct ParentPkg {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BadPeerIssue {
    pub parents: Vec<ParentPkg>,
    pub optional: bool,
    #[serde(rename = "wantedRange")]
    pub wanted_range: String,
    #[serde(rename = "foundVersion")]
    pub found_version: String,
    #[serde(rename = "resolvedFrom")]
    pub resolved_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerIssues {
    pub bad: BTreeMap<String, Vec<BadPeerIssue>>,
    pub missing: BTreeMap<String, Vec<MissingPeerIssue>>,
    pub conflicts: Vec<String>,
    pub intersections: BTreeMap<String, String>,
}

pub type IssuesByProjects = BTreeMap<String, PeerIssues>;

/// The issues an install or `pnpm dedupe` acts on: everything left
/// after `peerDependencyRules`, when any of it is worth acting on. A
/// missing peer every parent marks optional is not — pnpm's
/// install-time gate ignores it, and so does `pnpm peers check`.
pub struct PeerIssuesReport {
    issues: IssuesByProjects,
    /// Whether any non-optional peer is absent outright, as opposed to
    /// present at an unsatisfying version. Only the absent case is
    /// answerable by `autoInstallPeers`, so only it earns that hint.
    has_missing_peer: bool,
}

impl PeerIssuesReport {
    /// The listing `pnpm peers check` prints for the same issues. Empty
    /// when every issue is one that listing leaves out — a missing peer no
    /// parent conflicts over, say — so callers must handle an empty body.
    #[must_use]
    pub fn render(&self) -> String {
        render_peer_issues(&self.issues)
    }

    /// The `issuesByProjects` payload of pnpm's
    /// `pnpm:peer-dependency-issues` log.
    #[must_use]
    pub fn issues(&self) -> &IssuesByProjects {
        &self.issues
    }

    /// pnpm's `ERR_PNPM_PEER_DEP_ISSUES` body: the listing, then the
    /// ways out of the failure — auto-installing the peers first when
    /// any is absent, then switching the guard off.
    #[must_use]
    pub fn render_error(&self) -> String {
        let mut hints = Vec::new();
        if self.has_missing_peer {
            hints.push(
                "hint: To auto-install peer dependencies, add the following to \"pnpm-workspace.yaml\" in your project root:\n\n  autoInstallPeers: true",
            );
        }
        hints.push(
            "hint: To disable failing on peer dependency issues, add the following to pnpm-workspace.yaml in your project root:\n\n  strictPeerDependencies: false",
        );
        let hints = hints.join("\n");
        let rendered = self.render();
        let body = if rendered.is_empty() { hints } else { format!("{rendered}\n{hints}") };
        format!("[ERR_PNPM_PEER_DEP_ISSUES] Unmet peer dependencies\n\n{body}\n")
    }
}

/// The same issue set `pnpm peers check` reports, filtered by
/// `peerDependencyRules`, for the given importers.
///
/// Returns `None` when there is nothing worth acting on.
pub fn peer_issues_for_lockfile(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    importer_ids: &[String],
    rules: &PeerDependencyRules,
    catalogs: Option<&Catalogs>,
) -> Result<Option<PeerIssuesReport>, CatalogResolutionError> {
    let issues = filter_peer_issues(
        check_peer_dependencies_of_importers(lockfile, lockfile_dir, importer_ids, catalogs)?,
        rules,
    );
    let has_missing_peer = issues.values().any(|project_issues| {
        project_issues.missing.values().any(|entries| entries.iter().any(|entry| !entry.optional))
    });
    let has_issues =
        has_missing_peer || issues.values().any(|project_issues| !project_issues.bad.is_empty());
    Ok(has_issues.then_some(PeerIssuesReport { issues, has_missing_peer }))
}

/// The issues reachable from the given project directories, for the
/// commands that inspect a selection rather than the whole workspace.
pub fn check_peer_dependencies_from_lockfile(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    project_dirs: &[PathBuf],
    catalogs: Option<&Catalogs>,
) -> Result<IssuesByProjects, CatalogResolutionError> {
    let mut importer_ids: Vec<String> = project_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir))
        .filter(|importer_id| lockfile.importers.contains_key(importer_id))
        .collect();
    importer_ids.sort();
    importer_ids.dedup();
    check_peer_dependencies_of_importers(lockfile, lockfile_dir, &importer_ids, catalogs)
}

/// Walk the named importers, collecting every peer a package requires
/// but the recorded graph does not satisfy. Unfiltered — the caller
/// applies [`filter_peer_issues`].
pub fn check_peer_dependencies_of_importers(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    importer_ids: &[String],
    catalogs: Option<&Catalogs>,
) -> Result<IssuesByProjects, CatalogResolutionError> {
    let empty_packages = HashMap::new();
    let empty_snapshots = HashMap::new();
    let packages = lockfile.packages.as_ref().unwrap_or(&empty_packages);
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty_snapshots);
    let context = PeerWalkContext { lockfile, lockfile_dir, catalogs };

    let mut result: IssuesByProjects = BTreeMap::new();
    // Shared across importers so each package is evaluated once, matching
    // pnpm's lockfile walker, which threads one `walked` set through every
    // importer's step.
    let mut visited_packages = HashSet::new();

    for importer_id in importer_ids {
        let mut issues = PeerIssues {
            bad: BTreeMap::new(),
            missing: BTreeMap::new(),
            conflicts: Vec::new(),
            intersections: BTreeMap::new(),
        };

        let mut walk = InitialKeyWalk::new(&context);
        walk.collect(importer_id, &[], &mut issues)?;

        walk_snapshot(
            walk.keys,
            snapshots,
            packages,
            lockfile_dir,
            &mut visited_packages,
            &mut issues,
        );

        let merged = merge_missing_peers(&issues.missing);
        issues.conflicts = merged.conflicts;
        issues.intersections = merged.intersections;

        result.insert(importer_id.clone(), issues);
    }

    Ok(result)
}

struct CanonicalPathWithin {
    path: PathBuf,
    base: PathBuf,
}

fn canonical_path_within(path: &Path, base: &Path) -> Option<CanonicalPathWithin> {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return None;
    };
    canonical_path
        .starts_with(&canonical_base)
        .then_some(CanonicalPathWithin { path: canonical_path, base: canonical_base })
}

/// `base_dir` is the directory the `link:` target is relative to — the
/// importer's directory for importer dependencies, the lockfile directory for
/// snapshot dependencies. Targets escaping `lockfile_dir` are rejected.
fn resolve_link_version(base_dir: &Path, lockfile_dir: &Path, link_target: &str) -> Option<String> {
    let target_dir = canonical_path_within(&base_dir.join(link_target), lockfile_dir)?.path;
    let manifest = PackageManifest::from_path(target_dir.join("package.json")).ok()?;
    package_manifest_version(&manifest)
}

fn resolve_file_version(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    alias: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<String> {
    let key = spec.version.resolved_key(alias)?.without_peer();
    let metadata = lockfile.packages.as_ref()?.get(&key)?;
    if let Some(version) = &metadata.version {
        return Some(version.clone());
    }
    let LockfileResolution::Directory(directory) = &metadata.resolution else { return None };
    resolve_link_version(lockfile_dir, lockfile_dir, &directory.directory)
}

fn package_manifest_version(manifest: &PackageManifest) -> Option<String> {
    manifest.value().get("version").and_then(|version| version.as_str()).map(String::from)
}

/// A workspace package an importer reaches through `link:`, whose own
/// `peerDependencies` the importer has to satisfy.
struct LinkedPackagePeers<'a> {
    lockfile: &'a Lockfile,
    importer: &'a ProjectSnapshot,
    linked_importer: Option<&'a ProjectSnapshot>,
    importer_dir: &'a Path,
    linked_importer_dir: &'a Path,
    lockfile_dir: &'a Path,
    manifest: &'a PackageManifest,
    alias: &'a str,
    linked_version: &'a str,
    catalogs: Option<&'a Catalogs>,
    issues: &'a mut PeerIssues,
}

fn check_linked_package_peers(
    inputs: LinkedPackagePeers<'_>,
) -> Result<(), CatalogResolutionError> {
    let issues = inputs.issues;
    let Some(peer_deps) =
        inputs.manifest.value().get("peerDependencies").and_then(|deps_val| deps_val.as_object())
    else {
        return Ok(());
    };

    let current_parents = vec![ParentPkg {
        name: inputs.alias.to_string(),
        version: inputs.linked_version.to_string(),
    }];

    for (peer_name, peer_range_val) in peer_deps {
        let Some(peer_range) = peer_range_val.as_str() else { continue };
        let peer_range = resolve_peer_range(peer_name, peer_range, inputs.catalogs)?;
        check_one_linked_peer(LinkedPeerCheck {
            lockfile: inputs.lockfile,
            importer: inputs.importer,
            linked_importer: inputs.linked_importer,
            importer_dir: inputs.importer_dir,
            linked_importer_dir: inputs.linked_importer_dir,
            lockfile_dir: inputs.lockfile_dir,
            parents: &current_parents,
            optional: peer_is_optional(inputs.manifest, peer_name),
            peer_name,
            peer_range: &get_peer_version_range(&peer_range),
            issues,
        });
    }
    Ok(())
}

/// One peer dependency of a linked package, and the two importers that could
/// satisfy it.
struct LinkedPeerCheck<'a> {
    lockfile: &'a Lockfile,
    importer: &'a ProjectSnapshot,
    linked_importer: Option<&'a ProjectSnapshot>,
    importer_dir: &'a Path,
    linked_importer_dir: &'a Path,
    lockfile_dir: &'a Path,
    parents: &'a [ParentPkg],
    optional: bool,
    peer_name: &'a str,
    peer_range: &'a str,
    issues: &'a mut PeerIssues,
}

fn check_one_linked_peer(check: LinkedPeerCheck<'_>) {
    let issues = check.issues;
    let Ok(peer_pkg_name) = check.peer_name.parse::<PkgName>() else { return };

    // The linked package's own project comes second: a peer the depending
    // project provides is the one that ends up resolved.
    let resolved_ref = project_dependency(check.importer, &peer_pkg_name)
        .map(|spec| (spec, check.importer_dir))
        .or_else(|| {
            check
                .linked_importer
                .and_then(|importer| project_dependency(importer, &peer_pkg_name))
                .map(|spec| (spec, check.linked_importer_dir))
        });
    let Some((spec, dependency_dir)) = resolved_ref else {
        record_missing_peer(
            issues,
            check.peer_name,
            check.parents,
            check.optional,
            check.peer_range,
        );
        return;
    };

    let found_version = resolved_peer_version(
        check.lockfile,
        check.lockfile_dir,
        dependency_dir,
        &peer_pkg_name,
        spec,
    );
    let Some(found_version) = found_version else { return };
    record_bad_peer(
        issues,
        check.peer_name,
        check.parents,
        check.optional,
        check.peer_range,
        found_version,
    );
}

/// An unresolved peer is an issue unless the declaration marks it optional.
fn record_missing_peer(
    issues: &mut PeerIssues,
    peer_name: &str,
    parents: &[ParentPkg],
    optional: bool,
    wanted_range: &str,
) {
    if optional {
        return;
    }
    issues.missing.entry(peer_name.to_string()).or_default().push(MissingPeerIssue {
        parents: parents.to_vec(),
        optional,
        wanted_range: wanted_range.to_string(),
    });
}

/// A resolved peer outside the wanted range is an issue, optional or not.
fn record_bad_peer(
    issues: &mut PeerIssues,
    peer_name: &str,
    parents: &[ParentPkg],
    optional: bool,
    wanted_range: &str,
    found_version: String,
) {
    if satisfies(&found_version, wanted_range) {
        return;
    }
    issues.bad.entry(peer_name.to_string()).or_default().push(BadPeerIssue {
        parents: parents.to_vec(),
        optional,
        wanted_range: wanted_range.to_string(),
        found_version,
        resolved_from: Vec::new(),
    });
}

fn peer_is_optional(manifest: &PackageManifest, peer_name: &str) -> bool {
    manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(|meta_map| meta_map.get(peer_name))
        .and_then(|peer_meta| peer_meta.get("optional"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The version a project dependency spec resolves to, whether it names a
/// registry version, a linked directory or a file dependency.
fn resolved_peer_version(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    dependency_dir: &Path,
    peer_pkg_name: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<String> {
    if let Some(ver_peer) = spec.version.ver_peer() {
        return Some(ver_peer.version().to_string());
    }
    if let Some(link_target) = spec.version.as_link_target() {
        return Some(
            resolve_link_version(dependency_dir, lockfile_dir, link_target)
                .unwrap_or_else(|| format!("link:{link_target}")),
        );
    }
    spec.version.as_file_target().map(|file_target| {
        resolve_file_version(lockfile, lockfile_dir, peer_pkg_name, spec)
            .unwrap_or_else(|| format!("file:{file_target}"))
    })
}

fn project_dependency<'a>(
    importer: &'a ProjectSnapshot,
    name: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| importer.dev_dependencies.as_ref().and_then(|deps| deps.get(name)))
        .or_else(|| importer.optional_dependencies.as_ref().and_then(|deps| deps.get(name)))
}

fn resolve_peer_range(
    peer_name: &str,
    peer_range: &str,
    catalogs: Option<&Catalogs>,
) -> Result<String, CatalogResolutionError> {
    let Some(catalogs) = catalogs else { return Ok(peer_range.to_string()) };
    let wanted =
        WantedDependency { alias: peer_name.to_string(), bare_specifier: peer_range.to_string() };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok(found.resolution.specifier),
        CatalogResolutionResult::Unused => Ok(peer_range.to_string()),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => Err(misconfiguration.error),
    }
}

struct PeerWalkContext<'a> {
    lockfile: &'a Lockfile,
    lockfile_dir: &'a Path,
    catalogs: Option<&'a Catalogs>,
}

/// Walks the importers reachable through `link:` dependencies, gathering the
/// snapshot keys their dependency graphs start from.
struct InitialKeyWalk<'a> {
    context: &'a PeerWalkContext<'a>,
    keys: Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    visited_importers: HashSet<String>,
}

impl<'a> InitialKeyWalk<'a> {
    fn new(context: &'a PeerWalkContext<'a>) -> Self {
        InitialKeyWalk { context, keys: Vec::new(), visited_importers: HashSet::new() }
    }

    fn collect(
        &mut self,
        importer_id: &str,
        parents: &[ParentPkg],
        issues: &mut PeerIssues,
    ) -> Result<(), CatalogResolutionError> {
        if !self.visited_importers.insert(importer_id.to_string()) {
            return Ok(());
        }
        let Some(importer) = self.context.lockfile.importers.get(importer_id) else {
            return Ok(());
        };
        let importer_dir = self.context.lockfile_dir.join(importer_id);

        let groups =
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
        for (alias, spec) in groups.into_iter().flatten().flatten() {
            if let Some(key) = spec.version.resolved_key(alias) {
                self.keys.push((key, parents.to_owned()));
            } else if let Some(link_target) = spec.version.as_link_target() {
                let linked = LinkedDependency::resolve(
                    &importer_dir,
                    self.context.lockfile_dir,
                    link_target,
                );
                let Some(linked) = linked else { continue };
                self.follow_link(FollowLink {
                    importer,
                    importer_dir: &importer_dir,
                    alias: &alias.to_string(),
                    linked: &linked,
                    parents,
                    issues,
                })?;
            }
        }
        Ok(())
    }

    /// Check the linked package's own peer dependencies against what the two
    /// importers provide, then walk into it.
    fn follow_link(&mut self, inputs: FollowLink<'_>) -> Result<(), CatalogResolutionError> {
        let FollowLink { importer, importer_dir, alias, linked, parents, issues } = inputs;
        if let Some(manifest) = &linked.manifest {
            check_linked_package_peers(LinkedPackagePeers {
                lockfile: self.context.lockfile,
                importer,
                linked_importer: self.context.lockfile.importers.get(&linked.importer_id),
                importer_dir,
                linked_importer_dir: &linked.dir,
                lockfile_dir: self.context.lockfile_dir,
                manifest,
                alias,
                linked_version: &linked.version,
                catalogs: self.context.catalogs,
                issues,
            })?;
        }
        let mut next_parents = parents.to_owned();
        next_parents.push(ParentPkg { name: alias.to_string(), version: linked.version.clone() });
        self.collect(&linked.importer_id, &next_parents, issues)
    }
}

struct FollowLink<'a> {
    importer: &'a ProjectSnapshot,
    importer_dir: &'a Path,
    alias: &'a str,
    linked: &'a LinkedDependency,
    parents: &'a [ParentPkg],
    issues: &'a mut PeerIssues,
}

/// Where a `link:` dependency points, and what its manifest says.
struct LinkedDependency {
    dir: PathBuf,
    importer_id: String,
    manifest: Option<PackageManifest>,
    version: String,
}

impl LinkedDependency {
    /// One canonicalization and one manifest read per linked dependency: the
    /// version, the peer check, and the recursion all need the same answers,
    /// and this walk now runs on the install path.
    fn resolve(importer_dir: &Path, lockfile_dir: &Path, link_target: &str) -> Option<Self> {
        let CanonicalPathWithin { path: dir, base: canonical_lockfile_dir } =
            canonical_path_within(&importer_dir.join(link_target), lockfile_dir)?;
        let manifest = PackageManifest::from_path(dir.join("package.json")).ok();
        let version = manifest
            .as_ref()
            .and_then(package_manifest_version)
            .unwrap_or_else(|| "0.0.0".to_string());
        let importer_id = pnpm_workspace::importer_id_from_root_dir(&canonical_lockfile_dir, &dir);
        Some(LinkedDependency { dir, importer_id, manifest, version })
    }
}

fn walk_snapshot(
    initial_keys: Vec<(PkgNameVerPeer, Vec<ParentPkg>)>,
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &Path,
    visited: &mut HashSet<PkgNameVerPeer>,
    issues: &mut PeerIssues,
) {
    let mut stack = initial_keys;

    while let Some((key, parents)) = stack.pop() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let mut current_parents = parents;
        current_parents.push(ParentPkg {
            name: key.name.to_string(),
            version: get_pkg_version(&key, packages),
        });

        let snapshot = snapshots.get(&key);
        check_snapshot_peers(SnapshotPeers {
            key: &key,
            snapshot,
            packages,
            lockfile_dir,
            parents: &current_parents,
            issues,
        });

        stack.extend(child_keys(snapshot).map(|child| (child, current_parents.clone())));
    }
}

/// One walked package: its own snapshot, and the parent chain that reached
/// it.
struct SnapshotPeers<'a> {
    key: &'a PkgNameVerPeer,
    snapshot: Option<&'a SnapshotEntry>,
    packages: &'a HashMap<PkgNameVerPeer, PackageMetadata>,
    lockfile_dir: &'a Path,
    parents: &'a [ParentPkg],
    issues: &'a mut PeerIssues,
}

/// Record every peer dependency the package declares that its snapshot
/// leaves unsatisfied.
fn check_snapshot_peers(inputs: SnapshotPeers<'_>) {
    let issues = inputs.issues;
    let Some(meta) = inputs.packages.get(&inputs.key.without_peer()) else { return };
    let Some(peers) = &meta.peer_dependencies else { return };

    for (peer_name, peer_range) in peers {
        let peer_range = get_peer_version_range(peer_range);
        let optional = meta
            .peer_dependencies_meta
            .as_ref()
            .and_then(|meta_map| meta_map.get(peer_name))
            .is_some_and(|peer_meta| peer_meta.optional);

        let Ok(peer_pkg_name) = peer_name.parse::<PkgName>() else { continue };
        let dep_ref = inputs.snapshot.and_then(|entry| snapshot_dependency(entry, &peer_pkg_name));
        let Some(dep_ref) = dep_ref else {
            record_missing_peer(issues, peer_name, inputs.parents, optional, &peer_range);
            continue;
        };

        let Some(found_version) = resolved_snapshot_version(dep_ref, inputs.lockfile_dir) else {
            continue;
        };
        record_bad_peer(issues, peer_name, inputs.parents, optional, &peer_range, found_version);
    }
}

fn snapshot_dependency<'a>(
    snapshot: &'a SnapshotEntry,
    name: &PkgName,
) -> Option<&'a SnapshotDepRef> {
    snapshot
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| snapshot.optional_dependencies.as_ref().and_then(|deps| deps.get(name)))
}

/// The version a snapshot dependency reference resolves to. A reference that
/// is neither a registry version nor a link has no version to check against.
fn resolved_snapshot_version(dep_ref: &SnapshotDepRef, lockfile_dir: &Path) -> Option<String> {
    if let Some(ver_peer) = dep_ref.ver_peer() {
        return Some(ver_peer.version().to_string());
    }
    let link_target = dep_ref.as_link_target()?;
    Some(
        resolve_link_version(lockfile_dir, lockfile_dir, link_target)
            .unwrap_or_else(|| format!("link:{link_target}")),
    )
}

fn child_keys(snapshot: Option<&SnapshotEntry>) -> impl Iterator<Item = PkgNameVerPeer> + '_ {
    snapshot
        .into_iter()
        .flat_map(|snapshot| {
            snapshot
                .dependencies
                .iter()
                .flat_map(|deps| deps.iter())
                .chain(snapshot.optional_dependencies.iter().flat_map(|deps| deps.iter()))
        })
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
}

fn get_pkg_version(
    key: &PkgNameVerPeer,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
) -> String {
    let base_key = key.without_peer();
    packages
        .get(&base_key)
        .and_then(|meta| meta.version.clone())
        .unwrap_or_else(|| key.suffix.version().to_string())
}

fn satisfies(version: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    let Ok(parsed_version) = Version::parse(version) else {
        return version == range;
    };
    let Ok(parsed_range) = Range::parse(range) else {
        return version == range;
    };
    if parsed_version.satisfies(&parsed_range) {
        return true;
    }
    if !parsed_version.is_prerelease() {
        return false;
    }
    // pnpm asks semver for `includePrerelease`, which drops the rule
    // that a prerelease only satisfies a comparator carrying a
    // prerelease of its own `major.minor.patch` — `node-semver`'s Rust
    // port applies that rule unconditionally. What is left is the plain
    // bound check, and ordering still holds: `18.3.0-canary` satisfies
    // `^18.0.0`, while `2.0.0-beta.1` stays below `>=2.0.0`.
    parse_range_to_intervals(&preprocess_hyphen_ranges(range)).is_some_and(|intervals| {
        intervals.iter().any(|interval| interval.contains(&parsed_version))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound<Value> {
    Inclusive(Value),
    Exclusive(Value),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    lower: Bound<Version>,
    upper: Bound<Version>,
}

impl Interval {
    fn contains(&self, version: &Version) -> bool {
        let above_lower = match &self.lower {
            Bound::Inclusive(lower) => lower <= version,
            Bound::Exclusive(lower) => lower < version,
            Bound::Unbounded => true,
        };
        let below_upper = match &self.upper {
            Bound::Inclusive(upper) => version <= upper,
            Bound::Exclusive(upper) => version < upper,
            Bound::Unbounded => true,
        };
        above_lower && below_upper
    }
}

/// The bound as a user reads it, without the trailing `-0`.
///
/// For a bound [`derived_upper`] built, the suffix is an implementation
/// detail of prerelease matching. For one the user wrote out it is not
/// — but pnpm drops it too (`semver-range-intersect` renders
/// `intersect("<2.0.0-0", ">=1.0.0")` as `>=1.0.0 <2.0.0`), so telling
/// the two apart here would diverge rather than converge. Only the
/// rendering loses it: matching compares the parsed version, where the
/// suffix still excludes every prerelease of that release.
fn without_derived_suffix(version: &Version) -> String {
    if version.pre_release == [node_semver::Identifier::Numeric(0)] {
        format!("{}.{}.{}", version.major, version.minor, version.patch)
    } else {
        version.to_string()
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let upper = match &self.upper {
            Bound::Inclusive(version) | Bound::Exclusive(version) => {
                without_derived_suffix(version)
            }
            Bound::Unbounded => String::new(),
        };
        match (&self.lower, &self.upper) {
            (Bound::Unbounded, Bound::Unbounded) => write!(formatter, "*"),
            (Bound::Inclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">={version_lower}")
            }
            (Bound::Exclusive(version_lower), Bound::Unbounded) => {
                write!(formatter, ">{version_lower}")
            }
            (Bound::Unbounded, Bound::Inclusive(_)) => write!(formatter, "<={upper}"),
            (Bound::Unbounded, Bound::Exclusive(_)) => write!(formatter, "<{upper}"),
            (Bound::Inclusive(version_lower), Bound::Inclusive(version_upper)) => {
                if version_lower == version_upper {
                    write!(formatter, "{version_lower}")
                } else {
                    write!(formatter, ">={version_lower} <={upper}")
                }
            }
            (Bound::Inclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">={version_lower} <{upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Inclusive(_)) => {
                write!(formatter, ">{version_lower} <={upper}")
            }
            (Bound::Exclusive(version_lower), Bound::Exclusive(_)) => {
                write!(formatter, ">{version_lower} <{upper}")
            }
        }
    }
}

/// The tighter of two lower bounds.
fn max_lower(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (lower_rank(left_bound), lower_rank(right_bound)) {
        (None, _) => right_bound.clone(),
        (_, None) => left_bound.clone(),
        (left, right) => {
            if left >= right {
                left_bound.clone()
            } else {
                right_bound.clone()
            }
        }
    }
}

/// The tighter of two upper bounds.
fn min_upper(left_bound: &Bound<Version>, right_bound: &Bound<Version>) -> Bound<Version> {
    match (upper_rank(left_bound), upper_rank(right_bound)) {
        (None, _) => right_bound.clone(),
        (_, None) => left_bound.clone(),
        (left, right) => {
            if left <= right {
                left_bound.clone()
            } else {
                right_bound.clone()
            }
        }
    }
}

/// Orders lower bounds by the versions they admit, `None` for unbounded. At
/// the same version the exclusive bound is the higher one: it excludes that
/// version, the inclusive one admits it.
fn lower_rank(bound: &Bound<Version>) -> Option<(&Version, u8)> {
    match bound {
        Bound::Unbounded => None,
        Bound::Inclusive(version) => Some((version, 0)),
        Bound::Exclusive(version) => Some((version, 1)),
    }
}

/// Orders upper bounds by the versions they admit, `None` for unbounded. At
/// the same version the exclusive bound is the lower one.
fn upper_rank(bound: &Bound<Version>) -> Option<(&Version, u8)> {
    match bound {
        Bound::Unbounded => None,
        Bound::Exclusive(version) => Some((version, 0)),
        Bound::Inclusive(version) => Some((version, 1)),
    }
}

fn is_valid_interval(lower: &Bound<Version>, upper: &Bound<Version>) -> bool {
    match (lower, upper) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version <= right_version
        }
        (Bound::Inclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Inclusive(right_version)) => {
            left_version < right_version
        }
        (Bound::Exclusive(left_version), Bound::Exclusive(right_version)) => {
            left_version < right_version
        }
    }
}

/// Pad a partial version to `major.minor.patch`, reading the `x`, `X` and
/// `*` placeholders as zero. Anything else non-numeric is left untouched for
/// the caller's parser to reject; content past the patch component (a
/// prerelease or build tag) is carried through.
fn normalize_version_str(version_raw: &str) -> String {
    let version_raw = version_raw.trim();
    let version_parts: Vec<&str> = version_raw.split('.').collect();
    let numeric: Vec<String> =
        version_parts.iter().take(3).map(|part| part.replace(['x', 'X', '*'], "0")).collect();
    if !numeric.iter().all(|part| part.chars().all(|character| character.is_ascii_digit())) {
        return version_raw.to_string();
    }
    let mut padded = numeric;
    padded.resize(3, "0".to_string());
    let rest = version_parts.get(3..).unwrap_or_default();
    padded.iter().map(String::as_str).chain(rest.iter().copied()).collect::<Vec<_>>().join(".")
}

/// How many of `major.minor.patch` a range's version actually pins.
/// `1` and `1.x` pin one, `1.2` pins two, `1.2.3` pins three. npm's
/// comparators widen to the next unpinned level — `~1` reaches `2.0.0`,
/// not `1.1.0` — so the count has to survive the padding
/// [`normalize_version_str`] applies.
fn version_specificity(version_raw: &str) -> usize {
    let mut specificity = 0;
    for part in version_raw.trim().split('.') {
        let head = part.split(['-', '+']).next().unwrap_or(part);
        if head.is_empty() || head.chars().all(|character| matches!(character, 'x' | 'X' | '*')) {
            break;
        }
        specificity += 1;
        if specificity == 3 {
            break;
        }
    }
    specificity
}

fn at(major: u64, minor: u64, patch: u64) -> Version {
    Version { major, minor, patch, build: Vec::new(), pre_release: Vec::new() }
}

/// An upper bound npm derived rather than the user writing it out:
/// `^1.2.3` desugars to `<2.0.0-0`, not `<2.0.0`, so that no prerelease
/// of 2.0.0 slips in. A bound the user spelled in full (`<2.0.0`) keeps
/// its plain form and does admit `2.0.0-rc.1`. The suffix is dropped
/// again when the interval is rendered — see [`Interval`]'s `Display`.
fn derived_upper(version: Version) -> Version {
    Version { pre_release: vec![node_semver::Identifier::Numeric(0)], ..version }
}

/// The exclusive upper bound of the level `specificity` leaves
/// unpinned: the next major for a bare major, the next minor for
/// `major.minor`.
fn next_unpinned(version: &Version, specificity: usize) -> Version {
    let next = if specificity >= 2 {
        at(version.major, version.minor + 1, 0)
    } else {
        at(version.major + 1, 0, 0)
    };
    derived_upper(next)
}

fn parse_comparator(comparator: &str) -> Option<Interval> {
    let comparator = comparator.trim();
    if comparator == "*" || comparator.is_empty() {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }

    let (operator, version_str) = split_operator(comparator);
    let specificity = version_specificity(version_str);
    // Nothing is pinned (`x`, `~x`, `^*`): every comparator over it
    // admits every version.
    if specificity == 0 {
        return Some(Interval { lower: Bound::Unbounded, upper: Bound::Unbounded });
    }
    let version = Version::parse(normalize_version_str(version_str)).ok()?;

    match operator {
        "=" if specificity == 3 => Some(Interval {
            lower: Bound::Inclusive(version.clone()),
            upper: Bound::Inclusive(version),
        }),
        // A partial bare version is npm's implicit range: `1.2` is
        // every 1.2.x, not the single version 1.2.0.
        "=" => Some(Interval {
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        "=>" => Some(Interval { lower: Bound::Inclusive(version), upper: Bound::Unbounded }),
        ">" if specificity == 3 => {
            Some(Interval { lower: Bound::Exclusive(version), upper: Bound::Unbounded })
        }
        // `>1.2` excludes all of 1.2.x, so it starts at 1.3.0.
        ">" => Some(Interval {
            lower: Bound::Inclusive(next_unpinned(&version, specificity)),
            upper: Bound::Unbounded,
        }),
        "<=" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Inclusive(version) })
        }
        // `<=1.2` admits all of 1.2.x.
        "<=" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
        }),
        "<" if specificity == 3 => {
            Some(Interval { lower: Bound::Unbounded, upper: Bound::Exclusive(version) })
        }
        // `<1.2` excludes all of 1.2.x, prereleases included.
        "<" => Some(Interval {
            lower: Bound::Unbounded,
            upper: Bound::Exclusive(derived_upper(version)),
        }),
        "^" => Some(Interval {
            upper: Bound::Exclusive(caret_upper(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        "~" => Some(Interval {
            upper: Bound::Exclusive(next_unpinned(&version, specificity)),
            lower: Bound::Inclusive(version),
        }),
        _ => None,
    }
}

/// The comparator's operator and the version text after it. A version with
/// no operator is npm's implicit `=`.
fn split_operator(comparator: &str) -> (&str, &str) {
    for (prefix, operator) in
        [(">=", "=>"), (">", ">"), ("<=", "<="), ("<", "<"), ("^", "^"), ("~", "~")]
    {
        if let Some(rest) = comparator.strip_prefix(prefix) {
            return (operator, rest);
        }
    }
    ("=", comparator)
}

/// The exclusive upper bound of `^`: the next level up from the leftmost
/// non-zero component, or from the level the range leaves unpinned.
fn caret_upper(version: &Version, specificity: usize) -> Version {
    let upper = if specificity == 1 || version.major > 0 {
        at(version.major + 1, 0, 0)
    } else if specificity == 2 || version.minor > 0 {
        at(0, version.minor + 1, 0)
    } else {
        at(0, 0, version.patch + 1)
    };
    derived_upper(upper)
}

fn preprocess_hyphen_ranges(range: &str) -> String {
    let mut parts = Vec::new();
    for part in range.split("||") {
        let part = part.trim();
        if let Some((start, end)) = part.split_once(" - ") {
            parts.push(format!(">={} <={}", start.trim(), end.trim()));
        } else {
            parts.push(part.to_string());
        }
    }
    parts.join(" || ")
}

fn parse_range_to_intervals(range: &str) -> Option<Vec<Interval>> {
    let mut intervals = Vec::new();
    for part in range.split("||").map(str::trim).filter(|part| !part.is_empty()) {
        let part_interval = parse_comparator_set(part)?;
        if is_valid_interval(&part_interval.lower, &part_interval.upper) {
            intervals.push(part_interval);
        }
    }
    if intervals.is_empty() { None } else { Some(intervals) }
}

/// Intersect the space-separated comparators of one `||` alternative. An
/// empty intersection is reported as the empty interval, which the caller
/// then drops.
fn parse_comparator_set(part: &str) -> Option<Interval> {
    let mut interval = Interval { lower: Bound::Unbounded, upper: Bound::Unbounded };
    for comparator in part.split_whitespace() {
        let comparator_interval = parse_comparator(comparator)?;
        let lower = max_lower(&interval.lower, &comparator_interval.lower);
        let upper = min_upper(&interval.upper, &comparator_interval.upper);
        if !is_valid_interval(&lower, &upper) {
            let zero = Version::parse("0.0.0").expect("0.0.0 is a valid version");
            return Some(Interval {
                lower: Bound::Inclusive(zero.clone()),
                upper: Bound::Exclusive(zero),
            });
        }
        interval = Interval { lower, upper };
    }
    Some(interval)
}

fn intersect_intervals(left_intervals: &[Interval], right_intervals: &[Interval]) -> Vec<Interval> {
    let mut result = Vec::new();
    for left_interval in left_intervals {
        for right_interval in right_intervals {
            let lower = max_lower(&left_interval.lower, &right_interval.lower);
            let upper = min_upper(&left_interval.upper, &right_interval.upper);
            if is_valid_interval(&lower, &upper) {
                result.push(Interval { lower, upper });
            }
        }
    }
    result
}

fn intersect_multiple_ranges(version_ranges: &[String]) -> Option<String> {
    if version_ranges.is_empty() {
        return Some("*".to_string());
    }
    let mut current_intervals =
        parse_range_to_intervals(&preprocess_hyphen_ranges(&version_ranges[0]))?;
    for range in &version_ranges[1..] {
        let next_intervals = parse_range_to_intervals(&preprocess_hyphen_ranges(range))?;
        current_intervals = intersect_intervals(&current_intervals, &next_intervals);
        if current_intervals.is_empty() {
            return None;
        }
    }
    Some(
        current_intervals
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" || "),
    )
}

fn merge_missing_peers(missing: &BTreeMap<String, Vec<MissingPeerIssue>>) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut intersections = BTreeMap::new();

    for (peer_name, issues) in missing {
        if issues.iter().all(|issue| issue.optional) {
            continue;
        }
        if issues.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let ranges: Vec<&str> = issues.iter().map(|issue| issue.wanted_range.as_str()).collect();
        let unique: HashSet<&&str> = ranges.iter().collect();
        if unique.len() == 1 {
            intersections.insert(peer_name.clone(), issues[0].wanted_range.clone());
            continue;
        }
        let range_owned: Vec<String> =
            issues.iter().map(|issue| issue.wanted_range.clone()).collect();
        if let Some(intersection_str) = intersect_multiple_ranges(&range_owned) {
            intersections.insert(peer_name.clone(), intersection_str);
        } else {
            conflicts.push(peer_name.clone());
        }
    }

    MergeResult { conflicts, intersections }
}

struct MergeResult {
    conflicts: Vec<String>,
    intersections: BTreeMap<String, String>,
}

#[must_use]
pub fn filter_peer_issues(
    mut issues: IssuesByProjects,
    rules: &PeerDependencyRules,
) -> IssuesByProjects {
    if rules.ignore_missing.is_none()
        && rules.allow_any.is_none()
        && rules.allowed_versions.is_none()
    {
        return issues;
    }

    let (allow_all_matcher, allow_by_parent) =
        parse_allowed_versions(&rules.allowed_versions.clone().unwrap_or_default());
    let ignore_missing_matcher =
        pnpm_config::matcher::create_matcher(&rules.ignore_missing.clone().unwrap_or_default());
    let allow_any_matcher =
        pnpm_config::matcher::create_matcher(&rules.allow_any.clone().unwrap_or_default());

    for project_issues in issues.values_mut() {
        project_issues.missing = project_issues
            .missing
            .iter()
            .filter(|(peer_name, peer_issues)| {
                !ignore_missing_matcher.matches(peer_name)
                    && !peer_issues.iter().all(|issue| issue.optional)
            })
            .map(|(peer_name, peer_issues)| (peer_name.clone(), peer_issues.clone()))
            .collect();

        project_issues.bad = project_issues
            .bad
            .iter()
            .filter(|(peer_name, _)| !allow_any_matcher.matches(peer_name))
            .filter_map(|(peer_name, peer_issues)| {
                let remaining: Vec<BadPeerIssue> = peer_issues
                    .iter()
                    .filter(|issue| {
                        !is_version_allowed(issue, peer_name, &allow_all_matcher, &allow_by_parent)
                    })
                    .cloned()
                    .collect();
                (!remaining.is_empty()).then(|| (peer_name.clone(), remaining))
            })
            .collect();

        let merged = merge_missing_peers(&project_issues.missing);
        project_issues.conflicts = merged.conflicts;
        project_issues.intersections = merged.intersections;
    }

    issues
}

/// Whether an `allowedVersions` rule waives this mismatch, either
/// unconditionally for the peer or only under the parent that declares it.
fn is_version_allowed(
    issue: &BadPeerIssue,
    peer_name: &str,
    allow_all: &AllowAllMatcher,
    allow_by_parent: &AllowByParentMatcher,
) -> bool {
    if let Some(ranges) = allow_all.get(peer_name)
        && ranges.iter().any(|range| satisfies(&issue.found_version, range))
    {
        return true;
    }
    let Some(declaring_parent) = issue.parents.last() else {
        return false;
    };
    let Some(rules) = allow_by_parent.get(&declaring_parent.name) else {
        return false;
    };
    rules
        .iter()
        .filter(|rule| parent_range_matches(rule, &declaring_parent.version))
        .filter_map(|rule| rule.peer_rules.get(peer_name))
        .any(|ranges| ranges.iter().any(|range| satisfies(&issue.found_version, range)))
}

/// A rule with no parent range applies to every version of the parent.
fn parent_range_matches(rule: &ParentRule, parent_version: &str) -> bool {
    rule.parent_range.as_ref().is_none_or(|range| satisfies(parent_version, range))
}

type AllowAllMatcher = HashMap<String, Vec<String>>;
type AllowByParentMatcher = HashMap<String, Vec<ParentRule>>;

struct ParentRule {
    parent_range: Option<String>,
    peer_rules: HashMap<String, Vec<String>>,
}

fn parse_allowed_versions(
    allowed: &BTreeMap<String, String>,
) -> (AllowAllMatcher, AllowByParentMatcher) {
    let mut match_all: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_parent: AllowByParentMatcher = HashMap::new();

    for (selector, spec) in allowed {
        if let Some((parent, target)) = selector.split_once('>') {
            add_parent_rule(&mut by_parent, parent, target, spec);
        } else {
            let parsed = parse_wanted_dependency(selector);
            let target_name = parsed.alias.unwrap_or_else(|| selector.clone());
            match_all.entry(target_name).or_default().extend(split_ranges(spec));
        }
    }

    (match_all, by_parent)
}

fn add_parent_rule(by_parent: &mut AllowByParentMatcher, parent: &str, target: &str, spec: &str) {
    let parsed_parent = parse_wanted_dependency(parent.trim());
    let parent_name = parsed_parent.alias.unwrap_or_else(|| parent.trim().to_string());
    let parent_range = parsed_parent.bare_specifier;

    let parsed_peer = parse_wanted_dependency(target.trim());
    let peer_name = parsed_peer.alias.unwrap_or_else(|| target.trim().to_string());

    let parent_entry = by_parent.entry(parent_name).or_default();
    if let Some(rule) =
        parent_entry.iter_mut().find(|rule_entry| rule_entry.parent_range == parent_range)
    {
        rule.peer_rules.entry(peer_name).or_default().extend(split_ranges(spec));
    } else {
        let mut peer_rules = HashMap::new();
        peer_rules.insert(peer_name, split_ranges(spec));
        parent_entry.push(ParentRule { parent_range, peer_rules });
    }
}

fn split_ranges(spec: &str) -> Vec<String> {
    spec.split("||").map(|seg| seg.trim().to_string()).collect()
}

#[must_use]
pub fn render_peer_issues(issues_by_projects: &IssuesByProjects) -> String {
    let mut sections: Vec<String> = Vec::new();
    for project_issues in issues_by_projects.values() {
        push_bad_sections(project_issues, &mut sections);
        push_missing_sections(project_issues, &mut sections);
    }
    sections.join("\n\n")
}

fn push_bad_sections(project_issues: &PeerIssues, sections: &mut Vec<String>) {
    for (peer_name, issues) in &project_issues.bad {
        let header = format!("{} {}", yellow_bright("✕ unmet peer"), bold(peer_name));
        for (found_version, group) in &group_by_found_version(issues) {
            let installed = format!("  {} {}", cyan("Installed:"), dim(found_version));
            sections.push(format!("{}\n{}\n{}", header, installed, format_required_by(group)));
        }
    }
}

/// A missing peer is only worth reporting once the merge pass has decided
/// what the requirements add up to: an intersection to install, or a
/// conflict that cannot be satisfied at all.
fn push_missing_sections(project_issues: &PeerIssues, sections: &mut Vec<String>) {
    for (peer_name, issues) in &project_issues.missing {
        let is_conflict = project_issues.conflicts.contains(peer_name);
        if !project_issues.intersections.contains_key(peer_name) && !is_conflict {
            continue;
        }
        let label = if is_conflict { "✕ conflicting peer" } else { "✕ missing peer" };
        let header = format!("{} {}", red(label), bold(peer_name));
        sections.push(format!("{}\n{}", header, format_required_by(issues)));
    }
}

fn format_required_by(issues: &[impl RequiredByIssue]) -> String {
    let mut by_range: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issue in issues {
        let declaring = issue.parents().last().cloned().unwrap_or_default();
        let pkg = if declaring.name.is_empty() {
            "<unknown>".to_string()
        } else {
            format!("{}@{}", declaring.name, declaring.version)
        };
        let pkgs = by_range.entry(issue.wanted_range().to_string()).or_default();
        if !pkgs.contains(&pkg) {
            pkgs.push(pkg);
        }
    }

    let mut lines: Vec<String> = vec![format!("  {}", cyan("Wanted:"))];
    for (range, pkgs) in &by_range {
        lines.push(format!("    {}{}", cyan_bright(&format_range(range)), cyan(":")));
        for pkg in pkgs {
            lines.push(format!("      {}", dim(pkg)));
        }
    }
    lines.join("\n")
}

trait RequiredByIssue {
    fn parents(&self) -> &[ParentPkg];
    fn wanted_range(&self) -> &str;
}

impl RequiredByIssue for MissingPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

impl RequiredByIssue for BadPeerIssue {
    fn parents(&self) -> &[ParentPkg] {
        &self.parents
    }
    fn wanted_range(&self) -> &str {
        &self.wanted_range
    }
}

fn group_by_found_version(issues: &[BadPeerIssue]) -> BTreeMap<String, Vec<BadPeerIssue>> {
    let mut groups: BTreeMap<String, Vec<BadPeerIssue>> = BTreeMap::new();
    for issue in issues {
        groups.entry(issue.found_version.clone()).or_default().push(issue.clone());
    }
    groups
}

fn format_range(range: &str) -> String {
    if range.contains(' ') || range == "*" { format!(r#""{range}""#) } else { range.to_string() }
}

fn bold(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dim(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn yellow_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_yellow()).to_string()
}

fn red(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn cyan(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.cyan()).to_string()
}

fn cyan_bright(text: &str) -> String {
    let cleaned = sanitize(text);
    cleaned.as_ref().if_supports_color(Stream::Stdout, |t| t.bright_cyan()).to_string()
}

#[cfg(test)]
mod tests;
