use super::run::RunArgs;
use clap::Args;

/// The arguments of a command that stands for one named script —
/// `pnpm test`, `pnpm start`, `pnpm stop`.
///
/// pnpm has no command by these names at all: they reach `run` through the
/// `pnpm <script>` fallback, so parsing stops at the command name and every
/// later token is the script's, a `--` separator and anything shaped like a
/// pnpm flag included. Declaring options of their own here would claim
/// those tokens instead (pnpm/pnpm#13301), so the single `trailing_var_arg`
/// positional is the whole grammar — the shape `run` uses for the same
/// reason.
#[derive(Debug, Args)]
pub struct ScriptShortcutArgs {
    /// Arguments passed to the script, verbatim.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl ScriptShortcutArgs {
    pub(crate) fn into_run_args(self, script_name: &str, if_present: bool) -> RunArgs {
        RunArgs {
            script: RunArgs::script(script_name, self.args),
            if_present,
            resume_from: None,
            report_summary: false,
            no_bail: false,
            sort: true,
            parallel: false,
            sequential: false,
        }
    }

    pub fn run(
        self,
        script_name: &str,
        if_present: bool,
        dir: &std::path::Path,
        config: &pacquet_config::Config,
        silent: bool,
    ) -> miette::Result<()> {
        self.into_run_args(script_name, if_present).run(dir, config, silent)
    }
}

#[cfg(test)]
mod tests;
