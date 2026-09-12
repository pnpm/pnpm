use super::{
    super::install_test, CommandFuture, Config, InstallArgs, PipelineArgs, PipelineInvocation,
    ReporterType, RunCtx, UpdateCheckPolicy, WatchInvocation, install_with_config,
    install_with_update_check, reporter_emit, run_pipeline, run_watch,
};

pub(in super::super) fn install_test<'a>(
    ctx: &RunCtx<'a>,
    args: install_test::InstallTestArgs,
) -> miette::Result<CommandFuture<'a>> {
    let install_args = args.install_args;
    let mut run_args = super::super::run::RunArgs {
        script: super::super::run::RunArgs::script("test", args.args),
        if_present: ctx.if_present,
        resume_from: ctx.recursive_resume_from.map(str::to_string),
        report_summary: ctx.recursive_report_summary,
        no_bail: false,
        sort: true,
        reverse: false,
        parallel: ctx.recursive_parallel,
        sequential: false,
        dry_run: false,
        json: false,
    };

    let install_future = install_with_update_check(ctx, install_args, UpdateCheckPolicy::Skip)?;

    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let config = ctx.config;
    let reporter = ctx.reporter;

    Ok(Box::pin(async move {
        install_future.await?;

        let cfg = config()?;
        run_args.no_bail = !cfg.bail;
        run_args.sort = cfg.sort;
        run_args.reverse = cfg.reverse;
        if recursive {
            run_args.run_recursive(cfg, dir, reporter)?;
        } else {
            run_args.run(dir, cfg, reporter)?;
        }

        Ok(())
    }))
}

pub(in super::super) fn pipeline<'a>(
    ctx: &RunCtx<'a>,
    args: PipelineArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.watch {
        let config = ctx.config;
        return Ok(Box::pin(async move {
            let cfg = config()?;
            run_watch(&watch_invocation(args, cfg), &cfg.state_dir)
        }));
    }
    let (invocation, mut install_args) = pipeline_invocation(args);
    install_args.frozen_lockfile = true;
    install_args.dry_run = false;

    let install_future = if invocation.dry_run {
        None
    } else {
        Some(install_with_config(ctx, install_args, UpdateCheckPolicy::Skip)?)
    };
    let dir = ctx.dir;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = if let Some(install) = install_future { install.await? } else { config()? };
        let outcome = run_pipeline(&invocation, cfg, dir, reporter)?;
        // The run is recorded before the failure exit is raised, so a red
        // run reaches the server too.
        if invocation.report
            && let Some(upload) = outcome.upload
        {
            report_pipeline_run(cfg, invocation.report_to.as_deref(), upload, reporter).await;
        }
        if outcome.failed_tasks > 0 {
            return Err(super::super::pipeline::PipelineError::PipelineFail {
                count: outcome.failed_tasks,
            }
            .into());
        }
        Ok(())
    }))
}

fn watch_invocation(args: PipelineArgs, cfg: &Config) -> WatchInvocation {
    WatchInvocation {
        pipeline_name: args.name,
        repo: args.repo.expect("clap requires --repo with --watch"),
        branch: args.branch,
        interval: std::time::Duration::from_secs(args.interval),
        once: args.once,
        no_cache: args.no_cache,
        report: args.report || args.report_to.is_some(),
        report_to: args.report_to,
        npmrc_auth_file: cfg.npmrc_auth_file.clone(),
    }
}

/// The pipeline run the arguments ask for, and the install it runs first.
fn pipeline_invocation(args: PipelineArgs) -> (PipelineInvocation, InstallArgs) {
    let invocation = PipelineInvocation {
        name: args.name,
        // `--dry-run` prints the task graph and runs nothing, the
        // install included.
        dry_run: args.install_args.dry_run,
        json: args.json,
        no_cache: args.no_cache,
        full: args.full,
        base: args.base,
        report: args.report || args.report_to.is_some(),
        report_to: args.report_to,
    };
    (invocation, args.install_args)
}

/// Publish the run to the configured pnpr server. A run that could not be
/// reported still ran — the submission failure is a warning, not the
/// run's exit code.
async fn report_pipeline_run(
    cfg: &Config,
    report_to: Option<&str>,
    upload: super::super::pipeline::RunUpload,
    reporter: ReporterType,
) {
    let emit = reporter_emit(reporter);
    let warn = |message: String| {
        emit(&pnpm_reporter::LogEvent::Pnpm(pnpm_reporter::PnpmLog {
            level: pnpm_reporter::LogLevel::Warn,
            message,
            prefix: String::new(),
        }));
    };
    let Some(server) = report_to.or(cfg.pnpr_server.as_deref()) else {
        warn(
            "--report is set but neither --report-to nor pnprServer names a server; the run was not published"
                .to_string(),
        );
        return;
    };
    let client = pnpm_pnpr_client::PnprClient::new(server);
    let authorization = cfg.auth_headers.for_secure_url(server);
    let request = pnpm_pnpr_client::PublishPipelineRunRequest {
        workspace: upload.workspace,
        run_id: upload.run_id,
        summary: upload.summary,
        events: upload.events,
    };
    match client.publish_pipeline_run(&request, authorization.as_deref()).await {
        Ok(()) => emit(&pnpm_reporter::LogEvent::Pnpm(pnpm_reporter::PnpmLog {
            level: pnpm_reporter::LogLevel::Info,
            message: format!(
                "Run recorded on {server} as {}/{}",
                request.workspace, request.run_id,
            ),
            prefix: String::new(),
        })),
        Err(error) => warn(format!("failed to publish the pipeline run to {server}: {error}")),
    }
}
