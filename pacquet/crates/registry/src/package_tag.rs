use derive_more::{Display, From, TryInto};
use node_semver::Version;
use std::str::FromStr;

/// Version or tag that is attachable to a registry URL.
#[derive(Debug, Display, From, TryInto, Clone)]
pub enum PackageTag {
    /// Literally `latest`.
    #[display("latest")]
    Latest,
    /// Pinned version.
    Version(Version),
    /// A custom tag (e.g. `beta`, `next`).
    #[display("{_0}")]
    Tag(String),
}

impl FromStr for PackageTag {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "latest" {
            Ok(PackageTag::Latest)
        } else {
            match value.parse::<Version>() {
                Ok(version) => Ok(PackageTag::Version(version)),
                Err(_) => Ok(PackageTag::Tag(value.to_owned())),
            }
        }
    }
}
