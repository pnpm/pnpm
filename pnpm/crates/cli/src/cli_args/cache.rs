use clap::Subcommand;
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pacquet_config::{Config, ResolutionMode};
use pacquet_resolving_npm_resolver::mirror::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, get_registry_name, load_meta,
};
use pacquet_store_dir::StoreIndex;
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
            let glob = wax::Glob::new(&pattern).into_diagnostic()?;
            for entry in glob.walk(cache_dir).filter_map(std::result::Result::ok) {
                if !entry.file_type().is_file() {
                    continue;
                }
                if let Some(path_str) =
                    entry.path().strip_prefix(cache_dir).ok().and_then(|path| path.to_str())
                {
                    matches.push(path_str.replace('\\', "/"));
                }
            }
        }
        matches.sort();
        matches.dedup();
        Ok(matches)
    }

    pub fn run(self, config: &Config) -> miette::Result<()> {
        let cache_dir = Self::cache_dir(config);

        match self {
            CacheCommand::ListRegistries => {
                if let Ok(entries) = fs::read_dir(&cache_dir) {
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
            CacheCommand::Delete { packages } => {
                if !cache_dir.exists() {
                    return Ok(());
                }
                let meta_files = Self::find_metadata_files(config, &cache_dir, &packages)?;
                for meta_file in &meta_files {
                    fs::remove_file(cache_dir.join(meta_file)).into_diagnostic()?;
                }
                if !meta_files.is_empty() {
                    println!("{}", meta_files.join("\n"));
                }
            }
            CacheCommand::View { package } => {
                if !cache_dir.exists() {
                    println!("{{}}");
                    return Ok(());
                }
                Self::reject_path_traversal(&package)?;
                let registry_prefix = Self::registry_prefix(config)?;
                // pnpm matches the package name literally as a glob segment.
                let pattern = format!("{registry_prefix}/{package}.jsonl");

                let glob = wax::Glob::new(&pattern).into_diagnostic()?;
                let mut meta_file_paths = Vec::new();
                for entry in glob.walk(&cache_dir).filter_map(std::result::Result::ok) {
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    if let Some(path_str) =
                        entry.path().strip_prefix(&cache_dir).ok().and_then(|path| path.to_str())
                    {
                        meta_file_paths
                            .push((path_str.replace('\\', "/"), entry.path().to_path_buf()));
                    }
                }
                meta_file_paths.sort();

                // IndexMap preserves insertion order so the JSON key order is
                // deterministic (driven by the sorted file paths), matching
                // pnpm's plain-object output.
                let mut meta_files_by_path = IndexMap::new();

                // pnpm's cacheView opens a writable StoreIndex that creates
                // index.db when absent, so a fresh/empty store reports every
                // version as non-cached rather than erroring. `shared_readonly_in`
                // returns None when index.db does not exist, which we treat the
                // same way: every lookup is a miss.
                let store_index = StoreIndex::shared_readonly_in(&config.store_dir);
                let store_index =
                    store_index.as_ref().map(|index| index.lock().expect("store index mutex"));

                for (file_path, full_path) in meta_file_paths {
                    let Some(meta_object) = load_meta(&full_path) else { continue };
                    let mtime = fs::metadata(&full_path).and_then(|meta| meta.modified()).ok();

                    let mut cached_versions = Vec::new();
                    let mut non_cached_versions = Vec::new();

                    for (version, json_frag) in meta_object.versions.fragments() {
                        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&json_frag)
                        else {
                            continue;
                        };
                        let Some(integrity) = manifest
                            .get("dist")
                            .and_then(|dist| dist.get("integrity"))
                            .and_then(|integrity_value| integrity_value.as_str())
                        else {
                            continue;
                        };

                        let key = pacquet_store_dir::store_index_key(
                            integrity,
                            &format!("{}@{}", meta_object.name, version),
                        );
                        let is_cached = store_index
                            .as_ref()
                            .is_some_and(|index| index.contains_key(&key).unwrap_or(false));
                        if is_cached {
                            cached_versions.push(version.clone());
                        } else {
                            non_cached_versions.push(version.clone());
                        }
                    }

                    // The output groups versions per registry. The registry
                    // directory is the top-level component of the cache-relative
                    // path; for scoped packages the file lives one level deeper
                    // (`<registry>/@scope/name.jsonl`), so `parent()` would be
                    // wrong. Mirrors pnpm's cacheView walk to the top-most dir.
                    let registry_name = Path::new(&file_path).components().next().map_or_else(
                        || ".".to_string(),
                        |component| component.as_os_str().to_string_lossy().into_owned(),
                    );

                    meta_files_by_path.insert(
                        registry_name.replace('+', ":"),
                        json!({
                            "cachedVersions": cached_versions,
                            "nonCachedVersions": non_cached_versions,
                            "cachedAt": mtime.map(|time| {
                            chrono::DateTime::<chrono::Utc>::from(time)
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                        }),
                            "distTags": meta_object.dist_tags,
                        }),
                    );
                }

                println!(
                    "{}",
                    serde_json::to_string_pretty(&meta_files_by_path).into_diagnostic()?,
                );
            }
        }

        Ok(())
    }
}
