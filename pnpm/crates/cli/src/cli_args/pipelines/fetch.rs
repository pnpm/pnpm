use super::{
    DedicatedProjectRuns,
    DedicatedProjects,
    InstallFamilyPlan,
    State,
    anchor_active_project,
    select_install_family_plan,
};
use crate::cli_args::fetch::FetchArgs;
use miette::Context;
use pnpm_config::Config;
use pnpm_reporter::Reporter;
use std::path::Path;

pub(crate) async fn run_fetch<Rep: Reporter + 'static>(
    args: FetchArgs,
    config: &'static Config,
    manifest_path: &Path,
) -> miette::Result<()> {
    let plan = if selects_projects(config) {
        let prefix = manifest_path.parent().expect("manifest path always has a parent dir");
        // Fetching a lockfile does not require sibling projects to finish first.
        let recursive_sort = false;
        select_install_family_plan::<Rep>(
            config,
            prefix,
            manifest_path,
            recursive_sort,
            false,
            false,
        )?
    } else {
        InstallFamilyPlan::Single
    };
    match plan {
        InstallFamilyPlan::PerProject(projects) => {
            fetch_dedicated_projects::<Rep>(args, config, *projects).await
        }
        InstallFamilyPlan::Shared(selection) => {
            if selection.selected_dirs.is_empty() {
                return Ok(());
            }
            let state = State::init(manifest_path.to_path_buf(), config, true)
                .wrap_err("initialize the state")?;
            args.run::<Rep>(state, Some(&selection)).await
        }
        InstallFamilyPlan::Single => {
            fetch_active_lockfile::<Rep>(args, config, manifest_path).await
        }
    }
}

/// A shared lockfile without a filter fetches every importer, so it needs
/// no project manifests on disk.
fn selects_projects(config: &Config) -> bool {
    let is_filtered = !config.filter.is_empty() || !config.filter_prod.is_empty();
    config.recursive && (is_filtered || !config.shares_one_lockfile())
}

async fn fetch_dedicated_projects<Rep: Reporter + 'static>(
    args: FetchArgs,
    config: &'static Config,
    projects: DedicatedProjects,
) -> miette::Result<()> {
    DedicatedProjectRuns {
        config,
        projects,
        require_lockfile: true,
        http_client: Some(State::new_http_client(config)?),
        prune_excludes: false,
        sync_injected_deps: false,
        pipelined: false,
    }
    .run::<Rep, _, _>(|state| {
        let args = args.clone();
        Box::pin(args.run::<Rep>(state, None))
    })
    .await
}

async fn fetch_active_lockfile<Rep: Reporter + 'static>(
    args: FetchArgs,
    config: &'static Config,
    manifest_path: &Path,
) -> miette::Result<()> {
    let config = if !config.shares_one_lockfile() && config.workspace_dir.is_some() {
        let mut config = config.clone();
        anchor_active_project(&mut config, manifest_path);
        config.leak()
    } else {
        config
    };
    let state =
        State::init(manifest_path.to_path_buf(), config, true).wrap_err("initialize the state")?;
    args.run::<Rep>(state, None).await
}
