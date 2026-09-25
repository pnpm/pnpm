#[derive(Debug, Clone, clap::Args)]
pub struct WatchArgs {
    /// Watch a git repository and run the pipeline for every new revision
    /// of a branch, instead of running once against the current
    /// directory.
    #[clap(long, requires = "repo")]
    pub watch: bool,
    /// The repository the watch agent polls and builds — a URL or a local
    /// path, anything git accepts as a remote.
    #[clap(long, value_name = "REPO")]
    pub repo: Option<String>,
    /// The branch the watch agent follows.
    #[clap(long, default_value = "main", value_name = "NAME")]
    pub branch: String,
    /// Seconds between polls of the watched repository.
    #[clap(long, default_value_t = 30, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub interval: u64,
    /// With `--watch`: poll once, build if there is a new revision, and
    /// exit.
    #[clap(long, requires = "watch")]
    pub once: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct PipelineReportArgs {
    /// Publish the run's summary and event stream to the configured pnpr
    /// server (the `pnprServer` setting) once the run settles.
    #[clap(long)]
    pub report: bool,
    /// Publish the run to this pnpr server instead of the `pnprServer`
    /// setting — which also drives install offloading, so a server that
    /// only stores runs is better named here.
    #[clap(long = "report-to", value_name = "URL")]
    pub report_to: Option<String>,
}

/// The pipeline `pnpm pipeline` runs when no name is given.
pub const DEFAULT_PIPELINE_NAME: &str = "default";

#[derive(Debug, clap::Args)]
pub struct PipelineArgs {
    /// The pipeline to run, from the `pipelines` section of
    /// `pnpm-workspace.yaml`. Defaults to "default".
    pub name: Option<String>,
    /// The install `pnpm pipeline` performs first is always a frozen
    /// install; these flags tune the rest of it. `--dry-run` prints the
    /// task graph without installing or running anything.
    #[clap(flatten)]
    pub install_args: crate::cli_args::install::InstallArgs,
    /// With `--dry-run`, print the tasks and their resolved dependency
    /// edges as JSON.
    #[clap(long)]
    pub json: bool,
    /// Run every task without reading or writing cached results or Cargo snapshots.
    #[clap(long = "no-cache")]
    pub no_cache: bool,
    /// Run the pipeline over every workspace project instead of the
    /// affected-since-base selection.
    #[clap(long)]
    pub full: bool,
    /// The git ref the affected selection diffs against (its merge base
    /// with HEAD). Overrides the `pipelineBase` setting.
    #[clap(long)]
    pub base: Option<String>,
    #[clap(flatten)]
    pub agent: WatchArgs,
    #[clap(flatten)]
    pub reporting: PipelineReportArgs,
}

/// The pipeline-specific inputs of one invocation, split off
/// [`PipelineArgs`] once the install half has been consumed.
pub struct PipelineInvocation {
    pub name: Option<String>,
    pub dry_run: bool,
    pub json: bool,
    pub no_cache: bool,
    pub full: bool,
    pub base: Option<String>,
    pub report: bool,
    pub report_to: Option<String>,
}
