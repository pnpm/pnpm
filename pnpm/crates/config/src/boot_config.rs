//! The policy pnpm resolves and verifies config dependencies and the package
//! manager under, before the configuration they contribute is loaded.

use crate::{Config, TrustPolicy, WorkspaceSettings};
use pnpm_workspace::{IncludeSource, SettingOwner, SettingSlot};
use serde::{Deserialize, Serialize};

/// The `bootConfig` block of `pnpm-workspace.yaml`, and the values the boot
/// phase falls back to.
///
/// Each field is a setting of the same name. Unset fields fall back to the
/// setting, unless a config dependency owns it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootConfig {
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub minimum_release_age_strict: Option<bool>,
    pub minimum_release_age_ignore_missing_time: Option<bool>,
    pub trust_policy: Option<TrustPolicy>,
    pub trust_policy_exclude: Option<Vec<String>>,
    pub trust_policy_ignore_after: Option<u64>,
}

impl BootConfig {
    /// The values `config` holds for the boot settings.
    pub(crate) fn capture(config: &Config) -> Self {
        BootConfig {
            minimum_release_age: config.minimum_release_age,
            minimum_release_age_exclude: config.minimum_release_age_exclude.clone(),
            minimum_release_age_strict: config.minimum_release_age_strict,
            minimum_release_age_ignore_missing_time: Some(
                config.minimum_release_age_ignore_missing_time,
            ),
            trust_policy: Some(config.trust_policy),
            trust_policy_exclude: config.trust_policy_exclude.clone(),
            trust_policy_ignore_after: config.trust_policy_ignore_after,
        }
    }

    /// Overlay the boot settings a layer of configuration sets.
    pub(crate) fn overlay(&mut self, settings: &WorkspaceSettings) {
        overlay(&mut self.minimum_release_age, settings.minimum_release_age);
        overlay(
            &mut self.minimum_release_age_exclude,
            settings.minimum_release_age_exclude.clone(),
        );
        overlay(&mut self.minimum_release_age_strict, settings.minimum_release_age_strict);
        overlay(
            &mut self.minimum_release_age_ignore_missing_time,
            settings.minimum_release_age_ignore_missing_time,
        );
        overlay(&mut self.trust_policy, settings.trust_policy);
        overlay(&mut self.trust_policy_exclude, settings.trust_policy_exclude.clone());
        overlay(&mut self.trust_policy_ignore_after, settings.trust_policy_ignore_after);
    }

    /// Apply the fields this block sets to `config`.
    fn apply_to(self, config: &mut Config) {
        overlay(&mut config.minimum_release_age, self.minimum_release_age);
        overlay(&mut config.minimum_release_age_exclude, self.minimum_release_age_exclude);
        overlay(&mut config.minimum_release_age_strict, self.minimum_release_age_strict);
        if let Some(ignore) = self.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = ignore;
        }
        if let Some(trust_policy) = self.trust_policy {
            config.trust_policy = trust_policy;
        }
        overlay(&mut config.trust_policy_exclude, self.trust_policy_exclude);
        overlay(&mut config.trust_policy_ignore_after, self.trust_policy_ignore_after);
    }

    /// Give each setting `owned` accepts by name the value it has here,
    /// unset ones included.
    fn assign_owned(self, config: &mut Config, owned: impl Fn(&str) -> bool) {
        if owned("minimumReleaseAge") {
            config.minimum_release_age = self.minimum_release_age;
        }
        if owned("minimumReleaseAgeExclude") {
            config.minimum_release_age_exclude = self.minimum_release_age_exclude;
        }
        if owned("minimumReleaseAgeStrict") {
            config.minimum_release_age_strict = self.minimum_release_age_strict;
        }
        if owned("minimumReleaseAgeIgnoreMissingTime") {
            config.minimum_release_age_ignore_missing_time =
                self.minimum_release_age_ignore_missing_time.unwrap_or_default();
        }
        if owned("trustPolicy") {
            config.trust_policy = self.trust_policy.unwrap_or_default();
        }
        if owned("trustPolicyExclude") {
            config.trust_policy_exclude = self.trust_policy_exclude;
        }
        if owned("trustPolicyIgnoreAfter") {
            config.trust_policy_ignore_after = self.trust_policy_ignore_after;
        }
    }
}

fn overlay<Value>(target: &mut Option<Value>, value: Option<Value>) {
    if value.is_some() {
        *target = value;
    }
}

impl Config {
    /// This configuration as the boot phase sees it: resolving and verifying
    /// config dependencies and the package manager.
    ///
    /// A value the environment or the command line gives wins, then a
    /// `bootConfig` value. A setting a config dependency owns falls back to
    /// the configuration below the workspace manifest, so a config
    /// dependency's policy never governs installing a config dependency.
    /// Every other setting keeps its value.
    ///
    /// Boxed, since the boot phase holds it across await points.
    #[must_use]
    pub fn for_boot(&self) -> Box<Config> {
        let mut config = Box::new(self.clone());
        self.boot_inherited
            .as_ref()
            .clone()
            .assign_owned(&mut config, |setting| self.is_owned_by_config_dependency(setting));
        if let Some(boot_config) = self.boot_config.as_deref().cloned() {
            boot_config.apply_to(&mut config);
        }
        self.boot_overrides
            .as_ref()
            .clone()
            .apply_to(&mut config);
        config
    }

    fn is_owned_by_config_dependency(&self, setting: &str) -> bool {
        self.workspace_includes
            .as_ref()
            .is_some_and(|includes| {
                matches!(
                    includes.owner(&SettingSlot::Setting(setting.to_string())),
                    SettingOwner::Include(IncludeSource::Package(_)),
                )
            })
    }
}
