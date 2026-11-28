//! The Rust release a project's toolchain file pins, as the `rust` row of
//! `pnpm outdated`.

use crate::{
    cargo_deps::toolchain::update::governing_file,
    cli_args::outdated::{
        OutdatedPackage,
        OutdatedQuery,
        OutdatedRun,
        TargetVersion,
        query::{
            OutdatedDisplayVersions,
            OutdatedMetadata,
        },
    },
};
use miette::Result;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;
use pnpm_rust_toolchain::{
    Channel,
    newer_release,
    read_toolchain_file,
};
use std::{
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        Mutex,
    },
};
use tokio::sync::OnceCell;

/// The name the toolchain is listed and selected under.
pub(crate) const RUST: &str = "rust";

/// Whether a toolchain file pnpm manages governs `project_dir`, which makes
/// `rust` one of the project's dependencies.
pub(super) fn declares_rust_toolchain(
    project_dir: &Path,
    config: &Config,
    include: &[DependencyGroup],
) -> Result<bool> {
    if !config.cargo.enabled || !include.contains(&DependencyGroup::Dev) {
        return Ok(false);
    }
    let Some(file) = governing_file(config, project_dir) else { return Ok(false) };
    Ok(read_toolchain_file(&file)?.is_ok())
}

/// The newer release found for each toolchain file. The projects of a
/// workspace that share one wait on a single lookup, so the distribution
/// server is asked once.
#[derive(Default)]
pub(crate) struct NewerRustReleases(Mutex<HashMap<PathBuf, Arc<OnceCell<PinAndNewer>>>>);

type PinAndNewer = Option<(Channel, Channel)>;

/// The `rust` row for the toolchain file governing `project_dir`, when its
/// pin has a newer release. Only a query for the latest versions lists it:
/// a pinned release does not move inside its range.
pub(super) async fn collect_rust_toolchain_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    run: &OutdatedRun,
) -> Result<Option<OutdatedPackage>> {
    if !matches!(query.target_version, TargetVersion::Latest)
        || !query.include_direct.contains(&DependencyGroup::Dev)
        || !query.matches_name(RUST)
    {
        return Ok(None);
    }
    let Some(file) = governing_file(&run.config, project_dir) else { return Ok(None) };
    let Some((pin, newer)) = pin_and_newer_release(&file, run).await? else { return Ok(None) };
    Ok(Some(outdated_row(&pin, &newer, workspace_name)))
}

async fn pin_and_newer_release(file: &Path, run: &OutdatedRun) -> Result<PinAndNewer> {
    let lookup = Arc::clone(
        run.rust_releases.0
            .lock()
            .expect("the Rust release cache is not poisoned")
            .entry(file.to_path_buf())
            .or_default(),
    );
    let found = lookup.get_or_try_init(|| look_up_newer_release(file, run)).await?;
    Ok(found.clone())
}

async fn look_up_newer_release(file: &Path, run: &OutdatedRun) -> Result<PinAndNewer> {
    let Ok(request) = read_toolchain_file(file)? else { return Ok(None) };
    let newer = newer_release(&run.config, &run.http_client, &request.channel).await?;
    Ok(newer.map(|newer| (request.channel, newer)))
}

fn outdated_row(pin: &Channel, newer: &Channel, workspace: Option<String>) -> OutdatedPackage {
    OutdatedPackage {
        alias: RUST.to_string(),
        package_name: RUST.to_string(),
        belongs_to: DependencyGroup::Dev,
        current: as_version(pin),
        target: as_version(newer),
        wanted: as_version(pin),
        github_action: false,
        metadata: OutdatedMetadata {
            update_prefix: "",
            deprecated: None,
            homepage: None,
            repository: None,
            workspace,
            display_versions: Some(Box::new(OutdatedDisplayVersions {
                current: pin.to_string(),
                target: newer.to_string(),
                wanted: pin.to_string(),
            })),
            trust: None,
        },
    }
}

/// The semver the table compares a release by. A dated channel has none,
/// so it is shown by its name alone.
fn as_version(channel: &Channel) -> node_semver::Version {
    let (major, minor, patch) = match channel {
        Channel::Version { major, minor, patch } => (*major, *minor, patch.unwrap_or(0)),
        Channel::Named { .. } => (0, 0, 0),
    };
    node_semver::Version { major, minor, patch, build: Vec::new(), pre_release: Vec::new() }
}
