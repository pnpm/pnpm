use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use std::path::{Path, PathBuf};

mod recursive;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script.
    pub command: String,

    /// Any additional arguments passed after the script name
    pub args: Vec<String>,

    /// You can use the --if-present flag to avoid exiting with a non-zero exit code when the
    /// script is undefined. This lets you run potentially undefined scripts without breaking the
    /// execution chain.
    #[clap(long)]
    pub if_present: bool,

    /// Run the script starting from the given package, skipping every
    /// package that sorts before it. Only meaningful together with the
    /// global `-r` / `--recursive` flag. Mirrors pnpm's `--resume-from`.
    #[clap(long = "resume-from")]
    pub resume_from: Option<String>,

    /// Save the execution result of every package to
    /// `pnpm-exec-summary.json`. Only meaningful together with the
    /// global `-r` / `--recursive` flag. Mirrors pnpm's
    /// `--report-summary`.
    #[clap(long = "report-summary")]
    pub report_summary: bool,

    /// Keep running the remaining packages after a script fails instead
    /// of aborting on the first failure. Only meaningful together with
    /// the global `-r` / `--recursive` flag. Mirrors pnpm's `--no-bail`
    /// (recursive runs bail by default).
    #[clap(long = "no-bail")]
    pub no_bail: bool,
}

impl RunArgs {
    /// Execute the subcommand for a single project.
    pub fn run(self, manifest_path: PathBuf) -> miette::Result<()> {
        let RunArgs { command, args, if_present, .. } = self;

        let manifest = PackageManifest::from_path(manifest_path)
            .wrap_err("getting the package.json in current directory")?;

        if let Some(script) = manifest.script(&command, if_present)? {
            let mut command = script.to_string();
            // append an empty space between script and additional args
            command.push(' ');
            // then append the additional args
            command.push_str(&args.join(" "));
            execute_shell(command.trim())?;
        }

        Ok(())
    }

    /// Execute the subcommand for every project in the workspace, in
    /// topological order. The recursive counterpart of [`Self::run`],
    /// selected when the global `-r` / `--recursive` flag is set.
    pub fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        recursive::run_recursive(self, config, dir)
    }
}
