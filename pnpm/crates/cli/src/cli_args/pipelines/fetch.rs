use super::{
    DedicatedProjectRuns,
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
    let is_filtered = !config.filter.is_empty() || !config.filter_prod.is_empty();
    // A shared lockfile without a filter fetches every importer, so it needs
    // no project manifests on disk.
    if config.recursive && (is_filtered || !config.shares_one_lockfile()) {
        let prefix = manifest_path.parent().expect("manifest path always has a parent dir");
        // Fetching a lockfile does not require sibling projects to finish first.
        let recursive_sort = false;
        let plan = select_install_family_plan::<Rep>(
            config,
            prefix,
            manifest_path,
            recursive_sort,
            false,
            false,
        )?;
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                return DedicatedProjectRuns {
                    config,
                    projects: *projects,
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
                .await;
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let state = State::init(manifest_path.to_path_buf(), config, true)
                    .wrap_err("initialize the state")?;
                return args.run::<Rep>(state, Some(&selection)).await;
            }
            InstallFamilyPlan::Single => {}
        }
    }
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
