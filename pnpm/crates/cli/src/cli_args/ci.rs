use super::install::InstallArgs;
use clap::Args;

// `pnpm ci` takes no `pnpm clean` options: it never removes the lockfile,
// and `--lockfile` here is the `lockfile` setting.
#[derive(Debug, Args)]
pub struct CiArgs {
    #[clap(flatten)]
    pub install_args: InstallArgs,
}
