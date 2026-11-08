//! The next lockfile format, written under `lockfileVersion: '10.0'` while it
//! is experimental. See pnpm/rfcs#72.
//!
//! A snapshot is keyed by `name@version`, plus the patch group when there is
//! one, and every reference drops its peer groups. A package installed in
//! several peer variants has a list of snapshots under its key, and each
//! snapshot records its peer bindings as properties. A reference names a
//! version; the referrer's bindings select the variant.
//!
//! The in-memory [`Lockfile`] keeps the peer-suffixed keys, so only reading
//! and writing know about this format: [`Lockfile::to_next_sections`] builds
//! the next format's `importers` and `snapshots`, and [`restore_sections`]
//! builds the peer-suffixed ones back.

pub use package::{NextPackage, NextPackageOut};
pub use read::restore_sections;

pub(crate) use parse::{Shape, declares_next_format};

use crate::{
    Lockfile, LockfileSettings, PackageKey, PackageMetadata, PkgName, ProjectSnapshot,
    SnapshotDepRef, SnapshotEntry,
};
use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{self, Diagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod document;
mod package;
mod parse;
mod read;
mod tarjan;
mod write;

/// The `lockfileVersion` major of the next format.
pub const NEXT_LOCKFILE_MAJOR: u16 = 10;

/// The format pnpm writes the lockfile in: the `lockfile.format` setting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LockfileFormat {
    /// Snapshot keys and references carry the peer suffix.
    #[default]
    V9,
    /// Snapshots keyed by name and version, with their peer bindings as
    /// properties. Experimental, see pnpm/rfcs#72.
    Next,
}

impl LockfileFormat {
    /// The `lockfileVersion` a lockfile in this format declares.
    #[must_use]
    pub fn version(self) -> crate::LockfileVersion<9> {
        let major = match self {
            LockfileFormat::V9 => 9,
            LockfileFormat::Next => NEXT_LOCKFILE_MAJOR,
        };
        crate::LockfileVersion::<9>::try_from(crate::ComVer::new(major, 0))
            .expect("both formats are supported")
    }
}

/// The next format's `snapshots`: every peer variant of a package, under the
/// package's key.
pub type NextSnapshots = HashMap<PackageKey, Vec<NextSnapshotEntry>>;

/// One peer variant of a package.
///
/// References carry no peer groups. `dependencies` and `optionalDependencies`
/// hold only the package's own dependencies.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSnapshotEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,

    /// The package's own peers that are bound, linked next to it.
    #[serde(
        default,
        rename = "peers",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,

    /// The peers bound below this package from outside it.
    #[serde(
        default,
        rename = "transitivePeers",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub transitive_peer_dependencies: Option<HashMap<PkgName, SnapshotDepRef>>,

    /// The peers needed below this package that nothing provides.
    #[serde(default, rename = "missingPeers", skip_serializing_if = "Option::is_none")]
    pub missing_peer_dependencies: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patched: Option<bool>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

impl NextSnapshotEntry {
    /// Every peer binding: the package's own, then those from below.
    pub fn bindings(&self) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
        self.peer_dependencies
            .iter()
            .chain(self.transitive_peer_dependencies.iter())
            .flatten()
    }

    /// The reference `name` has where this snapshot is linked: a dependency,
    /// a bound peer, or a peer bound below it.
    #[must_use]
    pub fn reference_in_scope(&self, name: &PkgName) -> Option<&SnapshotDepRef> {
        [
            &self.dependencies,
            &self.optional_dependencies,
            &self.peer_dependencies,
            &self.transitive_peer_dependencies,
        ]
        .into_iter()
        .find_map(|map| map.as_ref()?.get(name))
    }
}

/// A lockfile in the next format, as read from the file. It has no
/// `snapshots`: each `packages` entry carries its resolved graph.
pub type NextLockfile =
    crate::LockfileDocument<ProjectSnapshot, serde::de::IgnoredAny, NextPackage>;

/// The next format's `packages`, built from the peer-suffixed `packages` and
/// the converted `snapshots`. A `packages` entry no snapshot uses is not
/// written: nothing can reach it, and an entry with metadata only would read
/// back as a package with an empty snapshot.
#[must_use]
pub fn merge_packages(
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    snapshots: Option<NextSnapshots>,
) -> HashMap<PackageKey, NextPackageOut<'_>> {
    snapshots
        .into_iter()
        .flatten()
        .map(|(key, variants)| {
            let metadata = packages.and_then(|packages| packages.get(&key.without_peer()));
            (key, NextPackageOut::new(metadata, variants))
        })
        .collect()
}

/// The peer-suffixed `packages` and the next format's `snapshots` an entry
/// map holds.
#[must_use]
pub fn split_packages(
    merged: HashMap<PackageKey, NextPackage>,
) -> (Option<HashMap<PackageKey, PackageMetadata>>, Option<NextSnapshots>) {
    use rayon::prelude::*;
    let (packages, snapshots): (Vec<_>, Vec<_>) = merged
        .into_par_iter()
        .map(|(key, package)| {
            let (metadata, variants) = package.split();
            (metadata.map(|metadata| (key.without_peer(), metadata)), (key, variants))
        })
        .unzip();
    (Some(packages.into_iter().flatten().collect()), Some(snapshots.into_iter().collect()))
}

/// The two sections that differ between the formats.
#[derive(Debug, Clone, PartialEq)]
pub struct NextSections {
    /// Importers whose versions have no peer groups, except where the
    /// importer's scope does not select the variant.
    pub importers: HashMap<String, ProjectSnapshot>,
    pub snapshots: Option<NextSnapshots>,
}

/// The peer-suffixed sections [`restore_sections`] builds.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredSections {
    pub importers: HashMap<String, ProjectSnapshot>,
    pub snapshots: Option<HashMap<PackageKey, SnapshotEntry>>,
}

/// What [`restore_sections`] reads besides the two sections.
#[derive(Debug, Clone, Copy)]
pub struct RestoreContext<'a> {
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub settings: Option<&'a LockfileSettings>,
}

/// Why a lockfile cannot be converted between the formats.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum NextFormatError {
    #[display("The peer name {name} recorded by {referrer} is not a valid package name")]
    #[diagnostic(code(ERR_PNPM_NEXT_LOCKFILE_INVALID_PEER_NAME))]
    InvalidPeerName { referrer: String, name: String },

    #[display(
        "The lockfile cannot be written in the next format, because reading it back would \
         differ: {detail}"
    )]
    #[diagnostic(code(ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE))]
    NotRepresentable { detail: String },
}

impl Lockfile {
    /// The lockfile a next-format document describes, with peer-suffixed keys.
    #[must_use]
    pub fn from_next(next: NextLockfile) -> Lockfile {
        let (packages, snapshots) = match next.packages {
            Some(merged) => crate::parallel_parse::pool().install(|| split_packages(merged)),
            None => (None, None),
        };
        let sections = NextSections { importers: next.importers, snapshots };
        let context =
            RestoreContext { packages: packages.as_ref(), settings: next.settings.as_ref() };
        let restored =
            crate::parallel_parse::pool().install(|| restore_sections(&sections, context));
        Lockfile {
            lockfile_version: next.lockfile_version,
            settings: next.settings,
            catalogs: next.catalogs,
            overrides: next.overrides,
            package_extensions_checksum: next.package_extensions_checksum,
            pnpmfile_checksum: next.pnpmfile_checksum,
            ignored_optional_dependencies: next.ignored_optional_dependencies,
            patched_dependencies: next.patched_dependencies,
            importers: restored.importers,
            packages,
            snapshots: restored.snapshots,
            time: next.time,
            extra: next.extra,
        }
    }

    /// Whether this lockfile is written in the next format.
    #[must_use]
    pub fn is_next_format(&self) -> bool {
        self.lockfile_version.major == NEXT_LOCKFILE_MAJOR
    }

    /// The `importers` and `snapshots` of the next format.
    ///
    /// Fails when the result would not read back to this lockfile's
    /// sections, so a write never loses a peer variant.
    pub fn to_next_sections(&self) -> Result<NextSections, NextFormatError> {
        crate::parallel_parse::pool().install(|| write::next_sections(self))
    }

    /// Render this lockfile as a next-format document.
    pub(crate) fn to_next_yaml_string(&self) -> Result<String, crate::SaveLockfileError> {
        let NextSections { importers, snapshots } =
            self.to_next_sections().map_err(crate::SaveLockfileError::NextFormat)?;
        let packages = merge_packages(self.packages.as_ref(), snapshots);
        crate::serialize_yaml::to_string(&document::NextDocument::new(self, &importers, &packages))
            .map_err(crate::SaveLockfileError::SerializeYaml)
    }
}

#[cfg(test)]
mod tests;
