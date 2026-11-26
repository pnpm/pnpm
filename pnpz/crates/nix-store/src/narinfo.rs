use crate::{
    NarHash,
    StorePath,
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;

/// How the archive a [`NarInfo`] points at is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Xz,
    Zstd,
}

/// A binary cache's description of one store path.
///
/// A signature covers [`NarInfo::fingerprint`]: the path, the hash and size of
/// its archive, and its references. It does not cover `url` or `compression`,
/// so a cache may recompress an archive without invalidating the signature,
/// and a client must treat `url` as untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarInfo {
    pub store_path: StorePath,
    /// The archive's location, relative to the cache root.
    pub url: String,
    pub compression: Compression,
    pub nar_hash: NarHash,
    pub nar_size: u64,
    /// The store paths this one refers to. May include `store_path` itself.
    pub references: Vec<StorePath>,
    /// `<key name>:<base64 signature>` entries.
    pub signatures: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("invalid narinfo: {reason}")]
#[diagnostic(code(ERR_PNPZ_INVALID_NARINFO))]
pub struct ParseNarInfoError {
    reason: String,
}

fn invalid(reason: impl Into<String>) -> ParseNarInfoError {
    ParseNarInfoError { reason: reason.into() }
}

impl NarInfo {
    pub fn parse(text: &str) -> Result<Self, ParseNarInfoError> {
        let field = |name: &str| {
            fields(text)
                .find_map(|(key, value)| (key == name).then_some(value))
                .ok_or_else(|| invalid(format!("the {name} field is missing")))
        };
        let references = field("References").unwrap_or_default();
        Ok(Self {
            store_path: StorePath::from_absolute(field("StorePath")?)
                .map_err(|error| invalid(error.to_string()))?,
            url: parse_url(field("URL")?)?,
            compression: parse_compression(field("Compression")?)?,
            nar_hash: NarHash::parse(field("NarHash")?)
                .map_err(|error| invalid(error.to_string()))?,
            nar_size: field("NarSize")?.parse().map_err(|_| invalid("NarSize is not a number"))?,
            references: references
                .split_ascii_whitespace()
                .map(StorePath::from_base_name)
                .collect::<Result<_, _>>()
                .map_err(|error| invalid(error.to_string()))?,
            signatures: fields(text)
                .filter_map(|(key, value)| (key == "Sig").then(|| value.to_string()))
                .collect(),
        })
    }

    /// The text a cache key signs.
    pub fn fingerprint(&self) -> String {
        let references: Vec<String> = self.references
            .iter()
            .map(StorePath::to_string)
            .collect();
        format!(
            "1;{};{};{};{}",
            self.store_path,
            self.nar_hash.to_nix_base32(),
            self.nar_size,
            references.join(","),
        )
    }
}

fn fields(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines()
        .filter_map(|line| line.split_once(": "))
}

/// Accepts only a path below the cache root, so that an unsigned field cannot
/// send the client to another host or outside the cache.
fn parse_url(url: &str) -> Result<String, ParseNarInfoError> {
    let is_relative = !url.is_empty()
        && !url.starts_with('/')
        && !url.contains("://")
        && url
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "..");
    if !is_relative {
        return Err(invalid(format!("URL {url:?} is not a path relative to the cache")));
    }
    Ok(url.to_string())
}

fn parse_compression(name: &str) -> Result<Compression, ParseNarInfoError> {
    match name {
        "none" => Ok(Compression::None),
        "xz" => Ok(Compression::Xz),
        "zstd" => Ok(Compression::Zstd),
        other => Err(invalid(format!("unsupported compression {other:?}"))),
    }
}

#[cfg(test)]
mod tests;
