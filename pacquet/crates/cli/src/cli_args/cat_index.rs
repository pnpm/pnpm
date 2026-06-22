use clap::Args;
use miette::{Context, IntoDiagnostic, Result};
use pacquet_config::Config;
use pacquet_lockfile::{
    Lockfile, LockfileResolution, PackageMetadata, PkgName, ProjectSnapshot, ResolvedDependencySpec,
};
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pacquet_store_dir::{
    PackageFilesIndex, StoreIndex, StoreIndexError, git_hosted_store_index_key, store_index_key,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    io::Write as _,
    path::{Path, PathBuf},
};

const MAX_JSON_SORT_DEPTH: usize = 128;

#[derive(Debug, Args)]
pub struct CatIndexArgs {
    /// The package specifier (e.g., `pkg@version`)
    pub wanted_dependency: String,
}

impl CatIndexArgs {
    pub async fn run<'a>(
        self,
        dir: &Path,
        config: impl FnOnce() -> Result<&'a Config>,
    ) -> Result<()> {
        let parsed = parse_wanted_dependency(&self.wanted_dependency);
        let Some(alias) = parsed.alias else {
            return Err(miette::miette!(
                "Cannot parse the \"{}\" selector",
                self.wanted_dependency
            ));
        };

        let config = config()?;
        let lockfile_dir = lockfile_dir(config, dir);
        let requested_bare = parsed.bare_specifier.as_deref();
        let keys = lockfile_store_index_keys(&lockfile_dir, dir, &alias, requested_bare)
            .wrap_err("load package key from lockfile")?;
        let fallback_pkg_ids = fallback_pkg_ids(&alias, requested_bare);
        let store_dir = config.store_dir.root().to_path_buf();
        let frozen_store = config.frozen_store;

        let pkg_files_index = tokio::task::spawn_blocking(move || {
            read_package_index(&store_dir, frozen_store, keys, fallback_pkg_ids)
        })
        .await
        .into_diagnostic()
        .wrap_err("read package index")?
        .into_diagnostic()?;

        let Some(pkg_files_index) = pkg_files_index else {
            return Err(miette::miette!(
                "No corresponding index file found. You can use pnpm list to see if the package is installed."
            ));
        };

        let mut value = serde_json::to_value(&pkg_files_index)
            .into_diagnostic()
            .wrap_err("serialize package index")?;
        sort_deep_keys(&mut value, 0)?;

        let json = serde_json::to_string_pretty(&value)
            .into_diagnostic()
            .wrap_err("render package index JSON")?;
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();

        Ok(())
    }
}

fn lockfile_dir(config: &Config, dir: &Path) -> PathBuf {
    match &config.workspace_dir {
        Some(workspace_dir) => workspace_dir.clone(),
        None => dir.to_path_buf(),
    }
}

fn lockfile_store_index_keys(
    lockfile_dir: &Path,
    dir: &Path,
    alias: &str,
    requested_bare: Option<&str>,
) -> Result<Vec<String>> {
    let Some(lockfile) = Lockfile::load_wanted_from_dir(lockfile_dir)
        .into_diagnostic()
        .wrap_err("load pnpm-lock.yaml")?
    else {
        return Ok(Vec::new());
    };
    let Ok(alias_name) = alias.parse::<PkgName>() else {
        return Ok(Vec::new());
    };
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for importer_id in importer_ids(lockfile_dir, dir) {
        let Some(importer) = lockfile.importers.get(&importer_id) else { continue };
        let Some(dependency) = find_dependency(importer, &alias_name) else { continue };
        let Some(snapshot_key) = dependency.version.resolved_key(&alias_name) else { continue };
        let metadata_key = snapshot_key.without_peer();
        let pkg_id = metadata_key.to_string();
        if !request_matches_dependency(alias, requested_bare, dependency, &pkg_id) {
            continue;
        }
        let Some(metadata) =
            lockfile.packages.as_ref().and_then(|packages| packages.get(&metadata_key))
        else {
            continue;
        };
        for key in metadata_store_index_keys(&pkg_id, metadata) {
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
}

fn importer_ids(lockfile_dir: &Path, current_dir: &Path) -> Vec<String> {
    let mut ids = Vec::with_capacity(2);
    if let Ok(relative) = current_dir.strip_prefix(lockfile_dir) {
        let id = if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
        };
        ids.push(id);
    }
    if !ids.iter().any(|id| id == ".") {
        ids.push(".".to_string());
    }
    ids
}

fn find_dependency<'a>(
    importer: &'a ProjectSnapshot,
    alias: &PkgName,
) -> Option<&'a ResolvedDependencySpec> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .find_map(|dependencies| {
            dependencies.as_ref().and_then(|dependencies| dependencies.get(alias))
        })
}

fn request_matches_dependency(
    alias: &str,
    requested_bare: Option<&str>,
    dependency: &ResolvedDependencySpec,
    pkg_id: &str,
) -> bool {
    let Some(requested_bare) = requested_bare else { return true };
    dependency.specifier == requested_bare || pkg_id == format!("{alias}@{requested_bare}")
}

fn metadata_store_index_keys(pkg_id: &str, metadata: &PackageMetadata) -> Vec<String> {
    match &metadata.resolution {
        LockfileResolution::Tarball(resolution) if resolution.git_hosted == Some(true) => {
            git_store_index_keys(pkg_id)
        }
        LockfileResolution::Tarball(resolution) => resolution
            .integrity
            .as_ref()
            .map(|integrity| vec![store_index_key(&integrity.to_string(), pkg_id)])
            .unwrap_or_default(),
        LockfileResolution::Registry(resolution) => {
            vec![store_index_key(&resolution.integrity.to_string(), pkg_id)]
        }
        LockfileResolution::Git(_) => git_store_index_keys(pkg_id),
        LockfileResolution::Binary(resolution) => {
            vec![store_index_key(&resolution.integrity.to_string(), pkg_id)]
        }
        LockfileResolution::Directory(_) | LockfileResolution::Variations(_) => Vec::new(),
    }
}

fn git_store_index_keys(pkg_id: &str) -> Vec<String> {
    vec![git_hosted_store_index_key(pkg_id, true), git_hosted_store_index_key(pkg_id, false)]
}

fn fallback_pkg_ids(alias: &str, requested_bare: Option<&str>) -> Vec<String> {
    let Some(requested_bare) = requested_bare else { return Vec::new() };
    let pkg_id = requested_bare
        .strip_prefix("npm:")
        .and_then(npm_alias_pkg_id)
        .unwrap_or_else(|| format!("{alias}@{requested_bare}"));
    vec![pkg_id]
}

fn npm_alias_pkg_id(target: &str) -> Option<String> {
    let at_index = target.bytes().enumerate().rev().find_map(|(idx, byte)| {
        (byte == b'@' && idx > usize::from(target.starts_with('@'))).then_some(idx)
    })?;
    let mut pkg_id = target[..at_index].to_string();
    pkg_id.push('@');
    pkg_id.push_str(&target[at_index + 1..]);
    Some(pkg_id)
}

fn read_package_index(
    store_dir: &Path,
    frozen_store: bool,
    keys: Vec<String>,
    fallback_pkg_ids: Vec<String>,
) -> std::result::Result<Option<PackageFilesIndex>, StoreIndexError> {
    if !store_dir.join("index.db").exists() {
        return Ok(None);
    }
    let store_index = if frozen_store {
        StoreIndex::open_immutable(store_dir)?
    } else {
        StoreIndex::open_readonly(store_dir)?
    };
    for key in keys {
        if let Some(pkg_files_index) = store_index.get(&key)? {
            return Ok(Some(pkg_files_index));
        }
    }
    for pkg_id in fallback_pkg_ids {
        if let Some(pkg_files_index) = store_index.get_by_pkg_id(&pkg_id)? {
            return Ok(Some(pkg_files_index));
        }
    }
    Ok(None)
}

fn sort_deep_keys(value: &mut Value, depth: usize) -> Result<()> {
    if depth > MAX_JSON_SORT_DEPTH {
        return Err(miette::miette!("Package index JSON is nested too deeply to print safely"));
    }
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            for (_, val) in &mut entries {
                sort_deep_keys(val, depth + 1)?;
            }
            *map = entries.into_iter().collect();
        }
        Value::Array(arr) => {
            for item in arr {
                sort_deep_keys(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
