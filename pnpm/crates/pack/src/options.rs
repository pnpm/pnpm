use super::{
    Arc, Catalogs, HashMap, ManifestFormat, NodeLinker, Path, PathBuf, PnpmfileHooks,
    lexical_normalize,
};

/// Inputs for [`crate::api`]. The CLI maps the resolved [`pnpm_config::Config`]
/// and command-line flags onto this struct.
pub struct PackOptions {
    /// Project directory to pack.
    pub dir: PathBuf,
    /// Workspace root, used to inject a root `LICENSE` into a
    /// sub-package tarball that lacks one.
    pub workspace_dir: Option<PathBuf>,
    pub scripts: PackScripts,
    pub manifest: PackManifestOptions,
    pub output: PackOutputOptions,
}

pub struct PackScripts {
    /// Skip the `prepack` / `prepare` / `postpack` lifecycle scripts.
    pub ignore: bool,
    /// `--unsafe-perm`: run lifecycle scripts without dropping privileges.
    /// Threaded from [`pnpm_config::Config::unsafe_perm`] so packing
    /// honors the same policy (and `TMPDIR` isolation) as an install.
    pub unsafe_perm: bool,
    /// `npm_config_user_agent` stamped on lifecycle scripts.
    pub user_agent: String,
    /// Extra directories prepended to `PATH` for lifecycle scripts.
    pub extra_bin_paths: Vec<PathBuf>,
    /// Extra environment variables for lifecycle scripts.
    pub extra_env: HashMap<String, String>,
}

pub struct PackManifestOptions {
    /// Parsed workspace catalogs, for `catalog:` specifier rewriting.
    pub catalogs: Catalogs,
    /// Directory holding `pnpm-workspace.yaml`, which a `file:` /
    /// `link:` catalog entry's relative path is measured from.
    pub catalogs_dir: Option<PathBuf>,
    /// Embed the project's `README.md` into the published manifest.
    pub embed_readme: bool,
    /// Node linker mode; `bundledDependencies` only work under
    /// [`NodeLinker::Hoisted`].
    pub node_linker: NodeLinker,
    /// `preferredManifestFormat` — which manifest of the packed project to
    /// read when several coexist. The packed tarball still carries exactly
    /// one normalized `package/package.json`; this only selects the source.
    pub preferred_format: Option<ManifestFormat>,
    /// Keep `packageManager` and publish-lifecycle scripts in the packed
    /// manifest.
    pub skip_obfuscation: bool,
    /// Loaded pnpmfiles whose `beforePacking` hook runs against the
    /// published manifest before the file list is computed, in
    /// application order (config-dependency plugin pnpmfiles first, then
    /// the workspace-root pnpmfile). Empty when none are configured.
    ///
    /// Holding the loaded hooks (rather than paths) lets a recursive pack
    /// share one worker per pnpmfile across every packed project instead
    /// of re-spawning it per project.
    pub before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
}

pub struct PackOutputOptions {
    /// gzip compression level (`0..=9`); `None` uses the zlib default.
    pub gzip_level: Option<u32>,
    /// Do everything except writing the tarball to disk.
    pub dry_run: bool,
    /// Directory to write the tarball into.
    pub destination: Option<String>,
    /// Custom output path template (`%s` = name, `%v` = version).
    pub out: Option<String>,
    /// In-memory tar entries (`package/<path>` → contents) with no file on
    /// disk, packed on top of `files_map` (superseding a same-named on-disk
    /// entry). Used for the composed CHANGELOG.md in `registry` changelog
    /// storage; the caller (which has registry access) fetches the previous
    /// version's changelog and renders the new section onto it.
    pub injected_files: Vec<(String, Vec<u8>)>,
    /// Per-invocation destination locks shared by recursive pack tasks.
    pub locks: Option<Arc<PackOutputLocks>>,
}

/// Locks recursive pack destinations for the lifetime of their write phase.
#[derive(Default)]
pub struct PackOutputLocks {
    by_path: tokio::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

impl PackOutputLocks {
    pub(super) async fn lock(&self, path: &Path) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut by_path = self.by_path.lock().await;
            Arc::clone(
                by_path
                    .entry(lexical_normalize(path))
                    .or_default(),
            )
        };
        lock.lock_owned().await
    }
}
