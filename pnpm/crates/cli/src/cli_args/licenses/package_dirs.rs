use miette::IntoDiagnostic;
use pnpm_config::{Config, NodeLinker};
use pnpm_deps_restorer::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use pnpm_modules_yaml::{Host, Modules, read_modules_manifest};
use pnpm_package_manifest::safe_read_package_json_from_dir;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
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
        let is_hoisted = config.node_linker == NodeLinker::Hoisted;
        let hoisted_dirs = if is_hoisted {
            let modules = load_modules(&modules_dir_name, lockfile_dir, project_dir)?;
            pnpm_deps_inspection::pkg_info::collect_hoisted_dirs(
                lockfile_dir,
                modules.as_ref().and_then(|manifest| manifest.hoisted_locations.as_ref()),
            )
        } else {
            BTreeMap::new()
        };
        let is_shamefully_hoist = !is_hoisted && hoists_everything_publicly(config);

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

fn hoists_everything_publicly(config: &Config) -> bool {
    config.shamefully_hoist
        || config.public_hoist_pattern
            .as_ref()
            .is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| pattern == "*")
            })
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

#[cfg(test)]
mod tests;
