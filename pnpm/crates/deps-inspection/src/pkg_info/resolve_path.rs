use std::path::{Component, Path, PathBuf};

use pnpm_lockfile::PkgNameVerPeer;

use super::{EdgeContext, InspectionLayout};

/// A lockfile-derived path component that could escape the directory
/// it is joined under — the same guard `pnpm licenses` applies before
/// dereferencing store paths built from lockfile keys. Rooted and
/// prefixed components are rejected by shape, not `is_absolute()`:
/// on Windows a rooted-but-prefixless `\escape` (or a prefix-only
/// `C:evil`) is not "absolute" yet still replaces the join base.
#[must_use]
pub fn is_unsafe_path_component(component: &str) -> bool {
    Path::new(component)
        .components()
        .any(|part| {
            matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
}

/// Filesystem path of a package addressed by `dep_path`. For a local
/// virtual store the path is constructed directly; for a global
/// virtual store the symlink through the parent's `node_modules` is
/// resolved instead; for the hoisted linker the location recorded in
/// `.modules.yaml` is used. A name that could traverse outside the
/// virtual store is never joined or dereferenced.
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
        return resolve_hoisted_package_path(layout, dep_path, name, version, alias, ctx);
    }
    resolve_virtual_store_package_path(layout, &store_name, name, alias, ctx)
}

/// The copy Node.js resolves from the parent package, or from the
/// project for a direct dependency: the one in the closest modules
/// directory above it. Copies on disk under the edge's alias are
/// preferred, since one dep path gets a directory per alias it is
/// installed under. Falls back to any copy on disk, then to the
/// recorded one under the alias, then to the first recorded one.
fn pick_hoisted_dir(
    recorded_dirs: &[PathBuf],
    relative_modules_dir: &Path,
    alias: &str,
    ctx: &EdgeContext<'_>,
) -> Option<PathBuf> {
    let existing: Vec<&PathBuf> = recorded_dirs
        .iter()
        .filter(|dir| dir.exists())
        .collect();
    let existing_under_alias: Vec<&PathBuf> = existing
        .iter()
        .copied()
        .filter(|dir| is_installed_under(dir, alias))
        .collect();
    let candidates =
        if existing_under_alias.is_empty() { &existing } else { &existing_under_alias };
    let resolve_from = ctx.parent_dir.as_deref().unwrap_or(&ctx.linked_path_base_dir);
    candidates
        .iter()
        .copied()
        .filter_map(|dir| Some((dir, owner_dir(dir, relative_modules_dir)?)))
        .filter(|(_, owner)| resolve_from.starts_with(owner))
        .max_by_key(|(_, owner)| owner.components().count())
        .map(|(dir, _)| dir)
        .or_else(|| candidates.first().copied())
        .or_else(|| recorded_dirs.iter().find(|dir| is_installed_under(dir, alias)))
        .or_else(|| recorded_dirs.first())
        .cloned()
}

/// Whether `pkg_dir` is `<modules dir>/<alias>`: its trailing
/// components spell the whole alias, not just the name of a scoped
/// package.
fn is_installed_under(pkg_dir: &Path, alias: &str) -> bool {
    let alias_path = Path::new(alias);
    pkg_dir.ends_with(alias_path)
        && pkg_dir
            .ancestors()
            .nth(alias_path.components().count())
            .and_then(Path::file_name)
            .is_some_and(|name| !name.to_string_lossy().starts_with('@'))
}

/// The directory whose modules directory holds `pkg_dir`.
fn owner_dir<'a>(pkg_dir: &'a Path, relative_modules_dir: &Path) -> Option<&'a Path> {
    let mut modules_dir = pkg_dir.parent()?;
    if modules_dir
        .file_name()
        .is_some_and(|component| component.to_string_lossy().starts_with('@'))
    {
        modules_dir = modules_dir.parent()?;
    }
    if modules_dir.ends_with(relative_modules_dir) {
        return modules_dir
            .ancestors()
            .nth(relative_modules_dir.components().count());
    }
    modules_dir.parent()
}

fn resolve_hoisted_package_path(
    layout: &InspectionLayout,
    dep_path: &PkgNameVerPeer,
    name: &str,
    version: &str,
    alias: &str,
    ctx: &EdgeContext<'_>,
) -> PathBuf {
    find_hoisted_dirs(&layout.hoisted_dirs, dep_path)
        .and_then(|dirs| pick_hoisted_dir(dirs, &layout.relative_modules_dir, alias, ctx))
        .unwrap_or_else(|| {
            resolve_hoisted_fallback(
                layout,
                dep_path,
                name,
                version,
                alias,
                &ctx.linked_path_base_dir,
            )
        })
}

fn find_hoisted_dirs<'a>(
    hoisted_dirs: &'a std::collections::BTreeMap<String, Vec<PathBuf>>,
    dep_path: &PkgNameVerPeer,
) -> Option<&'a [PathBuf]> {
    [dep_path.to_string(), dep_path.without_peer().to_string()]
        .iter()
        .flat_map(|key| [key.clone(), legacy_dep_path(key)])
        .find_map(|key| hoisted_dirs.get(&key))
        .map(Vec::as_slice)
}

/// `dep_path` with the leading `/` of old lockfile keys toggled.
fn legacy_dep_path(dep_path: &str) -> String {
    dep_path
        .strip_prefix('/')
        .map_or_else(|| format!("/{dep_path}"), str::to_string)
}

/// The hoisted linker places a dependency under its alias, so an
/// aliased package (`bar: npm:foo@1`) is probed at `node_modules/bar`.
fn resolve_hoisted_fallback(
    layout: &InspectionLayout,
    dep_path: &PkgNameVerPeer,
    name: &str,
    version: &str,
    alias: &str,
    project_dir: &Path,
) -> PathBuf {
    if !is_unsafe_path_component(alias) {
        let candidate_project = project_dir.join(&layout.relative_modules_dir).join(alias);
        if candidate_matches_version(&candidate_project, version) {
            return candidate_project;
        }
        let candidate_lockfile = layout.modules_dir.join(alias);
        if candidate_matches_version(&candidate_lockfile, version) {
            return candidate_lockfile;
        }
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
            // Scoped parents live one level deeper (`node_modules/@scope/pkg`).
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
        // The hoisted linker collapses the peer variants of one
        // package version onto the first dependency path it meets,
        // so only that one is recorded.
        if let Ok(key) = dep_path.parse::<pnpm_lockfile::PackageKey>() {
            hoisted_dirs
                .entry(key.without_peer().to_string())
                .or_insert_with(|| dirs.clone());
        }
        hoisted_dirs.insert(dep_path.clone(), dirs);
    }
    hoisted_dirs
}
