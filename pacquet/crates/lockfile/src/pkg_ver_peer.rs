use derive_more::{Display, Error};
use node_semver::{SemverError, Version};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt, str::FromStr};

/// Version slot of a [`PkgVerPeer`]: a semver, the raw path of an
/// injected workspace `file:<path>` dep, or an opaque non-semver
/// reference (typically a tarball or git URL). Mirrors pnpm's
/// `parseDepPath` `nonSemverVersion` arm in `packages/deps.path/src`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionPart {
    Semver(Version),
    /// Path portion of a `file:<path>` dep, scheme stripped.
    File(String),
    /// Non-semver reference preserved verbatim. Pnpm writes the raw
    /// reference (e.g. a `https://codeload.github.com/...` tarball URL,
    /// a `git+...` URL, or a custom resolution id) into the version
    /// slot of importer entries and `packages:` / `snapshots:` keys
    /// when no semver applies. Pacquet preserves the string so the
    /// lockfile round-trips byte-for-byte and downstream resolvers
    /// can inspect it.
    NonSemver(String),
}

impl fmt::Display for VersionPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionPart::Semver(version) => version.fmt(f),
            VersionPart::File(path) => write!(f, "file:{path}"),
            VersionPart::NonSemver(raw) => f.write_str(raw),
        }
    }
}

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
    version: VersionPart,
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
    #[must_use]
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
    #[must_use]
    pub fn version(&self) -> &'_ VersionPart {
        &self.version
    }

    /// Semver `Version` if this is a [`VersionPart::Semver`], else
    /// `None`. Use from call sites that need `major` / `minor` /
    /// `patch` — Display via [`PkgVerPeer::version`] covers the rest.
    #[must_use]
    pub fn version_semver(&self) -> Option<&'_ Version> {
        match &self.version {
            VersionPart::Semver(version) => Some(version),
            VersionPart::File(_) | VersionPart::NonSemver(_) => None,
        }
    }

    /// Get the peer part.
    #[must_use]
    pub fn peer(&self) -> &'_ str {
        self.peer.as_str()
    }

    /// Get the prefix variant.
    #[must_use]
    pub fn prefix(&self) -> Prefix {
        self.prefix
    }

    /// Destructure the struct into a tuple of version and peer.
    /// The prefix (if any) is dropped — keep the call shape
    /// backward-compatible with pre-runtime consumers. New callers
    /// that need the prefix should access it via
    /// [`PkgVerPeer::prefix`] before destructuring.
    #[must_use]
    pub fn into_tuple(self) -> (VersionPart, String) {
        let PkgVerPeer { prefix: _, version, peer } = self;
        (version, peer)
    }

    /// Return a copy with the peer-dependency suffix cleared.
    ///
    /// The result preserves the original `prefix` and `version` slots
    /// byte-for-byte, so no parse-and-render round-trip runs. Callers
    /// that need the metadata-map key for a peer-variant snapshot key
    /// should reach for this instead of stringifying and re-parsing —
    /// some legitimately-resolved snapshot keys carry a `version` slot
    /// (e.g. a workspace `link:<rel-path>(peer@x)` shape under
    /// `linkWorkspacePackages: true`) whose `Display` form would not
    /// re-parse as a [`PkgVerPeer`].
    #[must_use]
    pub fn without_peer(&self) -> PkgVerPeer {
        PkgVerPeer { prefix: self.prefix, version: self.version.clone(), peer: String::new() }
    }
}

/// Error when parsing [`PkgVerPeer`] from a string.
#[derive(Debug, Display, Error)]
pub enum ParsePkgVerPeerError {
    #[display("Failed to parse the version part: {_0}")]
    ParseVersionFailure(#[error(source)] SemverError),
    #[display("Mismatch parenthesis")]
    MismatchParenthesis,
    #[display("Empty path after `file:` scheme")]
    EmptyFilePath,
    #[display("`runtime:` and `file:` schemes are mutually exclusive")]
    ConflictingSchemes,
}

fn parse_version_part(input: &str) -> Result<VersionPart, ParsePkgVerPeerError> {
    if let Some(path) = input.strip_prefix("file:") {
        if path.is_empty() {
            return Err(ParsePkgVerPeerError::EmptyFilePath);
        }
        return Ok(VersionPart::File(path.to_string()));
    }
    // Upstream's `parse` in `deps/path/src/index.ts` (pnpm@1819226b51)
    // falls back to `nonSemverVersion` whenever `semver.valid` rejects
    // the version, so tarball / git URLs and other custom resolution
    // ids in the version slot still parse. Mirror that here — only an
    // empty body is a hard error.
    match input.parse::<Version>() {
        Ok(version) => Ok(VersionPart::Semver(version)),
        Err(err) if input.is_empty() => Err(ParsePkgVerPeerError::ParseVersionFailure(err)),
        Err(_) => Ok(VersionPart::NonSemver(input.to_string())),
    }
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

        // Pnpm never writes `runtime:file:...`; rejecting it here keeps
        // [`PkgVerPeer::to_string`] byte-stable (a silent acceptance would
        // round-trip back as `file:...` with the prefix dropped).
        if prefix == Prefix::Runtime && body.starts_with("file:") {
            return Err(ParsePkgVerPeerError::ConflictingSchemes);
        }

        if !body.ends_with(')') {
            if body.find(['(', ')']).is_some() {
                return Err(ParsePkgVerPeerError::MismatchParenthesis);
            }

            let version = parse_version_part(body)?;
            return Ok(PkgVerPeer { prefix, version, peer: String::new() });
        }

        let opening_parenthesis =
            body.find('(').ok_or(ParsePkgVerPeerError::MismatchParenthesis)?;
        let version = parse_version_part(&body[..opening_parenthesis])?;
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
