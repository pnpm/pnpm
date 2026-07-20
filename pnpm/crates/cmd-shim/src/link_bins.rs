use crate::{
    bin_resolver::{Command, get_bins_from_package_manifest, pkg_owns_bin},
    capabilities::{
        FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
        FsSetExecutable, FsWalkFiles, FsWrite,
    },
    shim::{
        generate_cmd_shim, generate_pwsh_shim, generate_sh_shim, is_shim_pointing_at,
        search_script_runtime,
    },
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use rayon::prelude::*;
use serde_json::Value;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
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
}

impl PackageBinSource {
    /// Construct a [`PackageBinSource`] tagged as
    /// [`BinOrigin::Direct`]. Use this for direct-dependency
    /// candidates and for any call site that doesn't need to
    /// distinguish direct from hoisted (per-slot bin linking,
    /// most tests).
    #[must_use]
    pub fn new(location: PathBuf, manifest: Arc<Value>) -> Self {
        Self { location, manifest, origin: BinOrigin::Direct }
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
}

/// Read `<location>/package.json` for each entry under `modules_dir` and link
/// its bins into `bins_dir`. See [`link_bins_of_packages`] for the
/// `extra_node_paths` contract.
pub fn link_bins<Sys>(
    modules_dir: &Path,
    bins_dir: &Path,
    extra_node_paths: &[String],
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
    link_bins_of_packages::<Sys>(&packages, bins_dir, extra_node_paths)
}

/// Read the installed packages directly under `modules_dir`, including
/// scoped packages one directory deeper.
pub fn collect_packages_in_modules_dir<Sys>(
    modules_dir: &Path,
) -> Result<Vec<PackageBinSource>, LinkBinsError>
where
    Sys: FsReadDir + FsReadFile,
{
    let mut packages = Vec::new();

    let entries = match Sys::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(packages),
        Err(error) => {
            return Err(LinkBinsError::ReadModulesDir { dir: modules_dir.to_path_buf(), error });
        }
    };

    for path in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        if name_str.starts_with('@') {
            // Scoped: walk one level deeper. Only `NotFound` is
            // plausibly skippable (a concurrent scope-dir delete);
            // other errors — `PermissionDenied`, `EIO`, AppArmor
            // deny — would silently drop every bin under this
            // scope, so surface them as `ReadModulesDir`. Matches
            // the policy the per-`modules_dir` read above already
            // uses.
            let scope_entries = match Sys::read_dir(&path) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(LinkBinsError::ReadModulesDir { dir: path.clone(), error });
                }
            };
            for sub_path in scope_entries {
                if let Some(pkg) = read_package::<Sys>(&sub_path)? {
                    packages.push(pkg);
                }
            }
            continue;
        }

        if let Some(pkg) = read_package::<Sys>(&path)? {
            packages.push(pkg);
        }
    }

    Ok(packages)
}

fn read_package<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<PackageBinSource>, LinkBinsError> {
    let manifest_path = location.join("package.json");
    let bytes = match Sys::read_file(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(LinkBinsError::ReadManifest { path: manifest_path, error }),
    };
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|error| LinkBinsError::ParseManifest { path: manifest_path, error })?;
    Ok(Some(PackageBinSource::new(location.to_path_buf(), Arc::new(manifest))))
}

/// Link every bin declared by `packages` into `bins_dir`, applying conflict
/// resolution between bins of the same name.
///
/// `extra_node_paths` is pnpm's `extraNodePaths` (the hidden hoisted
/// modules dir under the isolated linker unless `extendNodePath:
/// false`). When non-empty, each shim carries a `NODE_PATH` block
/// listing the target's own `node_modules` dirs followed by these
/// entries; when empty the shims stay `NODE_PATH`-free.
///
/// Pacquet's first iteration does not resolve same-package multi-version
/// conflicts via semver (used elsewhere for hoisting), since the
/// virtual-store layout means each bin source is a unique
/// `(package, version)` slot already.
pub fn link_bins_of_packages<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    extra_node_paths: &[String],
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
    link_bins_of_packages_with_excludes::<Sys>(
        packages,
        bins_dir,
        &std::collections::HashSet::new(),
        extra_node_paths,
    )
}

/// Like [`link_bins_of_packages`] but skips any bin whose name is in
/// `exclude_bins`. Used by global install to leave bins legitimately
/// owned by an already-installed global package untouched.
pub fn link_bins_of_packages_with_excludes<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
    exclude_bins: &std::collections::HashSet<String>,
    extra_node_paths: &[String],
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
    let mut chosen: HashMap<String, (Command, &PackageBinSource)> = HashMap::new();

    for pkg in packages {
        let pkg_name = pkg.manifest.get("name").and_then(Value::as_str).unwrap_or("");
        let commands = get_bins_from_package_manifest::<Sys>(&pkg.manifest, &pkg.location);
        for command in commands {
            match chosen.get(&command.name) {
                None => {
                    chosen.insert(command.name.clone(), (command, pkg));
                }
                Some((_, existing)) => {
                    let existing_name =
                        existing.manifest.get("name").and_then(Value::as_str).unwrap_or("");
                    if pick_winner(
                        &command.name,
                        existing_name,
                        existing.origin,
                        pkg_name,
                        pkg.origin,
                    ) {
                        chosen.insert(command.name.clone(), (command, pkg));
                    }
                }
            }
        }
    }

    for excluded in exclude_bins {
        chosen.remove(excluded);
    }

    if chosen.is_empty() {
        return Ok(());
    }

    Sys::create_dir_all(bins_dir)
        .map_err(|error| LinkBinsError::CreateBinDir { dir: bins_dir.to_path_buf(), error })?;

    // Each shim's read-shebang + write-file + chmod sequence is independent
    // across bin names. There is no shared state, so drive them on rayon.
    // The hot path is per-package-bin; without parallelism the per-shim
    // file I/O serialised across the whole `chosen` map.
    chosen.par_iter().try_for_each(|(bin_name, (command, _pkg))| {
        write_shim::<Sys>(&command.path, &bins_dir.join(bin_name), extra_node_paths)
    })?;

    Ok(())
}

/// Return `true` when `candidate` should replace `existing` for `bin_name`.
/// Applies a three-step direct-then-ownership-then-lexical comparison.
fn pick_winner(
    bin_name: &str,
    existing: &str,
    existing_origin: BinOrigin,
    candidate: &str,
    candidate_origin: BinOrigin,
) -> bool {
    match (existing_origin, candidate_origin) {
        (BinOrigin::Hoisted, BinOrigin::Direct) => return true,
        (BinOrigin::Direct, BinOrigin::Hoisted) => return false,
        _ => {}
    }
    let existing_owns = pkg_owns_bin(bin_name, existing);
    let candidate_owns = pkg_owns_bin(bin_name, candidate);
    match (existing_owns, candidate_owns) {
        (true, false) => false,
        (false, true) => true,
        _ => candidate < existing,
    }
}

/// Write the canonical bin shim for `target_path` at `shim_path`,
/// plus the `.cmd` and `.ps1` Windows-style siblings *when the host
/// is Windows*. Idempotent on warm reinstalls via
/// [`is_shim_pointing_at`].
///
/// The chmod step (`set_executable` for the canonical shim and
/// `ensure_executable_bits` for the target binary) is wired through the
/// [`FsSetExecutable`] / [`FsEnsureExecutableBits`] capability traits.
/// On Unix the production impls run the actual `chmod`; on Windows
/// they are no-ops (Windows has no equivalent permission concept), so
/// the call sites stay portable and don't need their own
/// `#[cfg(unix)]` gating.
fn write_shim<Sys>(
    target_path: &Path,
    shim_path: &Path,
    extra_node_paths: &[String],
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    // The node runtime binary is special: never wrap it in a shell
    // shim. The binary is symlinked on Unix and `node.exe` is
    // hardlinked on Windows.
    //
    // Two reasons this matters:
    //
    // 1. Parity. pnpm install in the same workspace symlinks `.bin/node`
    //    to the runtime binary; pacquet must do the same so the
    //    `same_global_virtual_store_layout_*` checks see the same
    //    dirent shape.
    // 2. Robustness against accidental shim-wrapping. The node binary
    //    itself has no shebang, but a prior bad install may leave a
    //    cmd-shim text file with `#!/bin/sh` at `<pkg>/bin/node`. If
    //    pacquet then cmd-shims that file, `search_script_runtime`
    //    parses the shebang as `prog: "/bin/sh"` and emits a shim
    //    whose target resolves to a non-existent path
    //    (`$basedir/../node/bin/../node/bin/node` — the `node` segment
    //    appears twice). A direct symlink / hardlink bypasses the
    //    parser entirely.
    if is_node_bin_name(shim_path) && link_node_bin(target_path, shim_path)? {
        return Ok(());
    }

    let runtime = search_script_runtime::<Sys>(target_path).map_err(|error| {
        LinkBinsError::ProbeShimSource { path: target_path.to_path_buf(), error }
    })?;

    // pnpm's shim `NODE_PATH`: the target's own `node_modules` dirs
    // first, then the caller's extras that aren't already present.
    // An empty extras list means "no NODE_PATH in shims at all"
    // (`extendNodePath: false`, a non-isolated linker, or no hoist
    // pattern), matching pnpm's bins linker.
    let node_path: Vec<String> = if extra_node_paths.is_empty() {
        Vec::new()
    } else {
        let mut merged = bin_node_paths(target_path);
        for extra in extra_node_paths {
            if !merged.contains(extra) {
                merged.push(extra.clone());
            }
        }
        merged
    };

    let sh_body = generate_sh_shim(target_path, shim_path, runtime.as_ref(), &node_path);
    // Windows siblings are off on Unix to match pnpm. The bodies
    // themselves still get computed inside the `cfg!(windows)` branch
    // below — moving the `generate_*` calls there keeps Unix builds
    // off the `relative_target_windows` allocation path entirely.
    let windows_shims = cfg!(windows).then(|| {
        let cmd_path = with_extension_appended(shim_path, "cmd");
        let ps1_path = with_extension_appended(shim_path, "ps1");
        let cmd_body = generate_cmd_shim(target_path, &cmd_path, runtime.as_ref(), &node_path);
        let ps1_body = generate_pwsh_shim(target_path, &ps1_path, runtime.as_ref(), &node_path);
        (cmd_path, cmd_body, ps1_path, ps1_body)
    });

    // Idempotent skip fires only when every flavor that *should* be
    // present is present and pointing at the right target. The `.sh`
    // flavor carries a `# cmd-shim-target=<path>` trailer that
    // [`is_shim_pointing_at`] reads; the `.cmd` and `.ps1` flavors
    // don't, so we compare them byte-for-byte against the freshly
    // generated body. That catches stale/corrupted siblings that an
    // existence-only check would let slip through: a manually-edited
    // `.cmd` pointing at a stale target, or a pacquet write with a
    // different relative path. Generated bodies are stable across
    // pacquet versions (only the `<target>` segment moves), so byte
    // equality is a sound equivalence check.
    //
    // When a `NODE_PATH` block is expected, the marker alone can't
    // prove the shim carries the right (or any) block, so require
    // byte equality; the marker-only branch additionally rejects a
    // stale `NODE_PATH` block when none is expected.
    let sh_marker_ok = match Sys::read_to_string(shim_path) {
        Ok(existing) if !node_path.is_empty() => existing == sh_body,
        Ok(existing) => {
            is_shim_pointing_at(&existing, target_path) && !existing.contains("NODE_PATH")
        }
        Err(_) => false,
    };
    let windows_ok = match &windows_shims {
        None => true,
        Some((cmd_path, cmd_body, ps1_path, ps1_body)) => {
            let cmd_ok = matches!(
                Sys::read_to_string(cmd_path),
                Ok(existing) if &existing == cmd_body,
            );
            let ps1_ok = matches!(
                Sys::read_to_string(ps1_path),
                Ok(existing) if &existing == ps1_body,
            );
            cmd_ok && ps1_ok
        }
    };
    let already_correct = sh_marker_ok && windows_ok;

    if !already_correct {
        // Unlink any pre-existing entry before writing. `Sys::write` opens
        // through a symlink, so without this a symlink planted at the bin
        // path (e.g. in a shared/writable global bin dir) would redirect the
        // write and clobber an arbitrary target. Removing first guarantees we
        // create a fresh regular file.
        remove_stale_bin(shim_path)?;
        Sys::write(shim_path, sh_body.as_bytes())
            .map_err(|error| LinkBinsError::WriteShim { path: shim_path.to_path_buf(), error })?;
        if let Some((cmd_path, cmd_body, ps1_path, ps1_body)) = &windows_shims {
            remove_stale_bin(cmd_path)?;
            Sys::write(cmd_path, cmd_body.as_bytes())
                .map_err(|error| LinkBinsError::WriteShim { path: cmd_path.clone(), error })?;
            remove_stale_bin(ps1_path)?;
            Sys::write(ps1_path, ps1_body.as_bytes())
                .map_err(|error| LinkBinsError::WriteShim { path: ps1_path.clone(), error })?;
        }
    }

    Sys::set_executable(shim_path)
        .map_err(|error| LinkBinsError::Chmod { path: shim_path.to_path_buf(), error })?;
    // Make the underlying script executable too: apply a minimum mode
    // of 0o755 without rewriting CRLF shebangs. Targets shipped by npm
    // already use LF in practice, so the simpler chmod-only path is
    // enough for the install tests this PR ports. `NotFound` is
    // swallowed because the target may legitimately have been
    // removed by an unrelated process between extraction and shim
    // linking.
    match Sys::ensure_executable_bits(target_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(LinkBinsError::Chmod { path: target_path.to_path_buf(), error });
        }
    }

    Ok(())
}

/// The `node_modules` directories relevant to a bin target in the
/// virtual-store layout — pnpm's `getBinNodePaths`. For a bin at
/// `.pnpm/pkg@ver/node_modules/pkg/bin/cli.js` this returns the
/// package's own `node_modules` (bundled deps) followed by the slot's
/// `node_modules` (sibling deps), so tools that resolve from CWD
/// (`import-local` in jest, eslint, ...) find the correct versions.
/// The target directory is realpath-resolved first so a symlinked
/// direct dependency yields its virtual-store slot paths.
fn bin_node_paths(target: &Path) -> Vec<String> {
    let target_dir = target.parent().unwrap_or_else(|| Path::new(""));
    let dir = dunce::canonicalize(target_dir).unwrap_or_else(|_| target_dir.to_path_buf());
    let Some(node_modules_dir) = dir.ancestors().find(|ancestor| {
        ancestor.file_name().is_some_and(|name| name == "node_modules")
            && ancestor
                .parent()
                .and_then(Path::file_name)
                .is_none_or(|parent_name| parent_name != "node_modules")
    }) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    if let Ok(rel) = dir.strip_prefix(node_modules_dir)
        && let Some(first) = rel.components().next()
    {
        let first_name = first.as_os_str().to_string_lossy();
        let pkg_dir = if first_name.starts_with('@') {
            match rel.components().nth(1) {
                Some(second) => node_modules_dir.join(first).join(second.as_os_str()),
                None => node_modules_dir.join(first),
            }
        } else {
            node_modules_dir.join(first)
        };
        result.push(pkg_dir.join("node_modules").to_string_lossy().into_owned());
    }
    result.push(node_modules_dir.to_string_lossy().into_owned());
    result
}

/// Whether `shim_path`'s file name is exactly `node` — the trigger for the
/// node-runtime short-circuit in [`write_shim`]. Lifted out so the check
/// is unit-testable and the call site reads as a predicate.
fn is_node_bin_name(shim_path: &Path) -> bool {
    matches!(shim_path.file_name().and_then(|s| s.to_str()), Some("node"))
}

/// Link the node runtime binary `target_path` into the bin slot
/// `shim_path` directly, without a cmd-shim wrapper. Returns `Ok(true)`
/// when the special case took effect (the caller must skip the regular
/// shim-writing path) and `Ok(false)` when it didn't apply and the
/// caller should fall through (Windows non-`.exe` source).
///
/// Two halves, by platform:
///
/// - **Unix** symlinks `shim_path` → absolute `target_path`. The
///   existing dirent (if any) is removed first because `fs::symlink`
///   rejects with `AlreadyExists` and we don't want to silently leave
///   a stale shim in place.
/// - **Windows** hardlinks `target_path` to `<shim_path>.exe`, falling
///   back to `fs::copy` on hardlink failure (cross-device, ACL deny,
///   ...). The source must end in `.exe`; otherwise the caller falls
///   through to the cmd-shim path.
///
/// `remove_file` rather than `Sys::write`-style truncation is
/// load-bearing on both platforms: if `shim_path` is currently a
/// regular file hardlinked to the source binary, truncating through
/// the hardlink would corrupt the binary itself. Removing the dirent
/// leaves the hardlinked content intact.
#[cfg(unix)]
fn link_node_bin(target_path: &Path, shim_path: &Path) -> Result<bool, LinkBinsError> {
    use std::os::unix::fs::symlink;
    remove_stale_bin(shim_path)?;
    symlink(target_path, shim_path).map_err(|error| LinkBinsError::LinkNodeBin {
        src: target_path.to_path_buf(),
        dst: shim_path.to_path_buf(),
        error,
    })?;
    Ok(true)
}

#[cfg(windows)]
fn link_node_bin(target_path: &Path, shim_path: &Path) -> Result<bool, LinkBinsError> {
    use std::fs;
    let is_exe = target_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return Ok(false);
    }
    let exe_path = with_extension_appended(shim_path, "exe");
    // Skip the remove + relink churn on warm installs when `node.exe`
    // already refers to the source binary.
    if is_same_file(&exe_path, target_path) {
        return Ok(true);
    }
    remove_stale_bin(&exe_path)?;
    if fs::hard_link(target_path, &exe_path).is_err() {
        fs::copy(target_path, &exe_path).map_err(|error| LinkBinsError::LinkNodeBin {
            src: target_path.to_path_buf(),
            dst: exe_path,
            error,
        })?;
    }
    Ok(true)
}

/// Whether `a` and `b` are the same file. [`same_file::Handle`] proves a hard
/// link cheaply via the OS file identity (device + inode on Unix, file index +
/// volume serial on Windows). When that identity can't be obtained — a missing
/// file, or a filesystem that doesn't expose a stable index — we fall back to
/// comparing the file contents after a quick size check, which also treats a
/// byte-identical copy as the same file.
#[cfg(windows)]
fn is_same_file(a: &Path, b: &Path) -> bool {
    if let (Ok(handle_a), Ok(handle_b)) =
        (same_file::Handle::from_path(a), same_file::Handle::from_path(b))
        && handle_a == handle_b
    {
        return true;
    }
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(meta_a), Ok(meta_b)) => meta_a.len() == meta_b.len() && have_equal_contents(a, b),
        _ => false,
    }
}

/// Compare two equally-sized files chunk by chunk, so an executable is never
/// fully buffered in memory and a mismatch returns as early as possible.
#[cfg(windows)]
fn have_equal_contents(a: &Path, b: &Path) -> bool {
    const CHUNK_SIZE: usize = 64 * 1024;
    let (Ok(mut file_a), Ok(mut file_b)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let mut buf_a = vec![0u8; CHUNK_SIZE];
    let mut buf_b = vec![0u8; CHUNK_SIZE];
    loop {
        let (Ok(read_a), Ok(read_b)) =
            (read_chunk(&mut file_a, &mut buf_a), read_chunk(&mut file_b, &mut buf_b))
        else {
            return false;
        };
        if read_a != read_b {
            return false;
        }
        if read_a == 0 {
            return true;
        }
        if buf_a[..read_a] != buf_b[..read_b] {
            return false;
        }
    }
}

/// Read up to `buf.len()` bytes, looping over short reads so a full chunk is
/// only short at end of file. Like [`std::io::Read::read_exact`] but tolerant
/// of EOF.
#[cfg(windows)]
fn read_chunk(reader: &mut impl std::io::Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Remove an existing dirent at `path`, swallowing `NotFound`. Used by
/// [`link_node_bin`] to clear any prior shim / symlink / hardlink
/// before laying down the new one. Any other IO error (`PermissionDenied`,
/// EROFS, `AppArmor` deny, ...) surfaces as [`LinkBinsError::RemoveStaleBin`]
/// so a real failure isn't hidden behind a silent skip.
fn remove_stale_bin(path: &Path) -> Result<(), LinkBinsError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LinkBinsError::RemoveStaleBin { path: path.to_path_buf(), error }),
    }
}

/// Append `<ext>` to `path` as a *new* extension segment (`foo` becomes
/// `foo.cmd`), regardless of any existing extension. `Path::with_extension`
/// would *replace* the existing extension, which is wrong for our case.
/// The bin name `tsc` keeps its own `tsc` and gains a sibling `tsc.cmd`,
/// rather than turning into `tsc.cmd` and losing the original `.sh` flavor.
fn with_extension_appended(path: &Path, ext: &str) -> PathBuf {
    let mut result = path.as_os_str().to_owned();
    result.push(".");
    result.push(ext);
    result.into()
}

/// Remove a bin shim previously written by [`link_bins_of_packages`].
///
/// Deletes `<name>`, plus the `<name>.ps1`, `<name>.cmd`, and `<name>.exe`
/// flavors on Windows; just `<name>` elsewhere. The `<name>.exe` flavor
/// matters because the `node` runtime bin is linked as `<name>.exe` by the
/// linker's node special-case, so without this a `node.exe` would survive
/// `remove -g` / `update -g` and stay reachable on `PATH`. A missing file is
/// not an error (rimraf-style).
pub fn remove_bin(bin_path: &Path) -> io::Result<()> {
    remove_if_exists(bin_path)?;
    if cfg!(windows) {
        remove_if_exists(&with_extension_appended(bin_path, "ps1"))?;
        remove_if_exists(&with_extension_appended(bin_path, "cmd"))?;
        remove_if_exists(&with_extension_appended(bin_path, "exe"))?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
