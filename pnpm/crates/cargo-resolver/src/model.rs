use semver::{Version, VersionReq};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Debug, Deserialize)]
pub(crate) struct CargoMetadata {
    pub(crate) packages: Vec<MetadataPackage>,
    pub(crate) workspace_members: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataPackage {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: Version,
    pub(crate) dependencies: Vec<MetadataDependency>,
    #[serde(default)]
    pub(crate) features: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataDependency {
    pub(crate) name: String,
    pub(crate) source: Option<String>,
    pub(crate) req: VersionReq,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) rename: Option<String>,
    #[serde(default)]
    pub(crate) optional: bool,
    #[serde(default = "default_true")]
    pub(crate) uses_default_features: bool,
    #[serde(default)]
    pub(crate) features: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryVersion {
    pub(crate) version: Version,
    pub(crate) dependencies: Vec<RegistryDependency>,
    pub(crate) features: BTreeMap<String, Vec<String>>,
    pub(crate) checksum: String,
    pub(crate) yanked: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryDependency {
    pub(crate) alias: String,
    pub(crate) name: String,
    pub(crate) requirement: VersionReq,
    pub(crate) kind: Option<String>,
    pub(crate) registry: Option<String>,
    pub(crate) optional: bool,
    pub(crate) default_features: bool,
    pub(crate) features: BTreeSet<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSelection {
    pub(crate) default_features: bool,
    pub(crate) features: BTreeSet<String>,
}

impl RegistryDependency {
    pub(crate) fn feature_selection(&self) -> FeatureSelection {
        FeatureSelection {
            default_features: self.default_features,
            features: self.features.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PackageKey {
    Root,
    Registry { name: String, compatibility: String },
}

impl fmt::Display for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("pnpm Cargo workspace"),
            Self::Registry { name, compatibility } => write!(formatter, "{name}@{compatibility}"),
        }
    }
}

const fn default_true() -> bool {
    true
}
