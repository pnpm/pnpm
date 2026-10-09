//! `pnpm dlx --package=rust@<channel> <tool>`: one command run with a Rust
//! release, which pnpm installs into the store.

use super::{Config, DlxError, DlxProgram, DlxSpawn, Path, Reporter, exit_unless_success, run_bin};
use crate::{State, shim_dispatch::rust_toolchain::RUST_SHIM_BINS};
use pnpm_package_manifest::package_manager_spec::split_spec;
use pnpm_rust_toolchain::{
    Channel, ToolchainRequest, find_toolchain_file, install_toolchain, parse_toolchain_name,
    read_toolchain_file,
};

/// The release `--package` names when it is `rust@<channel>` and the
/// command is one of the toolchain's tools. A bare `rust` is `stable`, the
/// channel rustup installs by default.
pub(super) fn rust_release(package: &[String], command: &str) -> Option<Channel> {
    let [spec] = package else { return None };
    let (name, channel) = split_spec(spec);
    (name == "rust" && RUST_SHIM_BINS.contains(&command))
        .then(|| parse_toolchain_name(channel.unwrap_or("stable")))
        .flatten()
}

/// Install `channel` and run its `command`.
pub(super) async fn run_rust<Reporter: self::Reporter>(
    config: &Config,
    channel: Channel,
    command: &str,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let request = rust_request(channel, spawn.cwd, args);
    let client = State::new_http_client(config).map_err(miette::Report::new)?;
    let toolchain = install_toolchain::<Reporter>(config, &client, &request)
        .await
        .map_err(miette::Report::new)?;
    let executable = toolchain.executable(command);
    if !executable.is_file() {
        return Err(DlxError::CommandNotFound { command: command.to_string() }.into());
    }
    // Cargo runs `cargo-clippy` and its other subcommands from
    // `$CARGO_HOME/bin` before `PATH`, where rustup keeps its proxies. A
    // rustup proxy runs the toolchain `RUSTUP_TOOLCHAIN` names when it is a
    // path.
    let mut extra_env = spawn.extra_env.clone();
    extra_env.insert("RUSTUP_TOOLCHAIN".to_string(), toolchain.dir.to_string_lossy().into_owned());
    let status = run_bin(
        DlxProgram::Provisioned { command, executable: &executable },
        args,
        vec![toolchain.bin_dir()],
        &DlxSpawn { extra_env: &extra_env, ..*spawn },
    )?;
    exit_unless_success(status);
    Ok(())
}

/// What `channel` is installed with: the profile, components, and targets
/// of the toolchain file that governs `cwd`, if pnpm reads it, and the
/// target of every `--target` in `args`.
fn rust_request(channel: Channel, cwd: &Path, args: &[String]) -> ToolchainRequest {
    let project = cwd
        .ancestors()
        .last()
        .and_then(|root| find_toolchain_file(cwd, root))
        .and_then(|file| read_toolchain_file(&file).ok()?.ok());
    let request = match project {
        Some(project) => ToolchainRequest { channel, ..project },
        None => ToolchainRequest::for_channel(channel),
    };
    request.with_targets(target_args(args))
}

/// The values of `--target` before a `--`, which ends the options the tool
/// reads itself.
fn target_args(args: &[String]) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut args = args
        .iter()
        .map(String::as_str)
        .take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        if arg == "--target" {
            targets.extend(args.next());
        } else if let Some(target) = arg.strip_prefix("--target=") {
            targets.push(target);
        }
    }
    targets
}

#[cfg(test)]
mod tests;
