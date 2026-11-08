//! The kinds of file a full install reads in the workspace outside
//! `node_modules`. `repeat_install_read_set` sorts every file a full install
//! opens into one of them, and the oracle runs an edit scenario for each.

/// A kind of file a full install reads in the workspace outside
/// `node_modules`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
pub enum Input {
    /// A project's manifest, recorded in `manifest_files`.
    Manifest,
    /// A lockfile, recorded in `files`.
    Lockfile,
    /// A pnpmfile, recorded in `pnpmfiles`.
    Pnpmfile,
    /// A patch file `patchedDependencies` names, recorded in `files`.
    PatchFile,
    /// A local tarball dependency, recorded in `local_dependencies`.
    LocalTarball,
    /// A file in a local directory dependency that is not a workspace
    /// project. `local_dependencies` records the directory.
    LocalDirectory,
    /// `.npmrc`.
    Npmrc,
    /// `pnpm-workspace.yaml`.
    WorkspaceManifest,
    /// A directory project discovery lists.
    ProjectDirectories,
}
