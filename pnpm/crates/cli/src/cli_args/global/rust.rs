//! `rust` as a global package: the Rust toolchain the Rust tool shims run
//! outside a project whose `rust-toolchain.toml` names one.
//!
//! The toolchain is not an npm package, so it has no group under the global
//! packages directory. It is the shims `pnpm shim add rust` writes, plus a
//! record of the channel next to them, which the dispatcher reads.

use crate::{
    cli_args::shim::{add_shims, remove_shims},
    shim_dispatch::rust_toolchain::{
        RUST_SHIM_PACKAGE, expire_for_shims, global_toolchain, install_for_shims,
        installed_for_shims, installed_release_for_shims, record_global_toolchain,
        remove_global_toolchain,
    },
};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_rust_toolchain::{Channel, Profile, ToolchainRequest};
use std::path::{Path, PathBuf};

/// The name `pnpm remove -g` and `pnpm update -g` take for the toolchain.
pub(super) const RUST_PARAM: &str = RUST_SHIM_PACKAGE;

/// The request `pnpm add -g rust@<channel>` installs: the toolchain with the
/// default profile.
fn global_request(channel: Channel) -> ToolchainRequest {
    ToolchainRequest {
        channel,
        profile: Profile::Default,
        components: Vec::new(),
        targets: Vec::new(),
    }
}

/// `pnpm add -g rust[@<channel>]`: install the toolchain, write the Rust
/// tool shims, and make `channel` the one they run outside a project.
pub(super) async fn add_global_rust<Reporter: self::Reporter>(
    config: &'static Config,
    global_bin_dir: &Path,
    channel: Channel,
) -> miette::Result<()> {
    let request = global_request(channel);
    // Downloaded first, so a channel that does not install leaves the
    // shims and the global channel as they were.
    install_for_shims::<Reporter>(&request).await?;
    add_shims(config, global_bin_dir, &[RUST_SHIM_PACKAGE.to_string()]).await?;
    record_global_toolchain(global_bin_dir, &request.channel)
        .into_diagnostic()
        .wrap_err("record the global Rust toolchain")?;
    report::<Reporter>(&format!("Installed {}", describe(&request)));
    Ok(())
}

/// `pnpm remove -g rust`: remove the Rust tool shims and the global
/// channel. Reports whether there was anything to remove.
pub(super) fn remove_global_rust<Reporter: self::Reporter>(
    config: &Config,
    global_bin_dir: &Path,
) -> miette::Result<bool> {
    let removed_toolchain = remove_global_toolchain(global_bin_dir)
        .into_diagnostic()
        .wrap_err("remove the global Rust toolchain record")?;
    let packages = [RUST_SHIM_PACKAGE.to_string()];
    let removed_shims = remove_shims(config, global_bin_dir, &packages)?
        .into_iter()
        .any(|(_, bins)| !bins.is_empty());
    let removed = removed_toolchain || removed_shims;
    if removed {
        report::<Reporter>("Removed the global Rust toolchain");
    }
    Ok(removed)
}

/// `pnpm update -g [rust]`: ask the distribution server which release the
/// global channel is now, and install it. `false` when no global toolchain
/// is installed.
pub(super) async fn update_global_rust<Reporter: self::Reporter>(
    global_bin_dir: &Path,
) -> miette::Result<bool> {
    let Some(request) = global_toolchain(global_bin_dir) else { return Ok(false) };
    let before = installed_release_for_shims(&request);
    expire_for_shims(&request)?;
    install_for_shims::<Reporter>(&request).await?;
    let after = installed_release_for_shims(&request);
    let message = match (before, &after) {
        (Some(before), Some(after)) if before == *after => {
            format!("Rust {} is up to date at {after}", request.channel)
        }
        (Some(before), Some(after)) => {
            format!("Updated Rust {} from {before} to {after}", request.channel)
        }
        _ => format!("Installed {}", describe(&request)),
    };
    report::<Reporter>(&message);
    Ok(true)
}

/// The Rust half of `pnpm update -g`: update the global toolchain when
/// `params` name it, or name nothing and no group is selected. Returns
/// whether it was updated, and the params left for the npm groups, `None`
/// when nothing is left to update.
pub(super) async fn update_from_params<Reporter: self::Reporter>(
    global_bin_dir: &Path,
    params: &[String],
    groups_selected: bool,
) -> miette::Result<(bool, Option<Vec<String>>)> {
    let (rust_named, params) = super::split_rust_param(params);
    let updated = if rust_named || (params.is_empty() && !groups_selected) {
        update_global_rust::<Reporter>(global_bin_dir).await?
    } else {
        false
    };
    if rust_named && params.is_empty() {
        if !updated {
            println!("No global Rust toolchain is installed");
        }
        return Ok((updated, None));
    }
    Ok((updated, Some(params)))
}

/// The global toolchain as `pnpm ls -g` lists it: the release it resolves to
/// and where it is installed.
pub(crate) fn listed_global_rust(global_bin_dir: &Path) -> Option<(String, PathBuf)> {
    let request = global_toolchain(global_bin_dir)?;
    let release = installed_release_for_shims(&request)
        .map_or_else(|| request.channel.to_string(), |release| release.to_string());
    let location = installed_for_shims(&request)
        .map_or_else(|| global_bin_dir.to_path_buf(), |toolchain| toolchain.dir);
    Some((release, location))
}

fn describe(request: &ToolchainRequest) -> String {
    match installed_release_for_shims(request) {
        Some(release) if release != request.channel => {
            format!("Rust {release} ({}) globally", request.channel)
        }
        _ => format!("Rust {} globally", request.channel),
    }
}

fn report<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: message.to_string(),
    }));
}
