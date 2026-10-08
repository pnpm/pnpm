//! `pnpm add rust[@<channel>]` in a project: pin the project's Rust
//! toolchain in its rustup toolchain file, and install it where the Cargo
//! integration is enabled.

use super::{installs_toolchains, link, managed_request};
use crate::{cargo_deps::checkout, ecosystem_install::InstallContext};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_install_coordinator::{InstallTask, PreparedInstall};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_rust_toolchain::{
    Channel, InstalledToolchain, find_toolchain_file, install_toolchain, with_channel,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The toolchain file a project at `root` is governed by: the nearest one
/// at or above it inside the workspace, or a new `rust-toolchain.toml` in
/// the project.
fn governing_file(context: &InstallContext, root: &Path) -> PathBuf {
    let boundary = checkout(context.config.workspace_dir.as_deref().unwrap_or(root));
    dunce::canonicalize(root)
        .ok()
        .zip(boundary)
        .and_then(|(root, boundary)| find_toolchain_file(&root, &boundary))
        .unwrap_or_else(|| root.join("rust-toolchain.toml"))
}

/// Set the channel of the toolchain file governing `root` to `channel`.
///
/// The file is the task's declared metadata, so it is written while the
/// task prepares and restored by the install plan if the add fails. The
/// toolchain is linked when the add publishes.
pub(crate) fn plan<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    root: &Path,
    channel: Channel,
) -> InstallTask<'static> {
    let file = governing_file(&context, root);
    let metadata = vec![file.clone()];
    let prepare = async move {
        let existing = match fs::read_to_string(&file) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("read {}", file.display()));
            }
        };
        let updated = with_channel(existing.as_deref(), &channel)
            .map_err(|reason| {
                let shown = file.display();
                miette::miette!("cannot pin Rust in {shown}: {reason}")
            })?;
        pnpm_fs::write_atomic(&file, updated.as_bytes())
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", file.display()))?;
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: format!("Pinned Rust {channel} in {}", file.display()),
            prefix: file
                .parent()
                .unwrap_or(&file)
                .to_string_lossy()
                .into_owned(),
        }));
        let toolchain = install::<Reporter>(&context, &file).await?;
        Ok(vec![PinnedToolchain { file, toolchain }])
    };
    InstallTask::new(metadata, prepare)
}

/// Install the toolchain `file` now names, where the Cargo integration is
/// enabled and `runtimeOnFail` lets pnpm install toolchains.
async fn install<Reporter: self::Reporter>(
    context: &InstallContext,
    file: &Path,
) -> Result<Option<InstalledToolchain>> {
    let config = context.config;
    if !config.cargo.enabled || !installs_toolchains(config) {
        return Ok(None);
    }
    let Some(request) = managed_request::<Reporter>(file)? else { return Ok(None) };
    install_toolchain::<Reporter>(config, &context.http_client, &request).await
        .map(Some)
        .map_err(miette::Report::new)
}

struct PinnedToolchain {
    file: PathBuf,
    toolchain: Option<InstalledToolchain>,
}

impl PreparedInstall for PinnedToolchain {
    fn publish(&mut self) -> Result<()> {
        let (Some(toolchain), Some(dir)) = (&self.toolchain, self.file.parent()) else {
            return Ok(());
        };
        link(dir, &toolchain.dir)
    }

    fn rollback(&mut self) -> Result<()> {
        // The link only adds a toolchain from the store; the file it is
        // read against is restored by the plan.
        Ok(())
    }

    fn retain(self: Box<Self>) {}
}
