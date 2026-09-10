pub(super) use maintenance::{
    bin, bugs, cache, cat_file, cat_index, clean, config, config_get, config_set, docs, doctor,
    find_hash, ignored_builds, not_implemented, prefix, repo, root, self_update, setup, shim,
    store, with,
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
    team::TeamArgs,
    undeprecate::UndeprecateArgs,
    unpublish::UnpublishArgs,
    unstar::UnstarArgs,
    version::VersionArgs,
    view::ViewArgs,
    why::WhyArgs,
    with::WithArgs,
};
use crate::{State, config_deps::prepare_config};
use clap::CommandFactory;

use miette::Context;
use pnpm_config::Config;
use pnpm_default_reporter::DefaultReporter;
use pnpm_reporter::{NdjsonReporter, SilentReporter};

pub(super) fn recursive<'a>(_ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(async move {
        let mut cmd = crate::cli_args::CliArgs::command();
        let _ = cmd.find_subcommand_mut("recursive").expect("recursive subcommand").print_help();
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
        let config = (ctx.global_config)()?;
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
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path.to_path_buf();
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let command_state =
            State::init(manifest_path, config, false).wrap_err("initialize the state")?;
        let outcome = match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(command_state).await?
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(command_state).await?,
            ReporterType::Silent => args.run::<SilentReporter>(command_state).await?,
        };
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
    let command_state = (ctx.state)(true)?;
    macro_rules! run_audit {
        ($reporter:ty) => {
            Box::pin(async move {
                if args.run::<$reporter>(command_state).await? == AuditOutcome::Vulnerable {
                    #[expect(
                        clippy::exit,
                        reason = "`audit` exits non-zero when vulnerabilities are found, mirroring pnpm"
                    )]
                    std::process::exit(1);
                }
                Ok(())
            })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_audit!(DefaultReporter),
        ReporterType::Ndjson => run_audit!(NdjsonReporter),
        ReporterType::Silent => run_audit!(SilentReporter),
    })
}

pub(super) fn list<'a>(ctx: &RunCtx<'a>, args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
}

pub(super) fn ll<'a>(ctx: &RunCtx<'a>, mut args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    args.long = true;
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
}

pub(super) fn licenses<'a>(
    ctx: &RunCtx<'a>,
    args: LicensesArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
}

pub(super) fn why<'a>(ctx: &RunCtx<'a>, args: WhyArgs) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(args.run((ctx.state)(true)?)))
}

pub(super) fn sbom<'a>(ctx: &RunCtx<'a>, args: SbomArgs) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(args.run((ctx.state)(true)?)))
}

pub(super) fn peers<'a>(ctx: &RunCtx<'a>, args: PeersArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let recursive = ctx.recursive;
    let dir = ctx.dir;
    Ok(Box::pin(async move {
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
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(super) fn lane<'a>(ctx: &RunCtx<'a>, args: LaneArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let result = args.run(cfg);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn version<'a>(
    ctx: &RunCtx<'a>,
    args: VersionArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(cfg, dir, recursive).await
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(cfg, dir, recursive).await,
            ReporterType::Silent => args.run::<SilentReporter>(cfg, dir, recursive).await,
        }
    }))
}

// `pack` prints the tarball summary (or JSON) its handler returns; the
// reporter type only affects the lifecycle-script output, so it's threaded
// into `run` and the result printed here, mirroring pnpm's `handler` → CLI
// print split. `run` is async (it may invoke `beforePacking` pnpmfile
// hooks), so the work is deferred into the returned future.
pub(super) fn pack<'a>(ctx: &RunCtx<'a>, args: PackArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let reporter = ctx.reporter;
    async fn run<Reporter: pnpm_reporter::Reporter>(
        args: PackArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<String> {
        let hooks = prepare_config::<Reporter>(config, dir).await?;
        args.run::<Reporter>(dir, config, recursive, hooks).await
    }
    Ok(Box::pin(async move {
        let output = if args.json {
            run::<PackJsonReporter>(args, dir, config, recursive).await?
        } else {
            match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    run::<DefaultReporter>(args, dir, config, recursive).await?
                }
                ReporterType::Ndjson => run::<NdjsonReporter>(args, dir, config, recursive).await?,
                ReporterType::Silent => run::<SilentReporter>(args, dir, config, recursive).await?,
            }
        };
        if !output.is_empty() {
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
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    args.flags.report_summary |= ctx.recursive_report_summary;
    async fn run<Reporter: pnpm_reporter::Reporter>(
        args: PublishArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = prepare_config::<Reporter>(config, dir).await?;
        args.run::<Reporter>(dir, config, recursive, hooks).await
    }
    if args.flags.json {
        return Ok(Box::pin(run::<SilentReporter>(args, dir, config, recursive)));
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(run::<DefaultReporter>(args, dir, config, recursive))
        }
        ReporterType::Ndjson => Box::pin(run::<NdjsonReporter>(args, dir, config, recursive)),
        ReporterType::Silent => Box::pin(run::<SilentReporter>(args, dir, config, recursive)),
    })
}

/// `stage` shares `publish`'s dispatch shape: the values are read off `ctx`
/// before the boxed future so it captures only owned/concrete values (see
/// [`publish`]), and the subcommand's output is printed here, sanitized,
/// mirroring pnpm's `handler` → CLI print split.
pub(super) fn stage<'a>(
    ctx: &RunCtx<'a>,
    mut args: StageArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    args.flags.report_summary |= ctx.recursive_report_summary;
    async fn print_output<Reporter: pnpm_reporter::Reporter>(
        args: StageArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = if args.params.first().is_some_and(|subcommand| subcommand == "publish") {
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
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(print_output::<DefaultReporter>(args, dir, config, recursive))
        }
        ReporterType::Ndjson => {
            Box::pin(print_output::<NdjsonReporter>(args, dir, config, recursive))
        }
        ReporterType::Silent => {
            Box::pin(print_output::<SilentReporter>(args, dir, config, recursive))
        }
    })
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
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(Box::pin(async move { args.run(cfg, dir).await }))
}

mod registry;

mod maintenance;
