pub(super) use maintenance::{
    bin, bugs, cache, cat_file, cat_index, clean, config, config_get, config_set, docs, doctor,
    find_hash, ignored_builds, not_implemented, prefix, repo, root, self_update, setup, shim,
    store, tasks, with,
};
pub(super) use registry::{
    access, deprecate, dist_tag, login, logout, owner, ping, search, star, stars, team,
    undeprecate, unpublish, unstar, view, whoami,
};

use super::{
    access::AccessArgs,
    audit::{AuditArgs, AuditOutcome},
    bin::BinArgs,
    bugs::BugsArgs,
    cache::CacheCommand,
    cat_file::CatFileArgs,
    cat_index::CatIndexArgs,
    change::ChangeArgs,
    clean::CleanArgs,
    config::{ConfigArgs, ConfigGetAliasArgs, ConfigSetAliasArgs, ConfigSubcommand},
    deprecate::DeprecateArgs,
    dispatch::{CommandFuture, RunCtx, apply_update_config},
    dist_tag::DistTagArgs,
    docs::DocsArgs,
    doctor::{DoctorArgs, DoctorOutcome},
    find_hash::FindHashArgs,
    ignored_builds::IgnoredBuildsArgs,
    lane::LaneArgs,
    licenses::LicensesArgs,
    list::ListArgs,
    login::LoginArgs,
    logout::LogoutArgs,
    not_implemented::NotImplementedError,
    outdated::{OutdatedArgs, OutdatedOutcome},
    owner::OwnerArgs,
    pack::{PackArgs, PackJsonReporter},
    pack_app::PackAppArgs,
    peers::{PeersArgs, PeersOutcome},
    ping::PingArgs,
    prefix::PrefixArgs,
    publish::PublishArgs,
    repo::RepoArgs,
    reporter::ReporterType,
    root::RootArgs,
    sbom::SbomArgs,
    search::SearchArgs,
    self_update::SelfUpdateArgs,
    setup::SetupArgs,
    shim::ShimArgs,
    stage::StageArgs,
    star::StarArgs,
    stars::StarsArgs,
    store::StoreCommand,
    tasks::TasksArgs,
    team::TeamArgs,
    undeprecate::UndeprecateArgs,
    unpublish::UnpublishArgs,
    unstar::UnstarArgs,
    version::VersionArgs,
    view::ViewArgs,
    why::WhyArgs,
    with::WithArgs,
};
use crate::{
    cli_args::reporter::{CliReporter, EventFilter},
    config_deps::prepare_config,
};
use clap::CommandFactory;
use pnpm_config::Config;

pub(super) fn recursive<'a>(_ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(async move {
        let mut cmd = crate::cli_args::CliArgs::command();
        let _ = cmd
            .find_subcommand_mut("recursive")
            .expect("recursive subcommand")
            .print_help();
        #[expect(clippy::exit, reason = "`recursive` exits non-zero, mirroring pnpm")]
        std::process::exit(1);
    }))
}

// `outdated` is a read-only query: it prints a report to stdout and never
// installs. The reporter type only routes the `globalWarn` channel (skipped
// GitHub Actions repositories). It reports back whether any dependency was
// outdated; process termination stays here, at the top-level harness, rather
// than inside the command.
pub(super) fn outdated<'a>(
    ctx: &RunCtx<'a>,
    args: OutdatedArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.loaders.global_config)()?;
        return Ok(Box::pin(async move {
            if args.run_global(config).await? == OutdatedOutcome::Outdated {
                #[expect(
                    clippy::exit,
                    reason = "`outdated` exits non-zero when a dependency is outdated, mirroring pnpm"
                )]
                std::process::exit(1);
            }
            Ok(())
        }));
    }
    let command_state = ctx.prepared_state(false);
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        let outcome = args.run::<CliReporter>(command_state).await?;
        if outcome == OutdatedOutcome::Outdated {
            #[expect(
                clippy::exit,
                reason = "`outdated` exits non-zero when a dependency is outdated, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

pub(super) fn audit<'a>(ctx: &RunCtx<'a>, args: AuditArgs) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(true);
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        if Box::pin(args.run::<CliReporter>(command_state)).await? == AuditOutcome::Vulnerable {
            #[expect(
                clippy::exit,
                reason = "`audit` exits non-zero when vulnerabilities are found, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

pub(super) fn list<'a>(ctx: &RunCtx<'a>, args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move {
        apply_update_config(config, dir).await?;
        args.run(config, dir, recursive).await
    }))
}

pub(super) fn ll<'a>(ctx: &RunCtx<'a>, mut args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    args.output.long = true;
    list(ctx, args)
}

pub(super) fn licenses<'a>(
    ctx: &RunCtx<'a>,
    args: LicensesArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move {
        apply_update_config(config, dir).await?;
        args.run(config, dir, recursive).await
    }))
}

pub(super) fn why<'a>(ctx: &RunCtx<'a>, args: WhyArgs) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(true);
    Ok(Box::pin(async move { args.run(command_state.await?).await }))
}

pub(super) fn sbom<'a>(ctx: &RunCtx<'a>, args: SbomArgs) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(true);
    Ok(Box::pin(async move { args.run(command_state.await?).await }))
}

pub(super) fn peers<'a>(ctx: &RunCtx<'a>, args: PeersArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.loaders.config)()?;
    let recursive = ctx.workspace.recursive;
    let dir = ctx.locations.dir;
    Ok(Box::pin(async move {
        apply_update_config(cfg, dir).await?;
        if args.run(cfg, dir, recursive)? != PeersOutcome::NoIssues {
            #[expect(
                clippy::exit,
                reason = "`peers` exits non-zero when peer issues are found or the subcommand is unknown, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

pub(super) fn change<'a>(ctx: &RunCtx<'a>, args: ChangeArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.loaders.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(super) fn lane<'a>(ctx: &RunCtx<'a>, args: LaneArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.loaders.config)()?;
    let result = args.run(cfg);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn version<'a>(
    ctx: &RunCtx<'a>,
    args: VersionArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    Ok(Box::pin(async move { args.run::<CliReporter>(cfg, dir, recursive).await }))
}

pub(super) fn pack<'a>(ctx: &RunCtx<'a>, args: PackArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    let reporter_flags = ctx.reporter_flags;
    Ok(Box::pin(async move {
        let hooks = if args.json {
            prepare_config::<PackJsonReporter>(config, dir).await?
        } else {
            apply_update_config(config, dir).await?
        };
        reporter_flags.configure_with(config);
        // The pnpmfile's `updateConfig` hook can change `reporter` and `loglevel`.
        let reporter = reporter_flags.select_with(config);
        let print_output = args.json || reporter != ReporterType::Silent;
        let output = if args.json {
            args.run::<PackJsonReporter>(dir, config, recursive, hooks).await?
        } else {
            args.run::<CliReporter>(dir, config, recursive, hooks).await?
        };
        if print_output && !output.is_empty() {
            println!("{output}");
        }
        Ok(())
    }))
}

/// `publish` packs the project, runs its prepublish/publish lifecycle scripts,
/// and uploads the tarball. Co-located with its sibling `pack` (both come from
/// pnpm's `releasing/commands`).
///
/// `dir` / `config` / `recursive` are read off `ctx` here, before the boxed
/// future, so the future captures only owned/concrete values and never holds
/// `&RunCtx` — whose higher-ranked config closures would otherwise make the
/// boxed [`CommandFuture`] not `Send`.
pub(super) fn publish<'a>(
    ctx: &RunCtx<'a>,
    mut args: PublishArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    args.flags.output.report_summary |= ctx.workspace.report_summary;
    async fn run<Reporter: pnpm_reporter::Reporter>(
        args: PublishArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = prepare_config::<Reporter>(config, dir).await?;
        args.run::<Reporter>(dir, config, recursive, hooks).await
    }
    if args.flags.output.json {
        return Ok(Box::pin(async move {
            let _quiet = EventFilter::All.apply();
            run::<CliReporter>(args, dir, config, recursive).await
        }));
    }
    Ok(Box::pin(run::<CliReporter>(args, dir, config, recursive)))
}

/// `stage` shares `publish`'s dispatch shape: the values are read off `ctx`
/// before the boxed future so it captures only owned/concrete values (see
/// [`publish`]), and the subcommand's output is printed here, sanitized,
/// mirroring pnpm's `handler` → CLI print split.
pub(super) fn stage<'a>(
    ctx: &RunCtx<'a>,
    mut args: StageArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let recursive = ctx.workspace.recursive;
    args.flags.output.report_summary |= ctx.workspace.report_summary;
    async fn print_output<Reporter: pnpm_reporter::Reporter>(
        args: StageArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = if args.params
            .first()
            .is_some_and(|subcommand| subcommand == "publish")
        {
            prepare_config::<Reporter>(config, dir).await?
        } else {
            Vec::new()
        };
        if let Some(output) = args.run::<Reporter>(dir, config, recursive, hooks).await? {
            let output = super::sanitize::sanitize(&output);
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Ok(())
    }
    Ok(Box::pin(print_output::<CliReporter>(args, dir, config, recursive)))
}

// `pack-app` reads `pnpm.app` from package.json, resolves a Node.js version
// over the network, and shells out to build the SEA executables. It needs
// config (proxy / TLS / registry) and the canonicalized `--dir` but no
// lockfile or install pipeline, so it dispatches off `config()` like the other
// read-only commands.
pub(super) fn pack_app<'a>(
    ctx: &RunCtx<'a>,
    args: PackAppArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    Ok(Box::pin(async move { args.run(cfg, dir).await }))
}

mod registry;

mod maintenance;
