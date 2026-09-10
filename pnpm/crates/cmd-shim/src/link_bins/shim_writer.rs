use super::{
    DirCreation, FsEnsureExecutableBits, FsReadHead, FsReadToString, FsSetExecutable, FsWrite,
    LinkBinsError, Path, PathBuf, ScriptRuntime, ShimTargetCache, chmod_tolerating_removal,
    generate_cmd_shim, generate_pwsh_shim, generate_sh_shim, io, is_node_bin_name,
    is_shim_pointing_at, link_node_bin, link_symlinked_executable, symlink_already_points_at,
};

/// Write the canonical bin shim for `target_path` at `shim_path`,
/// plus the `.cmd` and `.ps1` Windows-style siblings *when the host
/// is Windows*. Idempotent on warm reinstalls via
/// [`is_shim_pointing_at`].
///
/// `make_powershell_shim` (see [`wants_powershell_shim`](super::wants_powershell_shim)) drops the
/// `.ps1` sibling, and deletes any that an earlier install left
/// behind.
///
/// The chmod step (`set_executable` for the canonical shim and
/// `ensure_executable_bits` for the target binary) is wired through the
/// [`FsSetExecutable`] / [`FsEnsureExecutableBits`] capability traits.
/// On Unix the production impls run the actual `chmod`; on Windows
/// they are no-ops (Windows has no equivalent permission concept), so
/// the call sites stay portable and don't need their own
/// `#[cfg(unix)]` gating.
/// The per-bin inputs one [`write_shim`] call consumes.
#[derive(Clone, Copy)]
pub(super) struct ShimSpec<'a> {
    /// The bin file the shim executes, reached through the package's
    /// `node_modules` location — the path the shim body embeds.
    pub(super) target_path: &'a Path,
    /// [`target_path`](Self::target_path) with the package symlink
    /// resolved, used for the per-target probes (script runtime,
    /// executable bits) and as their [`ShimTargetCache`] key.
    pub(super) probe_path: &'a Path,
    pub(super) shim_path: &'a Path,
    pub(super) node_path: &'a [String],
    pub(super) prefer_symlinked_executables: bool,
    pub(super) make_powershell_shim: bool,
    /// Whether this run created the bin directory. Read by
    /// [`read_or_create_shim`], which documents what it is worth.
    pub(super) bin_dir: DirCreation,
}

pub(super) fn write_shim<Sys>(
    spec: ShimSpec<'_>,
    cache: &ShimTargetCache,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    // Not writing a `.ps1` is not enough to keep one out of the bin
    // dir: an install that did want one leaves it there, and
    // PowerShell keeps preferring it over the `.cmd` sibling. Delete
    // it up front, above the short-circuits below — they all return
    // without touching the Windows siblings.
    if !spec.make_powershell_shim {
        remove_stale_bin(&with_extension_appended(spec.shim_path, "ps1"))?;
    }

    let existing_shim = match read_or_create_shim::<Sys>(&spec, cache)? {
        ExistingShim::Written => return Ok(()),
        ExistingShim::Present(existing) => Some(existing),
        ExistingShim::Absent => None,
    };

    // pnpm's warm-install short-circuit: an existing symlink that
    // already resolves to the target is correct as-is — regardless of
    // `preferSymlinkedExecutables` — so a relink pass that doesn't
    // carry the setting (the injected-deps syncer's workspace-wide
    // relink, for one) leaves symlinked bins alone instead of
    // rewriting them into shims.
    if symlink_already_points_at(spec.shim_path, spec.target_path) {
        return cache.ensure_target_executable_once::<Sys>(spec.probe_path);
    }

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
    if is_node_bin_name(spec.shim_path) && link_node_bin(spec.target_path, spec.shim_path)? {
        return Ok(());
    }

    // pnpm's `preferSymlinkedExecutables`: link the bin file directly
    // instead of wrapping it in a shell shim. Unix only — the Windows
    // half returns `false` so bins keep their shims there, like pnpm.
    // Stays below the node-runtime special case, which links `node`
    // regardless of the setting.
    if spec.prefer_symlinked_executables
        && link_symlinked_executable::<Sys>(spec.target_path, spec.shim_path)?
    {
        return Ok(());
    }

    let runtime = cache.runtime_for::<Sys>(spec.probe_path).map_err(|error| {
        LinkBinsError::ProbeShimSource { path: spec.probe_path.to_path_buf(), error }
    })?;

    let sh_body =
        generate_sh_shim(spec.target_path, spec.shim_path, runtime.as_ref(), spec.node_path);
    let windows_shims = windows_shim_bodies(&spec, runtime.as_ref());

    let current = shim_body_matches(existing_shim.as_deref(), &sh_body, &spec)
        && windows_shims_match::<Sys>(windows_shims.as_ref());
    if !current {
        replace_shims::<Sys>(spec.shim_path, &sh_body, windows_shims.as_ref())?;
    }

    chmod_tolerating_removal(spec.shim_path, Sys::set_executable)?;
    cache.ensure_target_executable_once::<Sys>(spec.probe_path)?;

    Ok(())
}

/// What occupies the shim path.
enum ExistingShim {
    /// Nothing did, and a fresh shim was written in place.
    Written,
    /// The shim path holds this content.
    Present(String),
    /// Nothing is there, and the fresh-write path did not apply.
    Absent,
}

/// Read whatever occupies the shim path, taking the fresh-write fast path when
/// nothing does.
///
/// One read feeds both paths: `NotFound` means nothing occupies the shim path,
/// so a fresh install skips every stale-entry probe; existing content feeds the
/// marker checks without a second read.
///
/// In a bin directory this run created, that read almost never finds
/// anything, so the write goes first and falls back to the read when it
/// fails — see [`FsCreateDirAll::create_dir_all_reporting`](crate::capabilities::FsCreateDirAll::create_dir_all_reporting) for what
/// makes it "almost". Trying the write first anywhere else would add a
/// failed create to every shim already in place, which is what an
/// ordinary reinstall is made of.
fn read_or_create_shim<Sys>(
    spec: &ShimSpec<'_>,
    cache: &ShimTargetCache,
) -> Result<ExistingShim, LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    if spec.bin_dir == DirCreation::Created
        && fresh_write_applies(spec)
        && write_shim_fresh::<Sys>(spec, cache)?
    {
        return Ok(ExistingShim::Written);
    }
    let error = match Sys::read_to_string(spec.shim_path) {
        Ok(existing) => return Ok(ExistingShim::Present(existing)),
        Err(error) => error,
    };
    let fresh = error.kind() == io::ErrorKind::NotFound
        && fresh_write_applies(spec)
        && write_shim_fresh::<Sys>(spec, cache)?;
    Ok(if fresh { ExistingShim::Written } else { ExistingShim::Absent })
}

/// Whether the shim is one [`write_shim_fresh`] can produce. The node
/// runtime is linked rather than shimmed, and
/// `preferSymlinkedExecutables` links every bin on Unix.
fn fresh_write_applies(spec: &ShimSpec<'_>) -> bool {
    !(is_node_bin_name(spec.shim_path) || (spec.prefer_symlinked_executables && cfg!(unix)))
}

/// The Windows sibling shims a write produces.
struct WindowsShims {
    cmd_path: PathBuf,
    cmd_body: String,
    powershell: Option<(PathBuf, String)>,
}

/// Generate the Windows siblings. They are off on Unix to match pnpm, and the
/// bodies are computed only under `cfg!(windows)` so Unix builds stay off the
/// `relative_target_windows` allocation path entirely.
fn windows_shim_bodies(
    spec: &ShimSpec<'_>,
    runtime: Option<&ScriptRuntime>,
) -> Option<WindowsShims> {
    cfg!(windows).then(|| {
        let cmd_path = with_extension_appended(spec.shim_path, "cmd");
        let cmd_body = generate_cmd_shim(spec.target_path, &cmd_path, runtime, spec.node_path);
        let powershell = spec.make_powershell_shim.then(|| {
            let ps1_path = with_extension_appended(spec.shim_path, "ps1");
            let ps1_body = generate_pwsh_shim(spec.target_path, &ps1_path, runtime, spec.node_path);
            (ps1_path, ps1_body)
        });
        WindowsShims { cmd_path, cmd_body, powershell }
    })
}

/// Whether the shim already on disk points at the right target.
///
/// The `.sh` flavor carries a `# cmd-shim-target=<path>` trailer that
/// [`is_shim_pointing_at`] reads. When a `NODE_PATH` block is expected the
/// marker alone cannot prove the shim carries the right (or any) block, so
/// byte equality is required; the marker-only branch additionally rejects a
/// stale `NODE_PATH` block when none is expected. The probe looks for the
/// exact export the block opens with, so a target path that merely mentions
/// `NODE_PATH` cannot force a rewrite.
fn shim_body_matches(existing: Option<&str>, sh_body: &str, spec: &ShimSpec<'_>) -> bool {
    let Some(existing) = existing else {
        return false;
    };
    if !spec.node_path.is_empty() {
        return existing == sh_body;
    }
    is_shim_pointing_at(existing, spec.target_path) && !existing.contains("export NODE_PATH=")
}

/// Whether every Windows sibling that should be present is present and
/// byte-identical to what would be written.
///
/// The `.cmd` and `.ps1` flavors carry no target marker, so they are compared
/// byte for byte. That catches stale or corrupted siblings an existence-only
/// check would let slip through: a manually-edited `.cmd` pointing at a stale
/// target, or a pacquet write with a different relative path. Generated bodies
/// are stable across pacquet versions (only the `<target>` segment moves), so
/// byte equality is a sound equivalence check.
fn windows_shims_match<Sys>(windows_shims: Option<&WindowsShims>) -> bool
where
    Sys: FsReadToString,
{
    let Some(shims) = windows_shims else {
        return true;
    };
    let cmd_ok = matches!(
        Sys::read_to_string(&shims.cmd_path),
        Ok(existing) if existing == shims.cmd_body,
    );
    // A suppressed `.ps1` is already gone, so there is nothing left to compare.
    let ps1_ok = shims.powershell.as_ref().is_none_or(|(ps1_path, ps1_body)| {
        matches!(Sys::read_to_string(ps1_path), Ok(existing) if &existing == ps1_body)
    });
    cmd_ok && ps1_ok
}

/// Write every shim flavor, replacing whatever is there.
fn replace_shims<Sys>(
    shim_path: &Path,
    sh_body: &str,
    windows_shims: Option<&WindowsShims>,
) -> Result<(), LinkBinsError>
where
    Sys: FsWrite,
{
    replace_shim::<Sys>(shim_path, sh_body.as_bytes())?;
    let Some(shims) = windows_shims else {
        return Ok(());
    };
    replace_shim::<Sys>(&shims.cmd_path, shims.cmd_body.as_bytes())?;
    if let Some((ps1_path, ps1_body)) = &shims.powershell {
        replace_shim::<Sys>(ps1_path, ps1_body.as_bytes())?;
    }
    Ok(())
}

/// Create the shim(s) at a path nothing occupies. The exclusive create
/// refuses any pre-existing dirent — a dangling symlink included — so no
/// removal or content probe is needed first. `Ok(false)` sends the
/// caller to the general path: the create lost a race, found a dirent
/// after all, or `Sys` doesn't support exclusive creation.
fn write_shim_fresh<Sys>(
    spec: &ShimSpec<'_>,
    cache: &ShimTargetCache,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    let &ShimSpec { target_path, probe_path, shim_path, node_path, make_powershell_shim, .. } =
        spec;
    let runtime = cache.runtime_for::<Sys>(probe_path).map_err(|error| {
        LinkBinsError::ProbeShimSource { path: probe_path.to_path_buf(), error }
    })?;
    let sh_body = generate_sh_shim(target_path, shim_path, runtime.as_ref(), node_path);
    // Any failure — a lost race, a dangling symlink squatting on the
    // path, a `Sys` without exclusive creation — goes to the general
    // path, whose replacement is atomic. No content is ever read back
    // through the path here: a read would follow a raced symlink.
    if Sys::write_new(shim_path, sh_body.as_bytes()).is_err() {
        return Ok(false);
    }
    if cfg!(windows) {
        // The Windows siblings keep the replace shape: a missing
        // canonical shim proves nothing about `.cmd`/`.ps1` leftovers.
        let cmd_path = with_extension_appended(shim_path, "cmd");
        let cmd_body = generate_cmd_shim(target_path, &cmd_path, runtime.as_ref(), node_path);
        replace_shim::<Sys>(&cmd_path, cmd_body.as_bytes())?;
        if make_powershell_shim {
            let ps1_path = with_extension_appended(shim_path, "ps1");
            let ps1_body = generate_pwsh_shim(target_path, &ps1_path, runtime.as_ref(), node_path);
            replace_shim::<Sys>(&ps1_path, ps1_body.as_bytes())?;
        }
    }
    chmod_tolerating_removal(shim_path, Sys::set_executable)?;
    cache.ensure_target_executable_once::<Sys>(probe_path)?;
    Ok(true)
}

/// Replace whatever occupies `path` with a fresh regular file holding
/// `bytes`, atomically where `Sys` supports it (see
/// [`FsWrite::write_replace`]): no reader observes a torn shim,
/// concurrent installers writing the equivalent shim converge on
/// last-writer-wins, and a symlink planted at the path — the classic
/// unlink/symlink race in a shared or writable bin dir — is replaced as
/// a dirent, never followed. A `Sys` without atomic replacement (the DI
/// fakes' default) keeps the remove-then-write.
fn replace_shim<Sys: FsWrite>(path: &Path, bytes: &[u8]) -> Result<(), LinkBinsError> {
    match Sys::write_replace(path, bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            remove_stale_bin(path)?;
            Sys::write(path, bytes)
                .map_err(|error| LinkBinsError::WriteShim { path: path.to_path_buf(), error })
        }
        Err(error) => Err(LinkBinsError::WriteShim { path: path.to_path_buf(), error }),
    }
}

/// Remove an existing dirent at `path`, swallowing `NotFound`. Used by
/// [`link_node_bin`] to clear any prior shim / symlink / hardlink
/// before laying down the new one. Any other IO error (`PermissionDenied`,
/// EROFS, `AppArmor` deny, ...) surfaces as [`LinkBinsError::RemoveStaleBin`]
/// so a real failure isn't hidden behind a silent skip.
pub(super) fn remove_stale_bin(path: &Path) -> Result<(), LinkBinsError> {
    remove_if_exists(path)
        .map_err(|error| LinkBinsError::RemoveStaleBin { path: path.to_path_buf(), error })
}

/// Append `<ext>` to `path` as a *new* extension segment (`foo` becomes
/// `foo.cmd`), regardless of any existing extension. `Path::with_extension`
/// would *replace* the existing extension, which is wrong for our case.
/// The bin name `tsc` keeps its own `tsc` and gains a sibling `tsc.cmd`,
/// rather than turning into `tsc.cmd` and losing the original `.sh` flavor.
pub(super) fn with_extension_appended(path: &Path, ext: &str) -> PathBuf {
    let mut result = path.as_os_str().to_owned();
    result.push(".");
    result.push(ext);
    result.into()
}

/// Remove a bin shim previously written by [`link_bins_of_packages`](super::link_bins_of_packages).
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
    match pnpm_fs::remove_file_with_retry(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
