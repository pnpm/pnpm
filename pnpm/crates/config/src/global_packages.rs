//! `globalPackages`: the settings of the global packages, kept in the global
//! `config.yaml` and applied only by the commands that act on them.

use crate::{
    Config, ExplicitPaths, WorkspaceSettings, collect_explicit_settings, note_declared_registries,
};

/// What a command acts on.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CommandTarget {
    /// The project in the directory the command runs in.
    #[default]
    Project,
    /// The global packages, which `-g` commands install, update, and list.
    GlobalPackages,
}

/// The settings `globalPackages` names that a global group install does not
/// apply. The group config drops them so that a caller's project cannot
/// reach the global install through them.
pub const GLOBAL_PACKAGES_UNSUPPORTED_KEYS: &[&str] =
    &["catalog", "catalogs", "overrides", "packageExtensions", "patchedDependencies"];

impl Config {
    /// Apply [`Config::global_packages`] when the command acts on the global
    /// packages, in the slot of the workspace layer. Relative paths resolve
    /// against [`Config::dir`], as the rest of the global `config.yaml` does.
    pub(super) fn apply_global_packages_layer(
        &mut self,
        explicit: &mut ExplicitPaths,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
    ) {
        if self.cli.command_target != CommandTarget::GlobalPackages {
            return;
        }
        let Some(settings) = self.global_packages.as_deref().cloned() else {
            return;
        };
        self.warn_about_unsupported_global_packages_keys(&settings);
        explicit.note(&settings);
        note_declared_registries(declared_registries, &settings);
        collect_explicit_settings(&mut self.explicit_settings, &settings);
        let saved_workspace_dir = self.workspace_dir.take();
        let base_dir = self.dir.clone();
        settings.apply_to(self, &base_dir);
        self.workspace_dir = saved_workspace_dir;
    }

    fn warn_about_unsupported_global_packages_keys(&mut self, settings: &WorkspaceSettings) {
        let Ok(serde_json::Value::Object(set)) = serde_json::to_value(settings) else {
            return;
        };
        let ignored: Vec<&str> = GLOBAL_PACKAGES_UNSUPPORTED_KEYS
            .iter()
            .copied()
            .filter(|key| {
                set.get(*key)
                    .is_some_and(|value| !value.is_null())
            })
            .collect();
        if !ignored.is_empty() {
            self.npmrc_warnings.push(format!(
                "These globalPackages settings are not supported and are ignored: {}",
                ignored.join(", "),
            ));
        }
    }
}
