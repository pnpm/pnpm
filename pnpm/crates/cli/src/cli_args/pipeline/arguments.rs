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
