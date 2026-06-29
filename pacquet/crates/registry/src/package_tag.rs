use std::str::FromStr;

use derive_more::{Display, From, TryInto};
use node_semver::{Range, SemverError, Version};

/// Version or tag that is attachable to a registry URL.
#[derive(Debug, Display, Clone, From, TryInto)]
pub enum PackageTag {
    /// Literally `latest`.
    #[display("latest")]
    Latest,
    /// Pinned version.
    #[display("{}", _0)]
    Version(Version),
    /// A custom tag (e.g. `beta`, `next`).
    #[display("{}", _0)]
    Tag(String),
}

impl FromStr for PackageTag {
    type Err = SemverError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "latest" {
            Ok(PackageTag::Latest)
        } else if let Ok(version) = value.parse::<Version>() {
            Ok(PackageTag::Version(version))
        } else if Range::parse(value).is_ok() {
            value.parse::<Version>().map(PackageTag::Version)
        } else {
            Ok(PackageTag::Tag(value.to_owned()))
        }
    }
}
