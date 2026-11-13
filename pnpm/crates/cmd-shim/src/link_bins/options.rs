use std::{ffi::OsString, path::PathBuf};

/// Options shared by every bin one linking call writes — pnpm's
/// `LinkBinOptions`.
#[derive(Debug, Default, Clone)]
pub struct LinkBinsOptions {
    /// Compare full shim contents after replacing a target's interpreter or native binary.
    pub force: bool,
    /// pnpm's `extraNodePaths` — see [`super::link_bins_of_packages`].
    pub extra_node_paths: Vec<String>,
    /// pnpm's `preferSymlinkedExecutables`: on Unix, materialize each
    /// bin as a relative symlink to the target file instead of a shell
    /// shim. Inert on Windows, where bins always get shims. The node
    /// runtime binary is symlinked regardless of this setting.
    pub prefer_symlinked_executables: bool,
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
