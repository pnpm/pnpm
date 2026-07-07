use clap::Args;

use super::run::RunArgs;

/// Runs a package's "stop" script, if one was provided.
#[derive(Debug, Args)]
pub struct StopArgs {
    /// Arguments passed to the script after the script name.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Avoid exiting with a non-zero exit code when the script is undefined.
    #[clap(long)]
    pub if_present: bool,
}

impl StopArgs {
    pub(crate) fn into_run_args(self) -> RunArgs {
        let StopArgs { args, if_present } = self;
        RunArgs {
            command: Some("stop".to_string()),
            args,
            if_present,
            resume_from: None,
            report_summary: false,
            no_bail: false,
            sort: true,
        }
    }

    pub fn run(
        self,
        dir: &std::path::Path,
        config: &pacquet_config::Config,
        silent: bool,
    ) -> miette::Result<()> {
        self.into_run_args().run(dir, config, silent)
    }
}

#[cfg(test)]
mod tests;
