use clap::{Args, ValueEnum};
use pnpm_config::{Config, InitType};

/// Create a `package.json` file.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Set the module system for the package. Defaults to "module".
    #[clap(long = "init-type", value_name = "commonjs|module")]
    pub init_type: Option<InitTypeArg>,

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

    /// `--init-type` layered over the `initType` setting.
    pub(crate) fn effective_init_type(&self, config: &Config) -> InitType {
        self.init_type.map_or(config.init_type, InitTypeArg::into_config)
    }
}

/// `--init-type` value parser. CLI mirror of [`pnpm_config::InitType`] so the
/// config crate stays free of `clap` as a dependency.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum InitTypeArg {
    Commonjs,
    Module,
}

impl InitTypeArg {
    #[inline]
    fn into_config(self) -> InitType {
        match self {
            InitTypeArg::Commonjs => InitType::Commonjs,
            InitTypeArg::Module => InitType::Module,
        }
    }
}
