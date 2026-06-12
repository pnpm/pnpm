use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgName, PkgNameVerPeer, PkgVerPeer};
use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, str::FromStr};

/// Value of a single entry in [`SnapshotEntry::dependencies`](crate::SnapshotEntry::dependencies)
/// (or `optional_dependencies`).
///
/// A snapshot dependency can be written in one of three forms:
///
/// * A bare version with an optional peer-dependency suffix — the dependency
///   resolves to `<alias-name>@<version>` in the `snapshots:` map.
///
///   ```yaml
///   '@isaacs/cliui@8.0.2':
///     dependencies:
///       string-width: 5.1.2
///   ```
///
/// * An npm-alias of the form `<target-name>@<version>` — the dependency
///   resolves to `<target-name>@<version>` in the `snapshots:` map and the
///   entry key is only used as the directory name inside `node_modules`.
///
///   ```yaml
///   '@isaacs/cliui@8.0.2':
///     dependencies:
///       string-width-cjs: string-width@4.2.3
///   ```
///
/// * A `link:<path>` value — the dependency is a workspace sibling at
///   `<path>` relative to the lockfile root. Pnpm writes this shape for
///   injected workspace packages whose own dependencies resolve to other
///   workspace projects (and for which `excludeLinksFromLockfile` is off);
///   the dependency lives outside the virtual store and gets a direct
///   directory symlink at install time.
///
///   ```yaml
///   b@file:packages/b:
///     dependencies:
///       c: link:packages/c
///   ```
///
/// Detection mirrors pnpm's `refToRelative`: a reference starting with
/// `link:` short-circuits to the link variant (and `refToRelative` returns
/// `null`); a reference is an alias when a package name appears before the
/// version separator (either the first `@` occurs before any `(` and `:`,
/// or the reference begins with `@`).
///
/// Reference: <https://github.com/pnpm/pnpm/blob/1819226b51/deps/path/src/index.ts>
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Cow<'de, str>", into = "String")]
pub enum SnapshotDepRef {
    Plain(PkgVerPeer),
    Alias(PkgNameVerPeer),
    Link(String),
}

impl SnapshotDepRef {
    /// Resolve this reference to the `snapshots:` / `packages:` key it points
    /// to. `alias_name` is the key of the dependency entry (the name under
    /// which the package is linked into `node_modules`).
    ///
    /// Returns `None` for [`SnapshotDepRef::Link`] entries — `link:` deps
    /// don't live in the virtual store and have no snapshot key. Mirrors
    /// upstream's `refToRelative` returning `null` for `link:` references.
    #[must_use]
    pub fn resolve(&self, alias_name: &PkgName) -> Option<PkgNameVerPeer> {
        match self {
            SnapshotDepRef::Plain(ver_peer) => {
                Some(PkgNameVerPeer::new(alias_name.clone(), ver_peer.clone()))
            }
            SnapshotDepRef::Alias(key) => Some(key.clone()),
            SnapshotDepRef::Link(_) => None,
        }
    }

    /// Accessor for the version-with-peer part of this reference.
    ///
    /// Returns `None` for [`SnapshotDepRef::Link`] entries — a `link:` dep
    /// has no version slot.
    #[must_use]
    pub fn ver_peer(&self) -> Option<&'_ PkgVerPeer> {
        match self {
            SnapshotDepRef::Plain(ver_peer) => Some(ver_peer),
            SnapshotDepRef::Alias(key) => Some(&key.suffix),
            SnapshotDepRef::Link(_) => None,
        }
    }

    /// `Some(target)` when this reference is a `link:` workspace sibling;
    /// `None` otherwise. The returned string is the path portion *without*
    /// the `link:` prefix.
    #[must_use]
    pub fn as_link_target(&self) -> Option<&'_ str> {
        match self {
            SnapshotDepRef::Plain(_) | SnapshotDepRef::Alias(_) => None,
            SnapshotDepRef::Link(target) => Some(target.as_str()),
        }
    }
}

impl std::fmt::Display for SnapshotDepRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotDepRef::Plain(ver_peer) => ver_peer.fmt(f),
            SnapshotDepRef::Alias(key) => key.fmt(f),
            SnapshotDepRef::Link(target) => write!(f, "link:{target}"),
        }
    }
}

/// Returns `true` if `value` looks like an npm-alias reference (i.e. contains
/// a package name before the version separator). See [`SnapshotDepRef`] for
/// the exact rules.
fn looks_like_alias(value: &str) -> bool {
    if value.starts_with('@') {
        return true;
    }
    let Some(at_idx) = value.find('@') else {
        return false;
    };
    let before_paren = value.find('(').is_none_or(|idx| at_idx < idx);
    let before_colon = value.find(':').is_none_or(|idx| at_idx < idx);
    before_paren && before_colon
}

/// Error when parsing [`SnapshotDepRef`] from a string.
#[derive(Debug, Display, Error)]
pub enum ParseSnapshotDepRefError {
    #[display("{_0}")]
    ParsePlain(#[error(source)] ParsePkgVerPeerError),
    #[display("{_0}")]
    ParseAlias(#[error(source)] ParsePkgNameSuffixError<ParsePkgVerPeerError>),
}

impl FromStr for SnapshotDepRef {
    type Err = ParseSnapshotDepRefError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(target) = value.strip_prefix("link:") {
            return Ok(SnapshotDepRef::Link(target.to_string()));
        }
        if looks_like_alias(value) {
            let key =
                value.parse::<PkgNameVerPeer>().map_err(ParseSnapshotDepRefError::ParseAlias)?;
            Ok(SnapshotDepRef::Alias(key))
        } else {
            let ver_peer =
                value.parse::<PkgVerPeer>().map_err(ParseSnapshotDepRefError::ParsePlain)?;
            Ok(SnapshotDepRef::Plain(ver_peer))
        }
    }
}

impl<'a> TryFrom<Cow<'a, str>> for SnapshotDepRef {
    type Error = ParseSnapshotDepRefError;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<SnapshotDepRef> for String {
    fn from(value: SnapshotDepRef) -> Self {
        value.to_string()
    }
}

impl From<PkgVerPeer> for SnapshotDepRef {
    fn from(value: PkgVerPeer) -> Self {
        SnapshotDepRef::Plain(value)
    }
}

#[cfg(test)]
mod tests;
