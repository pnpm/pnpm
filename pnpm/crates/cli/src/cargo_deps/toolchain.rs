//! The Rust toolchain a rustup toolchain file in the checkout names,
//! installed by pnpm and linked beside the file.

use super::workspace_directory::{ensure_workspace_directory, force_workspace_symlink};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::{Config, RuntimeOnFail};
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_rust_toolchain::{
    TOOLCHAIN_LINK, Unmanaged, find_toolchain_file, install_toolchain, linked_bin_dir,
    read_toolchain_file,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Install the toolchain named by the toolchain file of each Cargo manifest,
/// and link it beside the file.
///
/// Every `runtimeOnFail` mode but `download` leaves the toolchain to
/// rustup, as it leaves an unmet runtime to the machine.
pub(super) async fn provision<Reporter: self::Reporter>(
    config: &Config,
    http_client: &ThrottledClient,
    manifests: &[PathBuf],
    checkout: Option<&Path>,
) -> Result<()> {
    let Some(checkout) = checkout else { return Ok(()) };
    if !matches!(config.runtime_on_fail, None | Some(RuntimeOnFail::Download)) {
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
        let request = match read_toolchain_file(&file)? {
            Ok(request) => request,
            Err(Unmanaged::UnknownChannel(channel)) => {
                Reporter::emit(&LogEvent::Global(GlobalLog {
                    level: LogLevel::Warn,
                    message: format!(
                        "pnpm does not install the Rust toolchain {} names: {channel:?} is not a release channel",
                        file.display(),
                    ),
                }));
                continue;
            }
            Err(Unmanaged::CustomPath | Unmanaged::NoChannel) => continue,
        };
        let toolchain = install_toolchain::<Reporter>(config, http_client, &request).await?;
        let dir = file.parent().expect("a toolchain file has a parent directory");
        link(dir, &toolchain.dir)?;
    }
    Ok(())
}

fn link(dir: &Path, toolchain: &Path) -> Result<()> {
    let (name, parents) = TOOLCHAIN_LINK.split_last().expect("the link has a name");
    let directory = ensure_workspace_directory(dir, parents)?;
    force_workspace_symlink(&directory, toolchain, name)
        .into_diagnostic()
        .wrap_err_with(|| format!("link the Rust toolchain into {}", directory.path.display()))?;
    Ok(())
}

/// `tool`, such as `cargo`, from the toolchain pnpm linked for `dir`, or
/// `tool` as `PATH` finds it where pnpm linked none.
pub(super) fn program(tool: &str, dir: &Path, checkout: Option<&Path>) -> PathBuf {
    checkout
        .zip(dunce::canonicalize(dir).ok())
        .and_then(|(checkout, dir)| linked_bin_dir(&dir, checkout))
        .map(|bin_dir| bin_dir.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)))
        .filter(|program| program.is_file())
        .unwrap_or_else(|| PathBuf::from(tool))
}
