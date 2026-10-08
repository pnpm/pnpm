use super::SyncAgentSkills;
use crate::{
    allow_build_key_from_ignored_build, importer_root_dir, is_git_hosted_dep_path,
    normalize_build_dep_path, selected_groups,
};
use pnpm_deps_restorer::parse_name_version_from_key;
use std::{
    collections::{BTreeMap, HashMap},
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
    /// The package directory, with symlinks resolved when it ships skills.
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
    let mut shipped: HashMap<String, ShippedSkills> = HashMap::new();
    for (importer_id, snapshot) in &input.lockfile.importers {
        let modules_dir =
            importer_root_dir(input.workspace_root, importer_id).join(modules_dir_name);
        for (alias, spec) in snapshot.dependencies_by_groups(groups.iter().copied()) {
            let Some(resolved) = spec.version.resolved_key(alias) else { continue };
            let alias = alias.to_string();
            let resolved = resolved.to_string();
            let Some(skills) = shipped
                .entry(probe_key(importer_id, &resolved))
                .or_insert_with(|| shipped_skills(&modules_dir.join(&alias)))
                .clone()
            else {
                continue;
            };
            let source = skill_source(skills, &resolved, &alias);
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

/// A registry package is one directory however many importers depend on
/// it. Any other key, such as `link:../lib`, can be relative to its
/// importer.
fn probe_key(importer_id: &str, resolved: &str) -> String {
    let dep_path = normalize_build_dep_path(resolved);
    if node_semver::Version::parse(parse_name_version_from_key(&dep_path).1).is_ok() {
        dep_path
    } else {
        format!("{importer_id}\0{resolved}")
    }
}

/// The skills a package directory ships, with the directory resolved when
/// there are any. `None` when the directory has skills but cannot be
/// resolved.
type ShippedSkills = Option<(Vec<String>, PathBuf)>;

/// Most packages ship no skills, so the directory is resolved only for the
/// ones that do.
fn shipped_skills(dir: &Path) -> ShippedSkills {
    let skills = skill_names(dir);
    if skills.is_empty() {
        return Some((skills, dir.to_path_buf()));
    }
    let package_dir = fs::canonicalize(dir).ok()?;
    Some((skills, package_dir))
}

fn skill_source(
    (skills, package_dir): (Vec<String>, PathBuf),
    dep_path: &str,
    alias: &str,
) -> SkillSource {
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
    SkillSource {
        skills,
        dep_path,
        approval_key,
        link_segment: link_segment.replace('/', "+"),
        package_dir,
    }
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
