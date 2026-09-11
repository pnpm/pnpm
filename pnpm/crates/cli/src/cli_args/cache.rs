use clap::Subcommand;
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pnpm_config::{Config, ResolutionMode};
use pnpm_fs::lexical_normalize;
use pnpm_resolving_npm_resolver::mirror::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, decode_registry_name,
    get_registry_name, load_meta,
};
use pnpm_store_dir::StoreIndex;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
use wax::walk::Entry;

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Lists the available packages metadata cache. Supports filtering by glob.
    List { packages: Vec<String> },
    /// Lists all registries that have their metadata cache locally.
    ListRegistries,
    /// Prints the path to the cache directory.
    Path,
    /// Views information from the specified package's cache.
    View { package: String },
    /// Deletes metadata cache for the specified package(s). Supports patterns.
    Delete { packages: Vec<String> },
}

impl CacheCommand {
    fn meta_dir(config: &Config) -> &'static str {
        if config.resolution_mode == ResolutionMode::TimeBased
            && !config.registry_supports_time_field
        {
            FULL_FILTERED_META_DIR
        } else {
            ABBREVIATED_META_DIR
        }
    }

    fn cache_dir(config: &Config) -> PathBuf {
        config.cache_dir.join(Self::meta_dir(config))
    }

    /// Lexically cleaned form of the configured cache directory, for
    /// `pnpm cache path` to hand to other tools.
    ///
    /// `dunce::canonicalize` would also resolve symlinks, which the
    /// TypeScript CLI's `path.resolve` does not — on macOS, where the
    /// temporary and home directories are symlinked, the two stacks would
    /// then print different paths for the same configuration.
    fn cleaned_cache_dir(config: &Config) -> PathBuf {
        lexical_normalize(&config.cache_dir)
    }

    /// Filesystem-safe slug of the configured registry, used as the top-level
    /// directory under the metadata cache root. A malformed registry URL is a
    /// configuration error, so we surface it rather than broadening the glob
    /// scope to every registry — important because `delete` is destructive.
    fn registry_prefix(config: &Config) -> miette::Result<String> {
        get_registry_name(&config.registry).into_diagnostic()
    }

    /// Reject names whose glob would escape the cache root. pnpm passes filter
    /// arguments straight into a glob; pacquet additionally guards against `..`
    /// segments so a crafted name can't match files outside the cache tree —
    /// `delete` removes whatever the glob resolves to.
    fn reject_path_traversal(name: &str) -> miette::Result<()> {
        if name.contains("..") {
            return Err(miette::miette!(
                "Invalid package name '{name}': path traversal sequences are not allowed"
            ));
        }
        Ok(())
    }

    fn find_metadata_files(
        config: &Config,
        cache_dir: &Path,
        packages: &[String],
    ) -> miette::Result<Vec<String>> {
        let registry_prefix = Self::registry_prefix(config)?;

        let patterns = if packages.is_empty() {
            vec![format!("{registry_prefix}/**")]
        } else {
            packages
                .iter()
                .map(|pkg| {
                    Self::reject_path_traversal(pkg)?;
                    // Filters are matched literally, as in pnpm — they are glob
                    // segments, not package names, so they are not re-encoded.
                    Ok(format!("{registry_prefix}/{pkg}.jsonl"))
                })
                .collect::<miette::Result<Vec<_>>>()?
        };

        let mut matches = Vec::new();
        for pattern in patterns {
            for (relative, _) in walk_metadata_files(cache_dir, &pattern)? {
                matches.push(relative);
            }
        }
        matches.sort();
        matches.dedup();
        Ok(matches)
    }

    pub fn run(self, config: &Config) -> miette::Result<()> {
        let cache_dir = Self::cache_dir(config);

        match self {
            CacheCommand::Path => {
                println!("{}", Self::cleaned_cache_dir(config).display());
            }
            CacheCommand::ListRegistries => {
                let Ok(entries) = fs::read_dir(&cache_dir) else {
                    return Ok(());
                };
                let mut registries: Vec<String> = entries
                    .filter_map(std::result::Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                registries.sort();
                if !registries.is_empty() {
                    println!("{}", registries.join("\n"));
                }
            }
            CacheCommand::List { packages } => {
                if !cache_dir.exists() {
                    return Ok(());
                }
                let meta_files = Self::find_metadata_files(config, &cache_dir, &packages)?;
                if !meta_files.is_empty() {
                    println!("{}", meta_files.join("\n"));
                }
            }
            CacheCommand::Delete { packages } => Self::delete(config, &packages)?,
            CacheCommand::View { package } => Self::view(config, &cache_dir, &package)?,
        }

        Ok(())
    }
    fn view(config: &Config, cache_dir: &Path, package: &str) -> miette::Result<()> {
        if !cache_dir.exists() {
            println!("{{}}");
            return Ok(());
        }
        Self::reject_path_traversal(package)?;
        let registry_prefix = Self::registry_prefix(config)?;
        // pnpm matches the package name literally as a glob segment.
        let mut meta_file_paths =
            walk_metadata_files(cache_dir, &format!("{registry_prefix}/{package}.jsonl"))?;
        meta_file_paths.sort();

        // pnpm's cacheView opens a writable StoreIndex that creates
        // index.db when absent, so a fresh/empty store reports every
        // version as non-cached rather than erroring.
        // `shared_readonly_in` returns None when index.db does not exist,
        // which we treat the same way: every lookup is a miss.
        let store_index = StoreIndex::shared_readonly_in(&config.store_dir);
        let store_index =
            store_index.as_ref().map(|index| index.lock().expect("store index mutex"));

        // IndexMap preserves insertion order so the JSON key order is
        // deterministic (driven by the sorted file paths), matching pnpm's
        // plain-object output.
        let mut meta_files_by_path = IndexMap::new();
        for (file_path, full_path) in meta_file_paths {
            let Some(meta_object) = load_meta(&full_path) else { continue };
            let (cached_versions, non_cached_versions) =
                split_cached_versions(&meta_object, store_index.as_deref());

            // The output groups versions per registry.
            meta_files_by_path.insert(
                decode_registry_name(&cache_registry_name(&file_path)),
                json!({
                    "cachedVersions": cached_versions,
                    "nonCachedVersions": non_cached_versions,
                    "cachedAt": cached_at(&full_path),
                    "distTags": meta_object.dist_tags,
                }),
            );
        }

        println!("{}", serde_json::to_string_pretty(&meta_files_by_path).into_diagnostic()?);
        Ok(())
    }

    /// A package's metadata can be cached under any of the metadata
    /// directories depending on the resolution mode used when it was
    /// fetched, so delete from all of them, not only the one the current
    /// mode reads.
    fn delete(config: &Config, packages: &[String]) -> miette::Result<()> {
        let mut deleted: Vec<String> = Vec::new();
        for meta_dir in [ABBREVIATED_META_DIR, FULL_META_DIR, FULL_FILTERED_META_DIR] {
            let dir = config.cache_dir.join(meta_dir);
            if !dir.exists() {
                continue;
            }
            let meta_files = Self::find_metadata_files(config, &dir, packages)?;
            for meta_file in &meta_files {
                fs::remove_file(dir.join(meta_file)).into_diagnostic()?;
            }
            deleted.extend(meta_files);
        }
        deleted.sort();
        deleted.dedup();
        if !deleted.is_empty() {
            println!("{}", deleted.join("\n"));
        }
        Ok(())
    }
}

/// The registry directory of a cache-relative metadata path: its top-level
/// component. For scoped packages the file lives one level deeper
/// (`<registry>/@scope/name.jsonl`), so `parent()` would be wrong. Mirrors
/// pnpm's cacheView walk to the top-most dir.
fn cache_registry_name(file_path: &str) -> String {
    Path::new(file_path).components().next().map_or_else(
        || ".".to_string(),
        |component| component.as_os_str().to_string_lossy().into_owned(),
    )
}

/// The metadata file's modification time as an RFC 3339 timestamp.
fn cached_at(full_path: &Path) -> Option<String> {
    let mtime = fs::metadata(full_path).and_then(|meta| meta.modified()).ok()?;
    Some(
        chrono::DateTime::<chrono::Utc>::from(mtime)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
}

/// Every cache file matching `pattern`, as `(cache-relative path with
/// POSIX separators, absolute path)` pairs.
fn walk_metadata_files(
    cache_dir: &Path,
    pattern: &str,
) -> miette::Result<Vec<(String, std::path::PathBuf)>> {
    let glob = wax::Glob::new(pattern).into_diagnostic()?;
    let mut matches = Vec::new();
    for entry in glob.walk(cache_dir).filter_map(std::result::Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(path_str) =
            entry.path().strip_prefix(cache_dir).ok().and_then(|path| path.to_str())
        {
            matches.push((path_str.replace('\\', "/"), entry.path().to_path_buf()));
        }
    }
    Ok(matches)
}

/// Split a cached packument's versions by whether the store holds the
/// tarball each one resolves to.
fn split_cached_versions(
    meta_object: &pnpm_registry::Package,
    store_index: Option<&StoreIndex>,
) -> (Vec<String>, Vec<String>) {
    let mut cached = Vec::new();
    let mut non_cached = Vec::new();
    for (version, json_frag) in meta_object.versions.fragments() {
        let Some(integrity) = version_integrity(json_frag.as_ref()) else { continue };
        let key = pnpm_store_dir::store_index_key(
            &integrity,
            &format!("{}@{}", meta_object.name, version),
        );
        let is_cached = store_index.is_some_and(|index| index.contains_key(&key).unwrap_or(false));
        if is_cached {
            cached.push(version.clone());
        } else {
            non_cached.push(version.clone());
        }
    }
    (cached, non_cached)
}

fn version_integrity(json_frag: &str) -> Option<String> {
    let manifest = serde_json::from_str::<serde_json::Value>(json_frag).ok()?;
    manifest.get("dist")?.get("integrity")?.as_str().map(ToOwned::to_owned)
}
