use super::audit::AuditOutcome;
use super::cli_command::{CliArgs, CliCommand};
use super::install::InstallArgs;
use super::outdated::OutdatedOutcome;
use super::pipelines::{
    DedupePipeline, DeployPipeline, InstallPipeline, PrunePipeline, apply_install_cli_config,
    derive_config_root_and_package_manager_to_sync,
};
use super::reporter::{ReporterType, configure_default_reporter, reporter_emit};
use super::{global, ignored_builds, rebuild, sanitize, whoami};
use crate::{State, config_overrides::ConfigOverrides};
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, Host};
use pacquet_default_reporter::DefaultReporter;
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::{ExecutionTimeLog, LogEvent, LogLevel, NdjsonReporter, SilentReporter};
use std::{future::Future, pin::Pin};

type CommandFuture<'a> = Pin<Box<dyn Future<Output = miette::Result<()>> + Send + 'a>>;

impl CliArgs {
    pub fn run_completion_if_requested(&self) -> miette::Result<bool> {
        match &self.command {
            CliCommand::Completion(args) => {
                args.run()?;
                Ok(true)
            }
            CliCommand::CompletionServer(args) => {
                args.run()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Try to finish `pacquet install` synchronously through the
    /// repeat-install fast path, before the caller builds the async
    /// runtime. `true` means the install completed (the "Already up to
    /// date" events were emitted); `false` means undecided — proceed
    /// with [`Self::run`], which loads its own config and re-runs the
    /// same check.
    ///
    /// Mirrors the install arm of [`Self::run`]'s dispatch: the same
    /// canonicalized `--dir`, the same config layering (`.npmrc` auth
    /// file seed + `--config.<key>` overrides). Workspace-filtered and
    /// recursive installs always take the full path.
    pub fn finished_via_install_fast_path(&self, config_overrides: &ConfigOverrides) -> bool {
        let started_at = now_millis();
        let CliCommand::Install(install_args) = &self.command else {
            return false;
        };
        if self.recursive || !self.filter.is_empty() || !self.filter_prod.is_empty() {
            return false;
        }
        let Ok(dir) = dunce::canonicalize(&self.dir) else {
            return false;
        };
        let loaded = Config { npmrc_auth_file: self.npmrc_auth_file.clone(), ..Config::default() }
            .current::<Host>(&dir);
        let Ok(mut config) = loaded else {
            return false;
        };
        config_overrides.apply(&mut config);
        configure_default_reporter(self.reporter, &dir);
        let emit = reporter_emit(self.reporter);
        let finished = install_args.finished_via_up_to_date_fast_path(&dir, &config, emit);
        if finished {
            // The fast path returns from `main` before `run` reaches its
            // end-of-command emit, so the `Done in ...` footer must be emitted
            // here too to match the non-fast-path output.
            emit(&LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at,
                ended_at: now_millis(),
            }));
        }
        finished
    }

    /// Execute the command. `config_overrides` carries `--config.<key>=<value>`
    /// tokens already stripped from argv by [`ConfigOverrides::extract`];
    /// they're layered on top of `.npmrc` / `pnpm-workspace.yaml` whenever
    /// `Config` is loaded, mirroring pnpm 11's
    /// "CLI > yaml > .npmrc > defaults" precedence.
    #[allow(
        clippy::large_stack_frames,
        reason = "the run function dispatches all CLI commands and contains large types like Install on the stack"
    )]
    pub async fn run(self, config_overrides: &ConfigOverrides) -> miette::Result<()> {
        if self.run_completion_if_requested()? {
            return Ok(());
        }

        let CliArgs { command, dir, npmrc_auth_file, recursive, reporter, filter, filter_prod } =
            self;

        // Canonicalize `--dir` so the bunyan-envelope `prefix` emitted by
        // the reporter is the same absolute, symlink-resolved path that
        // `@pnpm/cli.default-reporter` derives via `process.cwd()`. Without
        // this, a default `--dir=.` leaves `prefix` as `"."`, the reporter
        // never matches it against its `cwd`, and every progress / stats
        // line gets a redundant `.` path prefix prepended. Mirrors pnpm's
        // <https://github.com/pnpm/pnpm/blob/8eb1be4988/config/reader/src/index.ts#L270>
        // `cwd = fs.realpathSync(betterPathResolve(cliOptions.dir ...))`,
        // later assigned to `pnpmConfig.dir` (used as the install
        // `lockfileDir`, threaded into every event's `prefix`).
        let dir = dunce::canonicalize(&dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("canonicalizing the `--dir` argument: {}", dir.display()))?;
        // The default reporter renders paths relative to the install root and
        // its `Done in ...` footer over the whole command; seed both before any
        // event can fire.
        configure_default_reporter(reporter, &dir);
        let started_at = now_millis();
        let is_install_family = matches!(
            &command,
            CliCommand::Add(_)
                | CliCommand::Update(_)
                | CliCommand::Remove(_)
                | CliCommand::Install(_)
                | CliCommand::Dlx(_)
                | CliCommand::Link(_)
                | CliCommand::Import(_)
                | CliCommand::Dedupe(_)
                | CliCommand::Deploy(_)
                | CliCommand::Prune(_)
                | CliCommand::Fetch(_)
                | CliCommand::Unlink(_)
                | CliCommand::Create(_)
                | CliCommand::Runtime(_)
                // `rebuild` drives the frozen-install pipeline and emits
                // the same progress events, so it shares the `Done in ...`
                // footer.
                | CliCommand::Rebuild(_)
                | CliCommand::PatchCommit(_)
                | CliCommand::PatchRemove(_),
        );
        let manifest_path = dir.join("package.json");
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`) —
        // see [`loadNpmrcConfig`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/loadNpmrcFiles.ts#L48-L50).
        //
        // Production callers turbofish `Host` explicitly so the
        // dependency-injection plumbing is visible at the call site.
        // See [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339)
        // for the pattern and rationale.
        let config = || -> miette::Result<&'static mut Config> {
            // Seed `npmrc_auth_file` from the CLI flag before
            // `current()` reads `.npmrc`, so the override redirects the
            // user-level read. Mirrors pnpm's `--npmrc-auth-file`.
            Config { npmrc_auth_file: npmrc_auth_file.clone(), ..Config::default() }
                .current::<Host>(&dir)
                .map(|mut cfg| {
                    config_overrides.apply(&mut cfg);
                    // `--recursive` / `-r` is CLI-only upstream (not a
                    // `.npmrc` / yaml key), so it is set here from the
                    // global flag rather than through the yaml / env
                    // overlay. Mirrors pnpm's `Config.recursive`.
                    cfg.recursive = recursive;
                    // `--filter` / `--filter-prod` are likewise CLI-only
                    // (pnpm's `Config.filter` / `Config.filterProd`),
                    // so the parsed selector strings are threaded in
                    // from the global flags here.
                    cfg.filter.clone_from(&filter);
                    cfg.filter_prod.clone_from(&filter_prod);
                    Config::leak(cfg)
                })
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
        };
        // `require_lockfile` is the "this subcommand cannot run without a
        // lockfile loaded" signal, used by `State::init` to override
        // `config.lockfile=false`. Only `install --frozen-lockfile` needs
        // it today; other subcommands follow `config.lockfile`. Matches
        // pnpm's CLI: `--frozen-lockfile` is the strongest signal and
        // must not be silently dropped because `lockfile=false` was set
        // (or defaulted) in config.
        let state = |require_lockfile: bool| -> miette::Result<State> {
            State::init(manifest_path.clone(), config()?, require_lockfile)
                .wrap_err("initialize the state")
        };

        let dir_ref = &dir;
        let manifest_path_ref = &manifest_path;
        let command_future: CommandFuture<'_> = match command {
            CliCommand::Init => {
                let result =
                    PackageManifest::init(manifest_path_ref).wrap_err("initialize package.json");
                Box::pin(std::future::ready(result))
            }
            CliCommand::Add(args) if args.global => {
                let config = config()?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run_global::<DefaultReporter>(config, dir_ref))
                    }
                    ReporterType::Ndjson => {
                        Box::pin(args.run_global::<NdjsonReporter>(config, dir_ref))
                    }
                    ReporterType::Silent => {
                        Box::pin(args.run_global::<SilentReporter>(config, dir_ref))
                    }
                }
            }
            CliCommand::Add(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Update(args) if args.global => {
                let config = config()?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run_global::<DefaultReporter>(config))
                    }
                    ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config)),
                    ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config)),
                }
            }
            CliCommand::Update(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            // `outdated` is a read-only query: it prints a report to
            // stdout and never installs, so it has no reporter-typed
            // install pipeline to dispatch on. It reports back whether any
            // dependency was outdated; process termination stays here, at
            // the top-level harness, rather than inside the command.
            CliCommand::Outdated(args) if args.global => {
                let config = config()?;
                Box::pin(async move {
                    if args.run_global(config).await? == OutdatedOutcome::Outdated {
                        std::process::exit(1);
                    }
                    Ok(())
                })
            }
            CliCommand::Outdated(args) => {
                let command_state = state(false)?;
                Box::pin(async move {
                    if args.run(command_state).await? == OutdatedOutcome::Outdated {
                        std::process::exit(1);
                    }
                    Ok(())
                })
            }
            CliCommand::Audit(args) => {
                let command_state = state(true)?;
                macro_rules! run_audit {
                    ($reporter:ty) => {
                        Box::pin(async move {
                            if args.run::<$reporter>(command_state).await?
                                == AuditOutcome::Vulnerable
                            {
                                std::process::exit(1);
                            }
                            Ok(())
                        })
                    };
                }
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => run_audit!(DefaultReporter),
                    ReporterType::Ndjson => run_audit!(NdjsonReporter),
                    ReporterType::Silent => run_audit!(SilentReporter),
                }
            }
            CliCommand::List(args) => {
                args.run(config()?, dir_ref)?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Ll(mut args) => {
                args.long = true;
                args.run(config()?, dir_ref)?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Why(args) => Box::pin(args.run(state(true)?)),
            CliCommand::Remove(args) if args.global => {
                global::handle_global_remove(config()?, &args.package_names)?;
                Box::pin(std::future::ready(Ok(())))
            }
            // `whoami` is a read-only registry query: it resolves the
            // default registry's auth header from config and GETs
            // `-/whoami`, with no lockfile or install pipeline. It needs
            // an async future for the request but no reporter-typed
            // fan-out, so it dispatches off `config()` like the other
            // read-only commands.
            CliCommand::Whoami => {
                let cfg: &Config = config()?;
                Box::pin(async move {
                    let username = whoami::whoami(cfg).await?;
                    println!("{}", sanitize::sanitize(&username));
                    Ok(())
                })
            }
            CliCommand::DistTag(args) => {
                let cfg: &Config = config()?;
                Box::pin(async move {
                    if let Some(output) = args.run(cfg).await? {
                        let output = sanitize::sanitize(&output);
                        if output.is_empty() {
                            return Ok(());
                        }
                        println!("{output}");
                    }
                    Ok(())
                })
            }
            // `ping` is a read-only connectivity check: it resolves the
            // registry (and any auth header) from config and GETs
            // `-/ping`, with no lockfile or install pipeline, so it
            // dispatches off `config()` like the other read-only registry
            // commands.
            CliCommand::Ping(args) => {
                let cfg: &Config = config()?;
                Box::pin(async move {
                    let report = args.run(cfg).await?;
                    println!("{report}");
                    Ok(())
                })
            }
            // `pack` prints the tarball summary (or JSON) its handler
            // returns; the reporter type only affects the lifecycle-script
            // output, so it's threaded into `run` and the result printed
            // here, mirroring pnpm's `handler` → CLI print split. The
            // handler is synchronous, so this arm resolves to a ready
            // future once the output is printed.
            CliCommand::Pack(args) => {
                let output = match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        args.run::<DefaultReporter>(dir_ref, config()?, recursive)?
                    }
                    ReporterType::Ndjson => {
                        args.run::<NdjsonReporter>(dir_ref, config()?, recursive)?
                    }
                    ReporterType::Silent => {
                        args.run::<SilentReporter>(dir_ref, config()?, recursive)?
                    }
                };
                if !output.is_empty() {
                    println!("{output}");
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Remove(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Patch(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                        args.run::<DefaultReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                    ReporterType::Ndjson => Box::pin(async move {
                        args.run::<NdjsonReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                    ReporterType::Silent => Box::pin(async move {
                        args.run::<SilentReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                }
            }
            CliCommand::PatchCommit(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                    if Box::pin(args.run::<DefaultReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<DefaultReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
                ReporterType::Ndjson => Box::pin(async move {
                    if Box::pin(args.run::<NdjsonReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<NdjsonReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
                ReporterType::Silent => Box::pin(async move {
                    if Box::pin(args.run::<SilentReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<SilentReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
            },
            CliCommand::PatchRemove(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<DefaultReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
                ReporterType::Ndjson => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<NdjsonReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
                ReporterType::Silent => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<SilentReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
            },
            // `set-script` only rewrites `package.json#scripts`; it never
            // touches the lockfile or runs the install pipeline, so it
            // dispatches synchronously off the canonicalized `--dir` like
            // `init`, with no reporter-typed fan-out.
            CliCommand::SetScript(args) => {
                let result = args.run(manifest_path_ref);
                Box::pin(std::future::ready(result))
            }
            CliCommand::Install(args) => Box::pin(async move {
                // Boxed for `clippy::large_stack_frames`: the three
                // monomorphized install futures would otherwise each reserve
                // their full size in this frame.
                #[allow(
                    clippy::large_stack_frames,
                    reason = "the three monomorphized install futures would otherwise each reserve their full size in this frame"
                )]
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
                        derive_config_root_and_package_manager_to_sync(cfg, dir_ref)
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
                        manifest_path: manifest_path_ref.clone(),
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
            }),
            CliCommand::Deploy(args) => Box::pin(async move {
                #[allow(
                    clippy::large_stack_frames,
                    reason = "the three monomorphized deploy futures would otherwise each reserve their full size in this frame"
                )]
                {
                    let cfg = config()?;
                    apply_install_cli_config(cfg, &args.install_args);
                    let (config_root, package_manager_to_sync) =
                        derive_config_root_and_package_manager_to_sync(cfg, dir_ref)
                            .wrap_err("derive workspace root and package manager policy")?;
                    let pipeline =
                        DeployPipeline { args, cfg, config_root, package_manager_to_sync };
                    match reporter {
                        ReporterType::Default | ReporterType::AppendOnly => {
                            Box::pin(pipeline.run::<DefaultReporter>(dir_ref)).await?;
                        }
                        ReporterType::Ndjson => {
                            Box::pin(pipeline.run::<NdjsonReporter>(dir_ref)).await?;
                        }
                        ReporterType::Silent => {
                            Box::pin(pipeline.run::<SilentReporter>(dir_ref)).await?;
                        }
                    }
                }
                Ok(())
            }),
            CliCommand::Test => {
                let manifest = PackageManifest::from_path(manifest_path_ref.clone())
                    .wrap_err("getting the package.json in current directory")?;
                if let Some(script) = manifest.script("test", false)? {
                    execute_shell(script).wrap_err(format!("executing command: \"{script}\""))?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Run(args) => {
                if recursive {
                    args.run_recursive(config()?, dir_ref)?;
                } else {
                    args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Exec(args) => {
                if recursive {
                    args.run_recursive(config()?, dir_ref)?;
                } else {
                    args.run(dir_ref, config()?)?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Dlx(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(dir_ref, config()?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir_ref, config()?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir_ref, config()?)),
            },
            CliCommand::Create(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(dir_ref, config()?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir_ref, config()?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir_ref, config()?)),
            },
            CliCommand::Start => {
                let manifest = PackageManifest::from_path(manifest_path_ref.clone())
                    .wrap_err("getting the package.json in current directory")?;
                let command = manifest.script("start", true)?.unwrap_or("node server.js");
                execute_shell(command).wrap_err(format!("executing command: \"{command}\""))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Stop(args) => {
                args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Restart(args) => {
                args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::FindHash(args) => {
                args.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Runtime(args) => {
                args.reject_unsupported_global()?;
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Root(args) => {
                args.run(dir_ref)?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Config(args) => {
                args.run(config()?, dir_ref)?;
                Box::pin(std::future::ready(Ok(())))
            }
            // `pack-app` reads `pnpm.app` from package.json, resolves a
            // Node.js version over the network, and shells out to build the
            // SEA executables. It needs config (proxy / TLS / registry) and
            // the canonicalized `--dir` but no lockfile or install
            // pipeline, so it dispatches off `config()` like the other
            // read-only commands.
            CliCommand::PackApp(args) => {
                let cfg: &Config = config()?;
                Box::pin(async move { args.run(cfg, dir_ref).await })
            }
            CliCommand::Store(command) => {
                command.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Cache(command) => {
                command.run(config()?)?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::CatFile(args) => {
                args.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Link(args) => {
                let manifest_path = manifest_path_ref.clone();
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(config()?, manifest_path))
                    }
                    ReporterType::Ndjson => {
                        Box::pin(args.run::<NdjsonReporter>(config()?, manifest_path))
                    }
                    ReporterType::Silent => {
                        Box::pin(args.run::<SilentReporter>(config()?, manifest_path))
                    }
                }
            }
            CliCommand::Dedupe(args) => Box::pin(async move {
                let cfg = config()?;
                let (config_root, package_manager_to_sync) =
                    derive_config_root_and_package_manager_to_sync(cfg, dir_ref)
                        .wrap_err("derive workspace root and package manager policy")?;
                let dedupe = DedupePipeline {
                    args,
                    cfg,
                    config_root,
                    package_manager_to_sync,
                    manifest_path: manifest_path_ref.clone(),
                };
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(dedupe.run::<DefaultReporter>()).await?;
                    }
                    ReporterType::Ndjson => Box::pin(dedupe.run::<NdjsonReporter>()).await?,
                    ReporterType::Silent => Box::pin(dedupe.run::<SilentReporter>()).await?,
                }
                Ok(())
            }),
            CliCommand::Import(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::CatIndex(args) => Box::pin(async move {
                args.run(dir_ref, || config().map(|m| &*m)).await?;
                Ok(())
            }),
            CliCommand::Unlink(args) => {
                let manifest_path = manifest_path_ref.clone();
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(config()?, manifest_path))
                    }
                    ReporterType::Ndjson => {
                        Box::pin(args.run::<NdjsonReporter>(config()?, manifest_path))
                    }
                    ReporterType::Silent => {
                        Box::pin(args.run::<SilentReporter>(config()?, manifest_path))
                    }
                }
            }
            CliCommand::Fetch(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(state(true)?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(state(true)?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(state(true)?)),
            },
            CliCommand::Prune(args) => Box::pin(async move {
                let cfg = config()?;
                let (config_root, package_manager_to_sync) =
                    derive_config_root_and_package_manager_to_sync(cfg, dir_ref)
                        .wrap_err("derive workspace root and package manager policy")?;
                let pipeline = PrunePipeline {
                    args,
                    cfg,
                    config_root,
                    package_manager_to_sync,
                    manifest_path: manifest_path_ref.clone(),
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
            }),
            CliCommand::IgnoredBuilds(_) => {
                let output = ignored_builds::render_ignored_builds(config()?)?;
                print!("{output}");
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Rebuild(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(state(true)?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(state(true)?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(state(true)?)),
            },
            CliCommand::ApproveBuilds(args) => {
                // The settings/prompt work is synchronous; only the rebuild
                // is async, so the non-`Send` `config` / `state` closures
                // stay out of the awaited future.
                if let Some((rebuild_state, build_packages)) =
                    args.prepare(dir_ref, &config, &state)?
                {
                    let selected = Some(build_packages);
                    match reporter {
                        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                            rebuild::run_rebuild::<DefaultReporter>(&rebuild_state, selected).await
                        }),
                        ReporterType::Ndjson => Box::pin(async move {
                            rebuild::run_rebuild::<NdjsonReporter>(&rebuild_state, selected).await
                        }),
                        ReporterType::Silent => Box::pin(async move {
                            rebuild::run_rebuild::<SilentReporter>(&rebuild_state, selected).await
                        }),
                    }
                } else {
                    Box::pin(std::future::ready(Ok(())))
                }
            }
            CliCommand::Docs(args) => {
                let cfg = config()?;
                Box::pin(async move { args.run(cfg).await })
            }
            CliCommand::Completion(_) | CliCommand::CompletionServer(_) => {
                unreachable!("completion returns before configuration")
            }
        };

        command_future.await?;

        // The `Done in ...` footer covers the whole command, mirroring pnpm's
        // `pnpm:execution-time` emit in `main.ts`. Only the install-family
        // commands drive the visual reporter, so the rest stay silent.
        if is_install_family {
            reporter_emit(reporter)(&LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at,
                ended_at: now_millis(),
            }));
        }

        Ok(())
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}
