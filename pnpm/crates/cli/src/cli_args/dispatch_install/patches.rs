use super::{
    CommandFuture, Config, Context, DefaultReporter, InstallArgs, NdjsonReporter, PatchArgs,
    PatchCommitArgs, PatchRemoveArgs, Path, ReporterType, RunCtx, SilentReporter,
};
use crate::State;
use indexmap::IndexMap;

pub(in super::super) fn patch<'a>(
    ctx: &RunCtx<'a>,
    args: PatchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(false);
    let dir = ctx.locations.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            args.run::<DefaultReporter>(dir, command_state.await?).await?;
            Ok(())
        }),
        ReporterType::Ndjson => Box::pin(async move {
            args.run::<NdjsonReporter>(dir, command_state.await?).await?;
            Ok(())
        }),
        ReporterType::Silent => Box::pin(async move {
            args.run::<SilentReporter>(dir, command_state.await?).await?;
            Ok(())
        }),
    })
}

/// The state for the install that re-resolves after `patch-commit` or
/// `patch-remove`: the prepared config with the `patchedDependencies` the
/// step just recorded, so the install applies the same patches the
/// workspace now lists.
fn reresolving_state(
    manifest_path: &Path,
    config: &Config,
    patched_dependencies: IndexMap<String, String>,
) -> miette::Result<State> {
    let mut config = config.clone();
    config.patched_dependencies = Some(patched_dependencies);
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
    macro_rules! run_patch_commit {
        ($reporter:ty) => {
            Box::pin(async move {
                let config = config.await?;
                let state = State::init(manifest_path.to_path_buf(), config, false)
                    .wrap_err("initialize the state")?;
                if let Some(patched_dependencies) =
                    Box::pin(args.run::<$reporter>(dir, state)).await?
                {
                    let state = reresolving_state(manifest_path, config, patched_dependencies)?;
                    Box::pin(InstallArgs::for_reresolving_install().run::<$reporter>(state)).await?;
                }
                Ok(())
            })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_patch_commit!(DefaultReporter),
        ReporterType::Ndjson => run_patch_commit!(NdjsonReporter),
        ReporterType::Silent => run_patch_commit!(SilentReporter),
    })
}

pub(in super::super) fn patch_remove<'a>(
    ctx: &RunCtx<'a>,
    args: PatchRemoveArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let config = ctx.prepared_config();
    macro_rules! run_patch_remove {
        ($reporter:ty) => {
            Box::pin(async move {
                let config = config.await?;
                let state = State::init(manifest_path.to_path_buf(), config, false)
                    .wrap_err("initialize the state")?;
                let patched_dependencies = Box::pin(args.run(dir, state)).await?;
                let state = reresolving_state(manifest_path, config, patched_dependencies)?;
                Box::pin(InstallArgs::for_reresolving_install().run::<$reporter>(state)).await?;
                Ok(())
            })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_patch_remove!(DefaultReporter),
        ReporterType::Ndjson => run_patch_remove!(NdjsonReporter),
        ReporterType::Silent => run_patch_remove!(SilentReporter),
    })
}
