use super::{clean::CleanArgs, install::InstallArgs};
use clap::Args;

#[derive(Debug, Args)]
pub struct CiArgs {
    #[clap(flatten)]
    pub install_args: InstallArgs,

    #[clap(flatten)]
    pub clean_args: CleanArgs,
}
