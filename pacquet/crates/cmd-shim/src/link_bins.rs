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
/// `package.json`. Mirrors the per-package input shape of pnpm's
/// `linkBinsOfPackages`.
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
    /// Where this candidate came from. Mirrors upstream's
    /// `isDirectDependency: boolean` flag at
    /// <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92>:
    /// when a hoisted (transitive) dep and a direct dep both
    /// declare the same bin name, the direct dep must win so a
    /// project never gets its own tooling silently shadowed by a
    /// transitive's bin. Defaults to [`BinOrigin::Direct`] —
    /// constructions via [`PackageBinSource::new`] don't have to
    /// supply the field, and existing call sites that don't yet
    /// distinguish keep the pre-[#342] ownership/lexical-only
    /// behavior. Pacquet's hoist + hoisted-linker passes use
    /// [`PackageBinSource::with_origin`] to tag transitive
    /// candidates as [`BinOrigin::Hoisted`].
    ///
    /// [#342]: https://github.com/pnpm/pacquet/issues/342
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
/// over a hoisted dep's bin with the same name. Mirrors upstream's
/// `preferDirectCmds` partition at
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92>
/// where direct candidates are kept and hoisted candidates with a
/// name collision are dropped.
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
    #[diagnostic(code(pacquet_cmd_shim::create_bin_dir))]
    CreateBinDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read modules directory at {dir:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::read_modules_dir))]
    ReadModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read package manifest at {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::read_manifest))]
    ReadManifest {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to parse package manifest at {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::parse_manifest))]
    ParseManifest {
        path: PathBuf,
        #[error(source)]
        error: serde_json::Error,
    },

    #[display("Failed to read shim source {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::probe_shim_source))]
    ProbeShimSource {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write shim file at {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::write_shim))]
    WriteShim {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to chmod {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::chmod))]
    Chmod {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove stale bin at {path:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::remove_stale_bin))]
    RemoveStaleBin {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to link node runtime binary {src:?} -> {dst:?}: {error}")]
    #[diagnostic(code(pacquet_cmd_shim::link_node_bin))]
    LinkNodeBin {
        src: PathBuf,
        dst: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Read `<location>/package.json` for each entry under `modules_dir` and link
/// its bins into `bins_dir`. Mirrors pnpm v11's `linkBins(modulesDir, binsDir)`
/// at <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts>.
///
/// Skips:
/// - The `.bin` and `.pacquet` directories themselves (and any other
///   dot-prefixed entry, matching pnpm).
/// - Entries whose `package.json` cannot be read (legitimate when a directory
///   under `node_modules` happens to not be a package, e.g. an empty scope
///   directory).
///
/// Scoped packages are recursed: `node_modules/@scope/foo` becomes one
/// candidate. This mirrors `binNamesAndPaths` in upstream `linkBins`.
pub fn link_bins<Sys>(modules_dir: &Path, bins_dir: &Path) -> Result<(), LinkBinsError>
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
    link_bins_of_packages::<Sys>(&packages, bins_dir)
}

fn collect_packages_in_modules_dir<Sys>(
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

/// Link every bin declared by `packages` into `bins_dir`, applying the same
/// conflict resolution upstream uses.
///
/// Conflict resolution mirrors `resolveCommandConflicts`:
///
/// 1. **Direct wins over Hoisted.** If exactly one candidate is
///    [`BinOrigin::Direct`], it wins outright — a direct dep's bin
///    must never be shadowed by a transitive's bin with the same
///    name. Mirrors upstream's `preferDirectCmds` partition at
///    <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92>.
/// 2. Ownership wins. If exactly one package owns the bin name (via
///    [`pkg_owns_bin`]), it wins outright.
/// 3. Otherwise lexical comparison on the package name, lower wins. Stable
///    and deterministic regardless of the order packages were discovered.
///
/// Pacquet's first iteration does not resolve same-package multi-version
/// conflicts via semver (a feature upstream uses for hoisting), since the
/// virtual-store layout means each bin source is a unique
/// `(package, version)` slot already.
pub fn link_bins_of_packages<Sys>(
    packages: &[PackageBinSource],
    bins_dir: &Path,
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
        write_shim::<Sys>(&command.path, &bins_dir.join(bin_name))
    })?;

    Ok(())
}

/// Return `true` when `candidate` should replace `existing` for `bin_name`.
/// Matches the three-step direct-then-ownership-then-lexical-compare in
/// upstream's `preferDirectCmds` + `resolveCommandConflicts`.
fn pick_winner(
    bin_name: &str,
    existing: &str,
    existing_origin: BinOrigin,
    candidate: &str,
    candidate_origin: BinOrigin,
) -> bool {
    // Highest tier: a Direct candidate beats a Hoisted incumbent and
    // a Direct incumbent shuts out a Hoisted candidate. When both
    // sides agree (both Direct or both Hoisted), fall through to the
    // ownership / lexical rules so the existing tier behavior is
    // unchanged inside each origin bucket. Mirrors upstream's
    // `preferDirectCmds` partition at
    // <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92>.
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
        // Both own (or neither): fall through to lexical compare. Picking the
        // smaller name keeps results deterministic across input orderings.
        _ => candidate < existing,
    }
}

/// Write the canonical bin shim for `target_path` at `shim_path`,
/// plus the `.cmd` and `.ps1` Windows-style siblings *when the host
/// is Windows*. Idempotent on warm reinstalls via
/// [`is_shim_pointing_at`].
///
/// Platform gating mirrors pnpm:
///
/// - `@zkochan/cmd-shim` defaults `createCmdFile: isWindows`
///   ([index.js#L32](https://github.com/pnpm/cmd-shim/blob/0d79ca9534/src/index.ts#L32)),
///   so `.cmd` only lands on Windows.
/// - pnpm's `bins.linker` overrides `createPwshFile` per call as
///   `POWER_SHELL_IS_SUPPORTED && manifest.name !== 'pnpm'`, where
///   [`POWER_SHELL_IS_SUPPORTED = IS_WINDOWS`](https://github.com/pnpm/pnpm/blob/29a42efc3b/bins/linker/src/index.ts#L28).
///   So `.ps1` also only lands on Windows.
///
/// Earlier versions of pacquet emitted all three flavors
/// unconditionally on the theory that a Linux-installed
/// `node_modules` should stay usable when carried to Windows via
/// network share or git clone. That doesn't match pnpm — pnpm's
/// Windows install rebuilds the shims on extraction — and produced
/// extra `.cmd`/`.ps1` files in every slot on Unix, splitting the
/// GVS file lists between the two tools (see the
/// `same_global_virtual_store_layout_*` parity tests).
///
/// The chmod step (`set_executable` for the canonical shim and
/// `ensure_executable_bits` for the target binary, matching pnpm's
/// `fixBin(cmd.path, 0o755)` and `chmodShim`) is wired through the
/// [`FsSetExecutable`] / [`FsEnsureExecutableBits`] capability traits.
/// On Unix the production impls run the actual `chmod`; on Windows
/// they are no-ops (Windows has no equivalent permission concept), so
/// the call sites stay portable and don't need their own
/// `#[cfg(unix)]` gating.
fn write_shim<Sys>(target_path: &Path, shim_path: &Path) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    // The node runtime binary is special: never wrap it in a shell
    // shim. Mirrors pnpm v11's `cmd.name === 'node'` short-circuit in
    // [`bins/linker/src/index.ts`](https://github.com/pnpm/pnpm/blob/06d2d3deb2/bins/linker/src/index.ts#L281-L308),
    // which symlinks the binary on Unix and hardlinks `node.exe` on
    // Windows.
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

    let sh_body = generate_sh_shim(target_path, shim_path, runtime.as_ref());
    // Windows siblings are off on Unix to match pnpm. The bodies
    // themselves still get computed inside the `cfg!(windows)` branch
    // below — moving the `generate_*` calls there keeps Unix builds
    // off the `relative_target_windows` allocation path entirely.
    let windows_shims = cfg!(windows).then(|| {
        let cmd_path = with_extension_appended(shim_path, "cmd");
        let ps1_path = with_extension_appended(shim_path, "ps1");
        let cmd_body = generate_cmd_shim(target_path, &cmd_path, runtime.as_ref());
        let ps1_body = generate_pwsh_shim(target_path, &ps1_path, runtime.as_ref());
        (cmd_path, cmd_body, ps1_path, ps1_body)
    });

    // Idempotent skip fires only when every flavor that *should* be
    // present is present and pointing at the right target. The `.sh`
    // flavor carries a `# cmd-shim-target=<path>` trailer that
    // [`is_shim_pointing_at`] reads; the `.cmd` and `.ps1` flavors
    // don't, so we compare them byte-for-byte against the freshly
    // generated body. That catches stale/corrupted siblings that an
    // existence-only check would let slip through (Copilot flagged
    // this on
    // <https://github.com/pnpm/pacquet/pull/333#discussion_r3222744353>):
    // a manually-edited `.cmd` pointing at a stale target, or an
    // earlier pacquet write with a different relative path, would
    // bypass the rewrite under the prior `.is_ok()` gate. Generated
    // bodies are stable across pacquet versions (only the `<target>`
    // segment moves), so byte equality is a sound equivalence check.
    let sh_marker_ok = matches!(
        Sys::read_to_string(shim_path),
        Ok(existing) if is_shim_pointing_at(&existing, target_path),
    );
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
        Sys::write(shim_path, sh_body.as_bytes())
            .map_err(|error| LinkBinsError::WriteShim { path: shim_path.to_path_buf(), error })?;
        if let Some((cmd_path, cmd_body, ps1_path, ps1_body)) = &windows_shims {
            Sys::write(cmd_path, cmd_body.as_bytes())
                .map_err(|error| LinkBinsError::WriteShim { path: cmd_path.clone(), error })?;
            Sys::write(ps1_path, ps1_body.as_bytes())
                .map_err(|error| LinkBinsError::WriteShim { path: ps1_path.clone(), error })?;
        }
    }

    Sys::set_executable(shim_path)
        .map_err(|error| LinkBinsError::Chmod { path: shim_path.to_path_buf(), error })?;
    // Make the underlying script executable too. pnpm calls
    // `fixBin(cmd.path, 0o755)` to do this; we apply the same minimum
    // mode without rewriting CRLF shebangs (a feature pnpm inherits
    // from npm's `bin-links/lib/fix-bin.js`). Targets shipped by npm
    // already use LF in practice, so the simpler chmod-only path is
    // enough for the install tests this PR ports. `NotFound` is
    // swallowed because the target may legitimately have been
    // removed by an unrelated process between extraction and shim
    // linking. Everything else (`PermissionDenied`, `EROFS`,
    // AppArmor deny, foreign uid) surfaces as `LinkBinsError::Chmod`
    // so real failures don't disappear silently. Mirrors pnpm's
    // `fixBin` ENOENT guard.
    match Sys::ensure_executable_bits(target_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(LinkBinsError::Chmod { path: target_path.to_path_buf(), error });
        }
    }

    Ok(())
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
/// Mirrors the two halves of pnpm's `cmd.name === 'node'` branch:
///
/// - **Unix** symlinks `shim_path` → absolute `target_path`. The
///   existing dirent (if any) is removed first because `fs::symlink`
///   rejects with `AlreadyExists` and we don't want to silently leave
///   a stale shim in place.
/// - **Windows** hardlinks `target_path` to `<shim_path>.exe`, falling
///   back to `fs::copy` on hardlink failure (cross-device, ACL deny,
///   ...). The source must end in `.exe`; otherwise pnpm falls through
///   to the cmd-shim path and so do we.
///
/// `remove_file` rather than `Sys::write`-style truncation is
/// load-bearing on both platforms: if `shim_path` is currently a
/// regular file hardlinked to the source binary (a state an earlier
/// pacquet revision could leave behind), truncating through the
/// hardlink would corrupt the binary itself. Removing the dirent
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

#[cfg(test)]
mod tests;
