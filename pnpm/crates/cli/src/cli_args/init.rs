use super::self_update::{install_pnpm::is_release_installable, version_lt};
use crate::config_deps;
use clap::{Args, ValueEnum};
use pnpm_config::{Config, InitType, PNPM_VERSION};
use std::{path::Path, time::Duration};

/// How long the `latest` lookup may take before `pnpm init` gives up on it.
/// Much shorter than the resolver's usual timeout: the version is a nicety,
/// and a scaffold command that appears to hang is worse than one that pins
/// the running version.
const LATEST_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Create a `package.json` file.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Set the module system for the package. Defaults to "module".
    #[clap(long = "init-type", value_name = "commonjs|module")]
    pub init_type: Option<InitTypeArg>,

    /// Pin the latest pnpm version in package.json, through
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

    /// Whether `pnpm init` records a package-manager pin in the manifest it
    /// scaffolds at `init_dir`.
    ///
    /// A manifest created inside an existing workspace becomes a member of
    /// it and follows the pin at the workspace root, so only the root is
    /// pinned.
    pub(crate) fn pins_pnpm(&self, config: &Config, init_dir: &Path) -> bool {
        self.effective_init_package_manager(config)
            && config.workspace_dir.as_deref().is_none_or(|root| root == init_dir)
    }
}

/// The pnpm version the new project is pinned to: whatever the registry's
/// `latest` tag points at, so a project scaffolded by a long-outdated pnpm
/// does not inherit that staleness through its own pin.
///
/// Falls back to the running version whenever `latest` cannot be
/// established, and never lets `latest` move the pin backwards — the tag can
/// lag the running version when a new major has shipped without being
/// tagged. Scaffolding a `package.json` must not fail, hang, or wait on a
/// registry that is unreachable, so every failure degrades to the running
/// version instead of surfacing.
pub(crate) async fn version_to_pin(config: &Config) -> String {
    if config.offline || config.prefer_offline {
        return PNPM_VERSION.to_string();
    }
    // One attempt, no retry schedule: a registry that is not answering
    // should cost `pnpm init` a moment, not the whole timeout below.
    let mut config = config.clone();
    config.fetch_retries = 0;
    let lookup = Box::pin(config_deps::resolve_engine_version(&config, "pnpm", "latest"));
    let Ok(Ok(Some(resolved))) = tokio::time::timeout(LATEST_LOOKUP_TIMEOUT, lookup).await else {
        return PNPM_VERSION.to_string();
    };
    // A `latest` the maturity or trust policy rejects is not something to pin
    // a new project to, and `pnpm init` has nobody to prompt for approval. A
    // broken release is refused for the reason the pin exists at all: it is
    // shared, so pinning one the running wrapper happens to survive would
    // still break every teammate on the other wrapper.
    if resolved.policy_violation.is_some()
        || !is_release_installable(&resolved.version)
        || version_lt(&resolved.version, PNPM_VERSION)
    {
        return PNPM_VERSION.to_string();
    }
    resolved.version
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
