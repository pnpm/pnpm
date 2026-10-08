//! The Rust toolchain a rustup toolchain file in the checkout names,
//! installed by pnpm and linked beside the file.

use super::workspace_directory::{ensure_workspace_directory, force_workspace_symlink};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::{Config, RuntimeOnFail};
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_rust_toolchain::{
    InstalledToolchain, RustToolchainError, TOOLCHAIN_LINK, ToolchainRequest, Unmanaged,
    find_toolchain_file, install_toolchain, read_toolchain_file,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// The toolchains this process installed or found in the store, by the
/// toolchain file that names them.
///
/// The commands pnpm runs during an install take their toolchain from here
/// rather than from the `.pnpm/rust` link, which the checkout could have
/// committed: installing must not run what the repository ships.
static PROVISIONED: Mutex<BTreeMap<PathBuf, InstalledToolchain>> = Mutex::new(BTreeMap::new());

/// Install the toolchain named by the toolchain file of each Cargo manifest,
/// and link it beside the file, where [`installs_toolchains`] allows it.
pub(super) async fn provision<Reporter: self::Reporter>(
    config: &Config,
    http_client: &ThrottledClient,
    manifests: &[PathBuf],
    checkout: Option<&Path>,
) -> Result<()> {
    let Some(checkout) = checkout else { return Ok(()) };
    if !installs_toolchains(config) {
        return Ok(());
    }
    let files = manifests
        .iter()
        .filter_map(|manifest| {
            let dir = dunce::canonicalize(manifest.parent()?).ok()?;
            find_toolchain_file(&dir, checkout)
        })
        .collect::<BTreeSet<_>>();
    // One toolchain is hundreds of megabytes, so they are downloaded in turn.
    for file in files {
        let Some(request) = managed_request::<Reporter>(&file)? else { continue };
        let toolchain = match install_toolchain::<Reporter>(config, http_client, &request).await {
            Ok(toolchain) => toolchain,
            Err(
                error @ (RustToolchainError::UnsupportedHost
                | RustToolchainError::NotPublishedForHost { .. }),
            ) => {
                leave_to_rustup::<Reporter>(&file, &error.to_string());
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let dir = file.parent().expect("a toolchain file has a parent directory");
        link(dir, &toolchain.dir)?;
        PROVISIONED
            .lock()
            .expect("the provisioned toolchains are not poisoned")
            .insert(file, toolchain);
    }
    Ok(())
}

/// What the toolchain file at `file` asks pnpm to install. `None` for a
/// file left to rustup.
fn managed_request<Reporter: self::Reporter>(file: &Path) -> Result<Option<ToolchainRequest>> {
    let reason = match read_toolchain_file(file)? {
        Ok(request) => return Ok(Some(request)),
        Err(Unmanaged::CustomPath | Unmanaged::NoChannel) => return Ok(None),
        Err(Unmanaged::UnknownChannel(channel)) => format!("{channel:?} is not a release channel"),
        Err(Unmanaged::UnsupportedProfile(profile)) => {
            format!("the {profile:?} profile is not supported")
        }
    };
    leave_to_rustup::<Reporter>(file, &reason);
    Ok(None)
}

/// Every `runtimeOnFail` mode but `download` leaves the toolchain to
/// rustup, as it leaves an unmet runtime to the machine.
pub(crate) fn installs_toolchains(config: &Config) -> bool {
    matches!(config.runtime_on_fail, None | Some(RuntimeOnFail::Download))
}

fn leave_to_rustup<Reporter: self::Reporter>(file: &Path, reason: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: format!(
            "pnpm does not install the Rust toolchain {} names: {reason}",
            file.display(),
        ),
    }));
}

fn link(dir: &Path, toolchain: &Path) -> Result<()> {
    let (name, parents) = TOOLCHAIN_LINK.split_last().expect("the link has a name");
    let directory = ensure_workspace_directory(dir, parents)?;
    force_workspace_symlink(&directory, toolchain, name)
        .into_diagnostic()
        .wrap_err_with(|| format!("link the Rust toolchain into {}", directory.path.display()))?;
    Ok(())
}

/// `tool`, such as `cargo`, from the toolchain pnpm provisioned for `dir`, or
/// `tool` as `PATH` finds it where pnpm provisioned none.
pub(super) fn program(tool: &str, dir: &Path, checkout: Option<&Path>) -> PathBuf {
    checkout
        .zip(dunce::canonicalize(dir).ok())
        .and_then(|(checkout, dir)| find_toolchain_file(&dir, checkout))
        .and_then(|file| {
            PROVISIONED
                .lock()
                .expect("the provisioned toolchains are not poisoned")
                .get(&file)
                .map(|toolchain| toolchain.executable(tool))
        })
        .unwrap_or_else(|| PathBuf::from(tool))
}

#[cfg(test)]
mod tests;
