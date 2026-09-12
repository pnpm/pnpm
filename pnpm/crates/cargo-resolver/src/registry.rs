use crate::model::{DependencyKind, RegistryDependency, RegistryVersion};
use cargo_util_schemas::index::{IndexPackage, RegistryDependency as IndexDependency};
use miette::{IntoDiagnostic, Result, WrapErr};
use semver::{Version, VersionReq};
use std::collections::BTreeMap;

pub(crate) const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const CRATES_IO_INDEX: &str = "https://github.com/rust-lang/crates.io-index";
const CRATES_IO_SPARSE_SOURCE: &str = "sparse+https://index.crates.io/";

/// The base URL of the crates.io sparse index: the registry a crate's
/// index file is fetched from when nothing else is configured.
pub const CRATES_IO_SPARSE_INDEX: &str = "https://index.crates.io";

/// Whether `index_url` addresses the crates.io sparse index.
#[must_use]
pub fn is_crates_io(index_url: &str) -> bool {
    index_url.trim_end_matches('/') == CRATES_IO_SPARSE_INDEX
}

/// The `source` identifier of a sparse registry, as `cargo` spells it in
/// `.cargo/config.toml` and in `Cargo.lock`.
#[must_use]
pub fn sparse_source(index_url: &str) -> String {
    format!("sparse+{}/", index_url.trim_end_matches('/'))
}

/// The `source` a `Cargo.lock` records for packages taken from `index_url`.
/// crates.io keeps the canonical git identifier `cargo` itself writes even
/// when the sparse index served the metadata; every other registry is named
/// by its sparse index.
#[must_use]
pub fn registry_source(index_url: &str) -> String {
    if is_crates_io(index_url) { CRATES_IO_SOURCE.to_string() } else { sparse_source(index_url) }
}

#[must_use]
pub fn download_url(template: &str, name: &str, version: &str, checksum: &str) -> String {
    const MARKERS: [&str; 5] =
        ["{crate}", "{version}", "{prefix}", "{lowerprefix}", "{sha256-checksum}"];
    if !MARKERS.iter().any(|marker| template.contains(marker)) {
        return format!("{}/{name}/{version}/download", template.trim_end_matches('/'));
    }
    template
        .replace("{crate}", name)
        .replace("{version}", version)
        .replace("{prefix}", &index_prefix(name))
        .replace("{lowerprefix}", &index_prefix(&name.to_ascii_lowercase()))
        .replace("{sha256-checksum}", checksum)
}

pub(crate) struct Registry {
    packages: BTreeMap<String, Vec<RegistryVersion>>,
    source: String,
}

impl Registry {
    pub(crate) fn new(index_files: &BTreeMap<String, String>, source: &str) -> Result<Self> {
        let mut packages = BTreeMap::new();
        for (name, contents) in index_files {
            packages.insert(normalize_name(name), parse_index_file(name, contents)?);
        }
        Ok(Self { packages, source: source.to_string() })
    }

    /// Reject a dependency that names a registry other than the one being
    /// resolved from. An entry without a registry is served by this one, and
    /// a registry that mirrors crates.io keeps the upstream spelling in the
    /// entries it copies.
    pub(crate) fn validate_dependency_source(&self, registry: Option<&str>) -> Result<()> {
        let Some(registry) = registry else { return Ok(()) };
        if is_crates_io_source(registry) || same_registry(registry, &self.source) {
            return Ok(());
        }
        Err(miette::miette!(
            "dependency from Cargo registry {registry:?} cannot be resolved from {:?}",
            self.source,
        ))
    }

    pub(crate) fn versions(&self, name: &str) -> Option<&[RegistryVersion]> {
        self.packages.get(&normalize_name(name)).map(Vec::as_slice)
    }

    pub(crate) fn package(&self, name: &str) -> Result<&[RegistryVersion]> {
        self.versions(name).ok_or_else(|| {
            miette::miette!("sparse index metadata for crate {name} was not fetched")
        })
    }
}

/// Return the newest stable, non-yanked version from a crates.io sparse-index entry.
pub fn latest_version(name: &str, index_file: &str) -> Result<String> {
    let registry = Registry::new(
        &BTreeMap::from([(name.to_string(), index_file.to_string())]),
        CRATES_IO_SOURCE,
    )?;
    registry
        .package(name)?
        .iter()
        .rev()
        .find(|version| !version.yanked && version.version.pre.is_empty())
        .map(|version| version.version.to_string())
        .ok_or_else(|| miette::miette!("crate {name} has no stable, non-yanked version"))
}

fn parse_index_file(name: &str, contents: &str) -> Result<Vec<RegistryVersion>> {
    let mut versions = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        let package: IndexPackage<'_> = serde_json::from_str(line)
            .into_diagnostic()
            .wrap_err_with(|| format!("parse sparse index entry {name}:{}", line_index + 1))?;
        if let Some(version) = registry_version_from_index(package)? {
            versions.push(version);
        }
    }
    versions.sort_by(|left, right| left.version.cmp(&right.version));
    Ok(versions)
}

fn registry_version_from_index(package: IndexPackage<'_>) -> Result<Option<RegistryVersion>> {
    if package.v.is_some_and(|version| version > 3) {
        return Ok(None);
    }
    let dependencies =
        package.deps.into_iter().map(registry_dependency_from_index).collect::<Result<Vec<_>>>()?;
    let mut features: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, values) in package.features.into_iter().chain(package.features2.unwrap_or_default())
    {
        features
            .entry(name.into_owned())
            .or_default()
            .extend(values.into_iter().map(std::borrow::Cow::into_owned));
    }
    Ok(Some(RegistryVersion {
        version: package.vers,
        dependencies,
        features,
        checksum: package.cksum,
        yanked: package.yanked.unwrap_or(false),
    }))
}

fn registry_dependency_from_index(dependency: IndexDependency<'_>) -> Result<RegistryDependency> {
    let alias = dependency.name.into_owned();
    let name = dependency.package.map_or_else(|| alias.clone(), std::borrow::Cow::into_owned);
    let requirement = VersionReq::parse(&dependency.req)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse requirement for {name}"))?;
    Ok(RegistryDependency {
        alias,
        name,
        requirement,
        kind: DependencyKind::from_raw(dependency.kind.as_deref()),
        registry: dependency.registry.map(std::borrow::Cow::into_owned),
        optional: dependency.optional,
        default_features: dependency.default_features,
        features: dependency.features.into_iter().map(std::borrow::Cow::into_owned).collect(),
    })
}

pub(crate) fn matching_versions<'a>(
    versions: &'a [RegistryVersion],
    requirement: &'a VersionReq,
) -> impl DoubleEndedIterator<Item = &'a RegistryVersion> {
    versions.iter().filter(|version| !version.yanked && requirement.matches(&version.version))
}

fn is_crates_io_source(registry: &str) -> bool {
    matches!(registry, CRATES_IO_SOURCE | CRATES_IO_INDEX | CRATES_IO_SPARSE_SOURCE)
}

fn same_registry(left: &str, right: &str) -> bool {
    strip_source_kind(left) == strip_source_kind(right)
}

fn strip_source_kind(url: &str) -> &str {
    url.strip_prefix("sparse+")
        .or_else(|| url.strip_prefix("registry+"))
        .unwrap_or(url)
        .trim_end_matches('/')
}

pub(crate) fn compatibility_line(version: &Version) -> String {
    if version.major != 0 {
        version.major.to_string()
    } else if version.minor != 0 {
        format!("0.{}", version.minor)
    } else {
        format!("0.0.{}", version.patch)
    }
}

fn normalize_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// The directory part of a crate's sparse-index path, in the name's own
/// case: `1`, `2`, `3/<first letter>`, or `<first two>/<next two>`.
#[must_use]
pub fn index_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_string(),
        2 => "2".to_string(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    }
}
