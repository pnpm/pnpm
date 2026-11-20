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
use node_semver::{
    Range,
    Version,
};
use pnpm_text_sanitize::sanitize_inline;
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

/// One rendered line: changes equal in sign, name, package behind an
/// alias, version and link target are the same line.
type ChangeKey<'a> = (&'a str, bool, Option<&'a str>, Option<&'a str>, Option<&'a str>);

/// The dependencies each project with no previous install was asked for,
/// keyed by normalized prefix.
pub(super) type NewProjectRequests<'a> = HashMap<&'a str, RequestedDependencies>;

/// A command that edits manifests reports each project's manifest before
/// the install and after it. The manifest after it arrives once the
/// summary may have rendered, so the manifest before also tells: a
/// dependency it did not declare, or declared with a range the installed
/// version is outside of, was requested.
pub(super) struct RequestedDependencies {
    declared_before: Option<HashMap<String, String>>,
    changed: HashSet<String>,
}

impl RequestedDependencies {
    fn contains(&self, diff: &PackageDiff) -> bool {
        if self.changed.contains(&diff.name) {
            return true;
        }
        let Some(declared_before) = &self.declared_before else {
            return false;
        };
        match declared_before.get(&diff.name) {
            None => true,
            Some(spec) => {
                diff.added
                    && diff.version
                        .as_deref()
                        .is_some_and(|version| is_outside_range(version, spec))
            }
        }
    }
}

struct WorkspaceChange<'a> {
    diff: &'a PackageDiff,
    projects: Vec<&'a str>,
}

impl ReporterState {
    pub(super) fn workspace_summary_lines<'a>(
        &'a self,
        workspace: &'a WorkspaceSummary,
        requests: &NewProjectRequests<'_>,
        kind: DepKind,
    ) -> Vec<(String, &'a PackageDiff)> {
        let mut changes: BTreeMap<ChangeKey<'_>, WorkspaceChange<'_>> = BTreeMap::new();
        for (prefix, buckets) in self.project_diffs(workspace) {
            let requested = requests.get(prefix);
            for diff in buckets[diff_key(kind)].values() {
                if self.is_hidden_linked(diff)
                    || requested.is_some_and(|requested| !self.is_notable(requested, diff))
                {
                    continue;
                }
                changes
                    .entry(change_key(diff))
                    .or_insert_with(|| WorkspaceChange { diff, projects: Vec::new() })
                    .projects
                    .push(prefix);
            }
        }
        changes
            .into_values()
            .map(|change| {
                let line = format!(
                    "{}{}",
                    self.diff_line(change.diff),
                    self.projects_suffix(change.projects),
                );
                (line, change.diff)
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

    /// Whether a change of a project with no previous install tells more
    /// than that the project was installed: this command asked for it, or
    /// the dependency is deprecated or outdated.
    fn is_notable(&self, requested: &RequestedDependencies, diff: &PackageDiff) -> bool {
        requested.contains(diff) || self.deprecation_of(diff).is_some() || is_outdated(diff)
    }

    pub(super) fn new_project_requests<'a>(
        &self,
        workspace: &'a WorkspaceSummary,
    ) -> NewProjectRequests<'a> {
        workspace.new_projects
            .iter()
            .map(|prefix| (prefix.as_str(), self.requested_dependencies(prefix)))
            .collect()
    }

    fn requested_dependencies(&self, prefix: &str) -> RequestedDependencies {
        let edit = self.summary.manifest_diffs
            .get(prefix)
            .filter(|_| self.options.scope.edits_manifests);
        let initial = edit.and_then(|edit| edit.initial.as_ref()).map(remove_optional_from_prod);
        let updated = edit.and_then(|edit| edit.updated.as_ref()).map(remove_optional_from_prod);
        let mut requested =
            RequestedDependencies { declared_before: None, changed: HashSet::new() };
        let Some(initial) = initial else {
            return requested;
        };
        let mut declared_before = HashMap::new();
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
            declared_before.extend(before);
        }
        requested.declared_before = Some(declared_before);
        requested
    }

    /// The projects that made a change, as paths relative to the workspace
    /// root. Empty when only the current project made it.
    fn projects_suffix(&self, projects: Vec<&str>) -> String {
        self.project_names(projects)
            .map(|names| format!("  {}", self.rendering.colors.grey(&names)))
            .unwrap_or_default()
    }

    /// `projects` (normalized prefixes) as paths relative to the workspace
    /// root, or to the cwd outside a workspace summary, at most
    /// [`MAX_SHOWN_PROJECTS`] of them and then a count of the rest. `None`
    /// when the current project is the only one.
    pub(super) fn project_names(&self, mut projects: Vec<&str>) -> Option<String> {
        let current_project = normalized_prefix(&self.rendering.cwd, &self.rendering.cwd);
        if projects == [current_project.as_str()] {
            return None;
        }
        let root = match &self.summary.workspace {
            Some(workspace) => workspace.root.as_str(),
            None => current_project.as_str(),
        };
        let hidden = projects.len().saturating_sub(MAX_SHOWN_PROJECTS);
        if hidden > 0 {
            projects.select_nth_unstable(MAX_SHOWN_PROJECTS - 1);
            projects.truncate(MAX_SHOWN_PROJECTS);
        }
        projects.sort_unstable();
        let mut names = projects
            .iter()
            .map(|prefix| sanitize_inline(&format_prefix_no_trim(root, prefix)).into_owned())
            .collect::<Vec<_>>()
            .join(", ");
        if hidden > 0 {
            let _ = write!(names, " and {hidden} more");
        }
        Some(names)
    }
}

fn change_key(diff: &PackageDiff) -> ChangeKey<'_> {
    let aliased = diff.real_name
        .as_deref()
        .filter(|real_name| *real_name != diff.name);
    (&diff.name, diff.added, aliased, diff.version.as_deref(), diff.from.as_deref())
}

fn is_outdated(diff: &PackageDiff) -> bool {
    match (&diff.latest, &diff.version) {
        (Some(latest), Some(version)) => is_strictly_newer(latest, version),
        _ => false,
    }
}

/// Whether `version` is outside the semver range `spec`. A specifier that
/// is not a range, such as an alias or a link, is never outside.
fn is_outside_range(version: &str, spec: &str) -> bool {
    match (Range::parse(spec), Version::parse(version)) {
        (Ok(range), Ok(version)) => !range.satisfies(&version),
        _ => false,
    }
}
