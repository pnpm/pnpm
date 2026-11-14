use crate::importer_root_dir;
use pnpm_config::NodeLinker;
use pnpm_deps_restorer::{
    ImporterModulesDirs, is_local_directory_version, link_target_dir, normalize_build_dep_path,
    parse_name_version_from_key, validate_importer_id,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::SyncAgentSkills;

/// Resolves direct dependency directories across workspace importers.
pub(super) struct Locator {
    modules_dirs: ImporterModulesDirs,
}

impl Locator {
    pub(super) fn new(input: &SyncAgentSkills<'_>) -> Self {
        let modules_dirs = if input.config.modules_dir.starts_with(input.workspace_root)
            || input.config.explicit_settings.contains_key("modulesDir")
        {
            ImporterModulesDirs::from_config(input.config, input.workspace_root)
        } else {
            let name = input.config.modules_dir_name();
            ImporterModulesDirs::new(input.workspace_root, Path::new(name), name)
        };
        Self { modules_dirs }
    }

    /// Whether `importer_id` is a safe lockfile importer key with an installed
    /// modules directory (when the isolated linker requires one).
    pub(super) fn importer_ready(&self, input: &SyncAgentSkills<'_>, importer_id: &str) -> bool {
        if validate_importer_id(importer_id).is_err() {
            return false;
        }
        if input.config.node_linker == NodeLinker::Hoisted {
            return true;
        }
        self.modules_dirs.of_importer_id(importer_id).is_dir()
    }

    /// The installed directory of a non-`link:` direct dependency `alias`
    /// resolving to `dep_path`.
    pub(super) fn resolved_package_dir(
        &self,
        input: &SyncAgentSkills<'_>,
        importer_id: &str,
        alias: &str,
        dep_path: &str,
    ) -> Option<PathBuf> {
        if !is_package_name(alias) {
            return None;
        }
        if input.config.node_linker != NodeLinker::Hoisted {
            return Some(self.modules_dirs.of_importer_id(importer_id).join(alias));
        }
        let recorded = input.hoisted_locations?.get(dep_path)?;
        self.hoisted_package_dir(input, importer_id, alias, recorded)
    }

    /// The installed symlink and expected target directory of a `link:` direct
    /// dependency (`workspace:*` or `link:`).
    pub(super) fn link_dirs(
        &self,
        input: &SyncAgentSkills<'_>,
        importer_id: &str,
        alias: &str,
        target: &str,
    ) -> Option<(PathBuf, PathBuf)> {
        if !is_package_name(alias) {
            return None;
        }
        let installed_dir = self.modules_dirs.of_importer_id(importer_id).join(alias);
        if !installed_dir.is_dir() {
            return None;
        }
        let importer_dir = importer_root_dir(input.workspace_root, importer_id);
        let expected_dir = link_target_dir(&importer_dir, target);
        Some((installed_dir, expected_dir))
    }

    fn hoisted_package_dir(
        &self,
        input: &SyncAgentSkills<'_>,
        importer_id: &str,
        alias: &str,
        recorded: &[String],
    ) -> Option<PathBuf> {
        // Only the root's modules directory is the configured one: the
        // hoisted linker keeps `node_modules` in workspace projects.
        let root_modules_dir = input.config.modules_dir
            .strip_prefix(input.workspace_root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| input.config.modules_dir_name().into())
            .to_string_lossy()
            .replace('\\', "/");
        let direct_slot = Path::new(importer_id)
            .ancestors()
            .map(|dir| dir.to_string_lossy().replace('\\', "/"))
            .map(|dir| match dir.as_str() {
                "" | "." => format!("{root_modules_dir}/{alias}"),
                dir => format!("{dir}/node_modules/{alias}"),
            })
            .find(|location| recorded.contains(location))
            .map(|location| input.workspace_root.join(location));
        if direct_slot.is_some() {
            return direct_slot;
        }
        let alias_link = self.modules_dirs.of_importer_id(importer_id).join(alias);
        let alias_real = fs::canonicalize(&alias_link).ok()?;
        recorded
            .iter()
            .any(|loc| {
                fs::canonicalize(input.workspace_root.join(loc)).ok().as_ref() == Some(&alias_real)
            })
            .then_some(alias_link)
    }
}

/// Whether `alias` names a package, `name` or `@scope/name`, and so stays
/// inside the `node_modules` directory it is joined to.
pub(super) fn is_package_name(alias: &str) -> bool {
    let segments: Vec<&str> = alias.split('/').collect();
    let plain = |segment: &&str| {
        !segment.is_empty() && *segment != "." && *segment != ".." && !segment.contains('\\')
    };
    match segments.as_slice() {
        [name] => plain(name),
        [scope, name] => scope.starts_with('@') && plain(scope) && plain(name),
        _ => false,
    }
}

/// Build the normalized `<pkg_name>@link:<workspace_rel_dir>` depPath for a
/// linked directory dependency.
pub(super) fn link_dep_path(
    workspace_root: &Path,
    expected_dir: &Path,
    package_dir: &Path,
    alias: &str,
) -> String {
    let pkg_name = manifest_package_name(package_dir).unwrap_or_else(|| alias.to_string());
    let root = pnpm_fs::lexical_normalize(workspace_root);
    let rel = pnpm_fs::relative_path(&root, expected_dir).to_string_lossy().replace('\\', "/");
    let rel = if rel.is_empty() { "." } else { rel.as_str() };
    format!("{pkg_name}@link:{rel}")
}

/// Normalize the depPath of a `file:` directory that ships skills without a
/// `package.json` `name` field, where the local resolver synthesized the
/// directory's basename instead of the project's declared package name.
pub(super) fn resolved_dep_path(resolved: &str, package_dir: &Path, alias: &str) -> String {
    let normalized = normalize_build_dep_path(resolved);
    let (_, version) = parse_name_version_from_key(&normalized);
    if is_local_directory_version(&version) && manifest_package_name(package_dir).is_none() {
        return format!("{alias}@{version}");
    }
    resolved.to_string()
}

fn manifest_package_name(package_dir: &Path) -> Option<String> {
    let manifest_path = package_dir.join("package.json");
    if !pnpm_fs::is_symlink_or_junction(&manifest_path).is_ok_and(|symlink| !symlink)
        || !manifest_path.is_file()
    {
        return None;
    }
    let content = fs::read_to_string(&manifest_path).ok()?;
    let parsed: Value = serde_json::from_str(&content).ok()?;
    let name = parsed.get("name")?.as_str()?;
    is_package_name(name).then(|| name.to_string())
}
