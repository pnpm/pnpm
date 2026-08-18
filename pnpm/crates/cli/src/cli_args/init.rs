use clap::Args;
use pnpm_config::Config;

/// Create a `package.json` file.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Pin the pnpm version in package.json, through
    /// "devEngines.packageManager" and "packageManager", and auto-download
    /// pnpm when it is missing.
    #[clap(long = "init-package-manager", overrides_with = "no_init_package_manager")]
    pub init_package_manager: bool,

    /// Scaffold the manifest without a pnpm version pin.
    #[clap(long = "no-init-package-manager", overrides_with = "init_package_manager")]
    pub no_init_package_manager: bool,
}

impl InitArgs {
    /// `--init-package-manager` / `--no-init-package-manager` layered over
    /// the `initPackageManager` setting.
    pub(crate) fn effective_init_package_manager(&self, config: &Config) -> bool {
        if self.init_package_manager {
            true
        } else if self.no_init_package_manager {
            false
        } else {
            config.init_package_manager
        }
    }
}
