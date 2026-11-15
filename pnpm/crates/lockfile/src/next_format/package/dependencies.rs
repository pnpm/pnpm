//! The `dependencies` and `optionalDependencies` of a `packages` entry.
//!
//! A directory package records what its manifest declares in these maps:
//!
//! - a string is the declared specifier, and the reference too;
//! - a map gives the `specifier`, the `version` it resolved to, the bindings
//!   that name the snapshot (`with`), and `injected`. Without a `version`,
//!   the snapshots reference the dependency in their own maps, or not at all;
//! - a map without a `specifier` is a reference that no declaration records.
//!
//! A directory package that records no declarations, which an older pnpm
//! wrote, writes `dependencies` even when it is empty, and neither map holds a
//! string or a `specifier`. In every other package a string is a reference.

use super::super::{NamedBindings, NextFormatError, NextRef};
use crate::{DeclaredSpecifiers, PkgName, SnapshotDepRef};
use pnpm_package_manifest::DependencyGroup;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
};

/// One entry of [`PackageDeps`]. The string form is parsed as a reference,
/// which a declared specifier written as a string also is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NextPackageDep {
    Text(SnapshotDepRef),
    Map(Box<NextPackageDepMap>),
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextPackageDepMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<SnapshotDepRef>,
    #[serde(default, rename = "with", skip_serializing_if = "Option::is_none")]
    pub peers: Option<NamedBindings>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub injected: bool,
}

/// A `dependencies` or `optionalDependencies` map of a `packages` entry, by
/// alias. The alias is parsed when the entry is read.
pub type PackageDeps = HashMap<String, NextPackageDep>;

impl<'de> Deserialize<'de> for NextPackageDep {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct DepVisitor;

        impl<'de> Visitor<'de> for DepVisitor {
            type Value = NextPackageDep;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a specifier or a version, or a map of them")
            }

            fn visit_str<Error: de::Error>(self, text: &str) -> Result<NextPackageDep, Error> {
                SnapshotDepRef::try_from(Cow::Borrowed(text))
                    .map(NextPackageDep::Text)
                    .map_err(Error::custom)
            }

            fn visit_map<Map: MapAccess<'de>>(
                self,
                map: Map,
            ) -> Result<NextPackageDep, Map::Error> {
                NextPackageDepMap::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|map| NextPackageDep::Map(Box::new(map)))
            }
        }

        deserializer.deserialize_any(DepVisitor)
    }
}

impl NextPackageDep {
    fn declares(&self) -> bool {
        match self {
            NextPackageDep::Text(_) => true,
            NextPackageDep::Map(map) => map.specifier.is_some(),
        }
    }
}

/// What a written entry's two maps fold in.
#[derive(Clone, Copy)]
pub(super) enum Declarations<'a> {
    /// A package that is not a directory: the maps hold references.
    None,
    /// A directory package without a record.
    Unrecorded,
    Recorded(&'a DeclaredSpecifiers),
}

impl<'a> Declarations<'a> {
    pub(super) fn of(
        directory: bool,
        specifiers: Option<&'a DeclaredSpecifiers>,
    ) -> Declarations<'a> {
        match (directory, specifiers) {
            (false, _) => Declarations::None,
            (true, None) => Declarations::Unrecorded,
            (true, Some(specifiers)) => Declarations::Recorded(specifiers),
        }
    }

    /// The written map of `group`: `references`, with the group's
    /// declarations folded in.
    pub(super) fn written(
        self,
        references: Option<HashMap<PkgName, NextRef>>,
        group: DependencyGroup,
    ) -> Option<PackageDeps> {
        let mut references = references.unwrap_or_default();
        let mut written = PackageDeps::new();
        if let Declarations::Recorded(record) = self {
            for (alias, specifier) in record
                .group(group)
                .into_iter()
                .flatten()
            {
                let reference = PkgName::parse(alias.as_str())
                    .ok()
                    .and_then(|name| references.remove(&name));
                let injected = record.injected
                    .as_ref()
                    .is_some_and(|injected| injected.contains(alias));
                written.insert(alias.clone(), declared_dep(specifier, reference, injected));
            }
        }
        written.extend(
            references
                .into_iter()
                .map(|(alias, reference)| (alias.to_string(), self.reference_dep(reference))),
        );
        let marks_unrecorded =
            matches!(self, Declarations::Unrecorded) && group == DependencyGroup::Prod;
        (marks_unrecorded || !written.is_empty()).then_some(written)
    }
    /// A reference no declaration records: a bare version in a package
    /// that is not a directory, else a map without a `specifier`, so a
    /// reader does not take it for a declaration.
    fn reference_dep(self, reference: NextRef) -> NextPackageDep {
        match (self, reference) {
            (Declarations::None, NextRef::Plain(version)) => NextPackageDep::Text(version),
            (Declarations::Unrecorded | Declarations::Recorded(_), NextRef::Plain(version)) => {
                NextPackageDep::Map(Box::new(NextPackageDepMap {
                    version: Some(version),
                    ..Default::default()
                }))
            }
            (_, NextRef::Named { version, peers }) => {
                NextPackageDep::Map(Box::new(NextPackageDepMap {
                    version: Some(version),
                    peers: Some(peers),
                    ..Default::default()
                }))
            }
        }
    }
}

fn declared_dep(specifier: &str, reference: Option<NextRef>, injected: bool) -> NextPackageDep {
    match reference {
        Some(NextRef::Plain(version)) if !injected && version.to_string() == specifier => {
            NextPackageDep::Text(version)
        }
        reference => {
            let (version, peers) = match reference {
                Some(NextRef::Plain(version)) => (Some(version), None),
                Some(NextRef::Named { version, peers }) => (Some(version), Some(peers)),
                None => (None, None),
            };
            NextPackageDep::Map(Box::new(NextPackageDepMap {
                specifier: Some(specifier.to_string()),
                version,
                peers,
                injected,
            }))
        }
    }
}

/// The two maps of an entry as read: the references, and the declarations a
/// directory package records.
pub(super) struct ReadDeps {
    pub(super) dependencies: Option<HashMap<PkgName, NextRef>>,
    pub(super) optional_dependencies: Option<HashMap<PkgName, NextRef>>,
    pub(super) specifiers: Option<DeclaredSpecifiers>,
}

/// Reads the two maps of the entry `key`, a directory package when
/// `directory` is set.
pub(super) fn read(
    key: &crate::PackageKey,
    directory: bool,
    (dependencies, optional_dependencies): (Option<PackageDeps>, Option<PackageDeps>),
) -> Result<ReadDeps, NextFormatError> {
    let recorded =
        directory && records_declarations(dependencies.as_ref(), optional_dependencies.as_ref());
    let mut reader = Reader { key, recorded, injected: BTreeSet::new() };
    let (dependencies, declared) = reader.group(dependencies)?;
    let (optional_dependencies, optional_declared) = reader.group(optional_dependencies)?;
    let specifiers = recorded.then(|| DeclaredSpecifiers {
        dependencies: declared,
        optional_dependencies: optional_declared,
        injected: (!reader.injected.is_empty()).then_some(reader.injected),
    });
    Ok(ReadDeps { dependencies, optional_dependencies, specifiers })
}

/// Whether a directory package's two maps record its declarations: see the
/// module documentation.
fn records_declarations(
    dependencies: Option<&PackageDeps>,
    optional_dependencies: Option<&PackageDeps>,
) -> bool {
    let mut entries = dependencies
        .into_iter()
        .chain(optional_dependencies)
        .flatten();
    (dependencies.is_none() && optional_dependencies.is_none())
        || entries.any(|(_, dep)| dep.declares())
}

/// The references and declarations of one map as read.
type ReadGroup = (Option<HashMap<PkgName, NextRef>>, Option<HashMap<String, String>>);

struct Reader<'a> {
    key: &'a crate::PackageKey,
    recorded: bool,
    injected: BTreeSet<String>,
}

impl Reader<'_> {
    fn group(&mut self, deps: Option<PackageDeps>) -> Result<ReadGroup, NextFormatError> {
        let mut references = HashMap::new();
        let mut declared = HashMap::new();
        for (alias, dep) in deps.into_iter().flatten() {
            let (name, read) = self.entry(&alias, dep)?;
            if read.injected {
                self.injected.insert(alias.clone());
            }
            if let Some(specifier) = read.specifier {
                declared.insert(alias, specifier);
            }
            if let Some(reference) = read.reference {
                references.insert(name, reference);
            }
        }
        Ok((
            (!references.is_empty()).then_some(references),
            (!declared.is_empty()).then_some(declared),
        ))
    }

    fn entry(
        &self,
        alias: &str,
        dep: NextPackageDep,
    ) -> Result<(PkgName, ReadDep), NextFormatError> {
        let invalid = |detail: &str| NextFormatError::InvalidPackageDependency {
            key: self.key.to_string(),
            name: alias.to_string(),
            detail: detail.to_string(),
        };
        let name = PkgName::parse(alias).map_err(|_| invalid("not a package name"))?;
        let read = read_dep(dep, self.recorded).map_err(invalid)?;
        Ok((name, read))
    }
}

struct ReadDep {
    specifier: Option<String>,
    reference: Option<NextRef>,
    injected: bool,
}

/// One entry as read, `declared` when its entry records declarations.
fn read_dep(dep: NextPackageDep, declared: bool) -> Result<ReadDep, &'static str> {
    let map = match dep {
        NextPackageDep::Text(version) => {
            return Ok(ReadDep {
                specifier: declared.then(|| version.to_string()),
                reference: Some(NextRef::Plain(version)),
                injected: false,
            });
        }
        NextPackageDep::Map(map) => *map,
    };
    if !declared && (map.specifier.is_some() || map.injected) {
        return Err("a specifier in a package that records none");
    }
    let reference = match (map.version, map.peers) {
        (Some(version), None) => Some(NextRef::Plain(version)),
        (Some(version), Some(peers)) => Some(NextRef::Named { version, peers }),
        (None, None) if map.specifier.is_some() => None,
        (None, _) => return Err("no version"),
    };
    Ok(ReadDep { specifier: map.specifier, reference, injected: map.injected })
}
