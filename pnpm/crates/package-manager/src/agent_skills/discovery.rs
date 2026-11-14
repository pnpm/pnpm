mod local_identity;
mod locator;

use super::SyncAgentSkills;
use crate::{
    is_git_hosted_dep_path, normalize_build_dep_path, selected_groups, skill_approval_key,
};
use local_identity::{
    disambiguate_local_segments, is_link_dep_path, is_registry_dep_path, local_dir_identity,
    registry_version, source_identity_key,
};
use locator::{Locator, is_package_name, link_dep_path, resolved_dep_path};
use pnpm_lockfile::{ImporterDepVersion, PkgName, ProjectSnapshot};
use pnpm_package_manifest::DependencyGroup;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

/// How many levels of directories below `skills` are searched for skills.
/// Category directories let a package group its skills, as
/// `skills/engineering/implement/SKILL.md` does.
pub(super) const MAX_SKILL_DIR_DEPTH: usize = 3;

/// A skill directory a package ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Skill {
    /// The name of the directory holding `SKILL.md`, the last part of the
    /// entry names.
    pub(super) name: String,

    /// The skill directory relative to the package's `skills` directory.
    pub(super) dir: PathBuf,
}

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
    /// The skills it ships, sorted.
    pub(super) skills: Vec<Skill>,
}

/// The direct dependencies of every importer that ship skills.
///
/// A package that resolves to several registry versions across the
/// workspace is represented by its highest version, even when only an
/// older one ships skills.
pub(super) fn discover_skill_sources(input: &SyncAgentSkills<'_>) -> Vec<SkillSource> {
    let groups = selected_groups(input.included);
    let mut importers: Vec<_> = input.lockfile.importers.iter().collect();
    importers.sort_by(|left, right| left.0.cmp(right.0));
    let mut discovery = Discovery::new(input);
    for (importer_id, snapshot) in importers {
        discovery.visit_importer(importer_id, snapshot, &groups);
    }
    let mut sources: Vec<SkillSource> = discovery.by_key
        .into_values()
        .filter(|source| !source.skills.is_empty())
        .collect();
    disambiguate_local_segments(&mut sources);
    sources
}

struct Discovery<'a, 'b> {
    input: &'a SyncAgentSkills<'b>,
    locator: Locator,
    by_key: BTreeMap<String, SkillSource>,
    shipped: HashMap<String, ShippedSkills>,
}

impl<'a, 'b> Discovery<'a, 'b> {
    fn new(input: &'a SyncAgentSkills<'b>) -> Self {
        Self {
            input,
            locator: Locator::new(input),
            by_key: BTreeMap::new(),
            shipped: HashMap::new(),
        }
    }

    fn visit_importer(
        &mut self,
        importer_id: &str,
        snapshot: &ProjectSnapshot,
        groups: &[DependencyGroup],
    ) {
        if !self.locator.importer_ready(self.input, importer_id) {
            return;
        }
        for (alias, spec) in snapshot.dependencies_by_groups(groups.iter().copied()) {
            if let Some(source) = self.discover_dep(importer_id, alias, &spec.version) {
                keep_highest(&mut self.by_key, source);
            }
        }
    }

    fn discover_dep(
        &mut self,
        importer_id: &str,
        alias: &PkgName,
        version: &ImporterDepVersion,
    ) -> Option<SkillSource> {
        let alias_str = alias.to_string();
        if let ImporterDepVersion::Link(target) = version {
            return self.discover_link_dep(importer_id, &alias_str, target);
        }
        let resolved = version.resolved_key(alias)?;
        self.discover_resolved_dep(importer_id, &alias_str, &resolved.to_string())
    }

    fn discover_link_dep(
        &mut self,
        importer_id: &str,
        alias: &str,
        target: &str,
    ) -> Option<SkillSource> {
        let (installed_dir, expected_dir) =
            self.locator.link_dirs(self.input, importer_id, alias, target)?;
        let key = format!("link\0{}", expected_dir.to_string_lossy());
        let (skills, package_dir) = cached_skills(&mut self.shipped, key, &expected_dir)?;
        if skills.is_empty() {
            return None;
        }
        if fs::canonicalize(&installed_dir).ok().as_ref() != Some(&package_dir) {
            return None;
        }
        let dep_path = link_dep_path(self.input.workspace_root, &expected_dir, &package_dir, alias);
        Some(skill_source((skills, package_dir), &dep_path, alias))
    }

    fn discover_resolved_dep(
        &mut self,
        importer_id: &str,
        alias: &str,
        resolved: &str,
    ) -> Option<SkillSource> {
        let dir = self.locator.resolved_package_dir(self.input, importer_id, alias, resolved)?;
        let (skills, package_dir) =
            cached_skills(&mut self.shipped, probe_key(importer_id, resolved), &dir)?;
        let dep_path = if skills.is_empty() {
            resolved.to_string()
        } else {
            resolved_dep_path(resolved, &package_dir, alias)
        };
        Some(skill_source((skills, package_dir), &dep_path, alias))
    }
}

/// A registry package is one directory per peer variant however many
/// importers depend on it. Any other key, such as `file:../lib`, can be
/// relative to its importer or lockfile.
fn probe_key(importer_id: &str, resolved: &str) -> String {
    let dep_path = normalize_build_dep_path(resolved);
    if is_registry_dep_path(&dep_path) {
        resolved.to_string()
    } else {
        format!("{importer_id}\0{resolved}")
    }
}

/// The skills a package directory ships, with the directory resolved when
/// there are any. `None` when the directory has skills but cannot be
/// resolved.
type ShippedSkills = Option<(Vec<Skill>, PathBuf)>;

/// What probing one importer's directory of a package found.
enum Probe {
    /// Not installed for this importer, as in a filtered install. Another
    /// importer's directory of the same package may still be.
    Absent,
    Found(ShippedSkills),
}

/// [`probe_package`], probing a package once however many importers depend
/// on it. An absent directory is not cached, and its version takes no part
/// in choosing the highest: what it ships is unknown.
fn cached_skills(
    cache: &mut HashMap<String, ShippedSkills>,
    key: String,
    dir: &Path,
) -> ShippedSkills {
    if let Some(found) = cache.get(&key) {
        return found.clone();
    }
    match probe_package(dir) {
        Probe::Absent => None,
        Probe::Found(found) => {
            cache.insert(key, found.clone());
            found
        }
    }
}

/// Most packages ship no skills, so the directory is resolved only for the
/// ones that do. A skill directory that resolves outside the package is
/// not the package's skill. A name several categories ship, compared
/// ignoring case, is kept once, at its first directory: approvals and
/// links name a skill by its name alone.
fn probe_package(dir: &Path) -> Probe {
    let candidates = skills(dir);
    if candidates.is_empty() {
        return if dir.is_dir() {
            Probe::Found(Some((candidates, dir.to_path_buf())))
        } else {
            Probe::Absent
        };
    }
    let Ok(package_dir) = fs::canonicalize(dir) else { return Probe::Found(None) };
    let mut seen = HashSet::new();
    let shipped = candidates
        .into_iter()
        .filter(|skill| {
            fs::canonicalize(package_dir.join("skills").join(&skill.dir))
                .is_ok_and(|resolved| resolved.starts_with(&package_dir))
        })
        .filter(|skill| seen.insert(skill.name.to_lowercase()))
        .collect();
    Probe::Found(Some((shipped, package_dir)))
}

fn skill_source(
    (skills, package_dir): (Vec<Skill>, PathBuf),
    dep_path: &str,
    alias: &str,
) -> SkillSource {
    let dep_path = normalize_build_dep_path(dep_path);
    let approval_key = skill_approval_key(&dep_path);
    // The name ends at the first `@` past a scope: a git URL such as
    // `git+ssh://git@host/...` holds another one.
    let name = match pnpm_deps_path::version_separator_index(&dep_path) {
        Some(at) => &dep_path[..at],
        None => dep_path.as_str(),
    };
    let is_registry = is_registry_dep_path(&dep_path);
    // A registry or git package has a unique name. A tarball, a local
    // directory, or a lockfile key whose prefix is not a package name takes
    // the alias the project gave it.
    let link_segment =
        if (is_registry || is_git_hosted_dep_path(&dep_path)) && is_package_name(name) {
            name.to_string()
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

/// Record `source` under its source identity key unless a source already kept
/// there [`supersedes`] it. Registry versions of a package share their bare
/// package name so the highest version wins; local directory links and
/// injected copies of the same directory share one key; other sources keep
/// their distinct source key while collapsing peer variants.
fn keep_highest(by_key: &mut BTreeMap<String, SkillSource>, source: SkillSource) {
    let source_key = source_identity_key(&source.dep_path);
    match by_key.get(&source_key) {
        Some(kept) if !supersedes(&source, kept) => {}
        _ => {
            by_key.insert(source_key, source);
        }
    }
}

/// Whether `candidate` replaces `kept` for the same source key. Registry
/// versions compare by semver; local directory copies prefer live `link:`
/// targets over injected `file:` copies; peer variants of the same `dep_path`
/// pick one that ships skills, then the lower package directory, so the choice
/// does not depend on importer order.
fn supersedes(candidate: &SkillSource, kept: &SkillSource) -> bool {
    let ordering = match (registry_version(&candidate.dep_path), registry_version(&kept.dep_path)) {
        (Some(candidate_version), Some(kept_version)) => candidate_version.cmp(&kept_version),
        _ if candidate.dep_path == kept.dep_path => Ordering::Equal,
        _ if same_local_dir(&candidate.dep_path, &kept.dep_path) => {
            is_link_dep_path(&candidate.dep_path).cmp(&is_link_dep_path(&kept.dep_path))
        }
        _ => return false,
    };
    ordering
        .then_with(|| (!candidate.skills.is_empty()).cmp(&!kept.skills.is_empty()))
        .then_with(|| kept.package_dir.cmp(&candidate.package_dir))
        .is_gt()
}

fn same_local_dir(left: &str, right: &str) -> bool {
    matches!(
        (local_dir_identity(left), local_dir_identity(right)),
        (Some(left_id), Some(right_id)) if left_id == right_id,
    )
}

/// [`find_skills`] over the package's `skills` directory, sorted by
/// directory.
fn skills(package_dir: &Path) -> Vec<Skill> {
    let root = package_dir.join("skills");
    let mut found = find_skills(&root, Path::new(""), 1);
    found.sort_by(|left, right| left.dir.cmp(&right.dir));
    found
}

fn find_skills(dir: &Path, relative: &Path, depth: usize) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        collect_skill_entry(&entry, relative, depth, &mut found);
    }
    found
}

fn collect_skill_entry(
    entry: &fs::DirEntry,
    relative: &Path,
    depth: usize,
    found: &mut Vec<Skill>,
) {
    let Ok(name) = entry.file_name().into_string() else { return };
    if name.starts_with('.') || name.chars().any(char::is_control) {
        return;
    }
    let Ok(file_type) = entry.file_type() else { return };
    let path = entry.path();
    if !file_type.is_dir() && !path.is_dir() {
        return;
    }
    let dir_relative = relative.join(&name);
    if path.join("SKILL.md").is_file() {
        found.push(Skill { name, dir: dir_relative });
    } else if depth < MAX_SKILL_DIR_DEPTH
        && pnpm_fs::is_symlink_or_junction(&path).is_ok_and(|symlink| !symlink)
    {
        found.extend(find_skills(&path, &dir_relative, depth + 1));
    }
}

#[cfg(test)]
mod tests;
