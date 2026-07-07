use super::{
    dispatch::{CommandFuture, RunCtx},
    exec::ExecArgs,
    reporter::ReporterType,
    restart::RestartArgs,
    run::RunArgs,
    set_script::SetScriptArgs,
    stop::StopArgs,
};
use miette::Context;
use pacquet_package_manifest::PackageManifest;

pub(super) fn init<'a>(ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    let result = PackageManifest::init(ctx.manifest_path).wrap_err("initialize package.json");
    Ok(Box::pin(std::future::ready(result)))
}

// `set-script` only rewrites `package.json#scripts`; it never touches the
// lockfile or runs the install pipeline, so it dispatches synchronously off
// the canonicalized `--dir` like `init`, with no reporter-typed fan-out.
pub(super) fn set_script<'a>(
    ctx: &RunCtx<'a>,
    args: SetScriptArgs,
) -> miette::Result<CommandFuture<'a>> {
    let result = args.run(ctx.manifest_path);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn test<'a>(ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    run(ctx, run_args_for_script("test"))
}

pub(super) fn run<'a>(ctx: &RunCtx<'a>, args: RunArgs) -> miette::Result<CommandFuture<'a>> {
    let args = with_recursive_run_options(ctx, args);
    if ctx.recursive {
        args.run_recursive((ctx.config)()?, ctx.dir)?;
    } else {
        args.run(ctx.dir, (ctx.config)()?, matches!(ctx.reporter, ReporterType::Silent))?;
    }
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn fallback<'a>(
    ctx: &RunCtx<'a>,
    command: Vec<String>,
) -> miette::Result<CommandFuture<'a>> {
    let mut command = command.into_iter();
    let args = RunArgs {
        command: command.next(),
        args: command.collect(),
        if_present: false,
        resume_from: None,
        report_summary: false,
        no_bail: false,
        sort: true,
    };
    let args = with_recursive_run_options(ctx, args);
    if ctx.recursive {
        args.run_recursive((ctx.config)()?, ctx.dir)?;
    } else {
        args.run_fallback(ctx.dir, (ctx.config)()?, matches!(ctx.reporter, ReporterType::Silent))?;
    }
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn exec<'a>(ctx: &RunCtx<'a>, args: ExecArgs) -> miette::Result<CommandFuture<'a>> {
    let args = with_recursive_exec_options(ctx, args);
    if ctx.recursive {
        args.run_recursive((ctx.config)()?, ctx.dir)?;
    } else {
        args.run(ctx.dir, (ctx.config)()?)?;
    }
    Ok(Box::pin(std::future::ready(Ok(()))))
}

fn with_recursive_run_options(ctx: &RunCtx<'_>, mut args: RunArgs) -> RunArgs {
    args.resume_from = ctx.recursive_resume_from.map(str::to_string);
    args.report_summary = ctx.recursive_report_summary;
    args.no_bail = ctx.recursive_no_bail;
    args.sort = ctx.recursive_sort;
    args
}

fn with_recursive_exec_options(ctx: &RunCtx<'_>, mut args: ExecArgs) -> ExecArgs {
    args.resume_from = ctx.recursive_resume_from.map(str::to_string);
    args.report_summary = ctx.recursive_report_summary;
    args.no_bail = ctx.recursive_no_bail;
    args.sort = ctx.recursive_sort;
    args
}

pub(super) fn start<'a>(ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    run(ctx, run_args_for_script("start"))
}

pub(super) fn stop<'a>(ctx: &RunCtx<'a>, args: StopArgs) -> miette::Result<CommandFuture<'a>> {
    if ctx.recursive {
        run(ctx, args.into_run_args())
    } else {
        args.run(ctx.dir, (ctx.config)()?, matches!(ctx.reporter, ReporterType::Silent))?;
        Ok(Box::pin(std::future::ready(Ok(()))))
    }
}

pub(super) fn restart<'a>(
    ctx: &RunCtx<'a>,
    args: RestartArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?, matches!(ctx.reporter, ReporterType::Silent))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

fn run_args_for_script(command: &str) -> RunArgs {
    RunArgs {
        command: Some(command.to_string()),
        args: Vec::new(),
        if_present: false,
        resume_from: None,
        report_summary: false,
        no_bail: false,
        sort: true,
    }
}
