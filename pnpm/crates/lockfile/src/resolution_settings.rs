use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The settings `lockfile.includeResolutionSettings` records in the
/// lockfile's `settings`. Each one changes what a resolution writes, so a
/// lockfile resolved under other values is outdated. Every field is unset
/// unless that setting is on.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_dedupe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_injected_deps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_peer_dependents: Option<bool>,
    /// `true`, `false` or `"deep"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_workspace_packages: Option<Value>,
}

/// A field of [`ResolutionSettings`], displayed as its setting name.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSetting {
    #[display("autoDedupe")]
    AutoDedupe,
    #[display("dedupeInjectedDeps")]
    DedupeInjectedDeps,
    #[display("dedupePeerDependents")]
    DedupePeerDependents,
    #[display("linkWorkspacePackages")]
    LinkWorkspacePackages,
}

impl ResolutionSetting {
    /// The setting's key in the lockfile.
    #[must_use]
    pub fn lockfile_key(self) -> &'static str {
        match self {
            ResolutionSetting::AutoDedupe => "settings.autoDedupe",
            ResolutionSetting::DedupeInjectedDeps => "settings.dedupeInjectedDeps",
            ResolutionSetting::DedupePeerDependents => "settings.dedupePeerDependents",
            ResolutionSetting::LinkWorkspacePackages => "settings.linkWorkspacePackages",
        }
    }
}

/// A setting whose recorded value differs from the expected one. An unset
/// value is `None`.
#[derive(Debug, PartialEq)]
pub struct ResolutionSettingDifference {
    pub setting: ResolutionSetting,
    pub recorded: Option<Value>,
    pub expected: Option<Value>,
}

impl ResolutionSettings {
    /// The first setting whose value in `self` differs from `expected`.
    #[must_use]
    pub fn first_difference(&self, expected: &Self) -> Option<ResolutionSettingDifference> {
        self.values()
            .into_iter()
            .zip(expected.values())
            .find(|((_, recorded), (_, expected))| recorded != expected)
            .map(|((setting, recorded), (_, expected))| ResolutionSettingDifference {
                setting,
                recorded,
                expected,
            })
    }

    fn values(&self) -> [(ResolutionSetting, Option<Value>); 4] {
        [
            (ResolutionSetting::AutoDedupe, self.auto_dedupe.map(Value::Bool)),
            (ResolutionSetting::DedupeInjectedDeps, self.dedupe_injected_deps.map(Value::Bool)),
            (ResolutionSetting::DedupePeerDependents, self.dedupe_peer_dependents.map(Value::Bool)),
            (ResolutionSetting::LinkWorkspacePackages, self.link_workspace_packages.clone()),
        ]
    }
}

#[cfg(test)]
mod tests;
