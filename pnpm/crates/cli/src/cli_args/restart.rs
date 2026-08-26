use clap::Args;

use super::{reporter::ReporterType, run::RunArgs};

/// Restarts a package. Runs a package's "stop", "restart", and "start"
/// scripts, and associated pre- and post- scripts.
#[derive(Debug, Args)]
pub struct RestartArgs {
    /// Arguments passed to each script after the script name.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Avoid exiting with a non-zero exit code when a script is undefined.
    #[clap(long)]
    pub if_present: bool,
}

impl RestartArgs {
    pub fn run(
        self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        reporter: ReporterType,
    ) -> miette::Result<()> {
        let RestartArgs { args, if_present } = self;

        for script_name in ["stop", "restart", "start"] {
            RunArgs {
                script: RunArgs::script(script_name, args.clone()),
                if_present,
                resume_from: None,
                report_summary: false,
                no_bail: false,
                sort: true,
                reverse: false,
                parallel: false,
                sequential: false,
                dry_run: false,
                json: false,
            }
            .run(dir, config, reporter)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
