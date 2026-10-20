use super::{
    CommandFuture, Config, Context, InstallArgs, PatchArgs, PatchCommitArgs, PatchRemoveArgs, Path,
    RunCtx, installed_project_config, keeps_project_lockfiles,
    run_dedicated_lockfile_workspace_install,
};
use crate::{State, cli_args::reporter::CliReporter};
use indexmap::IndexMap;

pub(in super::super) fn patch<'a>(
    ctx: &RunCtx<'a>,
    args: PatchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = ctx.prepared_config();
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    Ok(Box::pin(async move {
        let config = installed_project_config(config.await?, manifest_path);
        let command_state = State::init(manifest_path.to_path_buf(), config, false)
            .wrap_err("initialize the state")?;
        Box::pin(args.run::<CliReporter>(dir, command_state)).await?;
        Ok(())
    }))
}

async fn run_reresolving_install(
    dir: &Path,
    manifest_path: &Path,
    config: &Config,
    patched_dependencies: IndexMap<String, String>,
) -> miette::Result<()> {
    let mut config = config.clone();
    config.patched_dependencies = Some(patched_dependencies);
    let dedicated_workspace_dir =
        keeps_project_lockfiles(&config).then(|| config.workspace_dir.clone()).flatten();
    config.workspace_dir.get_or_insert_with(|| dir.to_path_buf());
    let config = Config::leak(config);
    let args = InstallArgs::for_reresolving_install();
    if let Some(workspace_dir) = dedicated_workspace_dir {
        let http_client =
            State::new_http_client(config).wrap_err("initialize the install network")?;
        return run_dedicated_lockfile_workspace_install::<CliReporter>(
            &args,
            config,
            &workspace_dir,
            false,
            http_client,
        )
        .await;
    }
    let state =
        State::init(manifest_path.to_path_buf(), config, false).wrap_err("initialize the state")?;
    Box::pin(args.run::<CliReporter>(state)).await
}

pub(in super::super) fn patch_commit<'a>(
    ctx: &RunCtx<'a>,
    args: PatchCommitArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let config = ctx.prepared_config();
    Ok(Box::pin(async move {
        let config = config.await?;
        let state = State::init(
            manifest_path.to_path_buf(),
            installed_project_config(config, manifest_path),
            false,
        )
        .wrap_err("initialize the state")?;
        if let Some(patched_dependencies) = Box::pin(args.run::<CliReporter>(dir, state)).await? {
            Box::pin(run_reresolving_install(dir, manifest_path, config, patched_dependencies))
                .await?;
        }
        Ok(())
    }))
}

pub(in super::super) fn patch_remove<'a>(
    ctx: &RunCtx<'a>,
    args: PatchRemoveArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let config = ctx.prepared_config();
    Ok(Box::pin(async move {
        let config = config.await?;
        let state = State::init(manifest_path.to_path_buf(), config, false)
            .wrap_err("initialize the state")?;
        let patched_dependencies = Box::pin(args.run(dir, state)).await?;
        Box::pin(run_reresolving_install(dir, manifest_path, config, patched_dependencies)).await?;
        Ok(())
    }))
}
