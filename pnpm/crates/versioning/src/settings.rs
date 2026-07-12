use derive_more::Display;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A bump a release can apply. Ordered: `patch < minor < major`.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseBumpType {
    #[display("patch")]
    Patch,
    #[display("minor")]
    Minor,
    #[display("major")]
    Major,
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangelogStorage {
    #[display("registry")]
    Registry,
    #[display("repository")]
    Repository,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangelogSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<ChangelogStorage>,
}

/// Settings for native workspace release management, declared under the
/// `versioning` key of pnpm-workspace.yaml. Mirrors the TypeScript
/// `VersioningSettings` type field for field.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VersioningSettings {
    /// Groups of packages that always release together at one shared version.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixed: Vec<Vec<String>>,
    /// Packages permanently excluded from versioning and dependent
    /// propagation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Caps the bump a release from the current checkout may apply. Enforced
    /// on the final assembled release plan, after dependent propagation and
    /// fixed-group resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bump: Option<ReleaseBumpType>,
    /// Per-package prerelease lines: maps a package name to the prerelease
    /// tag of the line it is on (e.g. `"@example/cli": "alpha"`).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub prereleases: IndexMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelog: Option<ChangelogSettings>,
}

impl VersioningSettings {
    /// Whether nothing is configured — the state in which the `versioning`
    /// key is removed from pnpm-workspace.yaml instead of written empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &VersioningSettings::default()
    }
}
