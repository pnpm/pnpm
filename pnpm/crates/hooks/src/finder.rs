use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
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

#[must_use]
pub fn load_pnpmfiles(
    root: &Path,
    configured: Option<&[PathBuf]>,
) -> Option<Arc<dyn PnpmfileHooks>> {
    let mut paths = find_pnpmfiles(root, configured).into_iter();
    let first = paths.next()?;
    let first = load_pnpmfile_at(first);
    let Some(second) = paths.next() else {
        return Some(first);
    };

    let mut hooks = vec![first, load_pnpmfile_at(second)];
    hooks.extend(paths.map(load_pnpmfile_at));
    Some(Arc::new(CombinedPnpmfileHooks { hooks }))
}

struct CombinedPnpmfileHooks {
    hooks: Vec<Arc<dyn PnpmfileHooks>>,
}

fn clone_context(ctx: &HookContext) -> HookContext {
    HookContext { log: Arc::clone(&ctx.log), dir: ctx.dir.clone() }
}

#[async_trait]
impl PnpmfileHooks for CombinedPnpmfileHooks {
    async fn read_package(
        &self,
        mut pkg: Value,
        ctx: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        for hook in &self.hooks {
            pkg = (*hook.read_package(pkg, clone_context(&ctx)).await?).clone();
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
            let result = hook.after_all_resolved(lockfile.clone(), clone_context(&ctx)).await?;
            if !result.is_null() {
                lockfile = result;
                changed = true;
            }
        }
        Ok(if changed { lockfile } else { Value::Null })
    }

    async fn update_config(&self, mut config: Value, ctx: HookContext) -> Result<Value, HookError> {
        for hook in &self.hooks {
            config = hook.update_config(config, clone_context(&ctx)).await?;
        }
        Ok(config)
    }

    async fn before_packing(
        &self,
        mut manifest: Value,
        dir: &Path,
        ctx: HookContext,
    ) -> Result<Value, HookError> {
        for hook in &self.hooks {
            manifest = hook.before_packing(manifest, dir, clone_context(&ctx)).await?;
        }
        Ok(manifest)
    }

    async fn pre_resolution(&self, ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) {
        for hook in &self.hooks {
            hook.pre_resolution(
                PreResolutionHookContext {
                    wanted_lockfile: ctx.wanted_lockfile.clone(),
                    current_lockfile: ctx.current_lockfile.clone(),
                    exists_current_lockfile: ctx.exists_current_lockfile,
                    exists_non_empty_wanted_lockfile: ctx.exists_non_empty_wanted_lockfile,
                    lockfile_dir: ctx.lockfile_dir.clone(),
                    store_dir: ctx.store_dir.clone(),
                    registries: ctx.registries.clone(),
                },
                PreResolutionHookLogger {
                    info: Arc::clone(&logger.info),
                    warn: Arc::clone(&logger.warn),
                },
            )
            .await;
        }
    }

    async fn filter_log(&self, log: Value, ctx: HookContext) -> bool {
        for hook in &self.hooks {
            if !hook.filter_log(log.clone(), clone_context(&ctx)).await {
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

    async fn get_finder_names(&self) -> Result<Vec<String>, HookError> {
        let mut names = Vec::new();
        for hook in &self.hooks {
            names.extend(hook.get_finder_names().await?);
        }
        Ok(names)
    }

    async fn run_finder(&self, finder_name: &str, ctx: Value) -> Result<Value, HookError> {
        for hook in &self.hooks {
            if hook.get_finder_names().await?.iter().any(|name| name == finder_name) {
                return hook.run_finder(finder_name, ctx).await;
            }
        }
        Ok(Value::Bool(false))
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
