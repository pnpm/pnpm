use super::{
    CommandFuture, Config, Context, InstallArgs, PatchArgs, PatchCommitArgs, PatchRemoveArgs, Path,
    RunCtx, anchor_active_project, installed_project_config, keeps_project_lockfiles,
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

/// The state for the install that re-resolves after `patch-commit` or
/// `patch-remove`: the prepared config with the `patchedDependencies` the
/// step just recorded, anchored at the directory it recorded them under,
/// installing where [`installed_project_config`] reads from.
fn reresolving_state(
    dir: &Path,
    manifest_path: &Path,
    config: &Config,
    patched_dependencies: IndexMap<String, String>,
) -> miette::Result<State> {
    let mut config = config.clone();
    config.patched_dependencies = Some(patched_dependencies);
    if keeps_project_lockfiles(&config) {
        anchor_active_project(&mut config, manifest_path);
    }
    config.workspace_dir.get_or_insert_with(|| dir.to_path_buf());
    State::init(manifest_path.to_path_buf(), Config::leak(config), false)
        .wrap_err("initialize the state")
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
            let state = reresolving_state(dir, manifest_path, config, patched_dependencies)?;
            Box::pin(InstallArgs::for_reresolving_install().run::<CliReporter>(state)).await?;
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
        let state = reresolving_state(dir, manifest_path, config, patched_dependencies)?;
        Box::pin(InstallArgs::for_reresolving_install().run::<CliReporter>(state)).await?;
        Ok(())
    }))
}
