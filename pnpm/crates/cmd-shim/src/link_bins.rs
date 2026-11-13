pub use discovery::collect_packages_in_modules_dir;
pub use options::LinkBinsOptions;
pub use prepared::{PreparedPackageBins, link_bins_of_packages_precomputed};
pub use refresh::{DirectoryBinPlan, prune_stale_bins, prune_stale_bins_with};
pub use relocatable::bin_dir_is_relocatable;
pub use selection::choose_bins;
pub use shim_writer::remove_bin;
pub use source::{BinOrigin, PackageBinSource};

pub(super) use crate::{
    bin_resolver::{Command, PackageBin},
    capabilities::DirCreation,
};
use crate::{
    bin_resolver::{commands_from_precalculated_bins, get_bins_from_package_manifest},
    capabilities::{
        FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
        FsSetExecutable, FsWalkFiles, FsWrite,
    },
    shim::{
        ScriptRuntime, generate_cmd_shim, generate_pwsh_shim, is_sh_shim_basedir_anchor_current,
        is_sh_shim_hardened, is_sh_shim_msys_cmd_switch_doubled, is_shim_pointing_at,
        search_script_runtime,
    },
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::parse_manifest_bytes;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Error type for [`link_bins_of_packages`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkBinsError {
    #[display("Failed to create bin directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_CREATE_BIN_DIR))]
    CreateBinDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to resolve bin-link path {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_RESOLVE_PATH))]
    ResolvePath {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read modules directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_READ_MODULES_DIR))]
    ReadModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read package manifest at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_READ_MANIFEST))]
    ReadManifest {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to parse package manifest at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_PARSE_MANIFEST))]
    ParseManifest {
        path: PathBuf,
        #[error(source)]
        error: serde_json::Error,
    },

    #[diagnostic(transparent)]
    ReadProjectManifest(#[error(source)] pnpm_package_manifest::PackageManifestError),

    #[display("Failed to read shim source {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_PROBE_SHIM_SOURCE))]
    ProbeShimSource {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write shim file at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_WRITE_SHIM))]
    WriteShim {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Cannot update shared bin shim at {path:?} because frozenStore is enabled")]
    #[diagnostic(code(ERR_PNPM_FROZEN_STORE_WRITE))]
    FrozenStoreWrite { path: PathBuf },

    #[display("Failed to chmod {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_CHMOD))]
    Chmod {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove stale bin at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_REMOVE_STALE_BIN))]
    RemoveStaleBin {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to link node runtime binary {src:?} -> {dst:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_LINK_NODE_BIN))]
    LinkNodeBin {
        src: PathBuf,
        dst: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to symlink executable {src:?} -> {dst:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_CMD_SHIM_SYMLINK_BIN))]
    SymlinkBin {
        src: PathBuf,
        dst: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Memo of per-target probe work shared across [`link_bins_of_packages_cached`]
/// calls: the script-runtime (shebang) probe and the executable-bit fix-up,
/// both keyed by the target's symlink-resolved path. Many importers linking
/// the same virtual-store package repeat both against one underlying file,
/// so a caller that links several `node_modules/.bin` dirs in one pass
/// shares a cache and pays each probe once.
///
/// The memo assumes the targets' contents and permissions do not change
/// while it is alive. Scope a cache to a single linking pass — in
/// particular, do not carry one across a lifecycle-script (build) phase,
/// which may rewrite target files.
#[derive(Debug, Default, Clone)]
pub struct ShimTargetCache(Arc<ShimTargetCacheState>);

#[derive(Debug, Default)]
struct ShimTargetCacheState {
    runtimes: Mutex<HashMap<PathBuf, Option<ScriptRuntime>>>,
    executable_ensured: Mutex<HashSet<PathBuf>>,
}

impl ShimTargetCache {
    /// [`search_script_runtime`] with the result memoized under
    /// `probe_path`. Errors are not cached, so a transient failure does
    /// not poison later lookups.
    ///
    /// Concurrency note: the lock is not held across the probe, so two
    /// workers racing on one key may both probe. That's benign — the
    /// probe is idempotent and the memo converges — and it keeps a slow
    /// read from serializing every other target's probe behind it. Same
    /// trade as the store's `verifiedFilesCache`.
    fn runtime_for<Sys: FsReadHead>(&self, probe_path: &Path) -> io::Result<Option<ScriptRuntime>> {
        if let Some(runtime) = self.0.runtimes
            .lock()
            .expect("runtime memo lock")
            .get(probe_path)
        {
            return Ok(runtime.clone());
        }
        let runtime = search_script_runtime::<Sys>(probe_path)?;
        self.0.runtimes
            .lock()
            .expect("runtime memo lock")
            .insert(probe_path.to_path_buf(), runtime.clone());
        Ok(runtime)
    }

    /// [`ensure_target_executable`] at most once per `probe_path`.
    fn ensure_target_executable_once<Sys: FsEnsureExecutableBits>(
        &self,
        probe_path: &Path,
        installed_modules_dir: Option<&Path>,
    ) -> Result<(), LinkBinsError> {
        if self.0.executable_ensured
            .lock()
            .expect("executable memo lock")
            .contains(probe_path)
        {
            return Ok(());
        }
        ensure_target_executable::<Sys>(probe_path, installed_modules_dir)?;
        self.0.executable_ensured
            .lock()
            .expect("executable memo lock")
            .insert(probe_path.to_path_buf());
        Ok(())
    }
}

/// Read `<location>/package.json` for each entry under `modules_dir` and link
/// its bins into `bins_dir`. See [`link_bins_of_packages`] for the
/// `extra_node_paths` contract.
pub fn link_bins<Sys>(
    modules_dir: &Path,
    bins_dir: &Path,
    options: &LinkBinsOptions,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadDir
        + FsReadFile
        + FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let packages = collect_packages_in_modules_dir::<Sys>(modules_dir)?;
    link_bins_of_packages::<Sys>(&packages, bins_dir, options)
}

/// Link every bin declared by `packages` into `bins_dir`, applying conflict
/// resolution between bins of the same name.
///
/// `extra_node_paths` is pnpm's `extraNodePaths` (the hidden hoisted
/// modules dir under the isolated linker unless `extendNodePath:
/// false`). When non-empty, each shim carries a `NODE_PATH` block
/// listing the target's own `node_modules` dirs followed by these
/// entries; when empty the shims stay `NODE_PATH`-free.
pub fn link_bins_of_packages<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    options: &LinkBinsOptions,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_of_packages_with_excludes::<Sys>(packages, bins_dir, &HashSet::new(), options)
}

#[derive(Debug, Default, Clone)]
pub struct LinkBinsOutcome {
    pub held_back: bool,
    pub linked_commands: HashSet<String>,
}

/// [`link_bins_of_packages`] with a caller-scoped [`ShimTargetCache`],
/// for callers that link many `node_modules/.bin` dirs against the
/// same underlying packages in one pass.
///
/// Returns whether it held back a bin whose target is missing (see
/// [`PackageBinSource::build_pending`]), so the caller can link `bins_dir`
/// again once the builds ran.
pub fn link_bins_of_packages_cached<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_impl::<Sys>(packages, bins_dir, &HashSet::new(), options, cache)
        .map(|outcome| outcome.held_back)
}

/// [`link_bins_of_packages_cached`] returning the chosen command names alongside
/// the held-back flag.
pub fn link_bins_of_packages_outcome<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    exclude_bins: &HashSet<String>,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<LinkBinsOutcome, LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_impl::<Sys>(packages, bins_dir, exclude_bins, options, cache)
}

/// Like [`link_bins_of_packages`] but skips any bin whose name is in
/// `exclude_bins`. Used by global install to leave bins legitimately
/// owned by an already-installed global package untouched.
pub fn link_bins_of_packages_with_excludes<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    exclude_bins: &HashSet<String>,
    options: &LinkBinsOptions,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_impl::<Sys>(packages, bins_dir, exclude_bins, options, &ShimTargetCache::default())
        .map(|_| ())
}

fn link_bins_impl<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    exclude_bins: &HashSet<String>,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<LinkBinsOutcome, LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let chosen = choose_bins::<Sys>(packages, exclude_bins);
    let linked_commands = chosen
        .iter()
        .map(|(command, _)| command.name.clone())
        .collect();
    let held_back = link_chosen_bins::<Sys>(chosen, bins_dir, options, cache)?;
    Ok(LinkBinsOutcome { held_back, linked_commands })
}

mod chosen_bins;
use chosen_bins::link_chosen_bins;
#[cfg(test)]
mod tests;

mod shim_writer;
use shim_writer::{ShimSpec, remove_stale_bin};

mod frozen_store;

mod preferred_shim;

mod executable;
use executable::{
    bin_node_paths, chmod_tolerating_removal, ensure_target_executable, is_node_bin_name,
    link_node_bin, link_symlinked_executable, symlink_already_points_at, target_requires_shim,
};

mod conflicts;
use conflicts::{package_name, pick_winner};

mod discovery;
mod options;
mod prepared;
mod selection;
use selection::choose_bins_with;
mod refresh;

mod exclusions;
use exclusions::ExcludedBins;

mod linking_paths;
pub(super) use linking_paths::shim_node_path;

mod relocatable;
mod source;

mod windows_shim_policy;
use windows_shim_policy::WindowsShimPolicy;
