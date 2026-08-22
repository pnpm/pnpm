use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use derive_more::{Display, Error};
use serde_json::Value;

use super::{
    CustomFetcher, CustomResolver, HookContext, HookError, PnpmfileHooks, PreResolutionHookContext,
    PreResolutionHookLogger, ReadPackageResult,
};

#[must_use]
pub fn find_pnpmfile(root: &Path) -> Option<std::path::PathBuf> {
    let candidates = [".pnpmfile.mjs", ".pnpmfile.cjs"];

    for name in candidates {
        let path = root.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[must_use]
pub fn load_pnpmfile(root: &Path) -> Option<Arc<dyn PnpmfileHooks>> {
    let file = find_pnpmfile(root)?;
    Some(Arc::new(super::node_runtime::NodeJsHooks::new(file)))
}

#[must_use]
pub fn find_pnpmfiles(root: &Path, configured: Option<&[PathBuf]>) -> Vec<PathBuf> {
    let Some(configured) = configured else {
        return find_pnpmfile(root).into_iter().collect();
    };

    let mut paths = Vec::with_capacity(configured.len());
    for path in configured {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    paths
}

/// A pnpmfile named by the `pnpmfile` setting that is not on disk. Discovery of
/// the default `.pnpmfile.mjs` / `.pnpmfile.cjs` cannot produce this: an absent
/// default simply means the project has no pnpmfile, while a configured path
/// that resolves to nothing is a misconfiguration the install has to report.
#[derive(Debug, Display, Error)]
#[display("pnpmfile at \"{}\" is not found", path.display())]
pub struct MissingPnpmfileError {
    pub path: PathBuf,
}

/// Report the first path named by the `pnpmfile` setting that is not on disk.
/// Every loader validates before it hands a path to Node, so a misconfigured
/// setting is reported the same way whichever hook the command reaches first.
pub fn validate_configured_pnpmfiles(
    configured: Option<&[PathBuf]>,
) -> Result<(), MissingPnpmfileError> {
    let Some(configured) = configured else { return Ok(()) };
    match configured.iter().find(|path| !path.is_file()) {
        Some(missing) => Err(MissingPnpmfileError { path: missing.clone() }),
        None => Ok(()),
    }
}

/// Load every pnpmfile the project runs, in the order the `pnpmfile` setting
/// lists them. `Ok(None)` means the project has no pnpmfile at all.
pub fn load_pnpmfiles(
    root: &Path,
    configured: Option<&[PathBuf]>,
) -> Result<Option<Arc<dyn PnpmfileHooks>>, MissingPnpmfileError> {
    let paths = find_pnpmfiles(root, configured);
    validate_configured_pnpmfiles(configured)?;
    let mut hooks: Vec<_> = paths.into_iter().map(load_pnpmfile_at).collect();
    Ok(match hooks.len() {
        0 => None,
        1 => hooks.pop(),
        _ => Some(Arc::new(CombinedPnpmfileHooks { hooks })),
    })
}

struct CombinedPnpmfileHooks {
    hooks: Vec<Arc<dyn PnpmfileHooks>>,
}

#[async_trait]
impl PnpmfileHooks for CombinedPnpmfileHooks {
    async fn read_package(
        &self,
        mut pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        for hook in &self.hooks {
            pkg = (*hook.read_package(pkg, ctx.clone()).await?).clone();
        }
        Ok(Arc::new(pkg))
    }

    async fn after_all_resolved(
        &self,
        mut lockfile: Value,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        let mut changed = false;
        for hook in &self.hooks {
            let result = hook.after_all_resolved(lockfile.clone(), ctx.clone()).await?;
            if !result.is_null() {
                lockfile = result;
                changed = true;
            }
        }
        Ok(if changed { lockfile } else { Value::Null })
    }

    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) {
        for hook in &self.hooks {
            hook.pre_resolution(ctx.clone(), logger.clone()).await;
        }
    }

    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool {
        for hook in &self.hooks {
            if !hook.filter_log(log.clone(), ctx.clone()).await {
                return false;
            }
        }
        true
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        let mut includes_hooks = false;
        for hook in &self.hooks {
            includes_hooks |= hook.calculate_pnpmfile_checksum().await.is_some();
        }
        if !includes_hooks {
            return None;
        }

        let mut paths: Vec<&Path> =
            self.hooks.iter().filter_map(|hook| hook.source_path()).collect();
        paths.sort_unstable();
        let hashes: Option<Vec<String>> = paths
            .into_iter()
            .map(|path| pnpm_crypto_hash::create_hash_from_file(path).ok())
            .collect();
        let hashes = hashes?;
        Some(pnpm_crypto_hash::create_hash(&hashes.join(",")))
    }

    async fn get_custom_resolvers(&self) -> Result<Vec<Arc<dyn CustomResolver>>, HookError> {
        let mut resolvers = Vec::new();
        for hook in &self.hooks {
            resolvers.extend(hook.get_custom_resolvers().await?);
        }
        Ok(resolvers)
    }

    async fn get_custom_fetchers(&self) -> Result<Vec<Arc<dyn CustomFetcher>>, HookError> {
        let mut fetchers = Vec::new();
        for hook in &self.hooks {
            fetchers.extend(hook.get_custom_fetchers().await?);
        }
        Ok(fetchers)
    }
}

/// Load a pnpmfile from an explicit path (used for config-dependency
/// plugin pnpmfiles, which live at
/// `node_modules/.pnpm-config/<plugin>/pnpmfile.{mjs,cjs}`).
#[must_use]
pub fn load_pnpmfile_at(file: PathBuf) -> Arc<dyn PnpmfileHooks> {
    Arc::new(super::node_runtime::NodeJsHooks::new(file))
}

/// Whether `name` is a pnpm plugin package — one whose pnpmfile is
/// loaded automatically when it's a config dependency:
///
/// - unscoped `pnpm-plugin-*`,
/// - scoped `@pnpm/plugin-*`,
/// - scoped `@<org>/pnpm-plugin-*`.
#[must_use]
pub fn is_plugin_name(name: &str) -> bool {
    if name.starts_with("pnpm-plugin-") {
        return true;
    }
    if !name.starts_with('@') {
        return false;
    }
    name.starts_with("@pnpm/plugin-") || name.contains("/pnpm-plugin-")
}

/// Resolve the pnpmfile paths of every plugin among `config_dep_names`,
/// in lexical order:
///
/// - `config_modules_dir` is `node_modules/.pnpm-config`.
/// - A plugin whose directory is missing (the config-dep install didn't
///   run, or hasn't yet) is skipped silently.
/// - When the directory exists, `pnpmfile.mjs` is preferred, else
///   `pnpmfile.cjs` — the `.cjs` path is yielded even when absent so the
///   caller surfaces a "pnpmfile not found" error for the misconfigured
///   plugin.
pub fn calc_pnpmfile_paths_of_plugin_deps<'a>(
    config_modules_dir: &Path,
    config_dep_names: impl IntoIterator<Item = &'a str>,
) -> Vec<PathBuf> {
    let mut names: Vec<&str> =
        config_dep_names.into_iter().filter(|name| is_plugin_name(name)).collect();
    names.sort_unstable();
    names
        .into_iter()
        .filter_map(|name| {
            let plugin_dir = config_modules_dir.join(name);
            if !plugin_dir.exists() {
                return None;
            }
            let mjs = plugin_dir.join("pnpmfile.mjs");
            Some(if mjs.is_file() { mjs } else { plugin_dir.join("pnpmfile.cjs") })
        })
        .collect()
}
