use super::{
    dispatch::{CommandFuture, RunCtx, apply_update_config},
    exec::{ExecArgs, ExecDirs},
    init::InitArgs,
    machine_run_slot::{MachineRunSlot, acquire_machine_run_slot, stamp_held_slot},
    pkg::PkgArgs,
    reporter::{ReporterType, reporter_emit},
    restart::RestartArgs,
    run::RunArgs,
    script_shortcut::ScriptShortcutArgs,
    set_script::SetScriptArgs,
};
use miette::Context;
use pnpm_config::{Config, InitType};
use pnpm_package_manifest::{InitAuthor, InitOptions, PackageManifest};

// `init` looks the version it pins up on the registry, so unlike the other
// manifest-only commands here it dispatches a real future rather than a
// ready one.
pub(super) fn init<'a>(ctx: &RunCtx<'a>, args: &InitArgs) -> miette::Result<CommandFuture<'a>> {
    let config: &Config = (ctx.loaders.config)()?;
    let es_module = args.effective_init_type(config) == InitType::Module;
    let manifest_path = ctx.locations.cli_dir.join("package.json");
    // `config_self_update`, so a repo-controlled `pnpm-workspace.yaml` cannot
    // relax the release-age and trust policies governing the version pnpm
    // ends up downloading. A manifest that is already there skips the lookup
    // altogether: `PackageManifest::init` refuses to overwrite it, and
    // `pnpm init` should not wait on a registry to report an error it can
    // already see.
    let pin_config: Option<&Config> =
        if args.pins_pnpm(config, ctx.locations.cli_dir) && !manifest_path.exists() {
            Some((ctx.loaders.config_self_update)()?)
        } else {
            None
        };
    Ok(Box::pin(async move {
        let pinned_pnpm_version = match pin_config {
            Some(pin_config) => Some(super::init::version_to_pin(pin_config).await),
            None => None,
        };
        let options = InitOptions {
            es_module,
            pinned_pnpm_version: pinned_pnpm_version.as_deref(),
            author: InitAuthor {
                name: config.init_author_name.as_deref(),
                email: config.init_author_email.as_deref(),
                url: config.init_author_url.as_deref(),
            },
            license: config.init_license.as_deref(),
            version: config.init_version.as_deref(),
        };
        PackageManifest::init(&manifest_path, options).wrap_err("initialize package.json")
    }))
}

// `set-script` only rewrites `package.json#scripts`; it never touches the
// lockfile or runs the install pipeline, so it dispatches synchronously off
// the canonicalized `--dir` like `init`, with no reporter-typed fan-out.
pub(super) fn set_script<'a>(
    ctx: &RunCtx<'a>,
    args: SetScriptArgs,
) -> miette::Result<CommandFuture<'a>> {
    let result = args.run(ctx.locations.manifest_path);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn pkg<'a>(ctx: &RunCtx<'a>, args: PkgArgs) -> miette::Result<CommandFuture<'a>> {
    let result = if ctx.workspace.recursive {
        args.run_recursive((ctx.loaders.config)()?, ctx.locations.dir)
    } else {
        args.run(ctx.locations.manifest_path)
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
    let config = (ctx.loaders.config)()?;
    let cli_options = RecursiveCliOptions::from_ctx(ctx);
    let dir = ctx.locations.dir;
    let reporter = ctx.reporter;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let _slot = if args.dry_run || args.script_name().is_none() {
            None
        } else {
            take_machine_run_slot(config, reporter)?
        };
        let config: &'static Config = config;
        let args = with_recursive_run_options(cli_options, args, config);
        if recursive {
            args.run_recursive(config, dir, reporter)
        } else {
            args.run(dir, config, reporter)
        }
    }))
}

/// Hold a slot of the machine-wide run pool for the rest of the command,
/// and stamp it into the environment of the scripts and commands the
/// command spawns.
fn take_machine_run_slot(
    config: &mut Config,
    reporter: ReporterType,
) -> miette::Result<Option<MachineRunSlot>> {
    let slot = acquire_machine_run_slot(config, reporter_emit(reporter))?;
    stamp_held_slot(config, slot.as_ref());
    Ok(slot)
}

pub(super) fn fallback<'a>(
    ctx: &RunCtx<'a>,
    command: Vec<String>,
) -> miette::Result<CommandFuture<'a>> {
    let args = RunArgs {
        script: command,
        if_present: false,
        sequential: false,
        dry_run: false,
        json: false,
        workspace: crate::cli_args::recursive::RecursiveExecutionArgs {
            resume_from: None,
            report_summary: false,
            no_bail: false,
            sort: true,
            reverse: false,
            parallel: false,
        },
    };
    let config = (ctx.loaders.config)()?;
    let cli_options = RecursiveCliOptions::from_ctx(ctx);
    let dir = ctx.locations.dir;
    let cli_dir = ctx.locations.cli_dir;
    let reporter = ctx.reporter;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let _slot = take_machine_run_slot(config, reporter)?;
        let config: &'static Config = config;
        let args = with_recursive_run_options(cli_options, args, config);
        if recursive {
            args.run_recursive(config, dir, reporter)
        } else {
            args.run_fallback(ExecDirs { run: cli_dir, project: dir }, config, reporter)
        }
    }))
}

pub(super) fn exec<'a>(ctx: &RunCtx<'a>, args: ExecArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let cli_options = RecursiveCliOptions::from_ctx(ctx);
    let dir = ctx.locations.dir;
    let cli_dir = ctx.locations.cli_dir;
    let reporter = ctx.reporter;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let _slot = take_machine_run_slot(config, reporter)?;
        let config: &'static Config = config;
        let args = with_recursive_exec_options(cli_options, args, config);
        if recursive {
            args.run_recursive(config, dir, reporter).await
        } else {
            args.run(ExecDirs { run: cli_dir, project: dir }, config, reporter)
        }
    }))
}

/// The top-level recursive flags of a `run` / `exec` invocation, copied out
/// of [`RunCtx`] so the handler's future can merge them with `bail`, `sort`
/// and `reverse` only after `updateConfig` has had its say on those settings.
#[derive(Clone, Copy)]
struct RecursiveCliOptions<'a> {
    resume_from: Option<&'a str>,
    report_summary: bool,
    parallel: bool,
    if_present: bool,
}

impl<'a> RecursiveCliOptions<'a> {
    fn execution_args(self, config: &Config) -> super::recursive::RecursiveExecutionArgs {
        super::recursive::RecursiveExecutionArgs {
            resume_from: self.resume_from.map(str::to_string),
            report_summary: self.report_summary,
            no_bail: !config.bail,
            sort: config.sort,
            reverse: config.reverse,
            parallel: self.parallel,
        }
    }

    fn from_ctx(ctx: &RunCtx<'a>) -> Self {
        Self {
            resume_from: ctx.workspace.resume_from,
            report_summary: ctx.workspace.report_summary,
            parallel: ctx.workspace.parallel,
            if_present: ctx.workspace.if_present,
        }
    }
}

fn with_recursive_run_options(
    cli_options: RecursiveCliOptions<'_>,
    mut args: RunArgs,
    config: &Config,
) -> RunArgs {
    args.workspace = cli_options.execution_args(config);
    args.if_present |= cli_options.if_present;
    args
}

fn with_recursive_exec_options(
    cli_options: RecursiveCliOptions<'_>,
    mut args: ExecArgs,
    config: &Config,
) -> ExecArgs {
    args.workspace = cli_options.execution_args(config);
    args
}

pub(super) fn start<'a>(
    ctx: &RunCtx<'a>,
    args: ScriptShortcutArgs,
) -> miette::Result<CommandFuture<'a>> {
    run(ctx, args.into_run_args("start", ctx.workspace.if_present))
}

pub(super) fn stop<'a>(
    ctx: &RunCtx<'a>,
    args: ScriptShortcutArgs,
) -> miette::Result<CommandFuture<'a>> {
    if ctx.workspace.recursive {
        run(ctx, args.into_run_args("stop", ctx.workspace.if_present))
    } else {
        let config = (ctx.loaders.config)()?;
        let dir = ctx.locations.dir;
        let reporter = ctx.reporter;
        let if_present = ctx.workspace.if_present;
        Ok(Box::pin(async move {
            apply_update_config(config, dir, reporter).await?;
            let _slot = take_machine_run_slot(config, reporter)?;
            args.run("stop", if_present, dir, config, reporter)
        }))
    }
}

pub(super) fn restart<'a>(
    ctx: &RunCtx<'a>,
    mut args: RestartArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.if_present |= ctx.workspace.if_present;
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let _slot = take_machine_run_slot(config, reporter)?;
        args.run(dir, config, reporter)
    }))
}
