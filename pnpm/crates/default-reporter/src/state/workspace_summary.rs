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
    manifest_dep_versions,
    normalized_prefix,
    remove_optional_from_prod,
    summary::{
        Change,
        bucket_changes,
    },
};
use node_semver::{
    Range,
    Version,
};
use pnpm_reporter::SummaryLog;
use pnpm_text_sanitize::sanitize_inline;

/// What a `pnpm:summary` tells about the workspace it closes. Prefixes
/// are normalized against the reporter's cwd.
#[derive(Debug)]
pub(super) struct WorkspaceSummary {
    root: String,
    current_project: String,
    new_projects: HashSet<String>,
    project_names: HashMap<String, String>,
}

impl WorkspaceSummary {
    pub(super) fn new(cwd: &str, log: &SummaryLog, new_projects: &[String]) -> Self {
        WorkspaceSummary {
            root: normalized_prefix(cwd, &log.prefix),
            current_project: normalized_prefix(cwd, cwd),
            new_projects: new_projects
                .iter()
                .map(|prefix| normalized_prefix(cwd, prefix))
                .collect(),
            project_names: log.project_names
                .iter()
                .flatten()
                .map(|(prefix, name)| (normalized_prefix(cwd, prefix), name.clone()))
                .collect(),
        }
    }
}

/// One rendered line: changes equal in sign, name, package behind an
/// alias, replaced version, version and link target are the same line.
type ChangeKey<'a> =
    (&'a str, bool, Option<&'a str>, Option<&'a str>, Option<&'a str>, Option<&'a str>);

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
    change: Change<'a>,
    projects: Vec<&'a str>,
}

impl ReporterState {
    pub(super) fn workspace_summary_lines<'a>(
        &'a self,
        workspace: &'a WorkspaceSummary,
        requests: &NewProjectRequests<'_>,
        kind: DepKind,
    ) -> Vec<String> {
        let mut changes: BTreeMap<ChangeKey<'_>, WorkspaceChange<'_>> = BTreeMap::new();
        for (prefix, buckets) in self.project_diffs(workspace) {
            let requested = requests.get(prefix);
            for change in bucket_changes(&buckets[diff_key(kind)]) {
                if self.is_hidden_linked(change.diff)
                    || requested.is_some_and(|requested| !self.is_notable(requested, change.diff))
                {
                    continue;
                }
                changes
                    .entry(change_key(&change))
                    .or_insert_with(|| WorkspaceChange { change, projects: Vec::new() })
                    .projects
                    .push(prefix);
            }
        }
        changes
            .into_values()
            .map(|WorkspaceChange { change, projects }| {
                format!("{}{}", self.diff_line(&change), self.projects_suffix(&projects))
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
    /// the dependency is deprecated.
    fn is_notable(&self, requested: &RequestedDependencies, diff: &PackageDiff) -> bool {
        requested.contains(diff) || self.deprecation_of(diff).is_some()
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

    /// The projects that made a change. Empty when only the current
    /// project made it.
    fn projects_suffix(&self, projects: &[&str]) -> String {
        self.projects_label(projects)
            .map(|names| format!("  {}", self.rendering.colors.grey(&names)))
            .unwrap_or_default()
    }

    /// `None` when the current project is the only one of `projects`
    /// (normalized prefixes). Otherwise "in" and the name of the one
    /// project, or how many projects there are.
    fn projects_label(&self, projects: &[impl AsRef<str>]) -> Option<String> {
        let current_project = normalized_prefix(&self.rendering.cwd, &self.rendering.cwd);
        match projects {
            [project] if project.as_ref() == current_project => None,
            [project] => Some(format!(
                "in {}",
                sanitize_inline(&self.project_label(project.as_ref(), &current_project)),
            )),
            _ => Some(format!("in {} projects", projects.len())),
        }
    }

    /// The project's package name, or its path relative to the workspace
    /// root when it has none.
    fn project_label(&self, project: &str, current_project: &str) -> String {
        let workspace = self.summary.workspace.as_ref();
        if let Some(name) =
            workspace.and_then(|workspace| workspace.project_names.get(project))
        {
            return name.clone();
        }
        let root = workspace.map_or(current_project, |workspace| workspace.root.as_str());
        format_prefix_no_trim(root, project)
    }
}

fn change_key<'a>(change: &Change<'a>) -> ChangeKey<'a> {
    let diff = change.diff;
    let aliased = diff.real_name
        .as_deref()
        .filter(|real_name| *real_name != diff.name);
    (
        &diff.name,
        diff.added,
        aliased,
        change.replaced_version(),
        diff.version.as_deref(),
        diff.from.as_deref(),
    )
}

/// Whether `version` is outside the semver range `spec`. A specifier that
/// is not a range, such as an alias or a link, is never outside.
fn is_outside_range(version: &str, spec: &str) -> bool {
    match (Range::parse(spec), Version::parse(version)) {
        (Ok(range), Ok(version)) => !range.satisfies(&version),
        _ => false,
    }
}
