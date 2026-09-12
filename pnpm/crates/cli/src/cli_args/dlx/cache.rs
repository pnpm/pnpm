use super::{
    BTreeMap, CatalogResolutionResult, CatalogWantedDependency, Config, Context, DependencyGroup,
    DlxError, Duration, IntoDiagnostic, Path, PathBuf, RangeSpecStyle, Reporter, State,
    SupportedArchitectures, SupportedArchitecturesArgs, SystemTime, UNIX_EPOCH, Value, add_package,
    configured_catalogs, create_short_hash, force_symlink_dir, fs, json, parse_catalog_protocol,
    parse_manifest, parse_overrides_iter, parse_wanted_dependency, resolve_from_catalog,
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
    // The cache install is always fresh, so no lockfile is loaded from
    // the process working directory.
    config.lockfile = false;
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
    Ok(())
}

/// The command's cache directory, created and canonicalized so the prepare
/// dir carries no `..` segments. A relative `cacheDir` (e.g. `../pnpm-cache`)
/// would otherwise let the install's workspace-root walk pass through the
/// caller's project dir and mistake it for the dlx workspace.
pub(super) fn dlx_command_cache_dir(config: &Config, cache_key: &str) -> miette::Result<PathBuf> {
    let dlx_command_cache_dir = config.cache_dir.join("dlx").join(cache_key);
    fs::create_dir_all(&dlx_command_cache_dir).map_err(|source| DlxError::Cache {
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
pub(super) fn resolve_catalog_specs(
    pkgs: &[String],
    config: &Config,
) -> miette::Result<Vec<String>> {
    let uses_catalog = |pkg: &String| {
        parse_wanted_dependency(pkg)
            .bare_specifier
            .is_some_and(|bare_specifier| parse_catalog_protocol(&bare_specifier).is_some())
    };
    if !pkgs.iter().any(uses_catalog) {
        return Ok(pkgs.to_vec());
    }
    let catalogs = configured_catalogs(config)?;
    pkgs.iter()
        .map(|pkg| {
            let parsed = parse_wanted_dependency(pkg);
            let (Some(alias), Some(bare_specifier)) = (parsed.alias, parsed.bare_specifier) else {
                return Ok(pkg.clone());
            };
            let wanted = CatalogWantedDependency { alias: alias.clone(), bare_specifier };
            match resolve_from_catalog(&catalogs, &wanted) {
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
    let mut sorted: Vec<&str> = pkgs.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let registry_pairs: Vec<(&str, &str)> =
        registries.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut args = vec![json!(sorted), json!(registry_pairs)];
    if !allow_build.is_empty() {
        let mut sorted_allow: Vec<&str> = allow_build.iter().map(String::as_str).collect();
        sorted_allow.sort_unstable();
        args.push(json!({ "allowBuild": sorted_allow }));
    }
    if let Some(arch) = supported_architectures {
        for (key, values) in [("cpu", &arch.cpu), ("libc", &arch.libc), ("os", &arch.os)] {
            let Some(values) = values.as_ref().filter(|values| !values.is_empty()) else {
                continue;
            };
            let mut deduped: Vec<&str> = values.iter().map(String::as_str).collect();
            deduped.sort_unstable();
            deduped.dedup();
            args.push(json!({ "supportedArchitectures": { key: deduped } }));
        }
    }
    create_short_hash(&serde_json::to_string(&args).expect("serialize cache key inputs"))
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
    parse_manifest(&text).map_err(|error| DlxError::ReadManifest {
        path: path.display().to_string(),
        source: error.into(),
    })
}

/// Resolve caller catalogs before the cache install severs its workspace anchor.
fn resolve_cache_overrides(config: &mut Config) -> miette::Result<()> {
    if let Some(overrides) = config.overrides.as_ref()
        && config.workspace_dir.is_some()
        && overrides.values().any(|spec| spec.starts_with("catalog:"))
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
