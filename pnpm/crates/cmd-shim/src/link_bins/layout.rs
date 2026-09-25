use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Options shared by every bin one linking call writes — pnpm's
/// `LinkBinOptions`.
#[derive(Debug, Default, Clone)]
pub struct LinkBinsOptions {
    /// pnpm's `extraNodePaths` — see
    /// [`link_bins_of_packages`](super::link_bins_of_packages).
    pub extra_node_paths: Vec<String>,
    /// pnpm's `preferSymlinkedExecutables`: on Unix, materialize each
    /// bin as a relative symlink to the target file instead of a shell
    /// shim. Inert on Windows, where bins always get shims. The node
    /// runtime binary is symlinked regardless of this setting.
    pub prefer_symlinked_executables: bool,
    /// On Unix, keep shell shims while executing a sibling alias symlink so
    /// Node sees the command name in `process.argv[1]`.
    pub preserve_bin_name: bool,
    /// Bins written inside this directory name the paths inside it relative
    /// to themselves: the shim target marker, the shim `NODE_PATH` entries,
    /// and the node runtime symlink. `None` writes absolute paths. Inert on
    /// Windows.
    pub relocatable_root: Option<PathBuf>,
    /// The name of the project modules directory when it is not
    /// `node_modules` and `extendNodePath` is on. Bins linked into the `.bin`
    /// of a directory with this name get that directory first on `NODE_PATH`:
    /// Node only looks for packages in `node_modules` directories, so a tool
    /// installed there could not otherwise load the project's other packages,
    /// such as its plugins, ahead of its own.
    pub project_modules_dir_name: Option<OsString>,
    /// A modules directory pnpm installs packages into although it is not
    /// named `node_modules`: the root's custom `modulesDir` under the
    /// hoisted linker. Bin targets inside it get their executable bits the
    /// way targets under `node_modules` do.
    pub installed_modules_dir: Option<PathBuf>,
}

/// What the layout [`bin_layout_fingerprint`] keys on: the parts of
/// [`LinkBinsOptions`] that change what a linked bin looks like on disk.
/// Absolute `extra_node_paths` collapse to one placeholder so a cache-local
/// store or temp directory does not read as a different layout, and the
/// settings that only mean something on Unix contribute nothing there.
#[must_use]
pub fn bin_layout_fingerprint(options: &LinkBinsOptions) -> String {
    let extra_node_paths =
        options.extra_node_paths
            .iter()
            .map(|path| {
                if Path::new(path).is_absolute() { "<absolute>".to_string() } else { path.clone() }
            })
            .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "extraNodePaths": extra_node_paths,
        "preferSymlinkedExecutables": options.prefer_symlinked_executables && cfg!(unix),
        "preserveBinName": options.preserve_bin_name && cfg!(unix),
        "relocatableRoot": options.relocatable_root.is_some(),
        "projectModulesDirName": options.project_modules_dir_name.as_ref().map(|name| name.to_string_lossy()),
        "installedModulesDir": options.installed_modules_dir.is_some(),
    }))
    .expect("serialize bin layout fingerprint")
}
