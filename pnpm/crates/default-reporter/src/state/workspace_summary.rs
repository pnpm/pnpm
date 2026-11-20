//! The packages-diff summary of an install that covers a workspace of
//! several projects. Each line is one change, merged across the projects
//! that made it and attributed to them unless only the current project did.

use super::{
    BTreeMap,
    DepKind,
    DiffBuckets,
    HashMap,
    HashSet,
    PackageDiff,
    ReporterState,
    diff_key,
    format_prefix_no_trim,
    is_strictly_newer,
    manifest_dep_versions,
    normalized_prefix,
    remove_optional_from_prod,
};
use std::fmt::Write as _;

/// How many projects a change names before it counts the rest.
const MAX_SHOWN_PROJECTS: usize = 3;

/// What a `pnpm:summary` tells about the workspace it closes. Prefixes
/// are normalized against the reporter's cwd.
#[derive(Debug)]
pub(super) struct WorkspaceSummary {
    root: String,
    current_project: String,
    new_projects: HashSet<String>,
}

impl WorkspaceSummary {
    pub(super) fn new(cwd: &str, root: &str, new_projects: &[String]) -> Self {
        WorkspaceSummary {
            root: normalized_prefix(cwd, root),
            current_project: normalized_prefix(cwd, cwd),
            new_projects: new_projects
                .iter()
                .map(|prefix| normalized_prefix(cwd, prefix))
                .collect(),
        }
    }
}

/// One rendered line: changes equal in sign, name, version and link
/// target are the same line.
type ChangeKey<'a> = (&'a str, bool, Option<&'a str>, Option<&'a str>);

/// A command that edits manifests reports each project's manifest before
/// the install and after it. The manifest after it arrives once the
/// summary may have rendered, so a dependency the manifest before did not
/// declare counts as requested too.
struct RequestedDependencies {
    declared_before: Option<HashSet<String>>,
    changed: HashSet<String>,
}

impl RequestedDependencies {
    fn contains(&self, name: &str) -> bool {
        self.changed.contains(name)
            || self.declared_before
                .as_ref()
                .is_some_and(|declared| !declared.contains(name))
    }
}

struct WorkspaceChange<'a> {
    diff: &'a PackageDiff,
    projects: Vec<&'a str>,
}

impl ReporterState {
    pub(super) fn workspace_summary_lines(
        &self,
        workspace: &WorkspaceSummary,
        kind: DepKind,
    ) -> Vec<String> {
        let mut changes: BTreeMap<ChangeKey<'_>, WorkspaceChange<'_>> = BTreeMap::new();
        for (prefix, buckets) in self.project_diffs(workspace) {
            for diff in self.reported_changes(workspace, prefix, &buckets[diff_key(kind)]) {
                changes
                    .entry((&diff.name, diff.added, diff.version.as_deref(), diff.from.as_deref()))
                    .or_insert_with(|| WorkspaceChange { diff, projects: Vec::new() })
                    .projects
                    .push(prefix);
            }
        }
        changes
            .into_values()
            .map(|change| {
                format!(
                    "{}{}",
                    self.diff_line(change.diff),
                    self.projects_suffix(workspace, &change.projects),
                )
            })
            .collect()
    }

    fn project_diffs<'a>(
        &'a self,
        workspace: &'a WorkspaceSummary,
    ) -> impl Iterator<Item = (&'a str, &'a DiffBuckets)> {
        std::iter::once((workspace.current_project.as_str(), &self.summary.diff))
            .chain(
                self.summary.other_projects
                    .iter()
                    .map(|(prefix, buckets)| (prefix.as_str(), buckets)),
            )
    }

    /// A project with no previous install reports only the changes that
    /// tell more than that it was installed: the ones this command asked
    /// for, and deprecated or outdated dependencies.
    fn reported_changes<'a>(
        &'a self,
        workspace: &WorkspaceSummary,
        prefix: &'a str,
        bucket: &'a HashMap<String, PackageDiff>,
    ) -> impl Iterator<Item = &'a PackageDiff> {
        let requested = workspace.new_projects
            .contains(prefix)
            .then(|| self.requested_dependencies(prefix));
        bucket
            .values()
            .filter(|diff| !self.is_hidden_linked(diff))
            .filter(move |diff| {
                requested
                    .as_ref()
                    .is_none_or(|requested| {
                        requested.contains(&diff.name)
                            || self.is_deprecated(diff)
                            || is_outdated(diff)
                    })
            })
    }

    /// The dependencies this command added to or changed in the project's
    /// manifest.
    fn requested_dependencies(&self, prefix: &str) -> RequestedDependencies {
        let edit = self.summary.manifest_diffs.get(prefix);
        let initial = edit.and_then(|edit| edit.initial.as_ref()).map(remove_optional_from_prod);
        let updated = edit.and_then(|edit| edit.updated.as_ref()).map(remove_optional_from_prod);
        let mut requested =
            RequestedDependencies { declared_before: None, changed: HashSet::new() };
        let Some(initial) = initial else {
            return requested;
        };
        let mut declared_before = HashSet::new();
        for kind in [DepKind::Peer, DepKind::Prod, DepKind::Optional, DepKind::Dev] {
            let before = manifest_dep_versions(&initial, kind.header());
            if let Some(updated) = &updated {
                let after = manifest_dep_versions(updated, kind.header());
                requested.changed.extend(
                    before
                        .iter()
                        .chain(&after)
                        .filter(|(name, spec)| {
                            before.get(*name) != Some(spec) || after.get(*name) != Some(spec)
                        })
                        .map(|(name, _)| name.clone()),
                );
            }
            declared_before.extend(before.into_keys());
        }
        requested.declared_before = Some(declared_before);
        requested
    }

    fn is_deprecated(&self, diff: &PackageDiff) -> bool {
        let Some(version) = &diff.version else {
            return false;
        };
        let name = diff.real_name.as_ref().unwrap_or(&diff.name);
        self.summary.deprecated_packages.contains(&(name.clone(), version.clone()))
    }

    /// The projects that made a change, as paths relative to the workspace
    /// root. Empty when only the current project made it.
    fn projects_suffix(&self, workspace: &WorkspaceSummary, projects: &[&str]) -> String {
        if projects == [workspace.current_project.as_str()] {
            return String::new();
        }
        let mut shown: Vec<String> = projects
            .iter()
            .map(|prefix| format_prefix_no_trim(&workspace.root, prefix))
            .collect();
        shown.sort();
        let hidden = shown.len().saturating_sub(MAX_SHOWN_PROJECTS);
        shown.truncate(MAX_SHOWN_PROJECTS);
        let mut suffix = shown.join(", ");
        if hidden > 0 {
            let _ = write!(suffix, " and {hidden} more");
        }
        format!("  {}", self.rendering.colors.grey(&suffix))
    }
}

fn is_outdated(diff: &PackageDiff) -> bool {
    match (&diff.latest, &diff.version) {
        (Some(latest), Some(version)) => is_strictly_newer(latest, version),
        _ => false,
    }
}
