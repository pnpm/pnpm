use super::{
    CommandFuture, DefaultReporter, InstallArgs, NdjsonReporter, PatchArgs, PatchCommitArgs,
    PatchRemoveArgs, ReporterType, RunCtx, SilentReporter,
};

pub(in super::super) fn patch<'a>(
    ctx: &RunCtx<'a>,
    args: PatchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let command_state = (ctx.state)(false)?;
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            args.run::<DefaultReporter>(dir, command_state).await?;
            Ok(())
        }),
        ReporterType::Ndjson => Box::pin(async move {
            args.run::<NdjsonReporter>(dir, command_state).await?;
            Ok(())
        }),
        ReporterType::Silent => Box::pin(async move {
            args.run::<SilentReporter>(dir, command_state).await?;
            Ok(())
        }),
    })
}

pub(in super::super) fn patch_commit<'a>(
    ctx: &RunCtx<'a>,
    args: PatchCommitArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let state = ctx.state;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            if Box::pin(args.run::<DefaultReporter>(dir, state(false)?)).await? {
                Box::pin(
                    InstallArgs::for_reresolving_install().run::<DefaultReporter>(state(false)?),
                )
                .await?;
            }
            Ok(())
        }),
        ReporterType::Ndjson => Box::pin(async move {
            if Box::pin(args.run::<NdjsonReporter>(dir, state(false)?)).await? {
                Box::pin(
                    InstallArgs::for_reresolving_install().run::<NdjsonReporter>(state(false)?),
                )
                .await?;
            }
            Ok(())
        }),
        ReporterType::Silent => Box::pin(async move {
            if Box::pin(args.run::<SilentReporter>(dir, state(false)?)).await? {
                Box::pin(
                    InstallArgs::for_reresolving_install().run::<SilentReporter>(state(false)?),
                )
                .await?;
            }
            Ok(())
        }),
    })
}

pub(in super::super) fn patch_remove<'a>(
    ctx: &RunCtx<'a>,
    args: PatchRemoveArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let state = ctx.state;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            Box::pin(args.run(dir, state(false)?)).await?;
            Box::pin(InstallArgs::for_reresolving_install().run::<DefaultReporter>(state(false)?))
                .await?;
            Ok(())
        }),
        ReporterType::Ndjson => Box::pin(async move {
            Box::pin(args.run(dir, state(false)?)).await?;
            Box::pin(InstallArgs::for_reresolving_install().run::<NdjsonReporter>(state(false)?))
                .await?;
            Ok(())
        }),
        ReporterType::Silent => Box::pin(async move {
            Box::pin(args.run(dir, state(false)?)).await?;
            Box::pin(InstallArgs::for_reresolving_install().run::<SilentReporter>(state(false)?))
                .await?;
            Ok(())
        }),
    })
}
