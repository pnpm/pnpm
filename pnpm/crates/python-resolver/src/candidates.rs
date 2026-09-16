use crate::{
    lockfile::{LockedWheel, Target},
    packages::{Candidate, IndexCandidate},
    requires_python::declared_range,
};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement};
use serde::Deserialize;
use std::{collections::BTreeMap, fmt};
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
        [&self.core_metadata, &self.dist_info_metadata]
            .into_iter()
            .find_map(|value| match value {
                serde_json::Value::Bool(true) => Some(BTreeMap::new()),
                serde_json::Value::Object(digests) => Some(
                    digests
                        .iter()
                        .filter_map(|(name, digest)| {
                            Some((name.clone(), digest.as_str()?.to_string()))
                        })
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
        let Some((version, rank, candidate)) = installable_candidate(file, page_url, name, target)
        else {
            continue;
        };
        if candidates
            .get(&version)
            .is_none_or(|(previous, existing)| {
                (rank, candidate.label()) < (*previous, existing.label())
            })
        {
            candidates.insert(version, (rank, candidate));
        }
    }
    Ok(candidates
        .into_iter()
        .map(|(version, (_, candidate))| (version, candidate))
        .collect())
}

/// The candidate one index file offers, with the version and tag rank it
/// competes under. `None` for a file this target cannot install: a yanked
/// release, a non-wheel, a wheel whose `Requires-Python` excludes the
/// target interpreter, or one of the [unusable](usable) files an index
/// serves.
fn installable_candidate(
    file: IndexFile,
    page_url: &Url,
    name: &PackageName,
    target: &Target,
) -> Option<(Version, usize, Candidate)> {
    if !matches!(file.yanked, serde_json::Value::Null | serde_json::Value::Bool(false)) {
        return None;
    }
    let (wheel_name, version, rank) =
        usable(&file.filename, wheel_identity(&file.filename, &target.tags))??;
    if wheel_name != *name {
        return unusable(&file.filename, format_args!("wheel for {wheel_name}, not for {name}"));
    }
    if let Some(specifiers) = file.requires_python.as_deref().and_then(declared_range)
        && !specifiers.contains(target.environment.python_full_version())
    {
        return None;
    }
    let url = usable(&file.filename, page_url.join(&file.url).into_diagnostic())?;
    usable(&file.filename, validate_url(&url))?;
    let core_metadata = file.metadata_digests();
    let wheel = LockedWheel { name: file.filename, url: url.to_string(), hashes: file.hashes };
    usable(&wheel.name, wheel.integrity())?;
    Some((version, rank, Candidate::Wheel(IndexCandidate { wheel, core_metadata })))
}

/// What reading one index file produced, or `None` when the file is one
/// pnpm cannot use.
///
/// A page lists every release a distribution ever published, and a project
/// needs one of them. A file pnpm cannot read is therefore left out rather
/// than failing the page, the way a wheel built for another interpreter
/// is: an immutable old release is no reason a project cannot install the
/// version it asks for.
fn usable<Item>(filename: &str, read: Result<Item>) -> Option<Item> {
    match read {
        Ok(value) => Some(value),
        Err(error) => unusable(filename, error),
    }
}

/// Leave one index file out, recording why it cannot be used.
fn unusable<Item>(filename: &str, reason: impl fmt::Display) -> Option<Item> {
    tracing::debug!("skipping Python file {filename}: {reason}");
    None
}

/// What a wheel filename names: the distribution and version it carries,
/// and the compressed tag sets an interpreter has to accept to install it.
#[derive(Debug)]
pub struct WheelFilename {
    pub name: PackageName,
    pub version: Version,
    tags: [String; 3],
}

impl WheelFilename {
    /// Read a wheel filename, or `None` when the file is not a wheel at
    /// all — an index page lists sdists and signatures beside wheels.
    pub fn parse(filename: &str) -> Result<Option<Self>> {
        let Some(stem) = filename.strip_suffix(".whl") else { return Ok(None) };
        let parts = stem.split('-').collect::<Vec<_>>();
        if !(parts.len() == 5 || parts.len() == 6) || filename.contains(['/', '\\']) {
            bail!("invalid Python wheel filename: {filename}");
        }
        let tags = &parts[parts.len() - 3..];
        Ok(Some(Self {
            name: parts[0].parse().into_diagnostic()?,
            version: parts[1].parse().into_diagnostic()?,
            tags: [tags[0].to_string(), tags[1].to_string(), tags[2].to_string()],
        }))
    }

    /// The preference a target gives this wheel, or `None` when it accepts
    /// none of the wheel's tags. A lower rank is a tag the target prefers.
    #[must_use]
    pub fn rank(&self, tags: &[String]) -> Option<usize> {
        tags.iter()
            .position(|tag| {
                let accepted = tag.split('-').collect::<Vec<_>>();
                accepted.len() == 3
                    && self.tags
                        .iter()
                        .zip(accepted)
                        .all(|(declared, accepted)| {
                            declared
                                .split('.')
                                .any(|declared| declared == accepted)
                        })
            })
    }
}

/// The distribution, version, and tag rank a wheel filename names, or
/// `None` when the file is not a wheel this target can install. A lower
/// rank is a tag the target prefers.
pub fn wheel_identity(
    filename: &str,
    tags: &[String],
) -> Result<Option<(PackageName, Version, usize)>> {
    let Some(wheel) = WheelFilename::parse(filename)? else { return Ok(None) };
    Ok(wheel
        .rank(tags)
        .map(|rank| (wheel.name, wheel.version, rank)))
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

/// Parse a PEP 508 requirement.
pub fn parse_requirement(requirement: &str) -> Result<Requirement> {
    read_requirement(requirement)
        .map_err(|refusal| match refusal {
            Refusal::Unreadable(error) => error,
            Refusal::Unsupported(requirement) => {
                miette::miette!(
                    "unsupported scheme in direct URL Python requirements: {requirement}"
                )
            }
        })
}

/// Why pnpm cannot use a requirement, which decides whom it is a problem
/// for: a line pnpm cannot read at all comes from one broken wheel, and a
/// URL with an unsupported scheme is a requirement pnpm does not implement, which every
/// release declaring it will name.
#[derive(Debug)]
pub(crate) enum Refusal {
    Unreadable(miette::Report),
    Unsupported(String),
}

/// Read a PEP 508 requirement, telling a line pnpm cannot parse apart
/// from a URL scheme it parses and does not support.
pub(crate) fn read_requirement(requirement: &str) -> std::result::Result<Requirement, Refusal> {
    let parsed: Requirement = requirement
        .parse()
        .into_diagnostic()
        .map_err(Refusal::Unreadable)?;
    if let Some(pep508_rs::VersionOrUrl::Url(url)) = &parsed.version_or_url {
        let scheme = url.scheme();
        if !matches!(scheme, "http" | "https" | "git+https" | "git+ssh" | "git+file") {
            return Err(Refusal::Unsupported(requirement.to_string()));
        }
    }
    Ok(parsed)
}
