use clap::Subcommand;
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pnpm_config::{Config, ResolutionMode};
use pnpm_fs::lexical_normalize;
use pnpm_resolving_npm_resolver::mirror::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, decode_registry_name,
    get_registry_name, is_unreadable_registry_key, load_meta,
};
use pnpm_store_dir::StoreIndex;
use serde_json::json;
use std::{
    fs, io,
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
    /// Deletes registry metadata cache directories that this version of pnpm
    /// can no longer read. Leaves the descriptor-scoped caches under
    /// `metadata-private` alone, as the other subcommands do.
    Prune {
        /// Lists what would be deleted without removing anything.
        #[arg(long)]
        dry_run: bool,
    },
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
                    .map(|entry| decode_registry_name(&entry.file_name().to_string_lossy()))
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
            CacheCommand::Prune { dry_run } => Self::prune(config, dry_run)?,
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
        let store_index = store_index
            .as_ref()
            .map(|index| index.lock().expect("store index mutex"));

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

    /// Remove the mirror directories left behind by a pnpm that keyed them on
    /// the registry's host alone.
    ///
    /// Changing the key to carry the scheme and path stranded every directory
    /// written before it: the same registry now resolves to a different name,
    /// so the old one is never read and no per-package command reaches it,
    /// because those all scope their glob to the configured registry's current
    /// key. Only [`is_unreadable_registry_key`] decides what goes, so a mirror
    /// this version could still read is never a candidate.
    ///
    /// Prints each removed directory as `<meta-dir>/<registry-key>`, the
    /// registry key rather than its decoded URL because that is the name on
    /// disk. `dry_run` prints the same list and removes nothing.
    ///
    /// A root that cannot be read or a directory that cannot be removed does
    /// not abandon the remaining roots, and every such failure is reported
    /// rather than only the first, so one unwritable directory does not turn
    /// reclaiming a cache into a rerun for each of them. The directories that
    /// did go are still printed.
    ///
    /// The descriptor-scoped roots under `v11/metadata-private` are left alone,
    /// as every other `pnpm cache` subcommand leaves them alone.
    ///
    /// Every root is resolved against the cache directory before anything is
    /// removed, so the sweep stays inside the tree the configuration names. See
    /// [`confined_meta_root`].
    fn prune(config: &Config, dry_run: bool) -> miette::Result<()> {
        let mut outcome = PruneOutcome::default();
        match dunce::canonicalize(&config.cache_dir) {
            Ok(cache_dir) => {
                for meta_dir in [ABBREVIATED_META_DIR, FULL_META_DIR, FULL_FILTERED_META_DIR] {
                    outcome.prune_root(&cache_dir, meta_dir, dry_run);
                }
            }
            // No cache directory at all is nothing to reclaim, as an absent
            // root is.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => outcome.failures.push(format!(
                "Failed to resolve cache directory {:?}: {error}",
                config.cache_dir,
            )),
        }
        outcome.report(dry_run)
    }
}

/// What one `pnpm cache prune` managed and what it could not.
#[derive(Default)]
struct PruneOutcome {
    pruned: Vec<String>,
    failures: Vec<String>,
}

impl PruneOutcome {
    /// Reclaim the `meta_dir` root of the resolved `cache_dir`.
    ///
    /// Failing to read the root is recorded rather than passed over, because
    /// silence would report that nothing here is stale when the directory was
    /// never read at all. The remaining roots are the caller's to walk either
    /// way.
    fn prune_root(&mut self, cache_dir: &Path, meta_dir: &str, dry_run: bool) {
        let root = match confined_meta_root(cache_dir, meta_dir) {
            Ok(None) => return,
            Ok(Some(root)) => root,
            Err(error) => return self.failures.push(error),
        };
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) => {
                return self.failures.push(format!(
                    "Failed to read metadata cache directory {root:?}: {error}",
                ));
            }
        };
        for entry in entries {
            self.prune_entry(entry, meta_dir, dry_run);
        }
    }

    /// Reclaim one directory of a mirror root, if it is one this version can no
    /// longer read.
    ///
    /// An entry that cannot be read or typed is recorded rather than skipped,
    /// for the same reason an unreadable root is: on a filesystem that reports
    /// no type up front, `file_type` is the `lstat` that fails, and swallowing
    /// it would leave the whole root looking empty.
    fn prune_entry(&mut self, entry: io::Result<fs::DirEntry>, meta_dir: &str, dry_run: bool) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return self.failures.push(format!(
                    "Failed to read a metadata cache entry: {error}",
                ));
            }
        };
        let path = entry.path();
        match entry.file_type() {
            Ok(file_type) if !file_type.is_dir() => return,
            Ok(_) => {}
            Err(error) => {
                return self.failures.push(format!(
                    "Failed to inspect metadata cache entry {path:?}: {error}",
                ));
            }
        }
        let registry_key = entry
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !is_unreadable_registry_key(&registry_key) {
            return;
        }
        match remove_pruned_dir(&path, dry_run) {
            Ok(()) => self.pruned.push(format!("{meta_dir}/{registry_key}")),
            Err(error) => self.failures.push(error),
        }
    }

    /// Print the reclaimed directories on stdout, and fail if anything could
    /// not be reclaimed.
    ///
    /// When every removal succeeds a dry run's stdout matches the real thing
    /// byte for byte, so the two can be diffed and the list piped onward. The
    /// notice that nothing was deleted goes to stderr, where it reaches a reader
    /// without entering that list.
    ///
    /// A dry run always prints that notice, a count of zero included: silence is
    /// how a real prune says it found nothing, and the mode that exists to answer
    /// "is there anything here" has to answer out loud.
    ///
    /// Each failure gets its own stderr line and the returned error only counts
    /// them, because a diagnostic long enough to hold several paths comes back
    /// reflowed and guttered, splitting the paths it exists to report.
    fn report(mut self, dry_run: bool) -> miette::Result<()> {
        self.pruned.sort();
        if dry_run {
            let count = directory_count(self.pruned.len());
            eprintln!("Dry run: {count} would be deleted.");
        }
        if !self.pruned.is_empty() {
            println!("{}", self.pruned.join("\n"));
        }
        if self.failures.is_empty() {
            return Ok(());
        }
        for failure in &self.failures {
            eprintln!("{failure}");
        }
        let count = directory_count(self.failures.len());
        Err(miette::miette!("Failed to reclaim {count}"))
    }
}

/// The metadata root to sweep, resolved, once it is known to lie inside
/// `cache_dir`. `Ok(None)` when there is no such root, `Err` with the message to
/// report when it cannot be resolved or resolves outside.
///
/// `cacheDir` is a `pnpm-workspace.yaml` setting, so a checked-out project
/// chooses where this command deletes from. `read_dir` follows a symlinked root,
/// which would put every directory behind the link in reach of
/// `remove_dir_all` — and the names prune accepts are broad, being every name
/// that is not in the current key shape. Requiring the root to resolve inside
/// the cache directory keeps the sweep in the tree the configuration names.
///
/// The resolved path is what gets swept, so a link swapped in after the check
/// cannot redirect the removals either. This is the containment
/// `pnpm_deps_restorer`'s `confined_modules_dir` applies before its own sweep.
///
/// A `cacheDir` pointing somewhere unwelcome outright is not this check's to
/// catch: the sweep is then inside the configured directory, which is the
/// contract every `pnpm cache` subcommand already works to.
fn confined_meta_root(cache_dir: &Path, meta_dir: &str) -> Result<Option<PathBuf>, String> {
    let named = cache_dir.join(meta_dir);
    let root = match dunce::canonicalize(&named) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("Failed to resolve metadata cache directory {named:?}: {error}"));
        }
    };
    if !root.starts_with(cache_dir) {
        return Err(format!(
            "Refusing to prune {named:?}: it resolves to {root:?}, outside the cache directory {cache_dir:?}",
        ));
    }
    Ok(Some(root))
}

/// `count` with the noun it agrees with, so a single stale directory does not
/// report as "1 directories".
fn directory_count(count: usize) -> String {
    if count == 1 {
        return "1 directory".to_string();
    }
    format!("{count} directories")
}

/// `Err` carries the message to report, naming the directory, because a bare
/// `Permission denied` leaves the user nothing to act on.
///
/// A directory already gone counts as reclaimed. The cache is shared, so a
/// second prune, or one racing `pnpm store prune` from pnpm v11 over the same
/// `v11` tree, must not fail for having got what it wanted.
fn remove_pruned_dir(path: &Path, dry_run: bool) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove metadata cache directory {path:?}: {error}")),
    }
}

/// The registry directory of a cache-relative metadata path: its top-level
/// component. For scoped packages the file lives one level deeper
/// (`<registry>/@scope/name.jsonl`), so `parent()` would be wrong. Mirrors
/// pnpm's cacheView walk to the top-most dir.
fn cache_registry_name(file_path: &str) -> String {
    Path::new(file_path)
        .components()
        .next()
        .map_or_else(
            || ".".to_string(),
            |component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned()
            },
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
        if let Some(path_str) = entry
            .path()
            .strip_prefix(cache_dir)
            .ok()
            .and_then(|path| path.to_str())
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
    manifest
        .get("dist")?
        .get("integrity")?
        .as_str()
        .map(ToOwned::to_owned)
}
