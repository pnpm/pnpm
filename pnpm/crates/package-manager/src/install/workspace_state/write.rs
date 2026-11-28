//! The one workspace state writer every install and every passing check
//! goes through.

use super::{
    Catalogs,
    Config,
    PackageManifest,
    Path,
    PathBuf,
    WorkspaceState,
    build_workspace_state,
    recorded_auto_dedupe,
};
use pnpm_modules_yaml::Host;
use pnpm_workspace_state::WorkspaceStateSettings;

/// Everything a workspace state write records besides what the run's
/// configuration and manifests already say.
pub(crate) struct StateWrite<'a> {
    pub(crate) workspace_root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) layout: crate::RepeatInstallLayout<'a>,
    pub(crate) catalogs: &'a Catalogs,
    pub(crate) projects: StateProjects<'a>,
    /// The filesystem time, probed before the writer read what it validated,
    /// per [`build_workspace_state`].
    pub(crate) filesystem_now_ms: Option<i64>,
    pub(crate) carried: CarriedSettings<'a>,
    pub(crate) vouched: &'a crate::install_state::validation::Vouched<'a>,
}

/// The projects a write records.
#[derive(Clone, Copy)]
pub(crate) struct StateProjects<'a> {
    pub(crate) manifests: crate::DeclaredManifests<'a>,
    /// See [`crate::RepeatInstallManifests::workspace_projects`].
    pub(crate) workspace_projects: Option<&'a [(PathBuf, &'a PackageManifest)]>,
    /// See [`RecordValidation::lockfile`](crate::install_state::validation::RecordValidation::lockfile).
    pub(crate) lockfile: Option<&'a pnpm_lockfile::Lockfile>,
}

/// The settings a write takes from what earlier runs established rather than
/// from this run.
pub(crate) struct CarriedSettings<'a> {
    /// Effective `trustLockfile` of the run (`config.trust_lockfile` plus any
    /// CLI `--trust-lockfile` / `--no-trust-lockfile` override).
    pub(crate) trust_lockfile: bool,
    /// The dedupe baseline this write knows of, per [`recorded_auto_dedupe`].
    pub(crate) established_auto_dedupe: Option<bool>,
    /// Whether the lockfile this run leaves behind has the keys the declared
    /// registry prefixes call for (see [`super::registry_keys_current`]). Only
    /// then does the state record the prefixes (see
    /// [`WorkspaceStateSettings::registry_prefixes`](pnpm_workspace_state::WorkspaceStateSettings::registry_prefixes)).
    pub(crate) registry_keys_verified: bool,
    /// The `registryRoutes` to record, per
    /// [`recorded_registry_routes_after_install`](crate::install_state::recorded_registry_routes_after_install)
    /// for an install and the recorded ones for a passing check.
    pub(crate) registry_routes: Option<std::collections::BTreeMap<String, String>>,
    pub(crate) selected_registry_routes_resolved: bool,
    /// The install settings to carry forward when a run check ignores their drift.
    pub(crate) kept_settings: Option<&'a WorkspaceStateSettings>,
}

impl CarriedSettings<'_> {
    pub(crate) fn after_install(
        config: &Config,
        lockfile: Option<&pnpm_lockfile::Lockfile>,
        previous_routes: Option<&std::collections::BTreeMap<String, String>>,
        resolution: crate::install_state::ResolvedImporters,
        trust_lockfile: bool,
        established_auto_dedupe: Option<bool>,
    ) -> Self {
        Self {
            trust_lockfile,
            established_auto_dedupe,
            registry_keys_verified: super::registry_keys_current(config, lockfile),
            registry_routes: Some(crate::install_state::recorded_registry_routes_after_install(
                config,
                previous_routes,
                lockfile,
                resolution,
            )),
            selected_registry_routes_resolved: resolution
                == crate::install_state::ResolvedImporters::Selected,
            kept_settings: None,
        }
    }
}

/// The workspace state `write` records. Every writer goes through here, so a
/// field one of them carries over cannot be left out by another.
pub(crate) fn recorded_workspace_state(write: &StateWrite<'_>) -> WorkspaceState {
    let mut state = build_workspace_state::<Host>(
        write.workspace_root,
        write.config,
        write.layout.node_linker,
        write.effective_settings(),
        write.projects.manifests,
        write.filesystem_now_ms,
    );
    crate::install_state::validation::record_validation(
        &mut state,
        &crate::install_state::validation::RecordValidation {
            workspace_root: write.workspace_root,
            config: write.config,
            project_manifests: write.projects.manifests,
            workspace_projects: write.projects.workspace_projects,
            vouched: write.vouched,
            lockfile: write.projects.lockfile,
        },
    );
    state
}

impl StateWrite<'_> {
    fn effective_settings(&self) -> WorkspaceStateSettings {
        let mut settings = crate::install_state::current_settings_with_catalogs(
            self.config,
            self.layout.node_linker,
            self.layout.included,
            self.layout.supported_architectures,
            self.catalogs,
        );
        let carried = &self.carried;
        settings.trust_lockfile = carried.trust_lockfile.then_some(true);
        settings.auto_dedupe = recorded_auto_dedupe(self.config, carried.established_auto_dedupe);
        if !carried.registry_keys_verified {
            settings.registry_prefixes = None;
            settings.default_registry = None;
        }
        if settings.registry_routes != carried.registry_routes {
            settings.selected_registry_routes_resolved = carried.selected_registry_routes_resolved;
            settings.installed_registry_routes = settings.registry_routes.take();
            settings.registry_routes.clone_from(&carried.registry_routes);
        }
        if let Some(kept) = carried.kept_settings {
            carry_run_ignored_settings(&mut settings, kept);
        }
        settings
    }
}

fn carry_run_ignored_settings(
    settings: &mut WorkspaceStateSettings,
    kept: &WorkspaceStateSettings,
) {
    settings.installed_registry_routes.clone_from(&kept.installed_registry_routes);
    settings.dev = kept.dev;
    settings.optional = kept.optional;
    settings.production = kept.production;
    settings.runtime = kept.runtime;
    settings.runtime_on_fail = kept.runtime_on_fail;
    settings.store_dir.clone_from(&kept.store_dir);
    settings.trust_lockfile = kept.trust_lockfile;
    settings.virtual_store_dir.clone_from(&kept.virtual_store_dir);
}
