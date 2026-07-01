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

impl PackageTag {
    /// URL-encoded path segment for use in a registry request URL.
    /// The version or custom tag is percent-encoded so the path
    /// component stays syntactically valid regardless of the characters
    /// it contains (e.g. `+` build metadata in a version).
    #[must_use]
    pub fn registry_path_segment(&self) -> String {
        match self {
            PackageTag::Latest => "latest".to_string(),
            PackageTag::Version(v) => pacquet_network::encode_uri_component(&v.to_string()),
            PackageTag::Tag(tag) => pacquet_network::encode_uri_component(tag),
        }
    }
}

impl FromStr for PackageTag {
    type Err = SemverError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "latest" {
            Ok(PackageTag::Latest)
        } else if let Ok(version) = value.parse::<Version>() {
            Ok(PackageTag::Version(version))
        } else if Range::parse(value).is_ok() {
            // A semver range (e.g. `^18`) is neither an exact version nor a
            // dist-tag, so it is not a valid path segment for the abbreviated
            // registry endpoint. Report it as an error (the range never
            // parses as a `Version`) rather than treating it as a literal tag
            // that would issue a doomed `/pkg/^18` request.
            value.parse::<Version>().map(PackageTag::Version)
        } else {
            Ok(PackageTag::Tag(value.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests;
