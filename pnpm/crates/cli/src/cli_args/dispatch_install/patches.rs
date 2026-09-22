use super::{
    CommandFuture, Config, Context, DefaultReporter, InstallArgs, NdjsonReporter, PatchArgs,
    PatchCommitArgs, PatchRemoveArgs, Path, ReporterType, RunCtx, SilentReporter,
};
use crate::State;
use indexmap::IndexMap;
use std::sync::atomic::Ordering;

pub(in super::super) fn patch<'a>(
    ctx: &RunCtx<'a>,
    args: PatchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(false);
    let dir = ctx.locations.dir;
    let effective_reporter = ctx.effective_reporter;
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        match effective_reporter.load(Ordering::Relaxed).into() {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(async move {
                    args.run::<DefaultReporter>(dir, command_state).await?;
                    Ok(())
                })
                .await
            }
            ReporterType::Ndjson => {
                Box::pin(async move {
                    args.run::<NdjsonReporter>(dir, command_state).await?;
                    Ok(())
                })
                .await
            }
            ReporterType::Silent => {
                Box::pin(async move {
                    args.run::<SilentReporter>(dir, command_state).await?;
                    Ok(())
                })
                .await
            }
        }
    }))
}

/// The state for the install that re-resolves after `patch-commit` or
/// `patch-remove`: the prepared config with the `patchedDependencies` the
/// step just recorded, anchored at the directory it recorded them under.
fn reresolving_state(
    dir: &Path,
    manifest_path: &Path,
    config: &Config,
    patched_dependencies: IndexMap<String, String>,
) -> miette::Result<State> {
    let mut config = config.clone();
    config.patched_dependencies = Some(patched_dependencies);
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
    macro_rules! run_patch_commit {
        ($reporter:ty, $config:ident) => {
            Box::pin(async move {
                let state = State::init(manifest_path.to_path_buf(), $config, false)
                    .wrap_err("initialize the state")?;
                if let Some(patched_dependencies) =
                    Box::pin(args.run::<$reporter>(dir, state)).await?
                {
                    let state =
                        reresolving_state(dir, manifest_path, $config, patched_dependencies)?;
                    Box::pin(InstallArgs::for_reresolving_install().run::<$reporter>(state)).await?;
                }
                Ok(())
            })
        };
    }
    let effective_reporter = ctx.effective_reporter;
    Ok(Box::pin(async move {
        let config = config.await?;
        match effective_reporter.load(Ordering::Relaxed).into() {
            ReporterType::Default | ReporterType::AppendOnly => {
                run_patch_commit!(DefaultReporter, config).await
            }
            ReporterType::Ndjson => run_patch_commit!(NdjsonReporter, config).await,
            ReporterType::Silent => run_patch_commit!(SilentReporter, config).await,
        }
    }))
}

pub(in super::super) fn patch_remove<'a>(
    ctx: &RunCtx<'a>,
    args: PatchRemoveArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let config = ctx.prepared_config();
    macro_rules! run_patch_remove {
        ($reporter:ty, $config:ident) => {
            Box::pin(async move {
                let state = State::init(manifest_path.to_path_buf(), $config, false)
                    .wrap_err("initialize the state")?;
                let patched_dependencies = Box::pin(args.run(dir, state)).await?;
                let state = reresolving_state(dir, manifest_path, $config, patched_dependencies)?;
                Box::pin(InstallArgs::for_reresolving_install().run::<$reporter>(state)).await?;
                Ok(())
            })
        };
    }
    let effective_reporter = ctx.effective_reporter;
    Ok(Box::pin(async move {
        let config = config.await?;
        match effective_reporter.load(Ordering::Relaxed).into() {
            ReporterType::Default | ReporterType::AppendOnly => {
                run_patch_remove!(DefaultReporter, config).await
            }
            ReporterType::Ndjson => run_patch_remove!(NdjsonReporter, config).await,
            ReporterType::Silent => run_patch_remove!(SilentReporter, config).await,
        }
    }))
}
