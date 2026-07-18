use super::install::InstallArgs;
use clap::Args;

#[derive(Debug, Args)]
pub struct InstallTestArgs {
    #[clap(flatten)]
    pub install_args: InstallArgs,

    /// Arguments passed to the script after the script name.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}
