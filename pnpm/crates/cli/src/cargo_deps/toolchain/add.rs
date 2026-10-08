//! `pnpm add rust[@<channel>]` in a project: pin the project's Rust
//! toolchain in its rustup toolchain file, and install it where the Cargo
//! integration is enabled.

use super::{installs_toolchains, leave_to_rustup, link, managed_request};
use crate::{cargo_deps::checkout, ecosystem_install::InstallContext};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_install_coordinator::{InstallTask, PreparedInstall};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_rust_toolchain::{
    Channel, InstalledToolchain, RustToolchainError, TOOLCHAIN_LINK, find_toolchain_file,
    install_toolchain, with_channel,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The toolchain file a project at `root` is governed by: the nearest one
/// at or above it inside the workspace, or a new `rust-toolchain.toml` in
/// the project. A project whose directory resolves outside the workspace is
/// refused, so a link cannot send the write elsewhere.
fn governing_file(context: &InstallContext, root: &Path) -> Result<PathBuf> {
    let project = dunce::canonicalize(root)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve {}", root.display()))?;
    let boundary = match context.config.workspace_dir.as_deref() {
        Some(workspace) => checkout(workspace)
            .ok_or_else(|| {
                let workspace = workspace.display();
                miette::miette!("cannot pin Rust: the workspace {workspace} does not resolve")
            })?,
        None => project.clone(),
    };
    if !project.starts_with(&boundary) {
        let (project, boundary) = (project.display(), boundary.display());
        return Err(miette::miette!(
            "cannot pin Rust for {project}: it resolves outside the workspace {boundary}"
        ));
    }
    Ok(find_toolchain_file(&project, &boundary)
        .unwrap_or_else(|| project.join("rust-toolchain.toml")))
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
) -> Result<InstallTask<'static>> {
    let file = governing_file(&context, root)?;
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
        Ok(vec![PinnedToolchain { file, toolchain, replaced: None }])
    };
    Ok(InstallTask::new(metadata, prepare))
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
    refuse_occupied_link(file)?;
    match install_toolchain::<Reporter>(config, &context.http_client, &request).await {
        Ok(toolchain) => Ok(Some(toolchain)),
        // As an install leaves it: the pin stands, and rustup runs it.
        Err(
            error @ (RustToolchainError::UnsupportedHost
            | RustToolchainError::NotPublishedForHost { .. }),
        ) => {
            leave_to_rustup::<Reporter>(file, &error.to_string());
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// Refuse a `.pnpm/rust` that is not a link, which linking would move aside
/// and a rollback could not put back.
fn refuse_occupied_link(file: &Path) -> Result<()> {
    let Some(dir) = file.parent() else { return Ok(()) };
    let link_path = dir.join(TOOLCHAIN_LINK.iter().collect::<PathBuf>());
    if fs::symlink_metadata(&link_path).is_ok() && fs::read_link(&link_path).is_err() {
        let link_path = link_path.display();
        return Err(miette::miette!(
            "cannot link the Rust toolchain: {link_path} is not a link pnpm made. Remove it and run the add again."
        ));
    }
    Ok(())
}

pub(super) struct PinnedToolchain {
    pub(super) file: PathBuf,
    pub(super) toolchain: Option<InstalledToolchain>,
    /// The target `.pnpm/rust` had before publishing replaced it,
    /// `Some(None)` when there was no link, and `None` before publishing.
    pub(super) replaced: Option<Option<PathBuf>>,
}

impl PinnedToolchain {
    fn link_path(&self) -> Option<PathBuf> {
        Some(
            self.file
                .parent()?
                .join(TOOLCHAIN_LINK.iter().collect::<PathBuf>()),
        )
    }
}

impl PreparedInstall for PinnedToolchain {
    fn publish(&mut self) -> Result<()> {
        let (Some(toolchain), Some(dir), Some(link_path)) =
            (&self.toolchain, self.file.parent(), self.link_path())
        else {
            return Ok(());
        };
        // Read as written, so a link whose target is gone is put back too.
        self.replaced = Some(
            fs::read_link(&link_path)
                .ok()
                .map(|target| {
                    link_path
                        .parent()
                        .map_or_else(|| target.clone(), |parent| parent.join(&target))
                }),
        );
        link(dir, &toolchain.dir)
    }

    fn rollback(&mut self) -> Result<()> {
        // The file the link is read against is restored by the plan.
        let (Some(replaced), Some(dir), Some(link_path)) =
            (self.replaced.take(), self.file.parent(), self.link_path())
        else {
            return Ok(());
        };
        match replaced {
            Some(previous) => link(dir, &previous),
            None => match pnpm_fs::remove_symlink_dir(&link_path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("remove {}", link_path.display())),
                _ => Ok(()),
            },
        }
    }

    fn retain(self: Box<Self>) {}
}

#[cfg(test)]
mod tests;
