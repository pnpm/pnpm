use super::{
    BTreeMap, CatalogAnchor, CatalogResolutionResult, CatalogWantedDependency, Config, Context,
    DependencyGroup, DlxError, Duration, IntoDiagnostic, Path, PathBuf, RangeSpecStyle, Reporter,
    State, SupportedArchitectures, SupportedArchitecturesArgs, SystemTime, UNIX_EPOCH, Value,
    add_package, configured_catalogs, create_short_hash, force_symlink_dir, fs, json,
    parse_catalog_protocol, parse_manifest, parse_overrides_iter, parse_wanted_dependency,
    resolve_from_catalog,
};

/// Install the packages into a fresh prepare directory and point the
/// cache link at it.
pub(super) async fn prepare_cache_dir<Reporter: self::Reporter + 'static>(
    dlx_command_cache_dir: &Path,
    cache_link: &Path,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
    config: &'static mut Config,
) -> miette::Result<PathBuf> {
    let prepare_dir = get_prepare_dir(dlx_command_cache_dir, SystemTime::now(), std::process::id());
    if let Err(error) = install_into_cache::<Reporter>(
        &prepare_dir,
        pkgs,
        allow_build,
        supported_architectures,
        config,
    )
    .await
    {
        // Don't leave a half-installed prepare dir behind to accumulate
        // across failed runs: remove it on install failure. Best-effort
        // cleanup.
        let _ = fs::remove_dir_all(&prepare_dir);
        return Err(error);
    }
    // Best-effort: a parallel dlx process may have raced us to the link.
    // Either link is equally fresh, so ignore the failure and run from
    // our own prepare dir.
    let _ = force_symlink_dir(&prepare_dir, cache_link);
    Ok(prepare_dir)
}

/// Install `pkgs` into `prepare_dir` so their bins land in
/// `<prepare_dir>/node_modules/.bin`. Anchors `config` at the cache
/// directory (the `dir` / `lockfileDir` / `bin` overrides) and saves each
/// package to `dependencies`.
async fn install_into_cache<Reporter: self::Reporter + 'static>(
    prepare_dir: &Path,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
    config: &'static mut Config,
) -> miette::Result<()> {
    fs::create_dir_all(prepare_dir)
        .map_err(|source| DlxError::Cache { dir: prepare_dir.display().to_string(), source })?;
    let manifest_path = prepare_dir.join("package.json");
    fs::write(&manifest_path, json!({ "name": "dlx", "version": "0.0.0" }).to_string())
        .map_err(|source| DlxError::Cache { dir: manifest_path.display().to_string(), source })?;

    configure_cache_install(config, prepare_dir, pkgs, allow_build, supported_architectures)?;
    let config: &Config = config;

    for pkg in pkgs {
        let state = State::init(manifest_path.clone(), config, false)
            .wrap_err("initialize the dlx install state")?;
        add_package::<Reporter, _>(
            state,
            pkg,
            // dlx records the default caret range; the spec is throwaway.
            RangeSpecStyle::default(),
            // dlx never catalogs.
            None,
            // dlx must download to run the bin, so never lockfile-only.
            false,
            config.supported_architectures.clone(),
            [DependencyGroup::Prod],
        )
        .await?;
    }
    crate::cli_args::approve_builds::prompt_approve_install_builds::<Reporter>(
        config,
        prepare_dir,
        prepare_dir,
    )
    .await?;
    Ok(())
}

/// The command's cache directory, created and canonicalized so the prepare
/// dir carries no `..` segments. A relative `cacheDir` (e.g. `../pnpm-cache`)
/// would otherwise let the install's workspace-root walk pass through the
/// caller's project dir and mistake it for the dlx workspace.
pub(super) fn dlx_command_cache_dir(config: &Config, cache_key: &str) -> miette::Result<PathBuf> {
    let dlx_command_cache_dir = config.cache_dir.join("dlx").join(cache_key);
    fs::create_dir_all(&dlx_command_cache_dir)
        .map_err(|source| DlxError::Cache {
            dir: dlx_command_cache_dir.display().to_string(),
            source,
        })?;
    dunce::canonicalize(&dlx_command_cache_dir)
        .into_diagnostic()
        .wrap_err("canonicalizing the dlx cache directory")
}

/// Replace the `catalog:` specifier of each dlx package spec with the
/// specifier the caller's catalogs hold. Any other spec passes through
/// untouched, and the catalogs are only read when at least one spec needs
/// them. A misconfigured entry is reported as the pnpm error the caller
/// would get from `pnpm add`.
///
/// A `file:` / `link:` entry becomes an absolute path: dlx installs into
/// a cache directory outside the workspace, so nothing there can read a
/// path measured from `pnpm-workspace.yaml`.
pub(super) fn resolve_catalog_specs(
    pkgs: &[String],
    config: &Config,
) -> miette::Result<Vec<String>> {
    let uses_catalog = |pkg: &String| {
        parse_wanted_dependency(pkg).bare_specifier
            .is_some_and(|bare_specifier| parse_catalog_protocol(&bare_specifier).is_some())
    };
    if !pkgs.iter().any(uses_catalog) {
        return Ok(pkgs.to_vec());
    }
    let catalogs = configured_catalogs(config)?;
    let anchor = match config.workspace_dir.as_deref() {
        Some(workspace_dir) => CatalogAnchor::Reanchor { workspace_dir, consumer_dir: None },
        None => CatalogAnchor::AsWritten,
    };
    pkgs.iter()
        .map(|pkg| {
            let parsed = parse_wanted_dependency(pkg);
            let (Some(alias), Some(bare_specifier)) = (parsed.alias, parsed.bare_specifier) else {
                return Ok(pkg.clone());
            };
            let wanted = CatalogWantedDependency { alias: alias.clone(), bare_specifier };
            match resolve_from_catalog(&catalogs, &wanted, anchor) {
                CatalogResolutionResult::Found(found) => {
                    Ok(format!("{alias}@{}", found.resolution.specifier))
                }
                CatalogResolutionResult::Unused => Ok(pkg.clone()),
                CatalogResolutionResult::Misconfiguration(misconfiguration) => {
                    Err(miette::Report::new(misconfiguration.error))
                }
            }
        })
        .collect()
}

/// Build the `{ "default": registry, <alias>: url, … }` map fed into the
/// cache key.
fn build_registries_map(config: &Config) -> BTreeMap<String, String> {
    let mut map = config.resolved_registries();
    for (name, url) in &config.registries_by_prefix {
        map.insert(name.clone(), url.clone());
    }
    map
}

/// Build the dlx cache key from the sorted package specs, sorted
/// registries, the optional `allow_build` list, and each non-empty
/// `supportedArchitectures` axis (deduped + sorted, in `cpu` / `libc` /
/// `os` order), all hashed together. pacquet keys on the raw specs (not
/// resolved ids) and uses [`create_short_hash`] rather than a full-length
/// hex digest; the dlx caches are not shared between the two
/// implementations, so the key format is not a cross-tool contract.
pub(super) fn create_cache_key(
    pkgs: &[String],
    registries: &BTreeMap<String, String>,
    allow_build: &[String],
    supported_architectures: Option<&SupportedArchitectures>,
) -> String {
    let mut sorted: Vec<&str> = pkgs
        .iter()
        .map(String::as_str)
        .collect();
    sorted.sort_unstable();
    let registry_pairs: Vec<(&str, &str)> = registries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut args = vec![json!(sorted), json!(registry_pairs)];
    if !allow_build.is_empty() {
        let mut sorted_allow: Vec<&str> = allow_build
            .iter()
            .map(String::as_str)
            .collect();
        sorted_allow.sort_unstable();
        args.push(json!({ "allowBuild": sorted_allow }));
    }
    args.extend(architecture_key_inputs(supported_architectures));
    create_short_hash(&serde_json::to_string(&args).expect("serialize cache key inputs"))
}

/// What the platforms an install prepares for contribute to its cache
/// key.
///
/// The axes are recorded one axis at a time, each named once and sorted,
/// since rewriting them in another order asks for the same install.
///
/// A platform list is recorded as the platforms it resolves to, in the
/// order it named them. The order matters because which platform comes
/// first decides which runtime archive the install takes when this
/// machine is none of them, and resolving matters because `current` is a
/// different platform on each machine while the word is the same.
fn architecture_key_inputs(supported: Option<&SupportedArchitectures>) -> Vec<Value> {
    match supported {
        None => Vec::new(),
        Some(SupportedArchitectures::Axes(axes)) => {
            [("cpu", &axes.cpu), ("libc", &axes.libc), ("os", &axes.os)]
                .into_iter()
                .filter_map(|(key, values)| {
                    let values = values
                        .as_ref()
                        .filter(|values| !values.is_empty())?;
                    Some(json!({ "supportedArchitectures": { key: sorted_once(values) } }))
                })
                .collect()
        }
        Some(supported @ SupportedArchitectures::Platforms(_)) => {
            let named: Vec<String> = supported
                .host_platforms()
                .iter()
                .map(ToString::to_string)
                .collect();
            vec![json!({ "supportedArchitectures": named })]
        }
    }
}

/// The values as the cache key records them: named once, in an order
/// rewriting the configuration cannot change.
fn sorted_once(values: &[String]) -> Vec<&str> {
    let mut deduped: Vec<&str> = values
        .iter()
        .map(String::as_str)
        .collect();
    deduped.sort_unstable();
    deduped.dedup();
    deduped
}

/// Return the cache target behind `cache_link` when it is a symlink whose
/// own mtime is within `max_age_minutes` of `now`.
pub(super) fn get_valid_cache_dir(
    cache_link: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(cache_link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    // `dunce::canonicalize` (not `fs::canonicalize`) so the cache-hit path
    // matches the fresh-install branch's form — on Windows `fs::canonicalize`
    // returns a `\\?\` verbatim path that would feed a different
    // `node_modules/.bin` string into `PATH`.
    let target = dunce::canonicalize(cache_link).ok()?;
    let mtime = meta.modified().ok()?;
    let max_age = Duration::from_secs(max_age_minutes.saturating_mul(60));
    // Valid while `mtime + max_age >= now`. A negative elapsed time
    // (clock skew, `now` before `mtime`) is treated as still valid,
    // matching pnpm's numeric comparison.
    match now.duration_since(mtime) {
        Ok(age) => (age <= max_age).then_some(target),
        Err(_) => Some(target),
    }
}

/// The timestamped, pid-scoped subdirectory a fresh dlx install is
/// prepared in.
pub(super) fn get_prepare_dir(cache_path: &Path, now: SystemTime, pid: u32) -> PathBuf {
    let millis = now.duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis());
    // base36 (vs hex) keeps this segment short: it sits between the cache key
    // and pnpm's deep virtual-store layout, and long dlx paths overflow
    // Windows' MAX_PATH (260), which makes lifecycle scripts fail with a
    // `spawn cmd.exe ENOENT` (the cwd no longer resolves). time+pid stays
    // unique across concurrent dlx processes and a process's own retries.
    cache_path.join(format!("{}-{}", to_base36(millis), to_base36(u128::from(pid))))
}

/// Lowercase base36 (`0-9a-z`), matching JavaScript's
/// `Number.prototype.toString(36)` used by `getPrepareDir`.
fn to_base36(mut n: u128) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ASCII")
}

pub(super) fn read_json(path: &Path) -> Result<Value, DlxError> {
    let text = fs::read_to_string(path)
        .map_err(|source| DlxError::ReadManifest { path: path.display().to_string(), source })?;
    parse_manifest(&text)
        .map_err(|error| DlxError::ReadManifest {
            path: path.display().to_string(),
            source: error.into(),
        })
}

/// Remove the expired `pnpm dlx` cache entries below `<cacheDir>/dlx`.
///
/// Pacquet's port of pnpm v11's `cleanExpiredDlxCache`, which runs as part
/// of `pnpm store prune`: without it, every prepare directory a `pnpm dlx`
/// run supersedes stays on disk forever. A cache-key directory is removed
/// wholesale when its `pkg` link is older than `dlx_cache_max_age` minutes
/// (a `0` max age removes every entry without consulting mtimes); orphaned
/// prepare directories that no link points at are swept as well. A missing
/// `dlx` directory is not an error.
pub(crate) fn clean_expired_dlx_cache(
    cache_dir: &Path,
    dlx_cache_max_age: u64,
    now: SystemTime,
) -> miette::Result<()> {
    let dlx_cache_dir = cache_dir.join("dlx");
    let entries = match fs::read_dir(&dlx_cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("reading the dlx cache directory {}", dlx_cache_dir.display()));
        }
    };
    for entry in entries {
        let entry = entry
            .into_diagnostic()
            .wrap_err(format!(
                "reading an entry of the dlx cache directory {}",
                dlx_cache_dir.display()
            ))?;
        if !entry
            .file_type()
            .into_diagnostic()
            .wrap_err(format!("inspecting dlx cache entry {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let key_dir = entry.path();
        if dlx_cache_max_age == 0
            || cache_link_is_outdated(&key_dir.join("pkg"), dlx_cache_max_age, now)?
        {
            fs::remove_dir_all(&key_dir)
                .into_diagnostic()
                .wrap_err(format!("removing expired dlx cache entry {}", key_dir.display()))?;
        }
    }
    clean_orphans(&dlx_cache_dir)
}

/// Whether the `pkg` link's mtime is older than `max_age_minutes`. A
/// missing link is not outdated here: [`clean_orphans`] removes key
/// directories left behind without a link. A link newer than `now` (clock
/// skew) counts as fresh, matching the cache-hit check in
/// [`get_valid_cache_dir`].
fn cache_link_is_outdated(
    cache_link: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> miette::Result<bool> {
    let meta = match fs::symlink_metadata(cache_link) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("reading dlx cache link {}", cache_link.display()));
        }
    };
    let mtime = meta
        .modified()
        .into_diagnostic()
        .wrap_err(format!("reading mtime of dlx cache link {}", cache_link.display()))?;
    let max_age = Duration::from_secs(max_age_minutes.saturating_mul(60));
    Ok(now
        .duration_since(mtime)
        .is_ok_and(|age| age > max_age))
}

/// Sweep what the expiry pass leaves behind: key directories without a
/// `pkg` link are removed entirely, and prepare directories the link no
/// longer points at are removed from the key directories that keep one.
fn clean_orphans(dlx_cache_dir: &Path) -> miette::Result<()> {
    let entries = match fs::read_dir(dlx_cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("reading the dlx cache directory {}", dlx_cache_dir.display()));
        }
    };
    for entry in entries {
        let entry = entry
            .into_diagnostic()
            .wrap_err(format!(
                "reading an entry of the dlx cache directory {}",
                dlx_cache_dir.display()
            ))?;
        if !entry
            .file_type()
            .into_diagnostic()
            .wrap_err(format!("inspecting dlx cache entry {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let key_dir = entry.path();
        let cache_link = key_dir.join("pkg");
        match fs::symlink_metadata(&cache_link) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::remove_dir_all(&key_dir)
                    .into_diagnostic()
                    .wrap_err(format!("removing orphaned dlx cache entry {}", key_dir.display()))?;
                continue;
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err(format!("reading dlx cache link {}", cache_link.display()));
            }
        }
        let link_target = match dunce::canonicalize(&cache_link) {
            Ok(target) => Some(target),
            // A dangling link names nothing to keep: every prepare
            // directory beside it is an orphan.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err(format!("resolving dlx cache link {}", cache_link.display()));
            }
        };
        let children = fs::read_dir(&key_dir)
            .into_diagnostic()
            .wrap_err(format!("reading dlx cache entry {}", key_dir.display()))?;
        for child in children {
            let child = child
                .into_diagnostic()
                .wrap_err(format!("reading an entry of dlx cache entry {}", key_dir.display()))?;
            if child.file_name().as_os_str() == "pkg" {
                continue;
            }
            if link_target.as_deref() == Some(child.path().as_path()) {
                continue;
            }
            fs::remove_dir_all(child.path())
                .into_diagnostic()
                .wrap_err(format!(
                    "removing orphaned dlx prepare directory {}",
                    child.path().display()
                ))?;
        }
    }
    Ok(())
}

/// Resolve caller catalogs before the cache install severs its workspace anchor.
fn resolve_cache_overrides(config: &mut Config) -> miette::Result<()> {
    if let Some(overrides) = config.overrides.as_ref()
        && config.workspace_dir.is_some()
        && overrides
            .values()
            .any(|spec| spec.starts_with("catalog:"))
    {
        let catalogs = configured_catalogs(config)?;
        let resolved = parse_overrides_iter(overrides.iter(), &catalogs)
            .map_err(miette::Report::new)?
            .into_iter()
            .map(|entry| (entry.selector, entry.new_bare_specifier))
            .collect();
        config.overrides = Some(resolved);
    }
    Ok(())
}

pub(super) fn command_cache_dir(
    config: &Config,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
) -> miette::Result<PathBuf> {
    dlx_command_cache_dir(
        config,
        &create_cache_key(
            pkgs,
            &build_registries_map(config),
            allow_build,
            supported_architectures.apply_to(config.supported_architectures.clone()).as_ref(),
        ),
    )
}

/// Only requested packages and explicit CLI additions may run builds in the cache install.
fn apply_dlx_build_policy(config: &mut Config, pkgs: &[String], allow_build: &[String]) {
    config.dangerously_allow_all_builds = false;
    config.allow_builds.clear();
    for spec in pkgs {
        if let Some(alias) = parse_wanted_dependency(spec).alias {
            config.allow_builds.insert(alias, true);
        }
    }
    for name in allow_build {
        config.allow_builds.insert(name.clone(), true);
    }
}

pub(super) fn configure_cache_install(
    config: &mut Config,
    prepare_dir: &Path,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
) -> miette::Result<()> {
    // Per-axis CLI overrides (`--cpu` / `--os` / `--libc`) replace the
    // matching axis of the config-derived value for the dlx install.
    config.supported_architectures =
        supported_architectures.apply_to(config.supported_architectures.clone());

    config.modules_dir = prepare_dir.join("node_modules");
    config.virtual_store_dir = prepare_dir.join("node_modules").join(".pacquet");
    // Force the project-local virtual store so the whole prepare dir is
    // self-contained and can be symlinked as the cache entry. This is a
    // deliberate deviation from pnpm's dlx, which keeps
    // `enableGlobalVirtualStore ?? true`: pnpm caches only the
    // `node_modules` tree and lets the global store back it, whereas
    // pacquet symlinks the entire prepare dir, so its store must live
    // inside that dir (the installer picks `global_virtual_store_dir`
    // when this is on — see virtual_store_layout.rs).
    config.enable_global_virtual_store = false;
    // Keep a cache-local lockfile so approved builds can be rebuilt.
    config.lockfile = true;
    resolve_cache_overrides(config)?;
    // The throwaway cache project is not part of the caller's
    // workspace. If a caller has a settings-only pnpm-workspace.yaml,
    // carrying its workspace root here makes the install enumerate that
    // workspace and fail on the missing root package.json. Anchored
    // rather than `None`, which walks up from the cache dir and can
    // adopt a stray `pnpm-workspace.yaml` above it (pnpm/pnpm#13697).
    config.workspace_dir = Some(prepare_dir.to_path_buf());
    // Same reasoning for a pinned `lockfileDir`: it names the caller's
    // lockfile, which the throwaway install must not touch.
    config.lockfile_dir = None;
    // The caller's patches never apply to the throwaway install (pnpm's
    // dlx installs the package unpatched too). Their paths are relative
    // to the caller's workspace root, which the anchor above replaced, so
    // keeping them would also make every dlx invocation from a project
    // with `patchedDependencies` fail on a patch file missing under the
    // cache dir.
    config.patched_dependencies = None;
    // Build a *fresh* allow-list for the throwaway install — the dlx
    // packages themselves plus the CLI `--allow-build` entries — rather
    // than inheriting the caller project's `allow_builds` /
    // `dangerously_allow_all_builds`. Inheriting the caller's policy would
    // run build scripts the dlx invocation never opted into, and would
    // also leave the cache key (which hashes only pkgs + CLI allow_build)
    // unable to distinguish two callers with different policies.
    apply_dlx_build_policy(config, pkgs, allow_build);
    config.strict_dep_builds = false;
    Ok(())
}

pub(super) async fn get_or_prepare_cache<Reporter: self::Reporter + 'static>(
    config: &'static mut Config,
    pkgs: &[String],
    allow_build: &[String],
    supported_architectures: &SupportedArchitecturesArgs,
) -> miette::Result<PathBuf> {
    let command_dir = command_cache_dir(config, pkgs, allow_build, supported_architectures)?;
    let cache_link = command_dir.join("pkg");
    match get_valid_cache_dir(&cache_link, config.dlx_cache_max_age, SystemTime::now()) {
        Some(cached_dir) if cached_dir.join("pnpm-lock.yaml").is_file() => {
            configure_cache_install(
                config,
                &cached_dir,
                pkgs,
                allow_build,
                supported_architectures,
            )?;
            crate::cli_args::approve_builds::prompt_approve_install_builds::<Reporter>(
                config,
                &cached_dir,
                &cached_dir,
            )
            .await?;
            Ok(cached_dir)
        }
        _ => {
            prepare_cache_dir::<Reporter>(
                &command_dir,
                &cache_link,
                pkgs,
                allow_build,
                supported_architectures,
                config,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    fn key_dir_with_link(cache_dir: &Path, key: &str) -> PathBuf {
        let key_dir = cache_dir.join("dlx").join(key);
        let target = key_dir.join("prepare-1");
        fs::create_dir_all(&target).unwrap();
        symlink_dir(&target, &key_dir.join("pkg")).unwrap();
        key_dir
    }

    #[test]
    fn removes_expired_entries_and_keeps_fresh_ones() {
        let cache_dir = tempfile::tempdir().unwrap();
        let key_a = key_dir_with_link(cache_dir.path(), "key-a");
        let key_b = key_dir_with_link(cache_dir.path(), "key-b");

        let past_retention = SystemTime::now() + Duration::from_hours(48);
        clean_expired_dlx_cache(cache_dir.path(), 24 * 60, past_retention).unwrap();

        assert!(!key_a.exists(), "entry past the retention window should be removed");
        assert!(!key_b.exists(), "entry past the retention window should be removed");

        let key_a = key_dir_with_link(cache_dir.path(), "key-a");
        let key_b = key_dir_with_link(cache_dir.path(), "key-b");

        clean_expired_dlx_cache(cache_dir.path(), 24 * 60, SystemTime::now()).unwrap();

        assert!(key_a.exists(), "fresh link must survive within max age");
        assert!(key_b.exists(), "fresh link must survive within max age");
    }

    #[test]
    fn zero_max_age_removes_everything_without_stat() {
        let cache_dir = tempfile::tempdir().unwrap();
        let key = key_dir_with_link(cache_dir.path(), "some-key");
        fs::write(
            cache_dir
                .path()
                .join("dlx")
                .join("stray-file"),
            b"noise",
        )
        .unwrap();

        clean_expired_dlx_cache(cache_dir.path(), 0, SystemTime::now()).unwrap();

        assert!(!key.exists(), "zero max age removes every entry");
        assert!(
            cache_dir
                .path()
                .join("dlx")
                .join("stray-file")
                .exists(),
            "files in the dlx directory are ignored"
        );
    }

    #[test]
    fn removes_orphaned_prepare_dirs_but_keeps_the_link_target() {
        let cache_dir = tempfile::tempdir().unwrap();
        let key_dir = key_dir_with_link(cache_dir.path(), "key");
        let orphan = key_dir.join("prepare-orphan");
        fs::create_dir_all(&orphan).unwrap();
        let target = key_dir.join("prepare-1");

        // Far-future max age: nothing is expired, so only the orphan sweep runs.
        clean_expired_dlx_cache(cache_dir.path(), u64::MAX / 60, SystemTime::now()).unwrap();

        assert!(!orphan.exists(), "orphaned prepare dir should be removed");
        assert!(target.exists(), "the link target must be kept");
        assert!(key_dir.join("pkg").exists() || fs::symlink_metadata(key_dir.join("pkg")).is_ok());
    }

    #[test]
    fn removes_key_dirs_left_behind_without_a_pkg_link() {
        let cache_dir = tempfile::tempdir().unwrap();
        let key_dir = cache_dir
            .path()
            .join("dlx")
            .join("linkless-key");
        fs::create_dir_all(key_dir.join("prepare-1")).unwrap();

        clean_expired_dlx_cache(cache_dir.path(), u64::MAX / 60, SystemTime::now()).unwrap();

        assert!(!key_dir.exists(), "key dir without a pkg link should be removed");
    }

    #[test]
    fn missing_dlx_dir_is_not_an_error() {
        let cache_dir = tempfile::tempdir().unwrap();
        clean_expired_dlx_cache(cache_dir.path(), 0, SystemTime::now()).unwrap();
    }
}
