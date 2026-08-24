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

/// Which pnpmfiles an install runs, before any of them is read.
///
/// `global` is the user-level `globalPnpmfile`. It loads ahead of the project's
/// own and stays out of `pnpmfileChecksum`, so nothing that trusts the checksum
/// may treat it as accounted for — the same split pnpm's `requireHooks` makes
/// with `includeInChecksum: false`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PnpmfileSelection<'a> {
    pub configured: Option<&'a [PathBuf]>,
    pub global: Option<&'a Path>,
}

#[must_use]
pub fn find_pnpmfiles(root: &Path, selection: PnpmfileSelection<'_>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = selection.global.map(Path::to_path_buf).into_iter().collect();
    let project = match selection.configured {
        Some(configured) => configured.to_vec(),
        None => find_pnpmfile(root).into_iter().collect(),
    };
    for path in project {
        if !paths.contains(&path) {
            paths.push(path);
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
    selection: PnpmfileSelection<'_>,
) -> Result<(), MissingPnpmfileError> {
    // Discovery is the only source that may come back empty without complaint,
    // so both explicitly named sources are checked and the default is not.
    let mut named = selection
        .global
        .into_iter()
        .chain(selection.configured.unwrap_or(&[]).iter().map(PathBuf::as_path));
    match named.find(|path| !pnpmfile_exists(path)) {
        Some(missing) => Err(MissingPnpmfileError { path: missing.to_path_buf() }),
        None => Ok(()),
    }
}

/// Whether a configured path names a pnpmfile at all, mirroring pnpm's
/// `pnpmFileExistsSync`. The question is only "is there something here", not
/// "will Node load it": a path that exists but fails to evaluate — because the
/// pnpmfile itself requires a missing module, say — is an execution failure and
/// must keep reporting as one. Hence a bare existence test rather than
/// [`Path::is_file`], and the `.cjs` suffix pnpm appends to a path that names
/// neither module extension itself.
fn pnpmfile_exists(path: &Path) -> bool {
    let names_a_module = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| matches!(extension, "cjs" | "mjs"));
    if names_a_module {
        return path.exists();
    }
    let mut with_suffix = path.as_os_str().to_owned();
    with_suffix.push(".cjs");
    Path::new(&with_suffix).exists()
}

/// Load every pnpmfile the project runs, in the order the `pnpmfile` setting
/// lists them. `Ok(None)` means the project has no pnpmfile at all.
pub fn load_pnpmfiles(
    root: &Path,
    selection: PnpmfileSelection<'_>,
) -> Result<Option<Arc<dyn PnpmfileHooks>>, MissingPnpmfileError> {
    let paths = find_pnpmfiles(root, selection);
    validate_configured_pnpmfiles(selection)?;
    let checksum_skips = usize::from(selection.global.is_some());
    let hooks: Vec<_> = paths.into_iter().map(load_pnpmfile_at).collect();
    Ok(match (hooks.len(), checksum_skips) {
        (0, _) => None,
        // A lone project pnpmfile answers for its own checksum. Anything else
        // has to go through the wrapper, including a lone global one: its hash
        // must not become the project's.
        (1, 0) => hooks.into_iter().next(),
        _ => Some(Arc::new(CombinedPnpmfileHooks { hooks, checksum_skips })),
    })
}

struct CombinedPnpmfileHooks {
    hooks: Vec<Arc<dyn PnpmfileHooks>>,
    /// How many leading entries of [`Self::hooks`] the checksum leaves out.
    checksum_skips: usize,
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

        let mut paths: Vec<&Path> = self.hooks[self.checksum_skips..]
            .iter()
            .filter_map(|hook| hook.source_path())
            .collect();
        paths.sort_unstable();
        let hashes: Option<Vec<String>> = paths
            .into_iter()
            .map(|path| pnpm_crypto_hash::create_hash_from_file(path).ok())
            .collect();
        let hashes = hashes?;
        Some(match hashes.as_slice() {
            // `createHashFromMultipleFiles` answers with the file's own hash
            // when the set holds exactly one, rather than hashing a list of
            // one hash. Excluding the global pnpmfile can leave a single
            // project file behind, so the shortcut is reachable here and the
            // two implementations have to agree on the value they record.
            [only] => only.clone(),
            _ => pnpm_crypto_hash::create_hash(&hashes.join(",")),
        })
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
