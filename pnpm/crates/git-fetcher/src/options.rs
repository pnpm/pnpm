use crate::GitSourceCache;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_store_dir::{
    StoreDir,
    StoreIndexWriter,
};
use std::{
    path::Path,
    sync::Arc,
};

#[derive(Clone, Copy)]
pub struct PrepareScriptOptions<'a> {
    pub ignore: bool,
    pub unsafe_perm: bool,
    pub user_agent: Option<&'a str>,
    pub prepend_node_path: ScriptsPrependNodePath,
    pub shell: Option<&'a Path>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    /// The running pnpm, used to provide the package manager the
    /// dependency's build needs. `None` leaves the build to whatever is
    /// installed on the host.
    pub pnpm_execpath: Option<&'a Path>,
}

#[derive(Clone, Copy)]
pub struct GitStoreContext<'a> {
    pub dir: &'a StoreDir,
    /// Install-scoped store-index writer. When provided, the fetcher
    /// queues a `PackageFilesIndex` row at [`Self::files_index_file`]
    /// after import so a future install's warm prefetch finds the
    /// snapshot in `index.db` and skips the clone/checkout/prepare/
    /// packlist re-run. Passing `None` (e.g., from tests) silently
    /// skips the write — the install is still correct, just slower
    /// on the next run.
    pub index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Cache key the row lands at — for git resolutions this is always
    /// the git-hosted store-index-key form (`pkg_id\t{built|not-built}`).
    /// The dispatcher computes it once and threads it in.
    pub files_index_file: &'a str,
}

#[derive(Clone, Copy)]
pub struct GitSource<'a> {
    pub cache: &'a GitSourceCache,
    pub repo: &'a str,
    pub commit: &'a str,
    /// `path` field from the resolution. `None` packs the repo root.
    pub path: Option<&'a str>,
    /// Hosts that opt into `git init` + `git fetch --depth 1` instead
    /// of a full clone. Mirrors `Config::git_shallow_hosts`.
    pub shallow_hosts: &'a [String],
    /// Override for the `git` binary path. Production callers leave
    /// this `None` and the fetcher resolves `git` through `PATH`.
    /// Tests use it to inject a shim binary at an absolute path, so
    /// the test can observe the fetcher's argv without mutating
    /// process-global state.
    pub git_bin: Option<&'a Path>,
}
