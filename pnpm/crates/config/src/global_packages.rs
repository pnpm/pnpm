//! `globalPackagesConfig`: the settings of the global packages, kept in the global
//! `config.yaml` and applied only by the commands that act on them.

use crate::{
    CommandTarget, Config, ExplicitPaths, WorkspaceSettings, collect_explicit_settings,
    note_declared_registries,
};

/// The settings `globalPackagesConfig` names that a global group install does not
/// apply. The group config drops them so that a caller's project cannot
/// reach the global install through them.
pub const GLOBAL_PACKAGES_UNSUPPORTED_KEYS: &[&str] =
    &["catalog", "catalogs", "overrides", "packageExtensions", "patchedDependencies"];

/// The settings that decide which lifecycle scripts run.
const BUILD_POLICY_KEYS: &[&str] =
    &["allowBuilds", "dangerouslyAllowAllBuilds", "permissions", "skills"];

/// The build policy the layers below the workspace layer set, restored over
/// the workspace layer's own by [`Config::apply_global_packages_layer`].
pub(super) struct BuildPolicy {
    allow_builds: std::collections::HashMap<String, bool>,
    dangerously_allow_all_builds: bool,
    explicit_settings: Vec<(String, serde_json::Value)>,
}

impl Config {
    /// Apply [`Config::global_packages_config`] when the command acts on the global
    /// packages, in the slot of the workspace layer. Relative paths resolve
    /// against [`Config::dir`], as the rest of the global `config.yaml` does.
    /// `below_workspace` is [`Config::global_packages_build_policy`], taken
    /// before the workspace layer applied.
    pub(super) fn apply_global_packages_layer(
        &mut self,
        below_workspace: Option<BuildPolicy>,
        explicit: &mut ExplicitPaths,
        declared_registries: &mut crate::npmrc_auth::DeclaredRegistries,
    ) {
        let Some(below_workspace) = below_workspace else {
            return;
        };
        self.restore_build_policy(below_workspace);
        let Some(settings) = self.global_packages_config.as_deref().cloned() else {
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

    /// The build policy so far, for a command that acts on the global packages.
    pub(super) fn global_packages_build_policy(&self) -> Option<BuildPolicy> {
        (self.cli.command_target == CommandTarget::GlobalPackages).then(|| BuildPolicy {
            allow_builds: self.allow_builds.clone(),
            dangerously_allow_all_builds: self.dangerously_allow_all_builds,
            explicit_settings: BUILD_POLICY_KEYS
                .iter()
                .filter_map(|key| {
                    self.explicit_settings
                        .get(*key)
                        .map(|value| ((*key).to_string(), value.clone()))
                })
                .collect(),
        })
    }

    /// Replace the workspace layer's build policy with `policy`, the one of
    /// the layers below it. The global install fills in the decisions of the
    /// global packages directory's `pnpm-workspace.yaml` beneath every layer.
    fn restore_build_policy(&mut self, policy: BuildPolicy) {
        self.allow_builds = policy.allow_builds;
        self.dangerously_allow_all_builds = policy.dangerously_allow_all_builds;
        for key in BUILD_POLICY_KEYS {
            self.explicit_settings.remove(*key);
        }
        self.explicit_settings.extend(policy.explicit_settings);
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
                "These globalPackagesConfig settings are not supported and are ignored: {}",
                ignored.join(", "),
            ));
        }
    }
}
