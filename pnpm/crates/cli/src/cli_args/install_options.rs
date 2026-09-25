use super::install::resolve_bool_override;
use pnpm_config::Config;

#[derive(Debug, Default, Clone, clap::Args)]
pub struct ScriptExecutionArgs {
    /// Don't run lifecycle scripts of the project or its dependencies.
    /// Packages are still installed; only their build scripts are skipped,
    /// and the install won't fail because of it.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    #[clap(id = "ignore_scripts")]
    pub ignore: bool,
    /// Run lifecycle scripts even when the configuration disables them.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    #[clap(id = "no_ignore_scripts")]
    pub no_ignore: bool,
    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct OfflineArgs {
    /// Fail on a cache miss instead of fetching from the registry, using
    /// only packages already in the store.
    #[clap(long, overrides_with = "no_offline")]
    pub offline: bool,
    /// Allow network fetches even when the configuration enables offline
    /// mode.
    #[clap(long = "no-offline", overrides_with = "offline")]
    pub no_offline: bool,
    /// Prefer packages already in the cache over the network, even past
    /// their freshness window.
    #[clap(long, overrides_with = "no_prefer_offline")]
    pub prefer_offline: bool,
    /// Don't prefer cached packages even when the configuration enables
    /// it.
    #[clap(long = "no-prefer-offline", overrides_with = "prefer_offline")]
    pub no_prefer_offline: bool,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct AutoDedupeArgs {
    /// Deduplicate compatible dependency versions during installation.
    #[clap(long = "auto-dedupe", overrides_with = "no_auto_dedupe")]
    pub auto_dedupe: bool,
    /// Disable automatic deduplication configured in pnpm-workspace.yaml.
    #[clap(long = "no-auto-dedupe", overrides_with = "auto_dedupe")]
    pub no_auto_dedupe: bool,
}

impl AutoDedupeArgs {
    pub(crate) fn apply(&self, config: &mut Config) {
        config.auto_dedupe =
            resolve_bool_override(self.auto_dedupe, self.no_auto_dedupe, config.auto_dedupe);
    }
}

impl ScriptExecutionArgs {
    pub(crate) fn apply(&self, config: &mut Config) {
        config.ignore_scripts =
            resolve_bool_override(self.ignore, self.no_ignore, config.ignore_scripts);
        config.ignore_pnpmfile |= self.ignore_pnpmfile;
    }
}

impl OfflineArgs {
    pub(crate) fn apply(&self, config: &mut Config) {
        config.offline = resolve_bool_override(self.offline, self.no_offline, config.offline);
        config.prefer_offline = resolve_bool_override(
            self.prefer_offline,
            self.no_prefer_offline,
            config.prefer_offline,
        );
    }
}
