//! Which config a command loads.

use super::configuration::{ConfigTarget, RunAnchors, StoreUse};
use crate::cli_args::config_warnings::ConfigWarnings;
use pnpm_config::CommandTarget;

/// Which config a command loads: the one anchored at `--dir`, or the one
/// anchored at the pnpm home, with or without the `globalPackages` settings.
#[derive(Clone, Copy)]
pub(super) enum ConfigScope {
    Dir,
    PnpmHome,
    GlobalPackages,
}

impl ConfigScope {
    pub(super) fn target(
        self,
        anchors: &RunAnchors,
        store_use: StoreUse,
        warnings: ConfigWarnings,
    ) -> ConfigTarget<'_> {
        let is_global = !matches!(self, Self::Dir);
        let command_target = match self {
            Self::GlobalPackages => CommandTarget::GlobalPackages,
            Self::Dir | Self::PnpmHome => CommandTarget::Project,
        };
        let anchor = if is_global { &anchors.global_config } else { &anchors.dir };
        ConfigTarget { anchor, is_global, command_target, store_use, warnings }
    }
}
