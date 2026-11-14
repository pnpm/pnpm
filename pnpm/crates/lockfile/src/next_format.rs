//! The next lockfile format, under `lockfileVersion: '13.0'`: the number of
//! the pnpm major that first writes it by default. See pnpm/rfcs#72.
//!
//! A snapshot is keyed by `name@version`, plus the patch group when there is
//! one, and every reference drops its peer groups. A package installed with
//! several sets of peers lists its snapshots under its key, and each snapshot
//! records its peer bindings as properties. A reference names a version; the
//! referrer's bindings select the snapshot.
//!
//! The in-memory [`Lockfile`] keeps the peer-suffixed keys, so only reading
//! and writing know about this format: [`Lockfile::to_next_sections`] builds
//! the next format's `importers` and `snapshots`, and [`restore_sections`]
//! builds the peer-suffixed ones back.

pub use catalogs::{NextCatalogEntry, NextCatalogs};
pub use importer::{
    Implied, NextImporter, NextImporterDep, NextImporterDeps, NextNamedDep, NextResolvedDep,
};
pub(crate) use linked::LinkedImporters;
pub use package::{NextPackage, NextPackageOut, PeerDetails, PeerEntry};
pub use read::restore_sections;
pub use reference::{NamedBindings, NextRef, names_binding, without_groups};
pub use settings::NextSettings;

pub(crate) use parse::{Shape, declares_next_format};

use crate::{
    Lockfile, LockfileSettings, PackageKey, PackageMetadata, PkgName, ProjectSnapshot,
    SnapshotEntry, implied_version::injected_paths,
};
use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{self, Diagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod catalogs;
mod document;
mod importer;
mod linked;
mod normalize;
mod package;
mod parse;
mod read;
mod reference;
mod settings;
mod snapshot_wire;
mod tarjan;
mod write;

/// The `lockfileVersion` major of the next format: the pnpm major that first
/// writes it by default.
pub const NEXT_LOCKFILE_MAJOR: u16 = 13;

/// The format pnpm writes the lockfile in: the `lockfile.format` setting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LockfileFormat {
    /// Snapshot keys and references carry the peer suffix.
    #[default]
    V9,
    /// Snapshots keyed by name and version, with their peer bindings as
    /// properties, under `lockfileVersion: '13.0'`. Experimental, see
    /// pnpm/rfcs#72.
    V13,
}

impl LockfileFormat {
    /// The `lockfileVersion` a lockfile in this format declares.
    #[must_use]
    pub fn version(self) -> crate::LockfileVersion<9> {
        let major = match self {
            LockfileFormat::V9 => 9,
            LockfileFormat::V13 => NEXT_LOCKFILE_MAJOR,
        };
        crate::LockfileVersion::<9>::try_from(crate::ComVer::new(major, 0))
            .expect("both formats are supported")
    }
}

/// The next format's `snapshots`: every peer snapshot of a package, under the
/// package's key.
pub type NextSnapshots = HashMap<PackageKey, Vec<NextSnapshotEntry>>;

/// One peer snapshot of a package.
///
/// References carry no groups. `dependencies` and `optionalDependencies`
/// hold only the package's own dependencies.
/// Written and read through a wire form that puts both binding maps in one
/// `peerDependencies` map.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(from = "snapshot_wire::SnapshotWire")]
pub struct NextSnapshotEntry {
    pub id: Option<String>,
    pub dependencies: Option<HashMap<PkgName, NextRef>>,
    pub optional_dependencies: Option<HashMap<PkgName, NextRef>>,
    /// The package's own peers that are bound, linked next to it.
    pub peer_dependencies: Option<HashMap<PkgName, NextRef>>,
    /// The peers bound below this package from outside it.
    pub transitive_peer_dependencies: Option<HashMap<PkgName, NextRef>>,
    /// Overrides graph-derived unbound transitive peer names when present.
    pub unbound_peer_dependencies: Option<Vec<String>>,
    pub optional: bool,
}

impl NextSnapshotEntry {
    /// Every peer binding: the package's own, then those from below.
    pub fn bindings(&self) -> impl Iterator<Item = (&PkgName, &NextRef)> {
        self.peer_dependencies
            .iter()
            .chain(self.transitive_peer_dependencies.iter())
            .flatten()
    }

    /// The reference `name` has where this snapshot is linked: a dependency,
    /// a bound peer, or a peer bound below it.
    #[must_use]
    pub fn reference_in_scope(&self, name: &PkgName) -> Option<&NextRef> {
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
    crate::LockfileDocument<NextImporter, serde::de::IgnoredAny, NextPackage, NextSettings>;

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
        .map(|(key, snapshots)| {
            let metadata = packages.and_then(|packages| packages.get(&key.without_peer()));
            let out = NextPackageOut::new(&key, metadata, snapshots);
            (key, out)
        })
        .collect()
}

/// The peer-suffixed `packages` and the next format's `snapshots` an entry
/// map holds.
pub fn split_packages(
    merged: HashMap<PackageKey, NextPackage>,
) -> Result<SplitSections, NextFormatError> {
    use rayon::prelude::*;
    let (packages, snapshots): (Vec<_>, Vec<_>) = merged
        .into_par_iter()
        .map(|(key, package)| {
            let (metadata, snapshots) = package.split(&key)?;
            Ok(((key.without_peer(), metadata), (key, snapshots)))
        })
        .collect::<Result<Vec<_>, NextFormatError>>()?
        .into_iter()
        .unzip();
    Ok((Some(packages.into_iter().collect()), Some(snapshots.into_iter().collect())))
}

/// The peer-suffixed `packages` and the next format's `snapshots`.
pub type SplitSections = (Option<HashMap<PackageKey, PackageMetadata>>, Option<NextSnapshots>);

/// The two sections that differ between the formats.
#[derive(Debug, Clone, PartialEq)]
pub struct NextSections {
    /// Importers whose versions have no groups. A reference the importer's
    /// scope does not select names its snapshot by its bindings.
    pub importers: HashMap<String, NextImporter>,
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
    /// `patchedDependencies`: which packages are patched, with what hash,
    /// since no key or reference carries the patch group.
    pub patched_dependencies: Option<&'a std::collections::BTreeMap<String, String>>,
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

    #[display(
        "The lockfile does not say which version {name} resolves to in {importer}: nothing \
         follows from the specifier {specifier}"
    )]
    #[diagnostic(code(ERR_PNPM_NEXT_LOCKFILE_UNDERIVABLE_VERSION))]
    UnderivableVersion { importer: String, name: String, specifier: String },

    #[display(
        "The lockfile does not say which version {name} resolves to in the catalog {catalog}: \
         nothing follows from the specifier {specifier}"
    )]
    #[diagnostic(code(ERR_PNPM_NEXT_LOCKFILE_UNDERIVABLE_VERSION))]
    UnderivableCatalogVersion { catalog: String, name: String, specifier: String },

    #[display("The lockfile records no resolution for {key}")]
    #[diagnostic(code(ERR_PNPM_NEXT_LOCKFILE_MISSING_RESOLUTION))]
    MissingResolution { key: String },
}

/// Every importer with its versions written out.
fn resolve_importers(
    importers: HashMap<String, NextImporter>,
    implied: Implied<'_>,
) -> Result<HashMap<String, NextImporter>, NextFormatError> {
    importers
        .into_iter()
        .map(|(id, importer)| Ok((importer.resolve_specifiers(&id, implied)?, id)))
        .map(|entry| entry.map(|(importer, id)| (id, importer)))
        .collect()
}

impl Lockfile {
    /// The lockfile a next-format document describes, with peer-suffixed keys.
    ///
    /// Fails when an importer's or a catalog entry's version does not
    /// follow from its specifier.
    pub fn from_next(next: NextLockfile) -> Result<Lockfile, NextFormatError> {
        let (packages, snapshots) = match next.packages {
            Some(merged) => crate::parallel_parse::pool().install(|| split_packages(merged))?,
            None => (None, None),
        };
        let settings = next.settings.unwrap_or_default();
        let catalogs = settings.catalogs.map(catalogs::resolved).transpose()?;
        let injected = injected_paths(packages.iter().flat_map(HashMap::keys));
        let linked = linked::named_importers(&next.importers);
        let implied = Implied { catalogs: catalogs.as_ref(), injected: &injected, linked: &linked };
        let importers = resolve_importers(next.importers, implied)?;
        let sections = NextSections { importers, snapshots };
        let context = RestoreContext {
            packages: packages.as_ref(),
            settings: settings.base.as_ref(),
            patched_dependencies: settings.patched_dependencies.as_ref(),
        };
        let restored =
            crate::parallel_parse::pool().install(|| restore_sections(&sections, context));
        Ok(Lockfile {
            lockfile_version: next.lockfile_version,
            settings: settings.base,
            catalogs,
            overrides: settings.overrides,
            package_extensions_checksum: settings.package_extensions_checksum,
            pnpmfile_checksum: settings.pnpmfile_checksum,
            ignored_optional_dependencies: settings.ignored_optional_dependencies,
            patched_dependencies: settings.patched_dependencies,
            importers: restored.importers,
            packages,
            snapshots: restored.snapshots,
            time: next.time,
            extra: next.extra,
        })
    }

    /// Whether this lockfile is written in the next format.
    #[must_use]
    pub fn is_next_format(&self) -> bool {
        self.lockfile_version.major == NEXT_LOCKFILE_MAJOR
    }

    /// Rebuild peer keys after pruning changes a graph's peer bindings.
    ///
    /// Only snapshot identities and references to them may change. Unequal
    /// snapshots that would acquire the same key are rejected, and an error
    /// leaves this lockfile unchanged. Ordinary next-format writes still
    /// require an exact round trip without normalizing their input.
    pub fn normalize_peer_keys_for_next_format(&mut self) -> Result<(), NextFormatError> {
        let normalized = crate::parallel_parse::pool().install(|| normalize::normalize(self))?;
        self.importers = normalized.importers;
        self.snapshots = normalized.snapshots;
        Ok(())
    }

    /// The `importers` and `snapshots` of the next format.
    ///
    /// Fails when the result would not read back to this lockfile's
    /// sections, so a write never loses a peer snapshot.
    pub fn to_next_sections(&self) -> Result<NextSections, NextFormatError> {
        crate::parallel_parse::pool().install(|| write::next_sections(self))
    }

    /// Render this lockfile as a next-format document.
    pub(crate) fn to_next_yaml_string(&self) -> Result<String, crate::SaveLockfileError> {
        let NextSections { importers, snapshots } =
            self.to_next_sections().map_err(crate::SaveLockfileError::NextFormat)?;
        let packages = merge_packages(self.packages.as_ref(), snapshots);
        let injected = injected_paths(packages.keys());
        let linked = linked::linked_importers(&importers);
        let implied =
            Implied { catalogs: self.catalogs.as_ref(), injected: &injected, linked: &linked };
        let importers = collapse_importers(&importers, implied, &linked);
        crate::serialize_yaml::to_string(&document::NextDocument::new(self, &importers, &packages))
            .map_err(crate::SaveLockfileError::SerializeYaml)
    }
}

/// Every importer with the versions that follow from their specifiers left
/// out, and its `name` where a `workspace:` dependency links to it.
fn collapse_importers(
    importers: &HashMap<String, NextImporter>,
    implied: Implied<'_>,
    linked: &linked::LinkedImporters,
) -> HashMap<String, NextImporter> {
    let names = linked::names(linked);
    importers
        .iter()
        .map(|(id, importer)| {
            let mut collapsed = importer.collapsed(id, implied);
            collapsed.name = names.get(id.as_str()).map(|name| (*name).to_string());
            (id.clone(), collapsed)
        })
        .collect()
}

#[cfg(test)]
mod tests;
