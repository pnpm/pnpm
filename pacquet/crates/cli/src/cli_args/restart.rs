use clap::Args;

use super::run::RunArgs;

/// Restarts a package. Runs a package's "stop", "restart", and "start"
/// scripts, and associated pre- and post- scripts.
///
/// Ports the `restart` command from
/// <https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/restart.ts>.
///
/// Each script is executed through the full [`RunArgs`] pipeline, so
/// lifecycle hooks (`pre<name>` / `post<name>`) and environment setup
/// apply when `enablePrePostScripts` is set.
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
        config: &pacquet_config::Config,
        silent: bool,
    ) -> miette::Result<()> {
        let RestartArgs { args, if_present } = self;

        for script_name in ["stop", "restart", "start"] {
            RunArgs {
                command: Some(script_name.to_string()),
                args: args.clone(),
                if_present,
                resume_from: None,
                report_summary: false,
                no_bail: false,
            }
            .run(dir, config, silent)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
