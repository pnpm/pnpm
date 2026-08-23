use super::{
    dispatch::{CommandFuture, RunCtx},
    exec::ExecArgs,
    init::InitArgs,
    pkg::PkgArgs,
    reporter::{ReporterType, reporter_emit},
    restart::RestartArgs,
    run::RunArgs,
    script_shortcut::ScriptShortcutArgs,
    set_script::SetScriptArgs,
};
use miette::Context;
use pnpm_config::{Config, InitType, PNPM_VERSION};
use pnpm_package_manifest::{InitOptions, PackageManifest};
use std::path::Path;

pub(super) fn init<'a>(ctx: &RunCtx<'a>, args: &InitArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let options = InitOptions {
        es_module: args.effective_init_type(config) == InitType::Module,
        pinned_pnpm_version: pinned_pnpm_version(args, config, ctx.dir),
    };
    let result =
        PackageManifest::init(ctx.manifest_path, options).wrap_err("initialize package.json");
    Ok(Box::pin(std::future::ready(result)))
}

/// The pnpm version `pnpm init` records as the new project's pin, or `None`
/// when the manifest is scaffolded without one.
///
/// A manifest created inside an existing workspace becomes a member of it and
/// follows the pin at the workspace root, so only the root is pinned.
fn pinned_pnpm_version(args: &InitArgs, config: &Config, init_dir: &Path) -> Option<&'static str> {
    if !args.effective_init_package_manager(config) {
        return None;
    }
    if config.workspace_dir.as_deref().is_some_and(|root| root != init_dir) {
        return None;
    }
    Some(PNPM_VERSION)
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

pub(super) fn pkg<'a>(ctx: &RunCtx<'a>, args: PkgArgs) -> miette::Result<CommandFuture<'a>> {
    let result = if ctx.recursive {
        args.run_recursive((ctx.config)()?, ctx.dir)
    } else {
        args.run(ctx.manifest_path)
    };
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn test<'a>(
    ctx: &RunCtx<'a>,
    args: ScriptShortcutArgs,
) -> miette::Result<CommandFuture<'a>> {
    run(ctx, args.into_run_args("test", true))
}

pub(super) fn run<'a>(ctx: &RunCtx<'a>, args: RunArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let args = with_recursive_run_options(ctx, args, config);
    if ctx.recursive {
        args.run_recursive(
            config,
            ctx.dir,
            reporter_emit(ctx.reporter),
            matches!(ctx.reporter, ReporterType::Ndjson | ReporterType::Silent),
        )?;
    } else {
        args.run(ctx.dir, config, matches!(ctx.reporter, ReporterType::Silent))?;
    }
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn fallback<'a>(
    ctx: &RunCtx<'a>,
    command: Vec<String>,
) -> miette::Result<CommandFuture<'a>> {
    let args = RunArgs {
        script: command,
        if_present: false,
        resume_from: None,
        report_summary: false,
        no_bail: false,
        sort: true,
        reverse: false,
        parallel: false,
        sequential: false,
    };
    let config = (ctx.config)()?;
    let args = with_recursive_run_options(ctx, args, config);
    if ctx.recursive {
        args.run_recursive(
            config,
            ctx.dir,
            reporter_emit(ctx.reporter),
            matches!(ctx.reporter, ReporterType::Ndjson | ReporterType::Silent),
        )?;
    } else {
        args.run_fallback(ctx.dir, config, matches!(ctx.reporter, ReporterType::Silent))?;
    }
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn exec<'a>(ctx: &RunCtx<'a>, args: ExecArgs) -> miette::Result<CommandFuture<'a>> {
    let config: &'static Config = (ctx.config)()?;
    let args = with_recursive_exec_options(ctx, args, config);
    if ctx.recursive {
        let dir = ctx.dir;
        let emit = reporter_emit(ctx.reporter);
        Ok(Box::pin(async move { args.run_recursive(config, dir, emit).await }))
    } else {
        args.run(ctx.dir, config)?;
        Ok(Box::pin(std::future::ready(Ok(()))))
    }
}

fn with_recursive_run_options(ctx: &RunCtx<'_>, mut args: RunArgs, config: &Config) -> RunArgs {
    args.resume_from = ctx.recursive_resume_from.map(str::to_string);
    args.report_summary = ctx.recursive_report_summary;
    args.no_bail = !config.bail;
    args.sort = config.sort;
    args.reverse = config.reverse;
    args.parallel = ctx.recursive_parallel;
    args.if_present |= ctx.if_present;
    args
}

fn with_recursive_exec_options(ctx: &RunCtx<'_>, mut args: ExecArgs, config: &Config) -> ExecArgs {
    args.resume_from = ctx.recursive_resume_from.map(str::to_string);
    args.report_summary = ctx.recursive_report_summary;
    args.no_bail = !config.bail;
    args.sort = config.sort;
    args.reverse = config.reverse;
    args.parallel = ctx.recursive_parallel;
    args
}

pub(super) fn start<'a>(
    ctx: &RunCtx<'a>,
    args: ScriptShortcutArgs,
) -> miette::Result<CommandFuture<'a>> {
    run(ctx, args.into_run_args("start", ctx.if_present))
}

pub(super) fn stop<'a>(
    ctx: &RunCtx<'a>,
    args: ScriptShortcutArgs,
) -> miette::Result<CommandFuture<'a>> {
    if ctx.recursive {
        run(ctx, args.into_run_args("stop", ctx.if_present))
    } else {
        args.run(
            "stop",
            ctx.if_present,
            ctx.dir,
            (ctx.config)()?,
            matches!(ctx.reporter, ReporterType::Silent),
        )?;
        Ok(Box::pin(std::future::ready(Ok(()))))
    }
}

pub(super) fn restart<'a>(
    ctx: &RunCtx<'a>,
    mut args: RestartArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.if_present |= ctx.if_present;
    args.run(ctx.dir, (ctx.config)()?, matches!(ctx.reporter, ReporterType::Silent))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}
