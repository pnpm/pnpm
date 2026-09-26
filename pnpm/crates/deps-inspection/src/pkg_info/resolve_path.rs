use std::path::{Component, Path, PathBuf};

use pnpm_lockfile::PkgNameVerPeer;

use super::{EdgeContext, InspectionLayout};

#[must_use]
pub fn is_unsafe_path_component(component: &str) -> bool {
    Path::new(component)
        .components()
        .any(|part| {
            matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
}

/// Filesystem path of a package addressed by `dep_path`.
#[must_use]
pub fn resolve_package_path(
    layout: &InspectionLayout,
    dep_path: &PkgNameVerPeer,
    name: &str,
    version: &str,
    alias: &str,
    ctx: &EdgeContext<'_>,
) -> PathBuf {
    let store_name = dep_path.to_virtual_store_name(layout.virtual_store_dir_max_length);
    if is_unsafe_path_component(&store_name) || is_unsafe_path_component(name) {
        return layout.virtual_store_dir.clone();
    }
    if layout.is_hoisted {
        return resolve_hoisted_package_path(layout, dep_path, name, version, ctx);
    }
    resolve_virtual_store_package_path(layout, &store_name, name, alias, ctx)
}

fn pick_hoisted_dir(dirs: &[PathBuf], ctx: &EdgeContext<'_>) -> Option<PathBuf> {
    if let Some(parent_dir) = &ctx.parent_dir
        && let Some(nested) = dirs
            .iter()
            .find(|loc| loc.starts_with(parent_dir) && loc.exists())
    {
        return Some(nested.clone());
    }
    dirs.iter()
        .find(|loc| loc.starts_with(&ctx.linked_path_base_dir) && loc.exists())
        .or_else(|| dirs.iter().find(|loc| loc.exists()))
        .or_else(|| dirs.first())
        .cloned()
}

fn resolve_hoisted_package_path(
    layout: &InspectionLayout,
    dep_path: &PkgNameVerPeer,
    name: &str,
    version: &str,
    ctx: &EdgeContext<'_>,
) -> PathBuf {
    find_hoisted_dirs(&layout.hoisted_dirs, dep_path)
        .and_then(|dirs| pick_hoisted_dir(dirs, ctx))
        .unwrap_or_else(|| {
            resolve_hoisted_fallback(layout, dep_path, name, version, &ctx.linked_path_base_dir)
        })
}

fn find_hoisted_dirs<'a>(
    hoisted_dirs: &'a std::collections::BTreeMap<String, Vec<PathBuf>>,
    dep_path: &PkgNameVerPeer,
) -> Option<&'a [PathBuf]> {
    let key_str = dep_path.to_string();
    let without_peer_str = dep_path.without_peer().to_string();
    hoisted_dirs
        .get(&key_str)
        .or_else(|| hoisted_dirs.get(&without_peer_str))
        .or_else(|| {
            if let Some(stripped) = key_str.strip_prefix('/') {
                hoisted_dirs.get(stripped)
            } else {
                hoisted_dirs.get(&format!("/{key_str}"))
            }
        })
        .map(Vec::as_slice)
}

fn resolve_hoisted_fallback(
    layout: &InspectionLayout,
    dep_path: &PkgNameVerPeer,
    name: &str,
    version: &str,
    project_dir: &Path,
) -> PathBuf {
    let candidate_project = project_dir.join(&layout.modules_dir_name).join(name);
    if candidate_matches_version(&candidate_project, version) {
        return candidate_project;
    }
    let candidate_lockfile = layout.lockfile_dir.join(&layout.modules_dir_name).join(name);
    if candidate_matches_version(&candidate_lockfile, version) {
        return candidate_lockfile;
    }
    let store_name = dep_path.to_virtual_store_name(layout.virtual_store_dir_max_length);
    layout.virtual_store_dir
        .join(store_name)
        .join("node_modules")
        .join(name)
}

fn candidate_matches_version(candidate: &Path, expected_version: &str) -> bool {
    if expected_version.is_empty() {
        return candidate.exists();
    }
    if let Ok(Some(manifest)) = pnpm_package_manifest::safe_read_package_json_from_dir(candidate) {
        return manifest.get("version").and_then(|v| v.as_str()) == Some(expected_version);
    }
    false
}

fn resolve_virtual_store_package_path(
    layout: &InspectionLayout,
    store_name: &str,
    name: &str,
    alias: &str,
    ctx: &EdgeContext<'_>,
) -> PathBuf {
    let constructed = layout.virtual_store_dir
        .join(store_name)
        .join("node_modules")
        .join(name);

    if !layout.is_global_virtual_store() || is_unsafe_path_component(alias) {
        return constructed;
    }

    let node_modules_dir = match &ctx.parent_dir {
        Some(parent_dir) => {
            let mut dir = parent_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            if dir
                .file_name()
                .is_some_and(|component| component.to_string_lossy().starts_with('@'))
                && let Some(grandparent) = dir.parent()
            {
                dir = grandparent.to_path_buf();
            }
            dir
        }
        None => layout.modules_dir.clone(),
    };
    dunce::canonicalize(node_modules_dir.join(alias)).unwrap_or(constructed)
}

#[must_use]
pub fn collect_hoisted_dirs(
    lockfile_dir: &Path,
    hoisted_locations: Option<&std::collections::BTreeMap<String, Vec<String>>>,
) -> std::collections::BTreeMap<String, Vec<PathBuf>> {
    let mut hoisted_dirs = std::collections::BTreeMap::new();
    let Some(locations_by_dep) = hoisted_locations else {
        return hoisted_dirs;
    };
    for (dep_path, locations) in locations_by_dep {
        let dirs: Vec<_> = locations
            .iter()
            .filter_map(|location| pnpm_modules_yaml::hoisted_dir(lockfile_dir, location))
            .collect();
        if dirs.is_empty() {
            continue;
        }
        if let Ok(key) = dep_path.parse::<pnpm_lockfile::PackageKey>() {
            hoisted_dirs
                .entry(key.without_peer().to_string())
                .or_insert_with(|| dirs.clone());
        }
        hoisted_dirs.insert(dep_path.clone(), dirs);
    }
    hoisted_dirs
}
