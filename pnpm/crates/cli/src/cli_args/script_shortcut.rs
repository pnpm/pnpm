use super::run::RunArgs;
use clap::Args;

/// The arguments of a command that stands for one named script —
/// `pnpm test`, `pnpm start`, `pnpm stop`.
// pnpm has no command by these names at all: they reach `run` through the
// `pnpm <script>` fallback, so parsing stops at the command name and every
// later token is the script's, a `--` separator and anything shaped like a
// pnpm flag included. Declaring options of their own here would claim
// those tokens instead (pnpm/pnpm#13301), so the single `trailing_var_arg`
// positional is the whole grammar — the shape `run` uses for the same
// reason.
#[derive(Debug, Args)]
pub struct ScriptShortcutArgs {
    /// Arguments passed to the script, verbatim.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl ScriptShortcutArgs {
    pub(crate) fn into_run_args(self, script_name: &str) -> RunArgs {
        RunArgs {
            script: RunArgs::script(script_name, self.args),
            if_present: false,
            sequential: false,
            dry_run: false,
            json: false,
            workspace: crate::cli_args::recursive::RecursiveExecutionArgs {
                resume_from: None,
                report_summary: false,
                no_bail: false,
                sort: true,
                reverse: false,
                parallel: false,
            },
        }
    }
}

#[cfg(test)]
mod tests;
