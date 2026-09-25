use super::{
    FsEnsureExecutableBits, FsReadHead, LinkBinsError, Path, PathBuf, ScriptRuntime,
    ensure_target_executable, io, search_script_runtime,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

/// Memo of per-target probe work shared across
/// [`link_bins_of_packages_cached`](super::link_bins_of_packages_cached) calls:
/// the script-runtime (shebang) probe and the executable-bit fix-up, both keyed
/// by the target's symlink-resolved path. Many importers linking the same
/// virtual-store package repeat both against one underlying file, so a caller
/// that links several `node_modules/.bin` dirs in one pass shares a cache and
/// pays each probe once.
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
    pub(super) fn runtime_for<Sys: FsReadHead>(
        &self,
        probe_path: &Path,
    ) -> io::Result<Option<ScriptRuntime>> {
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
    pub(super) fn ensure_target_executable_once<Sys: FsEnsureExecutableBits>(
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
