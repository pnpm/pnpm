//! The one workspace state writer every install and every passing check
//! goes through.

use super::{
    Catalogs, Config, PackageManifest, Path, PathBuf, WorkspaceState, build_workspace_state,
    recorded_auto_dedupe,
};
use pnpm_modules_yaml::Host;

/// Everything a workspace state write records besides what the run's
/// configuration and manifests already say.
pub(crate) struct StateWrite<'a> {
    pub(crate) workspace_root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) layout: crate::RepeatInstallLayout<'a>,
    pub(crate) catalogs: &'a Catalogs,
    pub(crate) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    /// The filesystem time, probed before the writer read what it validated,
    /// per [`build_workspace_state`].
    pub(crate) filesystem_now_ms: Option<i64>,
    pub(crate) carried: CarriedSettings<'a>,
    pub(crate) vouched: &'a crate::optimistic_repeat_install::validation::Vouched<'a>,
}

/// The settings a write takes from what earlier runs established rather than
/// from this run.
pub(crate) struct CarriedSettings<'a> {
    /// The dedupe baseline this write knows of, per [`recorded_auto_dedupe`].
    pub(crate) established_auto_dedupe: Option<bool>,
    /// Whether the lockfile this run leaves behind has the keys the declared
    /// registry prefixes call for (see [`super::registry_keys_current`]). Only
    /// then does the state record the prefixes (see
    /// [`WorkspaceStateSettings::registry_prefixes`](pnpm_workspace_state::WorkspaceStateSettings::registry_prefixes)).
    pub(crate) registry_keys_verified: bool,
    /// The recorded dependency groups and store location to keep instead of
    /// the run's, for a writer that ignored their drift.
    pub(crate) kept_settings: Option<&'a pnpm_workspace_state::WorkspaceStateSettings>,
}

/// The workspace state `write` records. Every writer goes through here, so a
/// field one of them carries over cannot be left out by another.
pub(crate) fn recorded_workspace_state(write: &StateWrite<'_>) -> WorkspaceState {
    let mut state = build_workspace_state::<Host>(
        write.workspace_root,
        write.config,
        write.layout.node_linker,
        write.layout.included,
        write.layout.supported_architectures,
        write.catalogs,
        write.project_manifests,
        write.filesystem_now_ms,
    );
    state.settings.auto_dedupe =
        recorded_auto_dedupe(write.config, write.carried.established_auto_dedupe);
    if write.carried.registry_keys_verified {
        state.settings.registry_prefixes =
            crate::optimistic_repeat_install::recorded_registry_prefixes(write.config);
    }
    if let Some(kept) = write.carried.kept_settings {
        state.settings.dev = kept.dev;
        state.settings.optional = kept.optional;
        state.settings.production = kept.production;
        state.settings.store_dir.clone_from(&kept.store_dir);
        state.settings.virtual_store_dir.clone_from(&kept.virtual_store_dir);
    }
    crate::optimistic_repeat_install::validation::record_validation(
        &mut state,
        &crate::optimistic_repeat_install::validation::RecordValidation {
            workspace_root: write.workspace_root,
            config: write.config,
            project_manifests: write.project_manifests,
            vouched: write.vouched,
        },
    );
    state
}
