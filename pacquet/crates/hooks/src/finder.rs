use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::PnpmfileHooks;

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

/// Load a pnpmfile from an explicit path (used for config-dependency
/// plugin pnpmfiles, which live at
/// `node_modules/.pnpm-config/<plugin>/pnpmfile.{mjs,cjs}`).
#[must_use]
pub fn load_pnpmfile_at(file: PathBuf) -> Arc<dyn PnpmfileHooks> {
    Arc::new(super::node_runtime::NodeJsHooks::new(file))
}

/// Whether `name` is a pnpm plugin package — one whose pnpmfile is
/// loaded automatically when it's a config dependency. Mirrors pnpm's
/// [`isPluginName`](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L120-L124):
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
/// in lexical order. Mirrors pnpm's
/// [`calcPnpmfilePathsOfPluginDeps`](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L101-L118):
///
/// - `config_modules_dir` is `node_modules/.pnpm-config`.
/// - A plugin whose directory is missing (the config-dep install didn't
///   run, or hasn't yet) is skipped silently.
/// - When the directory exists, `pnpmfile.mjs` is preferred, else
///   `pnpmfile.cjs` — the `.cjs` path is yielded even when absent so the
///   caller surfaces a "pnpmfile not found" error for the misconfigured
///   plugin, matching upstream.
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
