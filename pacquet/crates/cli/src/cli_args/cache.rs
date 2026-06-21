use clap::Subcommand;
use miette::IntoDiagnostic;
use pacquet_config::{Config, ResolutionMode};
use pacquet_resolving_npm_resolver::mirror::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, encode_pkg_name,
    get_registry_name, load_meta,
};
use pacquet_store_dir::StoreIndex;
use serde_json::json;
use std::{collections::HashMap, fs, path::PathBuf};
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
    fn cache_dir(config: &Config) -> PathBuf {
        let meta_dir = if config.resolution_mode == ResolutionMode::TimeBased
            && !config.registry_supports_time_field
        {
            FULL_FILTERED_META_DIR
        } else {
            ABBREVIATED_META_DIR
        };
        config.cache_dir.join(meta_dir)
    }

    fn find_metadata_files(
        config: &Config,
        cache_dir: &std::path::Path,
        packages: &[String],
    ) -> miette::Result<Vec<String>> {
        let registry_prefix =
            get_registry_name(&config.registry).unwrap_or_else(|_| "*".to_string());

        let patterns = if packages.is_empty() {
            vec![format!("{}/**", registry_prefix)]
        } else {
            packages
                .iter()
                .map(|p| format!("{}/{}.jsonl", registry_prefix, encode_pkg_name(p)))
                .collect()
        };

        let mut matches = Vec::new();
        for pattern in patterns {
            let glob = wax::Glob::new(&pattern).into_diagnostic()?;
            for entry in glob.walk(cache_dir).filter_map(std::result::Result::ok) {
                if let Some(path_str) =
                    entry.path().strip_prefix(cache_dir).ok().and_then(|p| p.to_str())
                {
                    matches.push(path_str.replace('\\', "/"));
                }
            }
        }
        matches.sort();
        matches.dedup();
        Ok(matches)
    }

    pub fn run<'a>(
        self,
        config: impl FnOnce() -> miette::Result<&'a Config>,
    ) -> miette::Result<()> {
        let config = config()?;
        let cache_dir = Self::cache_dir(config);

        match self {
            CacheCommand::ListRegistries => {
                if let Ok(entries) = fs::read_dir(&cache_dir) {
                    let mut registries: Vec<String> = entries
                        .filter_map(std::result::Result::ok)
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
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
                    let file_path = cache_dir.join(meta_file);
                    let _ = fs::remove_file(&file_path);

                    // Also attempt to delete from other meta directories just in case
                    if let Ok(rel_path) = file_path.strip_prefix(&cache_dir) {
                        let _ = fs::remove_file(
                            config.cache_dir.join(ABBREVIATED_META_DIR).join(rel_path),
                        );
                        let _ =
                            fs::remove_file(config.cache_dir.join(FULL_META_DIR).join(rel_path));
                        let _ = fs::remove_file(
                            config.cache_dir.join(FULL_FILTERED_META_DIR).join(rel_path),
                        );
                    }
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
                let encoded_name = encode_pkg_name(&package);
                // Note: we bypass find_metadata_files to search purely by exact filename because find_metadata_files encodes again
                let registry_prefix =
                    get_registry_name(&config.registry).unwrap_or_else(|_| "*".to_string());
                let pattern = format!("{registry_prefix}/{encoded_name}.jsonl");

                let glob = wax::Glob::new(&pattern).into_diagnostic()?;
                let mut meta_file_paths = Vec::new();
                for entry in glob.walk(&cache_dir).filter_map(std::result::Result::ok) {
                    if let Some(path_str) =
                        entry.path().strip_prefix(&cache_dir).ok().and_then(|p| p.to_str())
                    {
                        meta_file_paths
                            .push((path_str.replace('\\', "/"), entry.path().to_path_buf()));
                    }
                }
                meta_file_paths.sort();

                let mut meta_files_by_path = HashMap::new();

                let store_index =
                    StoreIndex::open_readonly_in(&config.store_dir).into_diagnostic()?;

                for (file_path, full_path) in meta_file_paths {
                    let Some(meta_object) = load_meta(&full_path) else { continue };
                    let mtime = fs::metadata(&full_path).and_then(|m| m.modified()).ok();

                    let mut cached_versions = Vec::new();
                    let mut non_cached_versions = Vec::new();

                    for (version, json_frag) in meta_object.versions.fragments() {
                        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&json_frag)
                        else {
                            continue;
                        };
                        let Some(integrity) = manifest
                            .get("dist")
                            .and_then(|d| d.get("integrity"))
                            .and_then(|i| i.as_str())
                        else {
                            continue;
                        };

                        let key = pacquet_store_dir::store_index_key(
                            integrity,
                            &format!("{}@{}", meta_object.name, version),
                        );
                        if store_index.contains_key(&key).unwrap_or(false) {
                            cached_versions.push(version.clone());
                        } else {
                            non_cached_versions.push(version.clone());
                        }
                    }

                    let registry_name = PathBuf::from(&file_path)
                        .parent()
                        .map_or_else(|| ".".to_string(), |p| p.to_string_lossy().into_owned());

                    meta_files_by_path.insert(
                        registry_name.replace('+', ":"),
                        json!({
                            "cachedVersions": cached_versions,
                            "nonCachedVersions": non_cached_versions,
                            "cachedAt": mtime.map(|t| format!("{t:?}")),
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
