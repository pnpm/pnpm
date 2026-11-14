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

pub use undeclared::{UndeclaredPeer, UndeclaredPeerInspector, WorkspaceRootScope};

pub use filter::filter_peer_issues;
pub use render::{BadPeerIssue, MissingPeerIssue, render_peer_issues};

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use node_semver::Version;
use owo_colors::Stream;
use serde::Serialize;

use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionError, CatalogResolutionResult, WantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::PeerDependencyRules;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencySpec,
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

/// Workspace project manifests the caller already holds, keyed by importer
/// ID. A `link:` target that resolves to one of these importers is not read
/// from disk.
pub type ProjectManifests<'a> = HashMap<String, &'a PackageManifest>;

/// Loads the dedicated lockfile of a linked sibling project `(dir, name)`
/// when its importer snapshot does not live in the consuming project's
/// lockfile.
pub type LinkedLockfileLoader<'a> = dyn FnMut(&Path, Option<&str>) -> Option<Arc<Lockfile>> + 'a;

/// Inputs for [`check_peer_dependencies_from_lockfile`].
#[derive(Clone, Copy)]
pub struct LockfilePeerCheck<'a> {
    pub lockfile: &'a Lockfile,
    pub lockfile_dir: &'a Path,
    pub workspace_dir: Option<&'a Path>,
    pub project_dirs: &'a [PathBuf],
    pub catalogs: Option<&'a Catalogs>,
    pub resolve_peers_from_workspace_root: bool,
}

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
    resolve_peers_from_workspace_root: bool,
    project_manifests: &ProjectManifests<'_>,
) -> Result<Option<PeerIssuesReport>, CatalogResolutionError> {
    let issues = filter_peer_issues(
        check_peer_dependencies_of_importers(
            lockfile,
            lockfile_dir,
            importer_ids,
            catalogs,
            resolve_peers_from_workspace_root,
            project_manifests,
        )?,
        rules,
    );
    let has_missing_peer = issues
        .values()
        .any(|project_issues| {
            project_issues.missing
                .values()
                .any(|entries| entries.iter().any(|entry| !entry.optional))
        });
    let has_issues = has_missing_peer
        || issues
            .values()
            .any(|project_issues| !project_issues.bad.is_empty());
    Ok(has_issues.then_some(PeerIssuesReport { issues, has_missing_peer }))
}

/// The issues reachable from the given project directories, for the
/// commands that inspect a selection rather than the whole workspace.
pub fn check_peer_dependencies_from_lockfile(
    check: LockfilePeerCheck<'_>,
    linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>>,
) -> Result<IssuesByProjects, CatalogResolutionError> {
    let mut importer_ids: Vec<String> = check.project_dirs
        .iter()
        .map(|project_dir| {
            pnpm_workspace::importer_id_from_root_dir(check.lockfile_dir, project_dir)
        })
        .filter(|importer_id| check.lockfile.importers.contains_key(importer_id))
        .collect();
    importer_ids.sort();
    importer_ids.dedup();
    let roots = LinkRoots::new(check.lockfile_dir, check.workspace_dir);
    let manifests = ProjectManifests::new();
    let context = PeerWalkContext {
        lockfile: check.lockfile,
        roots: &roots,
        catalogs: check.catalogs,
        resolve_peers_from_workspace_root: check.resolve_peers_from_workspace_root,
        project_manifests: &manifests,
    };
    check_peer_dependencies_with_context(&context, &importer_ids, linked_lockfiles)
}

/// Walk the named importers, collecting every peer a package requires
/// but the recorded graph does not satisfy. Unfiltered — the caller
/// applies [`filter_peer_issues`].
pub fn check_peer_dependencies_of_importers(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    importer_ids: &[String],
    catalogs: Option<&Catalogs>,
    resolve_peers_from_workspace_root: bool,
    project_manifests: &ProjectManifests<'_>,
) -> Result<IssuesByProjects, CatalogResolutionError> {
    let roots = LinkRoots::same(lockfile_dir);
    let context = PeerWalkContext {
        lockfile,
        roots: &roots,
        catalogs,
        resolve_peers_from_workspace_root,
        project_manifests,
    };
    check_peer_dependencies_with_context(&context, importer_ids, None)
}

fn check_peer_dependencies_with_context(
    context: &PeerWalkContext<'_>,
    importer_ids: &[String],
    linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>>,
) -> Result<IssuesByProjects, CatalogResolutionError> {
    let empty_packages = HashMap::new();
    let empty_snapshots = HashMap::new();
    let packages = context.lockfile.packages.as_ref().unwrap_or(&empty_packages);
    let snapshots = context.lockfile.snapshots.as_ref().unwrap_or(&empty_snapshots);

    let mut result: IssuesByProjects = BTreeMap::new();
    // Shared across importers so each package is evaluated once, matching
    // pnpm's lockfile walker, which threads one `walked` set through every
    // importer's step.
    let mut visited_packages = HashSet::new();
    let mut linked_packages = LinkedPackages::new(
        context.roots,
        context.project_manifests,
        Some(context.lockfile),
        linked_lockfiles,
    );

    for importer_id in importer_ids {
        let mut issues = PeerIssues {
            bad: BTreeMap::new(),
            missing: BTreeMap::new(),
            conflicts: Vec::new(),
            intersections: BTreeMap::new(),
        };

        let initial_keys =
            collect_initial_keys(context, &mut linked_packages, importer_id, &mut issues)?;

        walk_snapshot(
            initial_keys,
            snapshots,
            packages,
            context.roots,
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
    roots: &'a LinkRoots<'a>,
    catalogs: Option<&'a Catalogs>,
    resolve_peers_from_workspace_root: bool,
    project_manifests: &'a ProjectManifests<'a>,
}

/// The snapshot keys an importer's dependency graph starts from, recording
/// along the way every peer the workspace packages it links directly leave
/// unmet. Stops at each `link:` edge: a linked workspace package's own linked
/// dependencies belong to its own report, not to its consumer's.
fn collect_initial_keys(
    context: &PeerWalkContext<'_>,
    linked_packages: &mut LinkedPackages<'_, '_, '_, '_>,
    importer_id: &str,
    issues: &mut PeerIssues,
) -> Result<Vec<PkgNameVerPeer>, CatalogResolutionError> {
    let mut keys = Vec::new();
    let Some(importer) = context.lockfile.importers.get(importer_id) else {
        return Ok(keys);
    };
    let importer_dir = context.roots.lockfile.join(importer_id);

    let groups =
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
    for (alias, spec) in groups.into_iter().flatten().flatten() {
        if let Some(key) = spec.version.resolved_key(alias) {
            keys.push(key);
        } else if let Some(link_target) = spec.version.as_link_target() {
            // pnpm's own lockfile walker stops at `link:` too. Following the
            // edge re-traverses every shared workspace package once per
            // importer that reaches it, which is quadratic in a workspace
            // whose projects depend on each other (pnpm/pnpm#14906).
            check_link(CheckLink {
                context,
                linked_packages,
                importer,
                importer_dir: &importer_dir,
                alias: &alias.to_string(),
                link_target,
                issues,
            })?;
        }
    }
    Ok(keys)
}

/// Check one linked workspace package's own peer dependencies against what
/// the consuming importer and the linked importer provide.
fn check_link(inputs: CheckLink<'_, '_, '_, '_, '_>) -> Result<(), CatalogResolutionError> {
    let CheckLink {
        context,
        linked_packages,
        importer,
        importer_dir,
        alias,
        link_target,
        issues,
    } = inputs;
    let Some(linked) = linked_packages.resolve(importer_dir, link_target) else {
        return Ok(());
    };
    let Some(manifest) = linked.manifest.as_deref() else { return Ok(()) };
    let linked_importer = linked.provider_in(context.lockfile, context.roots.lockfile);
    check_linked_package_peers(LinkedPackagePeers {
        manifest,
        alias,
        linked_version: &linked.version,
        catalogs: context.catalogs,
        issues,
        providers: crate::linked::PeerProviders {
            importer: crate::linked::ImporterProvider {
                lockfile: context.lockfile,
                lockfile_dir: context.roots.lockfile,
                snapshot: importer,
                dir: importer_dir,
            },
            linked_importer,
            roots: context.roots,
            resolve_peers_from_workspace_root: context.resolve_peers_from_workspace_root,
            project_manifests: context.project_manifests,
        },
    })
}

struct CheckLink<'a, 'r, 'm, 'b, 'l> {
    context: &'a PeerWalkContext<'a>,
    linked_packages: &'a mut LinkedPackages<'r, 'm, 'b, 'l>,
    importer: &'a ProjectSnapshot,
    importer_dir: &'a Path,
    alias: &'a str,
    link_target: &'a str,
    issues: &'a mut PeerIssues,
}

/// Where a `link:` dependency points, and what its manifest and dedicated
/// lockfile say.
struct LinkedDependency<'m> {
    dir: PathBuf,
    importer_id: String,
    manifest: Option<Cow<'m, PackageManifest>>,
    lockfile: Option<Arc<Lockfile>>,
    version: String,
}

impl LinkedDependency<'_> {
    fn provider_in<'a>(
        &'a self,
        consuming_lockfile: &'a Lockfile,
        consuming_lockfile_dir: &'a Path,
    ) -> Option<crate::linked::ImporterProvider<'a>> {
        if let Some(snapshot) = consuming_lockfile.importers.get(&self.importer_id) {
            return Some(crate::linked::ImporterProvider {
                lockfile: consuming_lockfile,
                lockfile_dir: consuming_lockfile_dir,
                snapshot,
                dir: &self.dir,
            });
        }
        let lockfile = self.lockfile.as_deref()?;
        let snapshot = lockfile.importers.get(".")?;
        Some(crate::linked::ImporterProvider {
            lockfile,
            lockfile_dir: &self.dir,
            snapshot,
            dir: &self.dir,
        })
    }
}

mod snapshot;
mod undeclared;
use snapshot::{extract_peer_version, satisfies, walk_snapshot};

#[cfg(test)]
mod tests;

mod ranges;
use ranges::intersect_multiple_ranges;

mod render;

mod filter;

use filter::merge_missing_peers;

mod linked;
use linked::{LinkRoots, LinkedPackagePeers, LinkedPackages, check_linked_package_peers};
