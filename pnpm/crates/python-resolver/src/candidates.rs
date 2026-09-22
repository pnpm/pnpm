use crate::{
    lockfile::{
        LockedSdist,
        LockedWheel,
        Target,
    },
    packages::{
        Candidate,
        Excluded,
        IndexCandidate,
        Offered,
        Releases,
    },
    requires_python::declared_range,
};
use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use pep440_rs::Version;
use pep508_rs::{
    PackageName,
    Requirement,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fmt,
};
use url::Url;

/// The containers a source distribution is published in that pnpm
/// unpacks, as PEP 625 and its predecessors name them.
const SOURCE_SUFFIXES: [&str; 2] = [".tar.gz", ".zip"];

/// The preference a target gives a source distribution, which is after
/// every wheel it accepts: a wheel is downloaded, and a source
/// distribution has to be built.
const SOURCE_RANK: usize = usize::MAX;

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

/// The versions of `name` one PEP 691 Simple API page offers a target,
/// and the releases it leaves out.
///
/// A version is offered by whichever of its files the target prefers: the
/// wheel tags are ranked, the first one that fits wins, and a release with
/// no wheel for this target is offered as its source distribution. Wheels
/// the interpreter's version is outside `requires_python` for, and yanked
/// files, are left out, so the resolution never picks something the
/// environment cannot install.
///
/// `page_url` is the URL the page was read from, which relative file URLs
/// resolve against.
pub fn candidates_from_page(
    page: &str,
    page_url: &Url,
    name: &PackageName,
    target: &Target,
) -> Result<Offered> {
    let page: SimplePage = serde_json::from_str(page)
        .into_diagnostic()
        .map_err(|err| err.wrap_err("Python index must support the Simple JSON API"))?;
    let mut candidates = BTreeMap::<Version, (usize, Candidate)>::new();
    let mut excluded = BTreeMap::<Version, Exclusion>::new();
    for file in page.files {
        match read_file(file, page_url, name, target) {
            Read::Ignored => {}
            Read::Excluded(version, reason) => {
                excluded
                    .entry(version)
                    .and_modify(|kept| *kept = (*kept).min(reason))
                    .or_insert(reason);
            }
            Read::Candidate(version, rank, candidate) => {
                if candidates
                    .get(&version)
                    .is_none_or(|(previous, existing)| {
                        (rank, candidate.label()) < (*previous, existing.label())
                    })
                {
                    candidates.insert(version, (rank, candidate));
                }
            }
        }
    }
    // A release with one file this target installs is not left out,
    // whatever its other files are for.
    excluded.retain(|version, _| !candidates.contains_key(version));
    Ok(Offered {
        candidates: candidates
            .into_iter()
            .map(|(version, (_, candidate))| (version, candidate))
            .collect(),
        excluded: Excluded {
            published: true,
            other_interpreters: releases(&excluded, Exclusion::OtherInterpreter),
            other_targets: releases(&excluded, Exclusion::OtherTarget),
        },
    })
}

fn releases(excluded: &BTreeMap<Version, Exclusion>, reason: Exclusion) -> Releases {
    excluded
        .iter()
        .filter(|(_, kept)| **kept == reason)
        .map(|(version, _)| version.clone())
        .collect()
}

/// What one index file offers this target.
enum Read {
    /// The candidate it offers, and the rank it competes under with the
    /// other files of its release. A lower rank is one the target prefers.
    Candidate(Version, usize, Candidate),
    /// A release this target cannot install this file of, and why.
    Excluded(Version, Exclusion),
    /// A file resolution does not consider: a yanked release, a file that
    /// is neither a wheel nor a source distribution, or one of the
    /// [unusable](usable) files an index serves.
    Ignored,
}

/// Why a release is not a candidate. Ordered by how precisely it names
/// what a project can do about it, so a release left out for several
/// reasons is reported under the most precise one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Exclusion {
    /// Every file of the release declares an interpreter range this
    /// target's Python is outside of.
    OtherInterpreter,
    /// The release publishes no wheel this target installs and no source
    /// distribution pnpm can unpack.
    OtherTarget,
}

fn read_file(file: IndexFile, page_url: &Url, name: &PackageName, target: &Target) -> Read {
    if !matches!(file.yanked, serde_json::Value::Null | serde_json::Value::Bool(false)) {
        return Read::Ignored;
    }
    let Some(Some(published)) =
        usable(&file.filename, published_file(&file.filename, name, &target.tags))
    else {
        return Read::Ignored;
    };
    // Asked before the tags, so a file this target is outside the
    // interpreter range of is reported as that rather than as one more
    // wheel for another machine.
    if let Some(specifiers) = file.requires_python.as_deref().and_then(declared_range)
        && !specifiers.contains(target.environment.python_full_version())
    {
        return Read::Excluded(published.version, Exclusion::OtherInterpreter);
    }
    let Some(rank) = published.rank else {
        return Read::Excluded(published.version, Exclusion::OtherTarget);
    };
    let Some(url) = usable(&file.filename, page_url.join(&file.url).into_diagnostic()) else {
        return Read::Ignored;
    };
    if usable(&file.filename, validate_url(&url)).is_none() {
        return Read::Ignored;
    }
    // The digest check names the file it refuses, so the reason it is
    // left out carries the filename without being told it.
    match index_candidate(file, &url, published.source) {
        Ok(candidate) => Read::Candidate(published.version, rank, candidate),
        Err(reason) => {
            tracing::debug!("skipping Python file: {reason}");
            Read::Ignored
        }
    }
}

fn index_candidate(file: IndexFile, url: &Url, source: bool) -> Result<Candidate> {
    if source {
        let sdist = LockedSdist { name: file.filename, url: url.to_string(), hashes: file.hashes };
        sdist.integrity()?;
        return Ok(Candidate::Sdist(sdist));
    }
    let core_metadata = file.metadata_digests();
    let wheel = LockedWheel { name: file.filename, url: url.to_string(), hashes: file.hashes };
    wheel.integrity()?;
    Ok(Candidate::Wheel(IndexCandidate { wheel, core_metadata }))
}

/// What a published file's name says it is, or `None` for a file that is
/// neither a wheel nor a source distribution of `name` — an index page
/// lists signatures, eggs and installers beside them.
struct PublishedFile {
    version: Version,
    /// `None` for a wheel this target accepts no tag of.
    rank: Option<usize>,
    source: bool,
}

fn published_file(
    filename: &str,
    name: &PackageName,
    tags: &[String],
) -> Result<Option<PublishedFile>> {
    if let Some(wheel) = WheelFilename::parse(filename)? {
        if wheel.name != *name {
            bail!("wheel for {}, not for {name}", wheel.name);
        }
        let rank = wheel.rank(tags);
        return Ok(Some(PublishedFile { version: wheel.version, rank, source: false }));
    }
    Ok(source_version(filename, name)?
        .map(|version| PublishedFile { version, rank: Some(SOURCE_RANK), source: true }))
}

/// The release a source distribution of `name` carries, or `None` when
/// the file is not one.
///
/// A source distribution is named `{distribution}-{version}` in a
/// container pnpm unpacks, and the distribution is spelled however
/// whoever built it spelled it. Which dash separates the two is
/// therefore decided by the distribution: the name is read back and
/// compared to the one whose page this is, so `python-u2flib-server`
/// carries `5.0.0` and not `server-5.0.0`.
pub fn source_version(filename: &str, name: &PackageName) -> Result<Option<Version>> {
    let Some(stem) = SOURCE_SUFFIXES
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix))
    else {
        return Ok(None);
    };
    if filename.contains(['/', '\\']) {
        bail!("invalid Python source distribution filename: {filename}");
    }
    Ok(stem
        .match_indices('-')
        .filter(|(index, _)| {
            stem[..*index]
                .parse::<PackageName>()
                .ok()
                .as_ref()
                == Some(name)
        })
        .find_map(|(index, _)| stem[index + 1..].parse::<Version>().ok()))
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

/// Preserves version constraints, extras, and markers. Direct URLs must use
/// HTTP(S), git+HTTPS, git+SSH, or git+file; other schemes return an error.
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
