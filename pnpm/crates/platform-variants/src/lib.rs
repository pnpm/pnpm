//! Platform tags of package variants (the pnpr platform-variants RFC).
//!
//! A variant archive declares the platforms it runs on as tags, such as
//! `linux-x64-musl` or `linux-x64-manylinux_2_28-napi8`. pnpr parses them
//! into [`Platform`]s, the structured form its package document and pnpm's
//! lockfile carry, and names the variant by [`variant_tag`]. pnpm derives
//! the same name from the lockfile's targets and picks a variant for a host
//! with [`select`].

pub use manifests::variant_manifests;
pub use selection::{AmbiguousSelection, Host, HostFloors, select, selection_order};
pub use tag::{is_valid_variant_tag, variant_tag};

mod host_floors;
mod manifests;
mod selection;
mod tag;

#[cfg(target_family = "wasm")]
pub(crate) use pnpm_process as process;
#[cfg(not(target_family = "wasm"))]
pub(crate) use std::process;

use derive_more::{Display, Error};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

/// A C library a Linux platform links against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Libc {
    Glibc,
    Musl,
}

/// A version floor: the lowest `major.minor` a host must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Floor {
    pub major: u32,
    pub minor: u32,
}

impl fmt::Display for Floor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for Floor {
    type Err = TagError;

    /// `11` and `11.0` both give `11.0`.
    fn from_str(value: &str) -> Result<Self, TagError> {
        let (major, minor) = value
            .split_once('.')
            .unwrap_or((value, "0"));
        Ok(Floor { major: parse_number(major)?, minor: parse_number(minor)? })
    }
}

impl Serialize for Floor {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Floor {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        String::deserialize(deserializer)?.parse().map_err(serde::de::Error::custom)
    }
}

/// The ABI of an addon outside N-API: the runtime it loads into and that
/// runtime's module version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeAbi {
    /// `node-<NODE_MODULE_VERSION>`.
    Node(u32),
    /// `electron-<abi>`.
    Electron(u32),
}

impl fmt::Display for NodeAbi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeAbi::Node(abi) => write!(f, "node-{abi}"),
            NodeAbi::Electron(abi) => write!(f, "electron-{abi}"),
        }
    }
}

impl FromStr for NodeAbi {
    type Err = TagError;

    fn from_str(value: &str) -> Result<Self, TagError> {
        match value.split_once('-') {
            Some(("node", abi)) => Ok(NodeAbi::Node(parse_number(abi)?)),
            Some(("electron", abi)) => Ok(NodeAbi::Electron(parse_number(abi)?)),
            _ => Err(TagError::new(value, "is not node-<abi> or electron-<abi>")),
        }
    }
}

impl Serialize for NodeAbi {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NodeAbi {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        String::deserialize(deserializer)?.parse().map_err(serde::de::Error::custom)
    }
}

/// One platform a variant runs on, in the structured form of pnpr's package
/// document and pnpm's lockfile. A field this vocabulary does not know fails
/// to deserialize, so a client never selects a platform it cannot match.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Platform {
    /// Node.js `process.platform`. Absent only for `wasm32`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Node.js `process.arch`. Absent only for `wasm32`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// Absent on Linux means glibc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<Libc>,
    /// glibc floor, with `manylinux` semantics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc_version: Option<Floor>,
    /// macOS or Windows floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<Floor>,
    /// The lowest N-API version the addon needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub napi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_abi: Option<NodeAbi>,
    /// Matches every host, ranked last.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wasm32: bool,
}

impl Platform {
    /// The platform with glibc written out on Linux, as pnpr's document
    /// writes it.
    #[must_use]
    pub fn with_explicit_glibc(mut self) -> Platform {
        if self.os.as_deref() == Some("linux") && self.libc.is_none() {
            self.libc = Some(Libc::Glibc);
        }
        self
    }

    /// The platform with glibc left out, as the lockfile writes it.
    #[must_use]
    pub fn without_explicit_glibc(mut self) -> Platform {
        if self.libc == Some(Libc::Glibc) {
            self.libc = None;
        }
        self
    }

    /// The C library, with an absent one on Linux read as glibc.
    fn effective_libc(&self) -> Option<Libc> {
        match self.libc {
            None if self.os.as_deref() == Some("linux") => Some(Libc::Glibc),
            libc => libc,
        }
    }
}

/// A platform tag that does not parse, or a platform that has no tag.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("platform tag {tag:?} {reason}")]
pub struct TagError {
    pub tag: String,
    pub reason: &'static str,
}

impl TagError {
    fn new(tag: &str, reason: &'static str) -> Self {
        TagError { tag: tag.to_string(), reason }
    }
}

/// A decimal number without leading zeros.
fn parse_number(value: &str) -> Result<u32, TagError> {
    let canonical = !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'));
    if !canonical {
        return Err(TagError::new(value, "is not a decimal number without leading zeros"));
    }
    value.parse().map_err(|_| TagError::new(value, "is too large"))
}

#[cfg(test)]
mod tests;
