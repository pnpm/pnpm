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

/// Where release changelogs live, defaulting to `registry`: no CHANGELOG.md is
/// committed; each release's section is composed at publish time and packed
/// into the published tarball, on top of the previously published version's
/// changelog. `repository` keeps a committed CHANGELOG.md in every package.
/// Mirrors the TypeScript `changelogStorage`.
#[must_use]
pub fn changelog_storage(versioning: Option<&VersioningSettings>) -> ChangelogStorage {
    versioning
        .and_then(|settings| settings.changelog.as_ref())
        .and_then(|changelog| changelog.storage)
        .unwrap_or(ChangelogStorage::Registry)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangelogSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<ChangelogStorage>,
}

/// An epic ties a group of member packages to a lead package, constraining
/// every member's major version to the band derived from the lead's major:
/// while the lead is on major `M`, members live in `M×100 … M×100+99`. Members
/// move independently inside the band; when a release plan takes the lead to a
/// new stable major, every member re-bases to the band floor in the same plan.
/// Mirrors the TypeScript `VersioningEpic` type.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicSettings {
    /// The package whose major version defines the band, referenced by name or
    /// by `./`-prefixed workspace directory (e.g. `pnpm`).
    pub lead: String,
    /// Selectors matching the member packages: name globs, `./`-prefixed
    /// directory globs, and `!`-prefixed negations.
    pub packages: Vec<String>,
}

/// Settings for native workspace release management, declared under the
/// `versioning` key of pnpm-workspace.yaml. Mirrors the TypeScript type of
/// the same name field for field.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VersioningSettings {
    /// Groups of packages that always release together at one shared version.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixed: Vec<Vec<String>>,
    /// Epics that band member packages' majors to a lead package's major.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub epics: Vec<EpicSettings>,
    /// Packages permanently excluded from versioning and dependent
    /// propagation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Caps the bump a release from the current checkout may apply. Enforced
    /// on the final assembled release plan, after dependent propagation and
    /// fixed-group resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bump: Option<ReleaseBumpType>,
    /// Per-package release lanes: maps a package name to the lane it is on
    /// (e.g. `"@example/cli": "alpha"`). A lane is a parallel release track
    /// that emits `X.Y.Z-tag.N` prereleases; every unlisted package is on the
    /// reserved default lane, `main`, and releases stable versions.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub lanes: IndexMap<String, String>,
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
