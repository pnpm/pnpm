//! Platform tags of package variants (the pnpr platform-variants RFC).
//!
//! A variant archive declares the platforms it runs on as tags, such as
//! `linux-x64-musl` or `linux-x64-manylinux_2_28-napi8`. pnpr parses them
//! into [`Platform`]s, the structured form its package document and pnpm's
//! lockfile carry, and names the variant by [`variant_tag`]. pnpm derives
//! the same name from the lockfile's targets and picks a variant for a host
//! with [`select`].

use derive_more::{Display, Error};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{cmp::Ordering, fmt, str::FromStr};

/// A C library a Linux platform links against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Libc {
    Glibc,
    Musl,
}

/// A version floor: the lowest `major.minor` a host must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Floor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.parse().map_err(serde::de::Error::custom)
    }
}

/// The ABI of an addon outside N-API: the runtime it loads into and that
/// runtime's module version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NodeAbi {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.parse().map_err(serde::de::Error::custom)
    }
}

/// One platform a variant runs on, in the structured form of pnpr's package
/// document and pnpm's lockfile. A field this vocabulary does not know fails
/// to deserialize, so a client never selects a platform it cannot match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// `<prefix><major>_<minor>`, such as `manylinux_2_28`.
fn parse_floor(part: &str, prefix: &str) -> Option<Result<Floor, TagError>> {
    let rest = part.strip_prefix(prefix)?;
    Some(match rest.split_once('_') {
        Some((major, minor)) => parse_number(major)
            .and_then(|major| parse_number(minor).map(|minor| Floor { major, minor })),
        None => Err(TagError::new(part, "has no minor version")),
    })
}

fn is_os_or_cpu(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

impl Platform {
    /// Parse a platform tag. Only the canonical form parses, so a platform
    /// has one tag and [`Platform::tag`] gives it back.
    pub fn parse_tag(tag: &str) -> Result<Platform, TagError> {
        if tag == "wasm32" {
            return Ok(Platform { wasm32: true, ..Platform::default() });
        }
        let mut parts = tag.split('-').peekable();
        let (Some(os), Some(cpu)) = (parts.next(), parts.next()) else {
            return Err(TagError::new(tag, "is not <os>-<cpu>"));
        };
        if !is_os_or_cpu(os) || !is_os_or_cpu(cpu) {
            return Err(TagError::new(tag, "has an invalid os or cpu"));
        }
        let mut platform = Platform {
            os: Some(os.to_string()),
            cpu: Some(cpu.to_string()),
            libc: (os == "linux").then_some(Libc::Glibc),
            ..Platform::default()
        };
        if parts.next_if_eq(&"musl").is_some() {
            if os != "linux" {
                return Err(TagError::new(tag, "names a C library outside Linux"));
            }
            platform.libc = Some(Libc::Musl);
        }
        if let Some(floor) = parts.peek().and_then(|part| parse_floor(part, "manylinux_")) {
            if platform.libc != Some(Libc::Glibc) {
                return Err(TagError::new(tag, "has a glibc floor without glibc"));
            }
            platform.libc_version = Some(floor?);
            parts.next();
        }
        let os_prefix = match os {
            "darwin" => Some("macos_"),
            "win32" => Some("windows_"),
            _ => None,
        };
        if let Some(floor) =
            os_prefix.and_then(|prefix| parts.peek().and_then(|part| parse_floor(part, prefix)))
        {
            platform.os_version = Some(floor?);
            parts.next();
        }
        if let Some(napi) = parts
            .peek()
            .and_then(|part| part.strip_prefix("napi"))
        {
            platform.napi = Some(parse_number(napi)?);
            parts.next();
        }
        if let Some(part) = parts.peek() {
            let abi = if let Some(abi) = part.strip_prefix("node") {
                NodeAbi::Node(parse_number(abi)?)
            } else if let Some(abi) = part.strip_prefix("electron") {
                NodeAbi::Electron(parse_number(abi)?)
            } else {
                return Err(TagError::new(tag, "has an unknown or misplaced part"));
            };
            platform.node_abi = Some(abi);
            parts.next();
        }
        if parts.next().is_some() {
            return Err(TagError::new(tag, "has an unknown or misplaced part"));
        }
        Ok(platform)
    }

    /// The platform's tag: its fields in the vocabulary's order, joined with
    /// `-`. glibc is written as no part.
    pub fn tag(&self) -> Result<String, TagError> {
        if self.wasm32 {
            return Ok("wasm32".to_string());
        }
        let (Some(os), Some(cpu)) = (&self.os, &self.cpu) else {
            return Err(TagError::new("", "has no os or cpu"));
        };
        let mut parts = vec![os.clone(), cpu.clone()];
        if self.libc == Some(Libc::Musl) {
            parts.push("musl".into());
        }
        if let Some(Floor { major, minor }) = self.libc_version {
            parts.push(format!("manylinux_{major}_{minor}"));
        }
        if let Some(Floor { major, minor }) = self.os_version {
            let prefix = if os == "win32" { "windows" } else { "macos" };
            parts.push(format!("{prefix}_{major}_{minor}"));
        }
        if let Some(napi) = self.napi {
            parts.push(format!("napi{napi}"));
        }
        match self.node_abi {
            Some(NodeAbi::Node(abi)) => parts.push(format!("node{abi}")),
            Some(NodeAbi::Electron(abi)) => parts.push(format!("electron{abi}")),
            None => {}
        }
        Ok(parts.join("-"))
    }

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

    /// The number of fields a host matches when it matches this platform.
    fn field_count(&self) -> usize {
        if self.wasm32 {
            return 1;
        }
        let libc = self.libc.is_some() || self.os.as_deref() == Some("linux");
        2 + usize::from(libc)
            + usize::from(self.libc_version.is_some())
            + usize::from(self.os_version.is_some())
            + usize::from(self.napi.is_some())
            + usize::from(self.node_abi.is_some())
    }

    /// Whether `host` runs this platform. A floor the host's value is unknown
    /// for matches, so a platform listed in `supportedArchitectures` without
    /// versions fetches its variants; an unknown ABI does not, since loading
    /// the wrong one fails.
    #[must_use]
    pub fn matches(&self, host: &Host) -> bool {
        if self.wasm32 {
            return true;
        }
        let floor = |floor: Option<Floor>, host: Option<Floor>| match (floor, host) {
            (Some(floor), Some(host)) => host >= floor,
            _ => true,
        };
        self.os.as_deref() == Some(host.os.as_str())
            && self.cpu.as_deref() == Some(host.cpu.as_str())
            && self.effective_libc() == host.effective_libc()
            && floor(self.libc_version, host.libc_version)
            && floor(self.os_version, host.os_version)
            && match (self.napi, host.napi) {
                (Some(napi), Some(host)) => host >= napi,
                _ => true,
            }
            && match self.node_abi {
                Some(abi) => host.node_abi == Some(abi),
                None => true,
            }
    }

    fn effective_libc(&self) -> Option<Libc> {
        match self.libc {
            None if self.os.as_deref() == Some("linux") => Some(Libc::Glibc),
            libc => libc,
        }
    }

    /// How well this platform fits a host it matches: more matched fields,
    /// then the highest glibc, OS, and N-API floors. `wasm32` ranks last.
    fn rank(&self) -> Rank {
        Rank {
            native: !self.wasm32,
            fields: self.field_count(),
            libc_version: self.libc_version,
            os_version: self.os_version,
            napi: self.napi,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    native: bool,
    fields: usize,
    libc_version: Option<Floor>,
    os_version: Option<Floor>,
    napi: Option<u32>,
}

/// The variant's name in routes and the lockfile: its platforms' tags,
/// sorted by their bytes and joined with `+`.
pub fn variant_tag(platforms: &[Platform]) -> Result<String, TagError> {
    let mut tags = platforms
        .iter()
        .map(Platform::tag)
        .collect::<Result<Vec<_>, _>>()?;
    tags.sort_unstable();
    Ok(tags.join("+"))
}

/// Whether `tag` can name a variant in a route: `[a-z0-9._+-]+`, never `.`
/// or `..`.
#[must_use]
pub fn is_valid_variant_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag != "."
        && tag != ".."
        && tag
            .bytes()
            .all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._+-".contains(&byte)
            })
}

/// What a client knows about the host it selects a variant for. A floor it
/// does not know is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Host {
    pub os: String,
    pub cpu: String,
    /// `None` outside Linux; on Linux, `None` means glibc.
    pub libc: Option<Libc>,
    pub libc_version: Option<Floor>,
    pub os_version: Option<Floor>,
    pub napi: Option<u32>,
    pub node_abi: Option<NodeAbi>,
}

impl Host {
    fn effective_libc(&self) -> Option<Libc> {
        match self.libc {
            None if self.os == "linux" => Some(Libc::Glibc),
            libc => libc,
        }
    }
}

/// Two variants fit a host equally well. Publication rejects versions where
/// this can happen, so a client fails rather than pick one.
#[derive(Debug, Display, Error, Clone, Copy, PartialEq, Eq)]
#[display("variants {first} and {second} fit the host equally well")]
pub struct AmbiguousSelection {
    pub first: usize,
    pub second: usize,
}

/// The index of the variant that fits `host` best, comparing each variant by
/// its best matching platform, or `None` when no variant matches.
pub fn select<'a, I>(variants: I, host: &Host) -> Result<Option<usize>, AmbiguousSelection>
where
    I: IntoIterator<Item = &'a [Platform]>,
{
    let mut best: Option<(Rank, usize)> = None;
    let mut tie: Option<usize> = None;
    for (index, platforms) in variants.into_iter().enumerate() {
        let Some(rank) = platforms
            .iter()
            .filter(|platform| platform.matches(host))
            .map(Platform::rank)
            .max()
        else {
            continue;
        };
        match best.map(|(best, _)| rank.cmp(&best)) {
            None | Some(Ordering::Greater) => {
                best = Some((rank, index));
                tie = None;
            }
            Some(Ordering::Equal) => tie = Some(index),
            Some(Ordering::Less) => {}
        }
    }
    match (best, tie) {
        (Some((_, first)), Some(second)) => Err(AmbiguousSelection { first, second }),
        (best, None) => Ok(best.map(|(_, index)| index)),
        (None, Some(_)) => Ok(None),
    }
}

/// The variants' order in the lockfile: native before `wasm32`, then by the
/// rank of each variant's best platform, highest first, so a first match
/// over the list agrees with [`select`] on every host where a single
/// platform of each variant matches.
#[must_use]
pub fn selection_order(variants: &[&[Platform]]) -> Vec<usize> {
    let best = |platforms: &[Platform]| {
        platforms
            .iter()
            .map(Platform::rank)
            .max()
    };
    let mut order: Vec<usize> = (0..variants.len()).collect();
    order.sort_by(|&a, &b| best(variants[b]).cmp(&best(variants[a])));
    order
}

#[cfg(test)]
mod tests;
