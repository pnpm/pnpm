pub use discovery::collect_packages_in_modules_dir;
pub use shim_writer::remove_bin;

use crate::{
    bin_resolver::{Command, get_bins_from_package_manifest, pkg_owns_bin},
    capabilities::{
        DirCreation, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead,
        FsReadToString, FsSetExecutable, FsWalkFiles, FsWrite,
    },
    shim::{
        ScriptRuntime, generate_cmd_shim, generate_pwsh_shim, generate_sh_shim,
        is_sh_shim_hardened, is_shim_pointing_at, search_script_runtime,
    },
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pnpm_package_manifest::parse_manifest_bytes;
use rayon::prelude::*;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// One package known to be installed at `location`, with its parsed
/// `package.json`. The per-package input to [`link_bins_of_packages`].
///
/// The manifest is shared via `Arc` rather than owned by value: the
/// lockfile-driven bin-link path looks up the same parsed manifest
/// from a process-wide map, so packing it into a [`PackageBinSource`]
/// is a refcount bump (cheap) rather than a deep clone of the JSON
/// tree (which would have been the bulk of the per-slot CPU work,
/// since the per-install clone count is `slots × children` =
/// thousands of times).
#[derive(Debug, Clone)]
pub struct PackageBinSource {
    pub location: PathBuf,
    pub manifest: Arc<Value>,
    /// Where this candidate came from. When a hoisted (transitive)
    /// dep and a direct dep both declare the same bin name, the
    /// direct dep must win so a project never gets its own tooling
    /// silently shadowed by a transitive's bin. Defaults to
    /// [`BinOrigin::Direct`] —
    /// constructions via [`PackageBinSource::new`] don't have to
    /// supply the field. Pacquet's hoist + hoisted-linker passes use
    /// [`PackageBinSource::with_origin`] to tag transitive
    /// candidates as [`BinOrigin::Hoisted`].
    pub origin: BinOrigin,
    /// The package directory with every symlink resolved — the
    /// virtual-store slot dir a symlinked [`location`] points at.
    /// Callers that already know it (the lockfile-driven passes
    /// derive it from the snapshot key and the store layout) set it so
    /// the shim `NODE_PATH` derives lexically; when `None` the linker
    /// falls back to one `canonicalize` per package.
    ///
    /// [`location`]: Self::location
    pub resolved_location: Option<PathBuf>,
}

impl PackageBinSource {
    /// Construct a [`PackageBinSource`] tagged as
    /// [`BinOrigin::Direct`]. Use this for direct-dependency
    /// candidates and for any call site that doesn't need to
    /// distinguish direct from hoisted (per-slot bin linking,
    /// most tests).
    #[must_use]
    pub fn new(location: PathBuf, manifest: Arc<Value>) -> Self {
        Self { location, manifest, origin: BinOrigin::Direct, resolved_location: None }
    }

    /// Tag this source with the given [`BinOrigin`]. Builder-style
    /// helper so call sites that need to mark candidates as
    /// [`BinOrigin::Hoisted`] don't have to spell out the struct
    /// literal.
    #[must_use]
    pub fn with_origin(mut self, origin: BinOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Record the symlink-resolved package directory. See
    /// [`Self::resolved_location`].
    #[must_use]
    pub fn with_resolved_location(mut self, resolved_location: PathBuf) -> Self {
        self.resolved_location = Some(resolved_location);
        self
    }
}

/// Whether a [`PackageBinSource`] came from a project's direct
/// dependencies or from a transitive dep that the hoister lifted to
/// `node_modules/<name>` / `node_modules/.pnpm/node_modules/<name>`.
///
/// Used by `pick_winner` (private) as the highest-precedence tier
/// in the conflict-resolution rule: a direct dep's bin always wins
/// over a hoisted dep's bin with the same name — direct candidates
/// are kept and hoisted candidates with a name collision are dropped.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BinOrigin {
    /// The candidate is a direct dependency of the importer
    /// installing it. Direct deps come from the per-importer
    /// `dependencies` / `devDependencies` / `optionalDependencies`
    /// maps in the lockfile / manifest.
    #[default]
    Direct,
    /// The candidate is a transitive dependency that the hoister
    /// lifted to a top-level (or per-`node_modules`) slot. Bins
    /// from these candidates are dropped when a same-named
    /// [`Self::Direct`] candidate is also present.
    Hoisted,
}

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
        if let Some(runtime) = self.0.runtimes.lock().expect("runtime memo lock").get(probe_path) {
            return Ok(runtime.clone());
        }
        let runtime = search_script_runtime::<Sys>(probe_path)?;
        self.0
            .runtimes
            .lock()
            .expect("runtime memo lock")
            .insert(probe_path.to_path_buf(), runtime.clone());
        Ok(runtime)
    }

    /// [`ensure_target_executable`] at most once per `probe_path`.
    fn ensure_target_executable_once<Sys: FsEnsureExecutableBits>(
        &self,
        probe_path: &Path,
    ) -> Result<(), LinkBinsError> {
        if self.0.executable_ensured.lock().expect("executable memo lock").contains(probe_path) {
            return Ok(());
        }
        ensure_target_executable::<Sys>(probe_path)?;
        self.0
            .executable_ensured
            .lock()
            .expect("executable memo lock")
            .insert(probe_path.to_path_buf());
        Ok(())
    }
}

/// Options shared by every bin one linking call writes — pnpm's
/// `LinkBinOptions`.
#[derive(Debug, Default, Clone)]
pub struct LinkBinsOptions {
    /// pnpm's `extraNodePaths` — see [`link_bins_of_packages`].
    pub extra_node_paths: Vec<String>,
    /// pnpm's `preferSymlinkedExecutables`: on Unix, materialize each
    /// bin as a relative symlink to the target file instead of a shell
    /// shim. Inert on Windows, where bins always get shims. The node
    /// runtime binary is symlinked regardless of this setting.
    pub prefer_symlinked_executables: bool,
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
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_of_packages_with_excludes::<Sys>(packages, bins_dir, &HashSet::new(), options)
}

/// [`link_bins_of_packages`] with a caller-scoped [`ShimTargetCache`],
/// for callers that link many `node_modules/.bin` dirs against the
/// same underlying packages in one pass.
pub fn link_bins_of_packages_cached<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_impl::<Sys>(packages, bins_dir, &HashSet::new(), options, cache)
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
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    link_bins_impl::<Sys>(packages, bins_dir, exclude_bins, options, &ShimTargetCache::default())
}

fn link_bins_impl<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    exclude_bins: &HashSet<String>,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let chosen = choose_bins::<Sys>(packages, exclude_bins);
    if chosen.is_empty() {
        return Ok(());
    }

    let bin_dir = Sys::create_dir_all_reporting(bins_dir)
        .map_err(|error| LinkBinsError::CreateBinDir { dir: bins_dir.to_path_buf(), error })?;

    // Each shim's read-shebang + write-file + chmod sequence is independent
    // across bin names. There is no shared state, so drive them on rayon.
    // The hot path is per-package-bin; without parallelism the per-shim
    // file I/O serialised across the whole `chosen` map.
    chosen.par_iter().try_for_each(|(command, pkg)| {
        // On Unix the symlink branch never writes a shim, so no bin
        // needs a NODE_PATH — skip `shim_node_path`'s per-package
        // canonicalize entirely.
        let node_path = if options.prefer_symlinked_executables && cfg!(unix) {
            Vec::new()
        } else {
            shim_node_path(pkg, &options.extra_node_paths)
        };
        let pkg_name = package_name(pkg);
        // The target's symlink-resolved path doubles as the memo key
        // for the per-target probes: importers that reach one
        // virtual-store file through different symlinks share it.
        // Without a resolved location, the literal path still dedupes
        // within whatever scope the caller gave the cache.
        let probe_path = pkg
            .resolved_location
            .as_ref()
            .and_then(|resolved| {
                command
                    .path
                    .strip_prefix(&pkg.location)
                    .ok()
                    .map(|bin_rel_path| resolved.join(bin_rel_path))
            })
            .unwrap_or_else(|| command.path.clone());
        write_shim::<Sys>(
            ShimSpec {
                target_path: &command.path,
                probe_path: &probe_path,
                shim_path: &bins_dir.join(&command.name),
                node_path: &node_path,
                prefer_symlinked_executables: options.prefer_symlinked_executables,
                make_powershell_shim: wants_powershell_shim(pkg_name),
                bin_dir,
            },
            cache,
        )
    })?;

    Ok(())
}

/// The bins `packages` provide, minus `exclude_bins`, each paired with the
/// package providing it. A name several packages provide goes to the one
/// that owns it, else to the first by name and highest version.
#[must_use]
pub fn choose_bins<'packages, Sys: FsWalkFiles>(
    packages: &'packages [PackageBinSource],
    exclude_bins: &std::collections::HashSet<String>,
) -> Vec<(Command, &'packages PackageBinSource)> {
    let mut chosen: HashMap<String, (Command, &PackageBinSource)> = HashMap::new();
    for pkg in packages {
        for command in get_bins_from_package_manifest::<Sys>(&pkg.manifest, &pkg.location) {
            let wins = chosen
                .get(&command.name)
                .is_none_or(|(_, existing)| pick_winner(&command.name, existing, pkg));
            if wins {
                chosen.insert(command.name.clone(), (command, pkg));
            }
        }
    }
    for excluded in exclude_bins {
        chosen.remove(excluded);
    }
    chosen.into_values().collect()
}

/// The `NODE_PATH` entries for one package's shims: the target's own
/// `node_modules` dirs first (pnpm's `getBinNodePaths`), then the
/// caller's extras that aren't already present. An empty extras list
/// means "no `NODE_PATH` in shims at all" (`extendNodePath: false`, a
/// non-isolated linker, or no hoist pattern), matching pnpm's bins
/// linker.
///
/// The result depends only on the package's symlink-resolved
/// directory — every bin lives under the package root — so a
/// caller-supplied [`PackageBinSource::resolved_location`] makes this
/// syscall-free; without one the package's `location` is
/// canonicalized once, covering all of its bins.
fn shim_node_path(pkg: &PackageBinSource, extra_node_paths: &[String]) -> Vec<String> {
    if extra_node_paths.is_empty() {
        return Vec::new();
    }
    let mut merged = if let Some(resolved) = &pkg.resolved_location {
        bin_node_paths(resolved)
    } else {
        let dir = dunce::canonicalize(&pkg.location).unwrap_or_else(|_| pkg.location.clone());
        bin_node_paths(&dir)
    };
    for extra in extra_node_paths {
        if !merged.contains(extra) {
            merged.push(extra.clone());
        }
    }
    merged
}

/// Whether the bins of `pkg_name` get a PowerShell shim next to the `.cmd`
/// one. The pnpm CLI opts out, because PowerShell resolves `pnpm.ps1` ahead of
/// `pnpm.cmd`: a shim written for one installation of the CLI would keep
/// shadowing every later one, including an upgrade that ships a different
/// executable.
fn wants_powershell_shim(pkg_name: &str) -> bool {
    pkg_name != "pnpm"
}

/// Return `true` when `candidate` should replace `existing` for `bin_name`.
fn pick_winner(bin_name: &str, existing: &PackageBinSource, candidate: &PackageBinSource) -> bool {
    match (existing.origin, candidate.origin) {
        (BinOrigin::Hoisted, BinOrigin::Direct) => return true,
        (BinOrigin::Direct, BinOrigin::Hoisted) => return false,
        _ => {}
    }
    let existing_name = package_name(existing);
    let candidate_name = package_name(candidate);
    let existing_owns = pkg_owns_bin(bin_name, existing_name);
    let candidate_owns = pkg_owns_bin(bin_name, candidate_name);
    match (existing_owns, candidate_owns) {
        (true, false) => return false,
        (false, true) => return true,
        _ => {}
    }
    if candidate_name != existing_name {
        return candidate_name < existing_name;
    }
    match (package_version(existing), package_version(candidate)) {
        (Some(existing_version), Some(candidate_version)) => candidate_version > existing_version,
        _ => false,
    }
}

fn package_name(pkg: &PackageBinSource) -> &str {
    pkg.manifest.get("name").and_then(Value::as_str).unwrap_or("")
}

fn package_version(pkg: &PackageBinSource) -> Option<Version> {
    pkg.manifest
        .get("version")
        .and_then(Value::as_str)
        .and_then(|version| Version::parse(version).ok())
}

#[cfg(test)]
mod tests;

mod shim_writer;
use shim_writer::{ShimSpec, remove_stale_bin, write_shim};

mod executable;
use executable::{
    bin_node_paths, chmod_tolerating_removal, ensure_target_executable, is_node_bin_name,
    link_node_bin, link_symlinked_executable, symlink_already_points_at,
};

mod discovery;
