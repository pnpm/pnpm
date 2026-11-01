//! The string form of platform tags.

use super::{Floor, Libc, NodeAbi, Platform, TagError, parse_number};
use std::iter::Peekable;

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

/// The parts of a tag after `<os>-<cpu>`, read in the vocabulary's order.
struct Parts<'tag, Iter: Iterator<Item = &'tag str>> {
    tag: &'tag str,
    parts: Peekable<Iter>,
}

impl<'tag, Iter: Iterator<Item = &'tag str>> Parts<'tag, Iter> {
    /// The next part, parsed by `parse`, when `parse` recognizes it.
    fn take<Value>(
        &mut self,
        parse: impl Fn(&str) -> Option<Result<Value, TagError>>,
    ) -> Result<Option<Value>, TagError> {
        let Some(parsed) = self.parts.peek().and_then(|part| parse(part)) else { return Ok(None) };
        self.parts.next();
        parsed.map(Some)
    }

    fn rejects(&self, reason: &'static str) -> TagError {
        TagError::new(self.tag, reason)
    }

    /// The C library of a platform on `os`, and its glibc floor: `musl`,
    /// then `manylinux_<major>_<minor>`, both only on Linux.
    fn libc(&mut self, os: &str) -> Result<(Option<Libc>, Option<Floor>), TagError> {
        let musl = self
            .take(|part| (part == "musl").then_some(Ok(())))?
            .is_some();
        let libc_version = self.take(|part| parse_floor(part, "manylinux_"))?;
        if os != "linux" {
            return match (musl, libc_version) {
                (false, None) => Ok((None, None)),
                _ => Err(self.rejects("names a C library outside Linux")),
            };
        }
        if musl && libc_version.is_some() {
            return Err(self.rejects("has a glibc floor without glibc"));
        }
        Ok((Some(if musl { Libc::Musl } else { Libc::Glibc }), libc_version))
    }
}

fn parse_node_abi(part: &str) -> Option<Result<NodeAbi, TagError>> {
    if let Some(abi) = part.strip_prefix("node") {
        return Some(parse_number(abi).map(NodeAbi::Node));
    }
    part.strip_prefix("electron")
        .map(|abi| parse_number(abi).map(NodeAbi::Electron))
}

impl Platform {
    /// Parse a platform tag. Only the canonical form parses, so a platform
    /// has one tag and [`Platform::tag`] gives it back.
    pub fn parse_tag(tag: &str) -> Result<Platform, TagError> {
        if tag == "wasm32" {
            return Ok(Platform { wasm32: true, ..Platform::default() });
        }
        let mut split = tag.split('-');
        let (Some(os), Some(cpu)) = (split.next(), split.next()) else {
            return Err(TagError::new(tag, "is not <os>-<cpu>"));
        };
        if !is_os_or_cpu(os) || !is_os_or_cpu(cpu) {
            return Err(TagError::new(tag, "has an invalid os or cpu"));
        }
        let mut parts = Parts { tag, parts: split.peekable() };
        let (libc, libc_version) = parts.libc(os)?;
        let os_prefix = match os {
            "darwin" => "macos_",
            "win32" => "windows_",
            _ => "\0",
        };
        let platform = Platform {
            os: Some(os.to_string()),
            cpu: Some(cpu.to_string()),
            libc,
            libc_version,
            os_version: parts.take(|part| parse_floor(part, os_prefix))?,
            napi: parts.take(|part| part.strip_prefix("napi").map(parse_number))?,
            node_abi: parts.take(parse_node_abi)?,
            wasm32: false,
        };
        match parts.parts.next() {
            Some(_) => Err(parts.rejects("has an unknown or misplaced part")),
            None => Ok(platform),
        }
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
