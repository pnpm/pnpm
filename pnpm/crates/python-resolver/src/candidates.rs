use crate::{
    lockfile::{LockedWheel, Target},
    packages::Candidate,
};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::{Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement};
use serde::Deserialize;
use std::collections::BTreeMap;
use url::Url;

#[derive(Deserialize)]
struct SimplePage {
    files: Vec<IndexFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexFile {
    filename: String,
    url: String,
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    yanked: serde_json::Value,
    requires_python: Option<String>,
    /// PEP 714: `core-metadata` is `false`, `true`, or the digests of the
    /// metadata file. PEP 658 spelled the same thing `dist-info-metadata`,
    /// which indexes still serve for older clients.
    #[serde(default)]
    core_metadata: serde_json::Value,
    #[serde(default)]
    dist_info_metadata: serde_json::Value,
}

impl IndexFile {
    fn metadata_digests(&self) -> Option<BTreeMap<String, String>> {
        [&self.core_metadata, &self.dist_info_metadata].into_iter().find_map(|value| match value {
            serde_json::Value::Bool(true) => Some(BTreeMap::new()),
            serde_json::Value::Object(digests) => Some(
                digests
                    .iter()
                    .filter_map(|(name, digest)| Some((name.clone(), digest.as_str()?.to_string())))
                    .collect(),
            ),
            _ => None,
        })
    }
}

/// The versions of `name` a target can install, read from one PEP 691
/// Simple API page.
///
/// A version is offered by whichever of its wheels the target prefers: the
/// tags are ranked, and the first one that fits wins. Versions with no
/// wheel for this target, wheels the interpreter's version is outside
/// `requires_python` for, and yanked files are all left out, so the
/// resolution never picks something the environment cannot install.
///
/// `page_url` is the URL the page was read from, which relative file URLs
/// resolve against.
pub fn candidates_from_page(
    page: &str,
    page_url: &Url,
    name: &PackageName,
    target: &Target,
) -> Result<BTreeMap<Version, Candidate>> {
    let page: SimplePage = serde_json::from_str(page)
        .into_diagnostic()
        .map_err(|err| err.wrap_err("Python index must support the Simple JSON API"))?;
    let mut candidates = BTreeMap::<Version, (usize, Candidate)>::new();
    for file in page.files {
        if !matches!(file.yanked, serde_json::Value::Null | serde_json::Value::Bool(false)) {
            continue;
        }
        let Some((wheel_name, version, rank)) = wheel_identity(&file.filename, &target.tags)?
        else {
            continue;
        };
        if wheel_name != *name {
            bail!("Python index for {name} contains a wheel for {wheel_name}");
        }
        if let Some(requirement) = &file.requires_python {
            let specifiers: VersionSpecifiers = requirement.parse().into_diagnostic()?;
            if !specifiers.contains(target.environment.python_full_version()) {
                continue;
            }
        }
        let url = page_url.join(&file.url).into_diagnostic()?;
        validate_url(&url)?;
        let core_metadata = file.metadata_digests();
        let wheel = LockedWheel { name: file.filename, url: url.to_string(), hashes: file.hashes };
        wheel.integrity()?;
        let candidate = Candidate { wheel, core_metadata };
        if candidates.get(&version).is_none_or(|(previous, existing)| {
            (rank, &candidate.wheel.name) < (*previous, &existing.wheel.name)
        }) {
            candidates.insert(version, (rank, candidate));
        }
    }
    Ok(candidates.into_iter().map(|(version, (_, candidate))| (version, candidate)).collect())
}

/// The distribution, version, and tag rank a wheel filename names, or
/// `None` when the file is not a wheel this target can install. A lower
/// rank is a tag the target prefers.
pub fn wheel_identity(
    filename: &str,
    tags: &[String],
) -> Result<Option<(PackageName, Version, usize)>> {
    let Some(stem) = filename.strip_suffix(".whl") else { return Ok(None) };
    let parts = stem.split('-').collect::<Vec<_>>();
    if !(parts.len() == 5 || parts.len() == 6) || filename.contains(['/', '\\']) {
        bail!("invalid Python wheel filename: {filename}");
    }
    let wheel_tags = &parts[parts.len() - 3..];
    let rank = tags.iter().position(|tag| {
        let actual = tag.split('-').collect::<Vec<_>>();
        actual.len() == 3
            && wheel_tags.iter().zip(actual).all(|(supported, actual)| {
                supported.split('.').any(|supported| supported == actual)
            })
    });
    rank.map(|rank| {
        Ok((parts[0].parse().into_diagnostic()?, parts[1].parse().into_diagnostic()?, rank))
    })
    .transpose()
}

/// Refuse a URL a Python artifact must not be fetched from: a scheme
/// other than HTTP(S), or credentials in the URL itself.
pub fn validate_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Python artifacts require HTTP(S) URLs without embedded credentials");
    }
    Ok(())
}

/// Parse a PEP 508 requirement, refusing the direct-URL form: pnpm
/// installs what an index serves, and a URL requirement names something
/// else.
pub fn parse_requirement(requirement: &str) -> Result<Requirement> {
    let parsed: Requirement = requirement.parse().into_diagnostic()?;
    if matches!(parsed.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_))) {
        bail!("direct URL Python requirements are not supported: {requirement}");
    }
    Ok(parsed)
}
