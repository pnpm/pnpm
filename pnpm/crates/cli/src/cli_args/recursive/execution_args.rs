#[derive(Debug, Clone, clap::Args)]
pub struct RecursiveExecutionArgs {
    /// Run the command starting from the given package, skipping every
    /// package that sorts before it. Only meaningful together with the
    /// global `-r` / `--recursive` flag (the `--resume-from` flag).
    #[clap(skip)]
    pub resume_from: Option<String>,
    /// Save the execution result of every package to
    /// `pnpm-exec-summary.json`. Only meaningful together with the
    /// global `-r` / `--recursive` flag (the `--report-summary` flag).
    #[clap(skip)]
    pub report_summary: bool,
    /// Keep running the remaining scripts after one fails instead of
    /// aborting on the first failure (the global `--no-bail` flag).
    /// Applies to a recursive run and to a `/pattern/` run that selects
    /// several scripts; both bail by default.
    #[clap(skip)]
    pub no_bail: bool,
    /// Sort recursive workspace projects topologically before running.
    #[clap(skip = true)]
    pub sort: bool,
    /// Reverse the project order of a recursive command.
    #[clap(skip = true)]
    pub reverse: bool,
    /// Start commands in all selected projects concurrently.
    #[clap(skip = true)]
    pub parallel: bool,
}
