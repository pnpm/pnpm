//! Pacquet port of upstream's
//! [`resolving/aqua-resolver/src/registry.ts`](https://github.com/pnpm/pnpm/pull/10970)
//! (co-developed in the same PR).
//!
//! Fetches a package definition from the
//! [aqua-registry](https://github.com/aquaproj/aqua-registry) and picks
//! the version-specific override block that governs a resolved version.

use std::{collections::BTreeMap, sync::Arc};

use node_semver::{Range, Version};
use pacquet_network::ThrottledClient;
use serde::Deserialize;

use crate::error::AquaResolverError;

/// One `replacements:` table mapping a `goos`/`goarch` token to the
/// vendor's preferred spelling (e.g. `amd64` → `x86_64`).
pub type Replacements = BTreeMap<String, String>;

/// A registry package definition (one entry of the YAML `packages:` list).
///
/// Only the fields the resolver consumes are modeled; the aqua-registry
/// YAML carries many more (`description`, `link`, `aliases`, ...) that
/// serde ignores. Every field is optional because the registry only
/// emits the ones a given package needs.
#[derive(Debug, Default, Deserialize)]
pub struct AquaRegistryPackage {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<AquaFile>>,
    #[serde(default)]
    pub replacements: Option<Replacements>,
    #[serde(default)]
    pub supported_envs: Option<Vec<String>>,
    #[serde(default)]
    pub overrides: Option<Vec<AquaOverride>>,
    #[serde(default)]
    pub checksum: Option<AquaChecksum>,
    #[serde(default)]
    pub version_constraint: Option<String>,
    #[serde(default)]
    pub version_overrides: Option<Vec<AquaVersionOverride>>,
}

/// A `version_overrides:` block: the same shape as the base package,
/// scoped to versions matching its `version_constraint`.
#[derive(Debug, Default, Deserialize)]
pub struct AquaVersionOverride {
    #[serde(default)]
    pub version_constraint: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<AquaFile>>,
    #[serde(default)]
    pub replacements: Option<Replacements>,
    #[serde(default)]
    pub supported_envs: Option<Vec<String>>,
    #[serde(default)]
    pub overrides: Option<Vec<AquaOverride>>,
    #[serde(default)]
    pub checksum: Option<AquaChecksum>,
}

/// One named executable the archive exposes: `name` is the bin name,
/// `src` (optional) is the path inside the extracted archive.
#[derive(Debug, Clone, Deserialize)]
pub struct AquaFile {
    pub name: String,
    #[serde(default)]
    pub src: Option<String>,
}

/// A per-platform `overrides:` entry. Selected when its `goos`/`goarch`
/// constraints (when present) match the target platform.
#[derive(Debug, Default, Deserialize)]
pub struct AquaOverride {
    #[serde(default)]
    pub goos: Option<String>,
    #[serde(default)]
    pub goarch: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub files: Option<Vec<AquaFile>>,
    #[serde(default)]
    pub replacements: Option<Replacements>,
    #[serde(default)]
    pub checksum: Option<AquaChecksum>,
}

/// A `checksum:` block. In a per-platform override it may carry only
/// `enabled: false` to disable checksum verification for that platform,
/// so every field is optional.
#[derive(Debug, Clone, Deserialize)]
pub struct AquaChecksum {
    #[serde(rename = "type", default)]
    pub checksum_type: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AquaRegistryDocument {
    packages: Vec<AquaRegistryPackage>,
}

/// The effective configuration governing a resolved version: either the
/// base package or one of its `version_overrides`. Borrows from the
/// owning [`AquaRegistryPackage`] so [`crate::template::expand_assets`]
/// can read whichever block [`find_matching_override`] selected without
/// cloning.
pub struct ResolvedSpec<'a> {
    pub asset: Option<&'a str>,
    pub format: Option<&'a str>,
    pub files: Option<&'a [AquaFile]>,
    pub replacements: Option<&'a Replacements>,
    pub supported_envs: Option<&'a [String]>,
    pub overrides: Option<&'a [AquaOverride]>,
    pub checksum: Option<&'a AquaChecksum>,
}

impl<'a> From<&'a AquaRegistryPackage> for ResolvedSpec<'a> {
    fn from(pkg: &'a AquaRegistryPackage) -> Self {
        ResolvedSpec {
            asset: pkg.asset.as_deref(),
            format: pkg.format.as_deref(),
            files: pkg.files.as_deref(),
            replacements: pkg.replacements.as_ref(),
            supported_envs: pkg.supported_envs.as_deref(),
            overrides: pkg.overrides.as_deref(),
            checksum: pkg.checksum.as_ref(),
        }
    }
}

impl<'a> From<&'a AquaVersionOverride> for ResolvedSpec<'a> {
    fn from(over: &'a AquaVersionOverride) -> Self {
        ResolvedSpec {
            asset: over.asset.as_deref(),
            format: over.format.as_deref(),
            files: over.files.as_deref(),
            replacements: over.replacements.as_ref(),
            supported_envs: over.supported_envs.as_deref(),
            overrides: over.overrides.as_deref(),
            checksum: over.checksum.as_ref(),
        }
    }
}

const AQUA_REGISTRY_BASE: &str =
    "https://raw.githubusercontent.com/aquaproj/aqua-registry/main/pkgs";

/// Fetch and decode the first package definition for `owner/repo`.
pub async fn fetch_aqua_registry_package(
    http_client: &ThrottledClient,
    owner: &str,
    repo: &str,
) -> Result<AquaRegistryPackage, AquaResolverError> {
    let url = format!("{AQUA_REGISTRY_BASE}/{owner}/{repo}/registry.yaml");
    let response =
        http_client.acquire_for_url(&url).await.get(&url).send().await.map_err(|error| {
            AquaResolverError::Network { url: url.clone(), error: Arc::new(error) }
        })?;
    if !response.status().is_success() {
        return Err(AquaResolverError::RegistryFetch {
            owner: owner.to_string(),
            repo: repo.to_string(),
            status: response.status().as_u16(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|error| AquaResolverError::Network { url: url.clone(), error: Arc::new(error) })?;
    let mut doc: AquaRegistryDocument =
        serde_saphyr::from_str(&body).map_err(|error| AquaResolverError::RegistryParse {
            owner: owner.to_string(),
            repo: repo.to_string(),
            error: Arc::new(error),
        })?;
    if doc.packages.is_empty() {
        return Err(AquaResolverError::RegistryEmpty {
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }
    Ok(doc.packages.swap_remove(0))
}

/// Select the override governing `version`, mirroring upstream's
/// `findMatchingOverride`: the first matching `version_overrides` entry,
/// else the base package when its own constraint matches, else the last
/// override (conventionally the catch-all `true`), else the base.
pub fn find_matching_override<'a>(pkg: &'a AquaRegistryPackage, version: &str) -> ResolvedSpec<'a> {
    let clean_version = version.strip_prefix('v').unwrap_or(version);

    if let Some(version_overrides) = pkg.version_overrides.as_deref() {
        for over in version_overrides {
            if matches_version_constraint(
                over.version_constraint.as_deref(),
                clean_version,
                version,
            ) {
                return ResolvedSpec::from(over);
            }
        }
    }

    if matches_version_constraint(pkg.version_constraint.as_deref(), clean_version, version) {
        return ResolvedSpec::from(pkg);
    }

    if let Some(last) = pkg.version_overrides.as_deref().and_then(<[_]>::last) {
        return ResolvedSpec::from(last);
    }

    ResolvedSpec::from(pkg)
}

fn matches_version_constraint(
    constraint: Option<&str>,
    clean_version: &str,
    raw_version: &str,
) -> bool {
    let Some(constraint) = constraint else { return false };
    match constraint {
        "true" => return true,
        "false" => return false,
        _ => {}
    }

    if let Some(range) =
        constraint.strip_prefix(r#"semver(""#).and_then(|rest| rest.strip_suffix(r#"")"#))
    {
        return semver_satisfies(clean_version, range);
    }

    if let Some(expected) = parse_exact_version_constraint(constraint) {
        return raw_version == expected || clean_version == expected;
    }

    false
}

/// Parse upstream's `Version == "X.Y.Z"` constraint form, tolerating
/// the optional whitespace its regex (`^Version\s*==\s*"(.+)"$`) allows.
fn parse_exact_version_constraint(constraint: &str) -> Option<&str> {
    let rest = constraint.strip_prefix("Version")?.trim_start();
    let rest = rest.strip_prefix("==")?.trim_start();
    rest.strip_prefix('"')?.strip_suffix('"')
}

/// `semver.satisfies(version, range, { includePrerelease: true })`,
/// matching the prerelease-tolerant check the rest of pacquet uses.
fn semver_satisfies(version: &str, range: &str) -> bool {
    let (Ok(parsed), Ok(parsed_range)) = (Version::parse(version), Range::parse(range)) else {
        return false;
    };
    if parsed.satisfies(&parsed_range) {
        return true;
    }
    if parsed.pre_release.is_empty() {
        return false;
    }
    Version {
        major: parsed.major,
        minor: parsed.minor,
        patch: parsed.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    }
    .satisfies(&parsed_range)
}

#[cfg(test)]
mod tests;
