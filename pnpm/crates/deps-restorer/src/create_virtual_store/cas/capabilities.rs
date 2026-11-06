use super::{CreateVirtualStoreError, CreateVirtualStoreOutput, retain_dependency_closure};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

pub(super) fn retain_filesystem_packages(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    fetched: &CreateVirtualStoreOutput,
) -> Result<(), CreateVirtualStoreError> {
    let Some(snapshots) = snapshots else { return Ok(()) };
    let Some(packages) = fetched.cas_paths_by_pkg_id.as_ref() else { return Ok(()) };
    let mut inspected = HashMap::new();
    let mut roots = Vec::new();
    for key in snapshots.keys() {
        if selected.contains_key(key) {
            continue;
        }
        if let Some(files) = package_paths(fetched, packages, key)
            && needs_directory(&files, &mut inspected)?
        {
            roots.push(key.clone());
        }
    }
    retain_dependency_closure(selected, snapshots, roots);
    Ok(())
}

fn package_paths(
    fetched: &CreateVirtualStoreOutput,
    packages: &crate::CasPathsByPkgId,
    key: &PackageKey,
) -> Option<HashMap<String, PathBuf>> {
    if let Some(files) = fetched.cas_paths_by_snapshot
        .as_ref()
        .and_then(|files| files.get(key))
    {
        let mut paths = HashMap::with_capacity(files.len());
        for (name, file) in files.iter() {
            paths.insert(name.clone(), file.clone());
        }
        return Some(paths);
    }
    let files = packages.get(&super::cas_paths_key(key))?;
    let mut paths = HashMap::with_capacity(files.cas_paths.len());
    for (name, file) in files.cas_paths.iter() {
        paths.insert(name.clone(), file.path.clone());
    }
    Some(paths)
}

fn needs_directory(
    files: &HashMap<String, PathBuf>,
    inspected: &mut HashMap<PathBuf, bool>,
) -> Result<bool, CreateVirtualStoreError> {
    for (name, path) in files {
        if Path::new(name).extension().is_some()
            && ![".js", ".cjs", ".mjs"]
                .iter()
                .any(|extension| name.ends_with(extension))
        {
            continue;
        }
        let requires_directory = if let Some(&result) = inspected.get(path) {
            result
        } else {
            let source = fs::read(path)
                .map_err(|source| CreateVirtualStoreError::LoadedBuildIo {
                    path: path.clone(),
                    source,
                })?;
            let result = source_needs_directory(&String::from_utf8_lossy(&source));
            inspected.insert(path.clone(), result);
            result
        };
        if requires_directory {
            return Ok(true);
        }
    }
    Ok(false)
}

fn source_needs_directory(source: &str) -> bool {
    [
        "node:fs",
        "'fs'",
        r#""fs""#,
        "`fs`",
        "'fs/promises'",
        r#""fs/promises""#,
        "`fs/promises`",
        "__dirname",
        "__filename",
        "import.meta",
        "process.dlopen",
    ]
    .iter()
    .any(|pattern| source.contains(pattern))
}

#[cfg(test)]
mod tests;
