use super::{
    BTreeMap, HashMap, HashSet, InternalDep, Participant, Path, Range, Version, VersioningError,
    VersioningSettings, WorkspaceProject, normalize_project_dir,
};

/// Whether a package reference is a workspace-relative directory path rather
/// than a package name — the additive extension to the changesets format,
/// needed only when workspace projects share a published name.
#[must_use]
pub fn is_dir_ref(reference: &str) -> bool {
    reference.starts_with("./")
}

/// The workspace-relative directory of a project, in canonical spelling.
#[must_use]
pub fn to_project_dir(workspace_dir: &Path, root_dir: &Path) -> String {
    let relative =
        pathdiff::diff_paths(root_dir, workspace_dir).unwrap_or_else(|| root_dir.to_path_buf());
    normalize_project_dir(&relative.to_string_lossy())
}

/// Resolves package references — bare names, or `./`-prefixed
/// workspace-relative directories — against the workspace. Names are
/// aliases: one that matches several projects cannot identify any of them
/// and callers must treat it as an error, never a silent pick.
pub struct ProjectRefIndex {
    pub(super) dirs: HashSet<String>,
    pub(super) dirs_by_name: HashMap<String, Vec<String>>,
}

impl ProjectRefIndex {
    /// The directories a reference resolves to: empty when unknown, two or
    /// more when the name is ambiguous.
    #[must_use]
    pub fn ref_to_dirs(&self, reference: &str) -> Vec<String> {
        if is_dir_ref(reference) {
            let dir = normalize_project_dir(reference);
            return if self.dirs.contains(&dir) { vec![dir] } else { Vec::new() };
        }
        self.dirs_by_name.get(reference).cloned().unwrap_or_default()
    }

    #[must_use]
    pub fn name_to_dirs(&self, name: &str) -> Vec<String> {
        self.dirs_by_name.get(name).cloned().unwrap_or_default()
    }
}

#[must_use]
pub fn index_project_refs(projects: &[WorkspaceProject], workspace_dir: &Path) -> ProjectRefIndex {
    let mut dirs = HashSet::new();
    let mut dirs_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for project in projects {
        let dir = to_project_dir(workspace_dir, &project.root_dir);
        dirs.insert(dir.clone());
        if let Some(name) = &project.name {
            dirs_by_name.entry(name.clone()).or_default().push(dir);
        }
    }
    ProjectRefIndex { dirs, dirs_by_name }
}

pub(super) fn collect_participants<'a>(
    projects: &'a [WorkspaceProject],
    workspace_dir: &Path,
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
) -> Result<BTreeMap<String, Participant<'a>>, VersioningError> {
    let mut ignored_dirs: HashSet<String> = HashSet::new();
    for reference in versioning.map(|settings| settings.ignore.as_slice()).unwrap_or_default() {
        ignored_dirs.extend(resolve_config_ref(refs, reference, "versioning.ignore")?);
    }

    let mut participants = releasable_participants(projects, workspace_dir, &ignored_dirs);
    let participant_dirs: HashSet<String> = participants.keys().cloned().collect();
    for project in projects {
        let dir = to_project_dir(workspace_dir, &project.root_dir);
        if !participant_dirs.contains(&dir) {
            continue;
        }
        let internal_deps = internal_deps_of(project, &participants, &participant_dirs, refs)?;
        participants.get_mut(dir.as_str()).expect("participant exists").internal_deps =
            internal_deps;
    }
    Ok(participants)
}

/// The projects that can release. What cannot is excluded automatically:
/// unnamed and versionless (private) packages, packages with non-semver
/// placeholder versions, and the explicitly frozen ones.
fn releasable_participants<'a>(
    projects: &'a [WorkspaceProject],
    workspace_dir: &Path,
    ignored_dirs: &HashSet<String>,
) -> BTreeMap<String, Participant<'a>> {
    let mut participants = BTreeMap::new();
    for project in projects {
        let (Some(name), Some(version)) = (project.name.as_deref(), project.version.as_deref())
        else {
            continue;
        };
        let dir = to_project_dir(workspace_dir, &project.root_dir);
        if Version::parse(version).is_err() || ignored_dirs.contains(&dir) {
            continue;
        }
        participants.insert(
            dir.clone(),
            Participant {
                name,
                dir,
                root_dir: &project.root_dir,
                current_version: version,
                internal_deps: Vec::new(),
            },
        );
    }
    participants
}

/// The project's production dependencies that resolve to another
/// participant.
fn internal_deps_of<'a>(
    project: &'a WorkspaceProject,
    participants: &BTreeMap<String, Participant<'_>>,
    participant_dirs: &HashSet<String>,
    refs: &ProjectRefIndex,
) -> Result<Vec<InternalDep<'a>>, VersioningError> {
    let mut internal_deps = Vec::new();
    for dep in &project.prod_dependencies {
        let Some(target_name) = internal_dep_target_name(&dep.alias, &dep.spec, refs) else {
            continue;
        };
        let target_dirs: Vec<String> = refs
            .name_to_dirs(&target_name)
            .into_iter()
            .filter(|target_dir| participant_dirs.contains(target_dir))
            .collect();
        let target_dir = match target_dirs.len() {
            0 => continue,
            1 => target_dirs.into_iter().next().expect("one element"),
            // A workspace: range naming an ambiguous package cannot be
            // linked at install time, so the release engine never
            // legitimately sees one.
            _ => return Err(ambiguous_package(project, participants, target_name, target_dirs)),
        };
        internal_deps.push(InternalDep {
            target_dir,
            target_name,
            field: dep.field,
            alias: &dep.alias,
            spec: &dep.spec,
        });
    }
    Ok(internal_deps)
}

fn ambiguous_package(
    project: &WorkspaceProject,
    participants: &BTreeMap<String, Participant<'_>>,
    reference: String,
    dirs: Vec<String>,
) -> VersioningError {
    let participant = participants
        .values()
        .find(|participant| participant.root_dir == project.root_dir)
        .expect("participant exists");
    VersioningError::AmbiguousPackage {
        context: format!("Package {} (./{})", participant.name, participant.dir),
        reference,
        dirs,
    }
}

/// Decides whether a dependency entry points at a workspace package. Aliased
/// specs targeting somewhere else (`npm:`, `file:`, git URLs, ...) are
/// external even when the alias collides with a workspace package name; a
/// plain semver range or `catalog:` entry on a workspace name is internal —
/// it is exactly the declaration the workspace-protocol check must reject.
fn internal_dep_target_name(alias: &str, spec: &str, refs: &ProjectRefIndex) -> Option<String> {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        let target_name = parse_workspace_spec_alias(rest).unwrap_or(alias);
        return (!refs.name_to_dirs(target_name).is_empty()).then(|| target_name.to_string());
    }
    if refs.name_to_dirs(alias).is_empty() {
        return None;
    }
    (spec.starts_with("catalog:") || Range::parse(spec).is_ok()).then(|| alias.to_string())
}

/// The alias of an aliased `workspace:<alias>@<range>` spec body, mirroring
/// `WorkspaceSpec.parse` from `@pnpm/workspace.spec-parser`: the alias must
/// not start with `.`, `_`, or `/` and ends at the last `@`.
pub(super) fn parse_workspace_spec_alias(rest: &str) -> Option<&str> {
    let at_index = rest.rfind('@').filter(|&index| index > 0)?;
    let alias = &rest[..at_index];
    if alias.starts_with(['.', '_', '/']) || alias.chars().skip(1).any(|character| character == '@')
    {
        return None;
    }
    Some(alias)
}

/// Resolves a package reference from `versioning` configuration. An unknown
/// reference is skipped — configuration may outlive a removed project — but
/// an ambiguous name is an error: it cannot be attributed, and silence here
/// is exactly the name-keying flaw this engine exists to fix.
pub(super) fn resolve_config_ref(
    refs: &ProjectRefIndex,
    reference: &str,
    setting_name: &str,
) -> Result<Vec<String>, VersioningError> {
    let dirs = refs.ref_to_dirs(reference);
    if dirs.len() > 1 {
        return Err(VersioningError::AmbiguousPackage {
            context: setting_name.to_string(),
            reference: reference.to_string(),
            dirs,
        });
    }
    Ok(dirs)
}

pub(super) fn assert_internal_deps_use_workspace_protocol(
    participants: &BTreeMap<String, Participant<'_>>,
) -> Result<(), VersioningError> {
    for participant in participants.values() {
        for dep in &participant.internal_deps {
            if !dep.spec.starts_with("workspace:") {
                return Err(VersioningError::InternalRange {
                    pkg_name: participant.name.to_string(),
                    alias: dep.alias.to_string(),
                    field: dep.field.to_string(),
                    spec: dep.spec.to_string(),
                });
            }
        }
    }
    Ok(())
}
