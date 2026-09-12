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

pub use filter::filter_peer_issues;
pub use render::{BadPeerIssue, MissingPeerIssue, render_peer_issues};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use node_semver::{Range, Version};
use owo_colors::Stream;
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

#[cfg(test)]
mod tests;

mod ranges;
use ranges::{intersect_multiple_ranges, parse_range_to_intervals, preprocess_hyphen_ranges};

mod render;

mod filter;

use filter::merge_missing_peers;

mod linked;
use linked::{
    CanonicalPathWithin, LinkedPackagePeers, canonical_path_within, check_linked_package_peers,
    package_manifest_version, record_bad_peer, record_missing_peer, resolve_link_version,
};
