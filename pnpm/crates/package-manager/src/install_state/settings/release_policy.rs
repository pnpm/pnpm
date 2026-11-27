use super::SettingsComparison;
use pnpm_config::version_policy::{
    drop_unresolved_package_version_specs,
    prune_project_version_specs,
};

impl SettingsComparison<'_> {
    fn excludes_match(
        &self,
        recorded: Option<&[String]>,
        live: Option<&[String]>,
        prune: bool,
    ) -> bool {
        if recorded == live {
            return true;
        }
        let Some(recorded) = recorded else { return false };
        let config = self.config;
        if !prune || !config.lockfile || recorded.is_empty() {
            return false;
        }
        self.prune_versions
            .as_ref()
            .is_some_and(|versions| {
                let pruned = if config.shares_one_lockfile() {
                    drop_unresolved_package_version_specs(recorded, versions)
                } else {
                    prune_project_version_specs(recorded, live.unwrap_or_default(), versions)
                };
                (!pruned.is_empty()).then_some(pruned.as_slice()) == live
            })
    }
    pub(super) fn minimum_release_age_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        return_drift_if!(
            self,
            "minimumReleaseAge",
            recorded.minimum_release_age != live.minimum_release_age,
        );
        let excludes_changed = !self.excludes_match(
            recorded.minimum_release_age_exclude.as_deref(),
            live.minimum_release_age_exclude.as_deref(),
            self.config.minimum_release_age_exclude_prune,
        );
        return_drift_if!(self, "minimumReleaseAgeExclude", excludes_changed);
        return_drift_if!(
            self,
            "minimumReleaseAgeIgnoreMissingTime",
            recorded.minimum_release_age_ignore_missing_time
                != live.minimum_release_age_ignore_missing_time,
        );
        return_drift_if!(
            self,
            "minimumReleaseAgeStrict",
            recorded.minimum_release_age_strict != live.minimum_release_age_strict,
        );
        None
    }
    pub(super) fn trust_policy_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        return_drift_if!(self, "trustPolicy", recorded.trust_policy != live.trust_policy);
        let excludes_changed = !self.excludes_match(
            recorded.trust_policy_exclude.as_deref(),
            live.trust_policy_exclude.as_deref(),
            self.config.trust_policy_exclude_prune,
        );
        return_drift_if!(self, "trustPolicyExclude", excludes_changed);
        return_drift_if!(
            self,
            "trustPolicyIgnoreAfter",
            recorded.trust_policy_ignore_after != live.trust_policy_ignore_after,
        );
        None
    }
}
