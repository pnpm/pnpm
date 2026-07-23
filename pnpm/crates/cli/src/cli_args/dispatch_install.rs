use super::{
    add::AddArgs,
    approve_builds::ApproveBuildsArgs,
    create::CreateArgs,
    dedupe::DedupeArgs,
    deploy::DeployArgs,
    dispatch::{CommandFuture, RunCtx},
    dlx::DlxArgs,
    fetch::FetchArgs,
    global,
    import::ImportArgs,
    install::InstallArgs,
    install_test,
    link::LinkArgs,
    patch::PatchArgs,
    patch_commit::PatchCommitArgs,
    patch_remove::PatchRemoveArgs,
    pipelines::{
        AddPipeline, DedupePipeline, DeployPipeline, InstallPipeline, PrunePipeline,
        RemovePipeline, UpdatePipeline, apply_install_cli_config,
        derive_config_root_and_package_manager_to_sync,
    },
    prune::PruneArgs,
    rebuild::RebuildArgs,
    remove::RemoveArgs,
    reporter::ReporterType,
    runtime::RuntimeArgs,
    unlink::UnlinkArgs,
    update::UpdateArgs,
};
use miette::Context;
use pacquet_default_reporter::DefaultReporter;
use pacquet_reporter::{NdjsonReporter, SilentReporter};

pub(super) fn add<'a>(ctx: &RunCtx<'a>, args: AddArgs) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.global_config)()?;
        args.apply_cli_config(config);
        let dir = ctx.dir;
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config, dir))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config, dir)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config, dir)),
        });
    }
    // Parsed up front: `AddPipeline::run` scaffolds a `package.json` through
    // `State::init`, and an invalid selector must be rejected before that.
    let config_dependencies = args.parse_config_dependencies()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        args.apply_cli_config(cfg);
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = AddPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
            config_dependencies,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn update<'a>(ctx: &RunCtx<'a>, args: UpdateArgs) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.global_config)()?;
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config)),
        });
    }
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = UpdatePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn remove<'a>(ctx: &RunCtx<'a>, args: RemoveArgs) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        global::handle_global_remove((ctx.global_config)()?, &args.package_names)?;
        return Ok(Box::pin(std::future::ready(Ok(()))));
    }
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = RemovePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn install<'a>(
    ctx: &RunCtx<'a>,
    args: InstallArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let config = ctx.config;
    Ok(Box::pin(async move {
        // Boxed for `clippy::large_stack_frames`: the three
        // monomorphized install futures would otherwise each reserve
        // their full size in this frame.
        {
            // CLI overrides for `offline` / `prefer_offline` live
            // alongside `--frozen-lockfile`: they upgrade an
            // unset / `false` yaml value to `true`, but cannot
            // turn an explicit yaml `true` back off. Matches
            // pnpm's CLI semantics — the flags are "enable", not
            // a toggle. Applied here (between `config()` and
            // `State::init`) while the loaded `Config` is still
            // mutable through `Config::leak`'s
            // `&'static mut Config` return.
            let cfg = config()?;
            apply_install_cli_config(cfg, &args);
            let require_lockfile = args.frozen_lockfile;
            let frozen_lockfile = args.frozen_lockfile;
            // Config dependencies are workspace-level state: their
            // `.pnpm-config` and env lockfile live at the lockfile /
            // workspace root, not the CLI cwd. Use the same root
            // `State::init` uses (`config.workspace_dir`, set when a
            // `pnpm-workspace.yaml` is found), falling back to `--dir`
            // for a single-package repo. Owned so it doesn't hold a
            // borrow of `cfg` across the `&mut` `updateConfig` pass.
            let (config_root, package_manager_to_sync) =
                derive_config_root_and_package_manager_to_sync(cfg, dir)
                    .wrap_err("derive workspace root and package manager policy")?;
            // Resolve + install configurational dependencies, then
            // run their `updateConfig` plugin hooks, before the main
            // install. The env lockfile must land at the top of
            // `pnpm-lock.yaml` before `State::init` loads the wanted
            // lockfile, and `updateConfig` must mutate `cfg` (still
            // `&'static mut`) before it's frozen and the install
            // reads it. Mirrors pnpm running both at
            // config-finalization.
            let pipeline = InstallPipeline {
                args,
                cfg,
                config_root,
                package_manager_to_sync,
                prefix: dir.to_path_buf(),
                manifest_path: manifest_path.to_path_buf(),
                recursive_sort,
                require_lockfile,
                frozen_lockfile,
            };
            match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(pipeline.run::<DefaultReporter>()).await?;
                }
                ReporterType::Ndjson => {
                    Box::pin(pipeline.run::<NdjsonReporter>()).await?;
                }
                ReporterType::Silent => {
                    Box::pin(pipeline.run::<SilentReporter>()).await?;
                }
            }
        }
        Ok(())
    }))
}

pub(super) fn install_test<'a>(
    ctx: &RunCtx<'a>,
    args: install_test::InstallTestArgs,
) -> miette::Result<CommandFuture<'a>> {
    let install_args = args.install_args;
    let run_args = super::run::RunArgs {
        command: Some("test".to_string()),
        args: args.args,
        if_present: ctx.if_present,
        resume_from: ctx.recursive_resume_from.map(str::to_string),
        report_summary: ctx.recursive_report_summary,
        no_bail: ctx.recursive_no_bail,
        sort: ctx.recursive_sort,
        sequential: false,
    };

    let install_future = install(ctx, install_args)?;

    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let config = ctx.config;
    let reporter = ctx.reporter;

    Ok(Box::pin(async move {
        install_future.await?;

        let cfg = config()?;
        if recursive {
            run_args.run_recursive(cfg, dir)?;
        } else {
            run_args.run(dir, cfg, matches!(reporter, ReporterType::Silent))?;
        }

        Ok(())
    }))
}

pub(super) fn deploy<'a>(ctx: &RunCtx<'a>, args: DeployArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        // Boxed for `clippy::large_stack_frames`: the three monomorphized
        // deploy futures would otherwise each reserve their full size in
        // this frame.
        {
            let cfg = config()?;
            apply_install_cli_config(cfg, &args.install_args);
            let (config_root, package_manager_to_sync) =
                derive_config_root_and_package_manager_to_sync(cfg, dir)
                    .wrap_err("derive workspace root and package manager policy")?;
            let pipeline = DeployPipeline { args, cfg, config_root, package_manager_to_sync };
            match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(pipeline.run::<DefaultReporter>(dir)).await?;
                }
                ReporterType::Ndjson => {
                    Box::pin(pipeline.run::<NdjsonReporter>(dir)).await?;
                }
                ReporterType::Silent => {
                    Box::pin(pipeline.run::<SilentReporter>(dir)).await?;
                }
            }
        }
        Ok(())
    }))
}

pub(super) fn dedupe<'a>(ctx: &RunCtx<'a>, args: DedupeArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let dedupe = DedupePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            manifest_path: manifest_path.to_path_buf(),
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(dedupe.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(dedupe.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(dedupe.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn prune<'a>(ctx: &RunCtx<'a>, args: PruneArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = PrunePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            manifest_path: manifest_path.to_path_buf(),
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => {
                Box::pin(pipeline.run::<NdjsonReporter>()).await?;
            }
            ReporterType::Silent => {
                Box::pin(pipeline.run::<SilentReporter>()).await?;
            }
        }
        Ok(())
    }))
}

pub(super) fn fetch<'a>(ctx: &RunCtx<'a>, args: FetchArgs) -> miette::Result<CommandFuture<'a>> {
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>((ctx.state)(true)?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>((ctx.state)(true)?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>((ctx.state)(true)?)),
    })
}

pub(super) fn import<'a>(ctx: &RunCtx<'a>, args: ImportArgs) -> miette::Result<CommandFuture<'a>> {
    let command_state = (ctx.state)(false)?;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(command_state))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
    })
}

pub(super) fn link<'a>(ctx: &RunCtx<'a>, args: LinkArgs) -> miette::Result<CommandFuture<'a>> {
    let manifest_path = ctx.manifest_path.to_path_buf();
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>((ctx.config)()?, manifest_path))
        }
        ReporterType::Ndjson => {
            Box::pin(args.run::<NdjsonReporter>((ctx.config)()?, manifest_path))
        }
        ReporterType::Silent => {
            Box::pin(args.run::<SilentReporter>((ctx.config)()?, manifest_path))
        }
    })
}

pub(super) fn unlink<'a>(ctx: &RunCtx<'a>, args: UnlinkArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        // Strip the matching `link:` overrides; stop early when there is
        // nothing to unlink.
        if !args.strip_link_overrides(cfg, manifest_path)? {
            return Ok(());
        }
        // Reinstall through the install-family pipeline, exactly as pnpm's
        // `unlink` delegates to its install handler, so `-r` / `--filter`
        // selection and per-project lockfiles apply. The reinstall forces a
        // fresh resolution so the removed `link:` overrides re-resolve from
        // the registry.
        let (config_root, package_manager_to_sync) =
            derive_config_root_and_package_manager_to_sync(cfg, dir)
                .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = InstallPipeline {
            args: InstallArgs::for_reresolving_install(),
            cfg,
            config_root,
            package_manager_to_sync,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
            require_lockfile: false,
            frozen_lockfile: false,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn rebuild<'a>(
    ctx: &RunCtx<'a>,
    args: RebuildArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let recursive_sort = ctx.recursive_sort;
    let recursive_no_bail = ctx.recursive_no_bail;
    let config = ctx.config;
    Ok(Box::pin(async move {
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_from_cli::<DefaultReporter>(
                    config()?,
                    dir.to_path_buf(),
                    manifest_path.to_path_buf(),
                    recursive_sort,
                    recursive_no_bail,
                ))
                .await?;
            }
            ReporterType::Ndjson => {
                Box::pin(args.run_from_cli::<NdjsonReporter>(
                    config()?,
                    dir.to_path_buf(),
                    manifest_path.to_path_buf(),
                    recursive_sort,
                    recursive_no_bail,
                ))
                .await?;
            }
            ReporterType::Silent => {
                Box::pin(args.run_from_cli::<SilentReporter>(
                    config()?,
                    dir.to_path_buf(),
                    manifest_path.to_path_buf(),
                    recursive_sort,
                    recursive_no_bail,
                ))
                .await?;
            }
        }
        Ok(())
    }))
}

pub(super) fn runtime<'a>(
    ctx: &RunCtx<'a>,
    args: RuntimeArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.global_config)()?;
        let dir = ctx.dir;
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config, dir))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config, dir)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config, dir)),
        });
    }
    let command_state = (ctx.state)(false)?;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(command_state))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
    })
}

pub(super) fn patch<'a>(ctx: &RunCtx<'a>, args: PatchArgs) -> miette::Result<CommandFuture<'a>> {
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

pub(super) fn patch_commit<'a>(
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

pub(super) fn patch_remove<'a>(
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

pub(super) fn dlx<'a>(ctx: &RunCtx<'a>, args: DlxArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(dir, (ctx.config)()?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir, (ctx.config)()?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir, (ctx.config)()?)),
    })
}

pub(super) fn create<'a>(ctx: &RunCtx<'a>, args: CreateArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(dir, (ctx.config)()?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir, (ctx.config)()?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir, (ctx.config)()?)),
    })
}

pub(super) fn approve_builds<'a>(
    ctx: &RunCtx<'a>,
    args: ApproveBuildsArgs,
) -> miette::Result<CommandFuture<'a>> {
    // The settings/prompt work is synchronous; only the rebuild is async, so
    // the non-`Send` `config` / `state` closures stay out of the awaited
    // future.
    let Some((rebuild_state, build_packages)) = args.prepare(ctx.dir, ctx.config, ctx.state)?
    else {
        return Ok(Box::pin(std::future::ready(Ok(()))));
    };
    let selected =
        super::rebuild::RebuildSelection { names: Some(build_packages), projects: Vec::new() };
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            super::rebuild::run_rebuild::<DefaultReporter>(&rebuild_state, selected, None).await
        }),
        ReporterType::Ndjson => Box::pin(async move {
            super::rebuild::run_rebuild::<NdjsonReporter>(&rebuild_state, selected, None).await
        }),
        ReporterType::Silent => Box::pin(async move {
            super::rebuild::run_rebuild::<SilentReporter>(&rebuild_state, selected, None).await
        }),
    })
}
