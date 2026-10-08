//! Dispatching the Rust tools to the toolchain a project's rustup
//! toolchain file names.
//!
//! The shims stand for the pseudo-package [`RUST_SHIM_PACKAGE`]. A
//! toolchain file selects only a release the Rust project signed, so the
//! toolchain is installed and run without the trust gate. It is installed
//! into the store pnpm's own configuration names, never one the project
//! does. Where no toolchain file applies, the toolchain `pnpm add -g rust`
//! installed runs, and without one the shim steps aside for the next
//! program of its name on `PATH`, such as a rustup proxy.

use super::{
    Candidate, ShimInvocation,
    run_program::{exec_program_with_bin_dirs, exec_program_with_env},
};
use pnpm_config::Config;
use pnpm_crypto_hash::create_hex_hash_bytes;
use pnpm_reporter::{LogEvent, Reporter};
use pnpm_rust_toolchain::{
    Channel, InstalledToolchain, ToolchainRequest, expire_resolution, find_toolchain_file,
    install_toolchain, installed_release, installed_toolchain, read_toolchain_file,
};
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

/// The package `pnpm shim add` takes for the Rust tools, and the key of
/// their `globalShims` entry.
pub(crate) const RUST_SHIM_PACKAGE: &str = "rust";

/// The tools a Rust toolchain installs that the shims stand for.
pub(crate) const RUST_SHIM_BINS: &[&str] =
    &["cargo", "cargo-clippy", "cargo-fmt", "clippy-driver", "rustc", "rustdoc", "rustfmt"];

/// Where toolchain installs anchor their configuration, for the same
/// reason as [`super::runtime_env::RUNTIME_ENVS_DIR_NAME`].
const RUST_ENVS_DIR_NAME: &str = "global-shim-rust";

/// The file in the global bin directory that names the toolchain
/// `pnpm add -g rust` installed, in the plain `rust-toolchain` format.
const GLOBAL_TOOLCHAIN_FILE: &str = ".pnpm-rust-toolchain";

/// The toolchain `pnpm add -g rust` installed for the shims in `bin_dir`.
pub(crate) fn global_toolchain(bin_dir: &Path) -> Option<ToolchainRequest> {
    read_toolchain_file(&bin_dir.join(GLOBAL_TOOLCHAIN_FILE)).ok()?.ok()
}

/// Make `channel` the toolchain the shims in `bin_dir` run outside a
/// project that names one.
pub(crate) fn record_global_toolchain(bin_dir: &Path, channel: &Channel) -> io::Result<()> {
    pnpm_fs::write_atomic(&bin_dir.join(GLOBAL_TOOLCHAIN_FILE), format!("{channel}\n").as_bytes())
}

/// Forget the global toolchain. Reports whether one was recorded.
pub(crate) fn remove_global_toolchain(bin_dir: &Path) -> io::Result<bool> {
    match std::fs::remove_file(bin_dir.join(GLOBAL_TOOLCHAIN_FILE)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Run the toolchain `pnpm add -g rust` installed, for an invocation no
/// project or rustup selection applies to, or the next `name` on `PATH`
/// when there is none.
pub(super) fn run_global_rust(
    shim: &ShimInvocation<'_>,
    args: &[OsString],
    state_dir: &Path,
) -> i32 {
    match global_toolchain(shim.bin_dir) {
        Some(request) => run_rust_toolchain(state_dir, &request, shim.name, args),
        None => run_next_on_path(shim, args),
    }
}

/// Install the toolchain `request` names into the store the shims run it
/// from, which pnpm's own configuration names.
pub(crate) async fn install_for_shims<Reporter: self::Reporter>(
    request: &ToolchainRequest,
) -> miette::Result<InstalledToolchain> {
    let state_dir = super::settings::trusted_shim_settings().state_dir;
    let config = trusted_rust_config(&state_dir)?;
    let client = crate::State::new_http_client(&config).map_err(miette::Report::new)?;
    install_toolchain::<Reporter>(&config, &client, request).await.map_err(miette::Report::new)
}

/// The release `request` last resolved to for the shims, and where it is
/// installed.
pub(crate) fn installed_release_for_shims(
    request: &ToolchainRequest,
) -> Option<(Channel, InstalledToolchain)> {
    let state_dir = super::settings::trusted_shim_settings().state_dir;
    installed_release(&trusted_rust_config(&state_dir).ok()?, request)
}

/// Make the next install of `request` for the shims ask the distribution
/// server which release a moving channel is now.
pub(crate) fn expire_for_shims(request: &ToolchainRequest) -> miette::Result<()> {
    let state_dir = super::settings::trusted_shim_settings().state_dir;
    expire_resolution(&trusted_rust_config(&state_dir)?, request);
    Ok(())
}

/// Who decides the toolchain a Rust tool run in some directory uses.
pub(super) enum RustSelection {
    /// The nearest toolchain file names a toolchain pnpm installs.
    Managed(Candidate),
    /// rustup does: the nearest toolchain file is one rustup handles itself,
    /// or a directory override (`rustup override set`) at or below the
    /// file's directory applies, which rustup ranks above the file.
    Rustup,
    /// Nothing about the directory does, so the machine's default applies.
    Unpinned,
}

/// Who decides the toolchain for `cwd`, reading the nearest toolchain file
/// at or above it and rustup's directory overrides in `rustup_settings`.
pub(super) fn find_rust_candidate(cwd: &Path, rustup_settings: Option<&Path>) -> RustSelection {
    let override_dir = rustup_settings.and_then(|settings| rustup_override_dir(cwd, settings));
    let file = cwd
        .ancestors()
        .last()
        .and_then(|root| find_toolchain_file(cwd, root));
    let Some((file, project_dir)) = file
        .as_deref()
        .and_then(|file| Some((file, file.parent()?)))
    else {
        return if override_dir.is_some() {
            RustSelection::Rustup
        } else {
            RustSelection::Unpinned
        };
    };
    if override_dir.is_some_and(|override_dir| override_dir.starts_with(project_dir)) {
        return RustSelection::Rustup;
    }
    managed_candidate(file, project_dir).map_or(RustSelection::Rustup, RustSelection::Managed)
}

fn managed_candidate(file: &Path, project_dir: &Path) -> Option<Candidate> {
    let bytes = std::fs::read(file).ok()?;
    let request = pnpm_rust_toolchain::read_toolchain_file(file).ok()?.ok()?;
    Some(Candidate::RustToolchain {
        project_dir: project_dir.to_path_buf(),
        request,
        identity: create_hex_hash_bytes(&bytes),
    })
}

/// Where rustup keeps the directory overrides `rustup override set` records.
pub(super) fn rustup_settings_file() -> Option<PathBuf> {
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| pnpm_config::home_dir().map(|home| home.join(".rustup")))?;
    Some(rustup_home.join("settings.toml"))
}

/// The nearest directory at or above `cwd` that rustup's `settings` file
/// records an override for.
fn rustup_override_dir(cwd: &Path, settings: &Path) -> Option<PathBuf> {
    let settings: toml::Table = std::fs::read_to_string(settings)
        .ok()?
        .parse()
        .ok()?;
    let overrides = settings.get("overrides")?.as_table()?;
    cwd.ancestors()
        .find(|dir| {
            dir.to_str()
                .is_some_and(|dir| overrides.contains_key(dir))
        })
        .map(Path::to_path_buf)
}

/// Whether the invocation picks its toolchain the way only rustup can: a
/// `+<toolchain>` first argument, or `RUSTUP_TOOLCHAIN`. Both outrank a
/// toolchain file for rustup, so they are left to it.
pub(super) fn overrides_toolchain(args: &[OsString]) -> bool {
    let plus_toolchain = args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|arg| arg.starts_with('+'));
    plus_toolchain || std::env::var_os("RUSTUP_TOOLCHAIN").is_some_and(|value| !value.is_empty())
}

/// Install the toolchain `request` names, if it is not in the store yet,
/// and run its `name` with its `bin` directory first on `PATH`, so a
/// `cargo` it runs finds the same toolchain's `rustc`.
pub(super) fn run_rust_toolchain(
    state_dir: &Path,
    request: &ToolchainRequest,
    name: &str,
    args: &[OsString],
) -> i32 {
    let toolchain = match prepare_toolchain(state_dir, request) {
        Ok(toolchain) => toolchain,
        Err(error) => {
            eprintln!("pnpm: failed to prepare Rust {}: {error:?}", request.channel);
            return 1;
        }
    };
    let program = toolchain.executable(name);
    if !program.is_file() {
        eprintln!(
            "pnpm: Rust {} as installed has no {name}. List the component that provides it under `components` in rust-toolchain.toml.",
            request.channel,
        );
        return 127;
    }
    // Cargo finds `cargo-clippy` and the other subcommands in
    // `$CARGO_HOME/bin` before `PATH`, where rustup keeps its proxies, and a
    // rustup proxy runs the toolchain `RUSTUP_TOOLCHAIN` names when it is a
    // path. A shim reached with it set leaves the choice to that proxy.
    exec_program_with_env(
        &program,
        &[toolchain.bin_dir()],
        &[("RUSTUP_TOOLCHAIN", toolchain.dir.as_os_str())],
        args,
    )
}

/// The toolchain `request` names, installed if it is not in the store yet.
/// One that is runs without the async runtime and the HTTP client an
/// install needs: editors run these tools over and over.
fn prepare_toolchain(
    state_dir: &Path,
    request: &ToolchainRequest,
) -> miette::Result<InstalledToolchain> {
    let config = trusted_rust_config(state_dir)?;
    if let Some(toolchain) = installed_toolchain(&config, request) {
        return Ok(toolchain);
    }
    crate::block_on_runtime("pacquet-global-shim-rust", async move {
        let client = crate::State::new_http_client(&config).map_err(miette::Report::new)?;
        install_toolchain::<StderrReporter>(&config, &client, request).await
            .map_err(miette::Report::new)
    })
}

/// Run the next `name` on `PATH` after the shim's own directory, for a
/// project whose toolchain pnpm does not manage, or a shim that is switched
/// off.
pub(super) fn run_next_on_path(shim: &ShimInvocation<'_>, args: &[OsString]) -> i32 {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let executable = pnpm_executor::current_executable().unwrap_or_default();
    let Some(program) = next_on_path(shim.name, shim.bin_dir, &executable, &path) else {
        eprintln!(
            "ERR_PNPM_SHIM_NO_TARGET  No rust-toolchain.toml names a toolchain for {} here, and no other {} is on PATH.",
            shim.name, shim.name,
        );
        return 127;
    };
    exec_program_with_bin_dirs(&program, &[], args)
}

/// The first `name` on `path` that is not a shim: outside `bin_dir`, which
/// holds the shim itself, and not a link back to `shim` from elsewhere,
/// which would run this dispatcher again. Only absolute entries are
/// searched, so a relative one cannot resolve against the project the shim
/// runs in.
fn next_on_path(name: &str, bin_dir: &Path, shim: &Path, path: &OsStr) -> Option<PathBuf> {
    let shim_dir = dunce::canonicalize(bin_dir).ok();
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .filter(|dir| dunce::canonicalize(dir).ok() != shim_dir)
        .filter_map(|dir| which::which_in(name, Some(&dir), &dir).ok())
        .find(|program| !same_file::is_same_file(program, shim).unwrap_or(false))
}

fn trusted_rust_config(state_dir: &Path) -> miette::Result<Config> {
    if state_dir.as_os_str().is_empty() {
        return Err(miette::miette!("the pnpm state directory could not be resolved"));
    }
    super::trusted_runtime_config(&state_dir.join(RUST_ENVS_DIR_NAME))
}

/// Reports what a toolchain install says on stderr, where it does not mix
/// with the output of the tool the shim runs.
struct StderrReporter;

impl Reporter for StderrReporter {
    fn emit(event: &LogEvent) {
        if let LogEvent::Global(log) = event {
            eprintln!("pnpm: {}", log.message);
        }
    }
}

#[cfg(test)]
mod tests;
