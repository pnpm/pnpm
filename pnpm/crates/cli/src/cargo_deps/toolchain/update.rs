//! `pnpm update --latest` for the Rust toolchain: move the release each
//! toolchain file pins to the newest release of its channel.

use super::add;
use crate::{
    cargo_deps::checkout,
    ecosystem_install::InstallContext,
};
use miette::Result;
use pnpm_config::Config;
use pnpm_install_coordinator::InstallTask;
use pnpm_reporter::Reporter;
use pnpm_rust_toolchain::{
    find_toolchain_file,
    newer_release,
    read_toolchain_file,
};
use std::{
    collections::BTreeSet,
    path::PathBuf,
};

/// A task for each toolchain file governing `project_dirs` whose pin has a
/// newer release. A file pnpm leaves to rustup is left as it is.
pub(crate) async fn plan<Reporter: self::Reporter + 'static>(
    context: &InstallContext,
    project_dirs: &[PathBuf],
) -> Result<Vec<InstallTask<'static>>> {
    let mut tasks = Vec::new();
    for file in governing_files(context.config, project_dirs) {
        let Ok(request) = read_toolchain_file(&file)? else { continue };
        let newer = newer_release(context.config, &context.http_client, &request.channel).await?;
        if let Some(release) = newer {
            tasks.push(add::pin::<Reporter>(context.clone(), file, release));
        }
    }
    Ok(tasks)
}

fn governing_files(config: &Config, project_dirs: &[PathBuf]) -> BTreeSet<PathBuf> {
    let workspace = config.workspace_dir.as_deref().and_then(checkout);
    project_dirs
        .iter()
        .filter_map(|dir| {
            let dir = dunce::canonicalize(dir).ok()?;
            let boundary = workspace.clone().unwrap_or_else(|| dir.clone());
            find_toolchain_file(&dir, &boundary)
        })
        .collect()
}
