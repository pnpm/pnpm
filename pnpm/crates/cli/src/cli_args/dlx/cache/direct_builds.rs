use crate::State;
use miette::{
    Context as _,
    IntoDiagnostic as _,
};
use pnpm_config::Config;
use pnpm_lockfile::{
    Lockfile,
    ResolvedDependencyMap,
};
use pnpm_modules_yaml::{
    Host as ModulesHost,
    read_modules_manifest,
    write_modules_manifest,
};
use pnpm_package_manager::allow_build_key_from_ignored_build;
use pnpm_reporter::Reporter;
use std::path::Path;

pub(super) fn has_pending_direct_non_registry_builds(
    config: &Config,
    cache_dir: &Path,
) -> miette::Result<bool> {
    if config.ignore_scripts {
        return Ok(false);
    }
    let Some(modules) =
        read_modules_manifest::<ModulesHost>(&cache_dir.join("node_modules")).into_diagnostic()?
    else {
        return Ok(false);
    };
    let Some(ignored) = modules.ignored_builds
        .as_ref()
        .filter(|keys| !keys.is_empty())
    else {
        return Ok(false);
    };
    let ignored_keys: Vec<String> = ignored
        .iter()
        .map(|dep_path| allow_build_key_from_ignored_build(dep_path.as_str()))
        .collect();
    Ok(!approved_direct_build_keys(config, cache_dir, &ignored_keys)?.is_empty())
}

/// Approve and rebuild explicitly requested top-level `dlx` packages whose
/// non-semver specifiers (`http(s):`, `file:`, `git+`, ...) have no package
/// alias before fetching or require exact `depPath` approval.
pub(super) async fn rebuild_direct_non_registry_packages<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    prepare_dir: &Path,
    manifest_path: &Path,
) -> miette::Result<&'static Config> {
    if config.ignore_scripts {
        return Ok(config);
    }
    let scan = crate::cli_args::ignored_builds::get_automatically_ignored_builds(config)?;
    let Some(ignored_keys) = scan.names
        .as_deref()
        .filter(|keys| !keys.is_empty())
    else {
        return Ok(config);
    };
    let approved_keys = approved_direct_build_keys(config, prepare_dir, ignored_keys)?;
    if approved_keys.is_empty() {
        return Ok(config);
    }
    let config = leak_config_with_approved_builds(config, &approved_keys);
    let rebuild_state = State::init(manifest_path.to_path_buf(), config, true)
        .wrap_err("initialize the dlx rebuild state")?;
    let selection = crate::cli_args::rebuild::RebuildSelection {
        names: Some(approved_keys.clone()),
        projects: Vec::new(),
    };
    crate::cli_args::rebuild::run_rebuild::<Reporter>(&rebuild_state, selection, None).await?;
    persist_pruned_ignored_builds(&scan.modules_dir, &approved_keys)?;
    Ok(config)
}

fn approved_direct_build_keys(
    config: &Config,
    prepare_dir: &Path,
    ignored_keys: &[String],
) -> miette::Result<Vec<String>> {
    let Some(lockfile) = Lockfile::load_wanted_from_dir(prepare_dir)
        .into_diagnostic()
        .wrap_err("read the dlx cache lockfile")?
    else {
        return Ok(Vec::new());
    };
    let Some(direct_deps) =
        lockfile.importers.get(".").and_then(|importer| importer.dependencies.as_ref())
    else {
        return Ok(Vec::new());
    };
    collect_direct_build_keys(config, direct_deps, ignored_keys)
}

fn collect_direct_build_keys(
    config: &Config,
    direct_deps: &ResolvedDependencyMap,
    ignored_keys: &[String],
) -> miette::Result<Vec<String>> {
    let policy = pnpm_deps_restorer::AllowBuildPolicy::from_config(config).into_diagnostic()?;
    let mut approved = Vec::new();
    for (alias, dep) in direct_deps {
        if config.allow_builds.get(&alias.to_string()) == Some(&false) {
            continue;
        }
        let Some(dep_path) = dep.version.resolved_key(alias).map(|key| key.to_string()) else {
            continue;
        };
        if policy.check(&dep_path) == Some(false) {
            continue;
        }
        let allow_key = allow_build_key_from_ignored_build(&dep_path);
        let is_non_registry = allow_key == pnpm_deps_restorer::normalize_build_dep_path(&dep_path);
        if is_non_registry && ignored_keys.contains(&allow_key) && !approved.contains(&allow_key) {
            approved.push(allow_key);
        }
    }
    approved.sort();
    Ok(approved)
}

fn leak_config_with_approved_builds(
    config: &Config,
    approved_keys: &[String],
) -> &'static mut Config {
    let mut updated = config.clone();
    for key in approved_keys {
        updated.allow_builds.insert(key.clone(), true);
    }
    Config::leak(updated)
}

fn persist_pruned_ignored_builds(
    modules_dir: &Path,
    approved_keys: &[String],
) -> miette::Result<()> {
    let Some(mut modules) = read_modules_manifest::<ModulesHost>(modules_dir).into_diagnostic()?
    else {
        return Ok(());
    };
    let Some(ignored) = modules.ignored_builds.as_mut() else {
        return Ok(());
    };
    let before = ignored.len();
    ignored.retain(|dep_path| {
        let key = allow_build_key_from_ignored_build(dep_path.as_str());
        !approved_keys.contains(&key)
    });
    if ignored.len() == before {
        return Ok(());
    }
    if ignored.is_empty() {
        modules.ignored_builds = None;
    }
    write_modules_manifest::<ModulesHost>(modules_dir, modules).into_diagnostic()
}
