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

pub(crate) struct Registry {
    packages: BTreeMap<String, Vec<RegistryVersion>>,
}

impl Registry {
    pub(crate) fn new(index_files: &BTreeMap<String, String>) -> Result<Self> {
        let mut packages = BTreeMap::new();
        for (name, contents) in index_files {
            packages.insert(normalize_name(name), parse_index_file(name, contents)?);
        }
        Ok(Self { packages })
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
    let registry = Registry::new(&BTreeMap::from([(name.to_string(), index_file.to_string())]))?;
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

pub(crate) fn validate_registry(registry: Option<&str>) -> Result<()> {
    if registry.is_none_or(|registry| {
        matches!(registry, CRATES_IO_SOURCE | CRATES_IO_INDEX | CRATES_IO_SPARSE_SOURCE)
    }) {
        Ok(())
    } else {
        Err(miette::miette!(
            "alternate Cargo registry {registry:?} is not supported by the crates.io proof of concept"
        ))
    }
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
