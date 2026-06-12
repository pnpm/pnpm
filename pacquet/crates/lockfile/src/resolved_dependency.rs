use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgName, PkgNameVerPeer, PkgVerPeer};
use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{self, Display},
    str::FromStr,
};

/// Map of resolved dependencies stored in a [`ProjectSnapshot`](crate::ProjectSnapshot).
///
/// The keys are package names.
pub type ResolvedDependencyMap = HashMap<PkgName, ResolvedDependencySpec>;

/// Value type of [`ResolvedDependencyMap`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResolvedDependencySpec {
    pub specifier: String,
    pub version: ImporterDepVersion,
}

/// Resolved `version` of an importer-level dependency.
///
/// Importer dependencies (the values inside `importers.<id>.dependencies`
/// in a pnpm v9 lockfile) carry one of three shapes for `version:`:
///
/// - A bare semver-with-peer string like `4.0.0` or `17.0.2(react@17.0.2)`,
///   meaning the dependency is in the shared virtual store under a
///   snapshot key built from the importer-map key and the version.
/// - An npm-alias of the form `<target-name>@<version>` (with optional
///   peer suffix), meaning the dependency resolves to a snapshot whose
///   package name differs from the importer-map key. Pnpm writes this
///   shape when a `catalog:` (or other) specifier resolves to an alias.
///   Detection mirrors upstream's `refToRelative`
///   (`deps/path/src/index.ts` at `pnpm/pnpm@8a80235c7b`): a reference
///   is an alias when it begins with `@` or when the first `@` occurs
///   before any `(` and `:`.
/// - A `link:<path>` value, meaning the dependency is a workspace
///   sibling at `<path>` relative to the importer's `rootDir`. The
///   workspace project is not duplicated in the virtual store — pnpm
///   creates a direct symlink to the sibling's directory.
/// - A `file:<path>` value (with an optional `(peer@suffix)` tail),
///   meaning the dependency is an injected workspace package (or
///   plain tarball / directory `file:` dep) materialised into the
///   virtual store by copy. The path lives verbatim after the
///   `file:` prefix; the peer suffix, when present, identifies a
///   peer-specific snapshot variant in `snapshots:`.
///
/// `ImporterDepVersion` encodes the distinction so consumers (the
/// installer, the build-sequence builder, the reporter) can branch on
/// shape without re-parsing the raw string at every call site.
///
/// Snapshot-level dependencies (the values inside `snapshots.*.dependencies`)
/// use [`crate::SnapshotDepRef`] instead, which carries the same
/// plain / alias / link distinction. `link:` can appear at the snapshot
/// level too, for injected workspace packages whose own dependencies
/// resolve to other workspace projects (see
/// [`crate::SnapshotDepRef::Link`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImporterDepVersion {
    /// Bare semver-with-peer; resolves to a snapshot in `snapshots:`
    /// keyed by `(importer-map key, version)`.
    Regular(PkgVerPeer),

    /// `<name>@<version-with-peer>`; resolves to a snapshot in
    /// `snapshots:` keyed by `(alias.name, alias.suffix)`. The
    /// importer-map key is only used as the directory name inside
    /// `node_modules`.
    Alias(PkgNameVerPeer),

    /// `link:<path>` value; resolves to a workspace sibling. The path
    /// is stored verbatim from the lockfile (relative to the
    /// importer's `rootDir`, or absolute) — interpreting it is the
    /// installer's job, not this layer's.
    Link(String),

    /// `file:<path>` (possibly with a `(peer@suffix)` tail). Mirrors
    /// upstream's importer-level emit for injected workspace
    /// dependencies that didn't dedupe back to `link:`. The full
    /// payload after `file:` is stored verbatim so the
    /// `snapshots:`-level depPath lookup uses the same key as the
    /// snapshot writer.
    File(String),
}

impl ImporterDepVersion {
    /// `Some(ver)` when this dependency resolves through the virtual
    /// store under the importer-map key (no alias rename); `None`
    /// otherwise. Mirrors upstream's `if (depPath.startsWith('link:'))`
    /// checks at the install layer.
    ///
    /// Use [`Self::resolved_key`] when you need a snapshot key that's
    /// also correct for [`Self::Alias`] entries — `as_regular` returns
    /// `None` for aliases, so callers that only handle the regular
    /// branch would silently drop aliased deps.
    #[must_use]
    pub fn as_regular(&self) -> Option<&'_ PkgVerPeer> {
        match self {
            ImporterDepVersion::Regular(v) => Some(v),
            ImporterDepVersion::Alias(_)
            | ImporterDepVersion::Link(_)
            | ImporterDepVersion::File(_) => None,
        }
    }

    /// `Some(alias)` when this dependency resolves through the virtual
    /// store under a name different from the importer-map key; `None`
    /// otherwise.
    #[must_use]
    pub fn as_alias(&self) -> Option<&'_ PkgNameVerPeer> {
        match self {
            ImporterDepVersion::Alias(alias) => Some(alias),
            ImporterDepVersion::Regular(_)
            | ImporterDepVersion::Link(_)
            | ImporterDepVersion::File(_) => None,
        }
    }

    /// `Some(target)` when this dependency is a `link:` sibling;
    /// `None` when it resolves through the virtual store. The
    /// returned string is the path portion *without* the `link:`
    /// prefix.
    #[must_use]
    pub fn as_link_target(&self) -> Option<&'_ str> {
        match self {
            ImporterDepVersion::Regular(_)
            | ImporterDepVersion::Alias(_)
            | ImporterDepVersion::File(_) => None,
            ImporterDepVersion::Link(target) => Some(target.as_str()),
        }
    }

    /// `Some(payload)` when this dependency is an injected `file:` dep;
    /// `None` otherwise. The returned string is the path (plus optional
    /// peer suffix) *without* the `file:` prefix.
    #[must_use]
    pub fn as_file_target(&self) -> Option<&'_ str> {
        match self {
            ImporterDepVersion::File(target) => Some(target.as_str()),
            ImporterDepVersion::Regular(_)
            | ImporterDepVersion::Alias(_)
            | ImporterDepVersion::Link(_) => None,
        }
    }

    /// `Some(key)` with the snapshot-map key this dependency resolves
    /// to; `None` for `link:` siblings. `importer_key` is the key of
    /// the entry in `importers.<id>.dependencies` (the directory name
    /// inside `node_modules`). For [`Self::Regular`] the resolved key
    /// is `(importer_key, version)`; for [`Self::Alias`] it's the
    /// alias's own `(name, suffix)` pair, mirroring upstream's
    /// `refToRelative`. For [`Self::File`] the key is `(importer_key,
    /// file:<payload>)` because the `file:` prefix is part of the
    /// snapshot key in `snapshots:`.
    #[must_use]
    pub fn resolved_key(&self, importer_key: &PkgName) -> Option<PkgNameVerPeer> {
        match self {
            ImporterDepVersion::Regular(ver) => {
                Some(PkgNameVerPeer::new(importer_key.clone(), ver.clone()))
            }
            ImporterDepVersion::Alias(alias) => Some(alias.clone()),
            ImporterDepVersion::File(payload) => format!("file:{payload}")
                .parse::<PkgVerPeer>()
                .ok()
                .map(|ver| PkgNameVerPeer::new(importer_key.clone(), ver)),
            ImporterDepVersion::Link(_) => None,
        }
    }

    /// The version-with-peer portion of this dependency, or `None` for
    /// `link:` / `file:` siblings. For [`Self::Alias`] this returns the
    /// alias's suffix, matching the version present in the snapshot
    /// key.
    #[must_use]
    pub fn ver_peer(&self) -> Option<&'_ PkgVerPeer> {
        match self {
            ImporterDepVersion::Regular(ver) => Some(ver),
            ImporterDepVersion::Alias(alias) => Some(&alias.suffix),
            ImporterDepVersion::Link(_) | ImporterDepVersion::File(_) => None,
        }
    }
}

/// Error when parsing [`ImporterDepVersion`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum ParseImporterDepVersionError {
    #[display("Failed to parse importer dependency version {value:?}: {source}")]
    Parse {
        value: String,
        #[error(source)]
        source: ParsePkgVerPeerError,
    },
    #[display("Failed to parse importer dependency version {value:?}: {source}")]
    ParseAlias {
        value: String,
        #[error(source)]
        source: ParsePkgNameSuffixError<ParsePkgVerPeerError>,
    },
}

/// Returns `true` when `value` is the npm-alias shape `<name>@<version>`
/// rather than a bare semver-with-peer. Mirrors upstream's
/// `refToRelative` (`deps/path/src/index.ts` at
/// `pnpm/pnpm@8a80235c7b`, lines 96-110): an `@` before any `(` or `:`
/// — or a leading `@` — means the reference is a full dep-path with
/// the name encoded in front. The shared helper inside
/// [`crate::SnapshotDepRef`] does the same check; the duplicated copy
/// here keeps the two parsers self-contained.
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

impl FromStr for ImporterDepVersion {
    type Err = ParseImporterDepVersionError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // `link:` keeps the path verbatim; the alias shape parses to
        // `PkgNameVerPeer`; everything else parses as a bare
        // semver-with-peer. The `link:` discriminator is upstream's
        // own — pnpm itself looks for the literal `link:` prefix at
        // install time (see `installDepsResolve` / `lockfileToDepGraph`).
        if let Some(target) = value.strip_prefix("link:") {
            return Ok(ImporterDepVersion::Link(target.to_string()));
        }
        if let Some(target) = value.strip_prefix("file:") {
            return Ok(ImporterDepVersion::File(target.to_string()));
        }
        if looks_like_alias(value) {
            return value.parse::<PkgNameVerPeer>().map(ImporterDepVersion::Alias).map_err(
                |source| ParseImporterDepVersionError::ParseAlias {
                    value: value.to_string(),
                    source,
                },
            );
        }
        value.parse::<PkgVerPeer>().map(ImporterDepVersion::Regular).map_err(|source| {
            ParseImporterDepVersionError::Parse { value: value.to_string(), source }
        })
    }
}

impl<'a> TryFrom<Cow<'a, str>> for ImporterDepVersion {
    type Error = ParseImporterDepVersionError;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ImporterDepVersion> for String {
    fn from(value: ImporterDepVersion) -> Self {
        match value {
            ImporterDepVersion::Regular(v) => v.to_string(),
            ImporterDepVersion::Alias(alias) => alias.to_string(),
            ImporterDepVersion::Link(target) => format!("link:{target}"),
            ImporterDepVersion::File(target) => format!("file:{target}"),
        }
    }
}

impl Serialize for ImporterDepVersion {
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: serde::Serializer,
    {
        match self {
            ImporterDepVersion::Regular(v) => v.serialize(serializer),
            ImporterDepVersion::Alias(alias) => serializer.serialize_str(&alias.to_string()),
            ImporterDepVersion::Link(target) => {
                let formatted = format!("link:{target}");
                serializer.serialize_str(&formatted)
            }
            ImporterDepVersion::File(target) => {
                let formatted = format!("file:{target}");
                serializer.serialize_str(&formatted)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ImporterDepVersion {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

impl From<PkgVerPeer> for ImporterDepVersion {
    fn from(value: PkgVerPeer) -> Self {
        ImporterDepVersion::Regular(value)
    }
}

impl From<PkgNameVerPeer> for ImporterDepVersion {
    fn from(value: PkgNameVerPeer) -> Self {
        ImporterDepVersion::Alias(value)
    }
}

impl Display for ImporterDepVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImporterDepVersion::Regular(v) => Display::fmt(v, f),
            ImporterDepVersion::Alias(alias) => Display::fmt(alias, f),
            ImporterDepVersion::Link(target) => write!(f, "link:{target}"),
            ImporterDepVersion::File(target) => write!(f, "file:{target}"),
        }
    }
}

#[cfg(test)]
mod tests;
