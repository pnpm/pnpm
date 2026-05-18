use derive_more::{Display, Error};
use node_semver::{SemverError, Version};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt, str::FromStr};

/// Suffix type of [`PkgNameVerPeer`](crate::PkgNameVerPeer) and
/// type of [`ResolvedDependencySpec::version`](crate::ResolvedDependencySpec::version).
///
/// Example: `1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)`
///
/// Runtime entries (pnpm v11's `node@runtime:` /  `deno@runtime:` /
/// `bun@runtime:` deps) carry a `runtime:` prefix in front of the
/// version part (e.g. `runtime:22.0.0`). Pacquet preserves that
/// prefix through [`Prefix`] so a round-trip stays byte-stable
/// against pnpm's `pnpm-lock.yaml` output. Mirrors upstream's
/// depPath shape at
/// [`engine/runtime/node-resolver`](https://github.com/pnpm/pnpm/blob/94240bc046/engine/runtime/node-resolver/src/index.ts).
///
/// **NOTE:** The peer part isn't guaranteed to be correct. It is only assumed to be.
#[derive(Debug, Display, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[display("{prefix}{version}{peer}")]
#[serde(try_from = "Cow<'de, str>", into = "String")]
pub struct PkgVerPeer {
    /// Scheme prefix (e.g. `runtime:`) preserved verbatim through
    /// the round-trip. [`Prefix::None`] for plain semver — the only
    /// shape pacquet recognises before pnpm v11.
    prefix: Prefix,
    version: Version,
    peer: String,
}

/// Optional scheme prefix on a [`PkgVerPeer`].
///
/// Pnpm v11 introduces `runtime:` for runtime dependencies; pacquet
/// preserves the substring so the resulting depPath
/// (e.g. `node@runtime:22.0.0`) round-trips correctly and downstream
/// consumers (the `--no-runtime` filter, the install dispatcher)
/// can discriminate runtime entries by their prefix instead of
/// substring-searching the depPath.
///
/// The enum is closed because pnpm only defines this one scheme so
/// far; if upstream adds another (e.g. `tag:`), add a variant here
/// rather than turning the prefix into an unbounded string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prefix {
    /// No prefix — plain semver, the default before pnpm v11.
    None,
    /// `runtime:` — runtime dependency specifier (`node@runtime:`,
    /// `deno@runtime:`, `bun@runtime:`).
    Runtime,
}

impl Prefix {
    /// String form, including the trailing `:` for non-`None`
    /// variants. Round-trips through the parser.
    pub const fn as_str(self) -> &'static str {
        match self {
            Prefix::None => "",
            Prefix::Runtime => "runtime:",
        }
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PkgVerPeer {
    /// Get the version part. The `runtime:` prefix (if any) is
    /// stripped — callers that need to discriminate runtime
    /// entries should consult [`PkgVerPeer::prefix`] instead.
    pub fn version(&self) -> &'_ Version {
        &self.version
    }

    /// Get the peer part.
    pub fn peer(&self) -> &'_ str {
        self.peer.as_str()
    }

    /// Get the prefix variant.
    pub fn prefix(&self) -> Prefix {
        self.prefix
    }

    /// Destructure the struct into a tuple of version and peer.
    /// The prefix (if any) is dropped — keep the call shape
    /// backward-compatible with pre-runtime consumers. New callers
    /// that need the prefix should access it via
    /// [`PkgVerPeer::prefix`] before destructuring.
    pub fn into_tuple(self) -> (Version, String) {
        let PkgVerPeer { prefix: _, version, peer } = self;
        (version, peer)
    }
}

/// Error when parsing [`PkgVerPeer`] from a string.
#[derive(Debug, Display, Error)]
pub enum ParsePkgVerPeerError {
    #[display("Failed to parse the version part: {_0}")]
    ParseVersionFailure(#[error(source)] SemverError),
    #[display("Mismatch parenthesis")]
    MismatchParenthesis,
}

impl FromStr for PkgVerPeer {
    type Err = ParsePkgVerPeerError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Strip a leading `runtime:` (or future scheme) before
        // anything else. Pnpm v11 writes the depPath with the
        // scheme prefix flush against the semver, e.g.
        // `runtime:22.0.0` — splitting it off here keeps the
        // parenthesis-based peer-suffix detection below unchanged.
        let (prefix, body) = if let Some(rest) = value.strip_prefix(Prefix::Runtime.as_str()) {
            (Prefix::Runtime, rest)
        } else {
            (Prefix::None, value)
        };

        if !body.ends_with(')') {
            if body.find(['(', ')']).is_some() {
                return Err(ParsePkgVerPeerError::MismatchParenthesis);
            }

            let version = body.parse().map_err(ParsePkgVerPeerError::ParseVersionFailure)?;
            return Ok(PkgVerPeer { prefix, version, peer: String::new() });
        }

        let opening_parenthesis =
            body.find('(').ok_or(ParsePkgVerPeerError::MismatchParenthesis)?;
        let version = body[..opening_parenthesis]
            .parse()
            .map_err(ParsePkgVerPeerError::ParseVersionFailure)?;
        let peer = body[opening_parenthesis..].to_string();
        Ok(PkgVerPeer { prefix, version, peer })
    }
}

impl<'a> TryFrom<Cow<'a, str>> for PkgVerPeer {
    type Error = ParsePkgVerPeerError;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<PkgVerPeer> for String {
    fn from(value: PkgVerPeer) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests;
