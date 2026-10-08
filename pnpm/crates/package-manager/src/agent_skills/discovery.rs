use super::SyncAgentSkills;
use crate::{
    allow_build_key_from_ignored_build, importer_root_dir, is_git_hosted_dep_path,
    normalize_build_dep_path, selected_groups,
};
use pnpm_deps_restorer::parse_name_version_from_key;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// A direct dependency that ships agent skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SkillSource {
    /// Peer-free depPath, the form `.modules.yaml` records.
    pub(super) dep_path: String,
    /// The key that approves the package in `permissions`.
    pub(super) approval_key: String,
    /// The package part of the entry names.
    pub(super) link_segment: String,
    /// The package directory, with symlinks resolved.
    pub(super) package_dir: PathBuf,
    /// The names of the skill directories, sorted.
    pub(super) skills: Vec<String>,
}

/// The direct dependencies of every importer that ship skills.
///
/// A package that resolves to several registry versions across the
/// workspace is represented by its highest version, even when only an
/// older one ships skills.
pub(super) fn discover_skill_sources(input: &SyncAgentSkills<'_>) -> Vec<SkillSource> {
    let groups = selected_groups(input.included);
    let modules_dir_name = input.config.modules_dir_name();
    let mut by_key: BTreeMap<String, SkillSource> = BTreeMap::new();
    for (importer_id, snapshot) in &input.lockfile.importers {
        let modules_dir =
            importer_root_dir(input.workspace_root, importer_id).join(modules_dir_name);
        for (alias, spec) in snapshot.dependencies_by_groups(groups.iter().copied()) {
            let Some(resolved) = spec.version.resolved_key(alias) else { continue };
            let alias = alias.to_string();
            let Some(source) =
                skill_source(&modules_dir.join(&alias), &resolved.to_string(), &alias)
            else {
                continue;
            };
            match by_key.get(&source.approval_key) {
                Some(kept) if !supersedes(&source, kept) => {}
                _ => {
                    by_key.insert(source.approval_key.clone(), source);
                }
            }
        }
    }
    by_key
        .into_values()
        .filter(|source| !source.skills.is_empty())
        .collect()
}

fn skill_source(dir: &Path, dep_path: &str, alias: &str) -> Option<SkillSource> {
    let package_dir = fs::canonicalize(dir).ok()?;
    let dep_path = normalize_build_dep_path(dep_path);
    let approval_key = allow_build_key_from_ignored_build(&dep_path);
    let (name, _) = parse_name_version_from_key(&dep_path);
    // A registry or git package has a unique name. A tarball or a local
    // directory does not, so it takes the name the project gave it.
    let link_segment = if approval_key == name || is_git_hosted_dep_path(&dep_path) {
        name
    } else {
        alias.to_string()
    };
    Some(SkillSource {
        skills: skill_names(&package_dir),
        dep_path,
        approval_key,
        link_segment: link_segment.replace('/', "+"),
        package_dir,
    })
}

/// Whether `candidate` replaces `kept` for the same approval key. Only
/// registry versions compare: any other key names one source already.
fn supersedes(candidate: &SkillSource, kept: &SkillSource) -> bool {
    let version = |source: &SkillSource| {
        node_semver::Version::parse(parse_name_version_from_key(&source.dep_path).1).ok()
    };
    matches!((version(candidate), version(kept)), (Some(candidate), Some(kept)) if candidate > kept)
}

/// The skills directly under `<package_dir>/skills`: the subdirectories
/// that hold a `SKILL.md`.
fn skill_names(package_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(package_dir.join("skills")) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}
