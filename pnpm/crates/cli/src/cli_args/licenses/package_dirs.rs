use miette::IntoDiagnostic;
use pnpm_config::{Config, NodeLinker};
use pnpm_deps_restorer::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use pnpm_modules_yaml::{Host, Modules, read_modules_manifest};
use pnpm_package_manifest::safe_read_package_json_from_dir;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// Where each listed package is installed: its virtual store slot, or
/// the directories the hoisted linker placed it in.
pub(super) struct PackageDirs {
    layout: VirtualStoreLayout,
    /// The directories a `nodeLinker: hoisted` install, which leaves the
    /// virtual store empty, recorded in `.modules.yaml`, keyed by
    /// dependency path.
    hoisted_dirs: BTreeMap<String, Vec<PathBuf>>,
    project_dir: PathBuf,
    lockfile_dir: PathBuf,
    modules_dir_name: PathBuf,
    is_hoisted: bool,
    is_shamefully_hoist: bool,
}

impl PackageDirs {
    pub(super) fn new(
        config: &Config,
        lockfile_dir: &Path,
        project_dir: &Path,
        layout: VirtualStoreLayout,
    ) -> miette::Result<Self> {
        let modules_dir_name = PathBuf::from(config.modules_dir_name());
        let modules = load_modules(&modules_dir_name, lockfile_dir, project_dir)?;
        let is_hoisted = is_hoisted_linker(config, modules.as_ref());
        let is_shamefully_hoist = is_shamefully_hoisted(config, modules.as_ref());
        let hoisted_dirs = if is_hoisted {
            collect_hoisted_dirs(
                lockfile_dir,
                modules.and_then(|manifest| manifest.hoisted_locations),
            )
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            layout,
            hoisted_dirs,
            project_dir: project_dir.to_path_buf(),
            lockfile_dir: lockfile_dir.to_path_buf(),
            modules_dir_name,
            is_hoisted,
            is_shamefully_hoist,
        })
    }

    pub(super) fn package_dirs(
        &self,
        key: &PackageKey,
        name: &str,
        version: &str,
    ) -> Cow<'_, [PathBuf]> {
        let hoisted_dir = self.hoisted_dirs
            .get(&key.to_string())
            .or_else(|| self.hoisted_dirs.get(&key.without_peer().to_string()));
        if let Some(dirs) = hoisted_dir {
            return Cow::Borrowed(dirs);
        }

        let vs_path = self.layout
            .slot_dir(key)
            .join("node_modules")
            .join(name);
        let dir = if self.is_hoisted {
            self.resolve_hoisted_candidate(&vs_path, name, version)
        } else if self.is_shamefully_hoist {
            self.resolve_shamefully_hoist_candidate(&vs_path, name)
        } else {
            vs_path
        };
        Cow::Owned(vec![dir])
    }

    fn resolve_hoisted_candidate(&self, vs_path: &Path, name: &str, version: &str) -> PathBuf {
        let candidate_project = self.project_dir.join(&self.modules_dir_name).join(name);
        if matches_candidate(&candidate_project, vs_path, version) {
            return candidate_project;
        }
        let candidate_root = self.lockfile_dir.join(&self.modules_dir_name).join(name);
        if matches_candidate(&candidate_root, vs_path, version) {
            return candidate_root;
        }
        vs_path.to_path_buf()
    }

    fn resolve_shamefully_hoist_candidate(&self, vs_path: &Path, name: &str) -> PathBuf {
        let candidate_project = self.project_dir.join(&self.modules_dir_name).join(name);
        if matches_virtual_store(&candidate_project, vs_path) {
            return candidate_project;
        }
        let candidate_root = self.lockfile_dir.join(&self.modules_dir_name).join(name);
        if matches_virtual_store(&candidate_root, vs_path) {
            return candidate_root;
        }
        vs_path.to_path_buf()
    }
}

fn load_modules(
    modules_dir_name: &Path,
    lockfile_dir: &Path,
    project_dir: &Path,
) -> miette::Result<Option<Modules>> {
    let root_modules_dir = lockfile_dir.join(modules_dir_name);
    let manifest = read_modules_manifest::<Host>(&root_modules_dir).into_diagnostic()?;
    if manifest.is_some() {
        return Ok(manifest);
    }
    let project_modules_dir = project_dir.join(modules_dir_name);
    if project_modules_dir == root_modules_dir {
        Ok(None)
    } else {
        read_modules_manifest::<Host>(&project_modules_dir).into_diagnostic()
    }
}

fn is_hoisted_linker(config: &Config, modules: Option<&Modules>) -> bool {
    config.node_linker == NodeLinker::Hoisted
        || modules.is_some_and(|manifest| {
            manifest.node_linker == Some(pnpm_modules_yaml::NodeLinker::Hoisted)
        })
}

fn is_shamefully_hoisted(config: &Config, modules: Option<&Modules>) -> bool {
    config.shamefully_hoist
        || modules.and_then(|manifest| manifest.shamefully_hoist).unwrap_or(false)
        || modules
            .and_then(|manifest| manifest.public_hoist_pattern.as_ref())
            .is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| pattern == "*")
            })
}

fn collect_hoisted_dirs(
    lockfile_dir: &Path,
    hoisted_locations: Option<BTreeMap<String, Vec<String>>>,
) -> BTreeMap<String, Vec<PathBuf>> {
    let mut hoisted_dirs = BTreeMap::new();
    let Some(locations_by_dep) = hoisted_locations else {
        return hoisted_dirs;
    };
    for (dep_path, locations) in locations_by_dep {
        let dirs: Vec<_> = locations
            .iter()
            .filter_map(|location| hoisted_dir(lockfile_dir, location))
            .collect();
        if dirs.is_empty() {
            continue;
        }
        // The hoisted linker collapses the peer variants of one
        // package version onto the first dependency path it meets,
        // so only that one is recorded.
        if let Ok(key) = dep_path.parse::<PackageKey>() {
            hoisted_dirs
                .entry(key.without_peer().to_string())
                .or_insert_with(|| dirs.clone());
        }
        hoisted_dirs.insert(dep_path, dirs);
    }
    hoisted_dirs
}

fn matches_virtual_store(candidate: &Path, expected_virtual_store: &Path) -> bool {
    let Ok(candidate_canonical) = dunce::canonicalize(candidate) else {
        return false;
    };
    let Ok(expected_canonical) = dunce::canonicalize(expected_virtual_store) else {
        return false;
    };
    candidate_canonical == expected_canonical
}

fn matches_candidate(
    candidate: &Path,
    expected_virtual_store: &Path,
    expected_version: &str,
) -> bool {
    if matches_virtual_store(candidate, expected_virtual_store) {
        return true;
    }
    if let Ok(Some(manifest)) = safe_read_package_json_from_dir(candidate) {
        return manifest.get("version").and_then(|version| version.as_str())
            == Some(expected_version);
    }
    false
}

/// A lockfile-relative location resolved against `lockfile_dir`, rebuilt
/// component by component so a location recorded with `/` or `\` gets the
/// platform's separator. `None` for a location that could leave
/// `lockfile_dir`.
fn is_safe_part(part: &str) -> bool {
    matches!(Path::new(part).components().next(), Some(Component::Normal(_))) && !part.contains(':')
}

fn hoisted_dir(lockfile_dir: &Path, location: &str) -> Option<PathBuf> {
    if location.starts_with('/') || location.starts_with('\\') || Path::new(location).is_absolute()
    {
        return None;
    }
    let mut dir = lockfile_dir.to_path_buf();
    for part in location.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if !is_safe_part(part) {
            return None;
        }
        dir.push(part);
    }
    Some(dir)
}

#[cfg(test)]
mod tests;
