//! One entry of the next format's `packages`: the package's metadata, its
//! resolved dependencies, and its peer bindings, with a `snapshots` list for a
//! package installed with several sets of peers.

pub use peers::{NextPeerDependency, NextPeerDependencyOut};

use super::{NextRef, NextSnapshotEntry};
use crate::{
    BundledDependencies, LockfileResolution, PackageMetadata, PkgName, RegistryResolution,
    StringOrList, TarballResolution,
};
use peers::{merge_peer_dependencies, split_peer_dependencies};
use serde::{Deserialize, Serialize};
use ssri::Integrity;
use std::collections::{BTreeMap, HashMap};

mod peers;

/// The metadata fields mirror [`PackageMetadata`], the graph fields
/// [`NextSnapshotEntry`]. The graph fields hold what every snapshot shares;
/// `snapshots` holds, per snapshot, what differs.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the pnpm-lock.yaml format."
    )
)]
pub struct NextPackage {
    /// The resolution without its integrity, and absent for a registry
    /// package without a revision, the common case, whose resolution is the
    /// integrity alone. Also absent for a package whose metadata the lockfile
    /// does not record, such as a directory dependency written by an older
    /// pnpm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<LockfileResolution>,
    /// The integrity of the package's tarball, whatever its resolution:
    /// every resolution kind that has one records it here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub engines: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<StringOrList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<BundledDependencies>,
    /// The peers the package declares, with `peerDependenciesMeta` folded in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies: Option<HashMap<String, NextPeerDependency>>,

    // The graph fields, as in [`NextSnapshotEntry`]. What every snapshot
    // shares, or the one snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub dependencies: Option<HashMap<PkgName, NextRef>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub optional_dependencies: Option<HashMap<PkgName, NextRef>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peers: Option<HashMap<PkgName, NextRef>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub transitive_peers: Option<HashMap<PkgName, NextRef>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,

    /// What each snapshot adds to the shared graph fields, when the package
    /// has several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<Vec<NextSnapshotEntry>>,
}

impl NextPackage {
    /// The shared graph fields, moved out.
    fn take_shared(&mut self) -> NextSnapshotEntry {
        NextSnapshotEntry {
            id: self.id.take(),
            dependencies: self.dependencies.take(),
            optional_dependencies: self.optional_dependencies.take(),
            peer_dependencies: self.peers.take(),
            transitive_peer_dependencies: self.transitive_peers.take(),
            optional: self.optional,
        }
    }
}

/// [`NextPackage`] borrowed from the lockfile it is written from, so a write
/// clones no metadata. The serde attributes mirror [`NextPackage`]'s.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the pnpm-lock.yaml format."
    )
)]
pub struct NextPackageOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<std::borrow::Cow<'a, LockfileResolution>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<&'a Integrity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<SortedMap<'a, String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<&'a Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<&'a Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<&'a StringOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<&'a BundledDependencies>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<BTreeMap<&'a str, NextPeerDependencyOut<'a>>>,
    #[serde(flatten)]
    pub shared: NextSnapshotEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<Vec<NextSnapshotEntry>>,
}

/// A borrowed map written with its keys sorted.
pub struct SortedMap<'a, Key, Value>(&'a HashMap<Key, Value>);

impl<Key, Value> Serialize for SortedMap<'_, Key, Value>
where
    Key: Serialize + std::fmt::Display,
    Value: Serialize + Sync,
{
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        crate::serialize_yaml::sorted_map(self.0, serializer)
    }
}

impl<'a> NextPackageOut<'a> {
    /// The entry for `metadata` installed as `snapshots`, sorted. A map entry
    /// every snapshot has with the same value is written once, in `shared`.
    #[must_use]
    pub fn new(metadata: Option<&'a PackageMetadata>, snapshots: Vec<NextSnapshotEntry>) -> Self {
        let (resolution, integrity) = match metadata {
            Some(metadata) => split_resolution(&metadata.resolution),
            None => (None, None),
        };
        let (shared, snapshots) = hoist(snapshots);
        NextPackageOut {
            resolution,
            integrity,
            version: metadata.and_then(|metadata| metadata.version.as_ref()),
            engines: metadata.and_then(|metadata| metadata.engines.as_ref()).map(SortedMap),
            cpu: metadata.and_then(|metadata| metadata.cpu.as_ref()),
            os: metadata.and_then(|metadata| metadata.os.as_ref()),
            libc: metadata.and_then(|metadata| metadata.libc.as_ref()),
            has_bin: metadata.and_then(|metadata| metadata.has_bin),
            prepare: metadata.and_then(|metadata| metadata.prepare),
            bundled_dependencies: metadata.and_then(|metadata| {
                metadata.bundled_dependencies.as_ref()
            }),
            peer_dependencies: metadata.and_then(|metadata| {
                merge_peer_dependencies(
                    metadata.peer_dependencies.as_ref(),
                    metadata.peer_dependencies_meta.as_ref(),
                )
            }),
            shared,
            snapshots,
        }
    }
}

/// What `snapshots` share, and what each adds, when there are several.
fn hoist(
    mut snapshots: Vec<NextSnapshotEntry>,
) -> (NextSnapshotEntry, Option<Vec<NextSnapshotEntry>>) {
    if snapshots.len() <= 1 {
        return (snapshots.pop().unwrap_or_default(), None);
    }
    let shared = {
        let (first, rest) = snapshots.split_first_mut().expect("several snapshots");
        NextSnapshotEntry {
            id: common_value(first.id.as_ref(), rest.iter().map(|snapshot| snapshot.id.as_ref())),
            dependencies: common_entries(first.dependencies.as_ref(), rest, |snapshot| {
                &snapshot.dependencies
            }),
            optional_dependencies: common_entries(
                first.optional_dependencies.as_ref(),
                rest,
                |snapshot| &snapshot.optional_dependencies,
            ),
            peer_dependencies: common_entries(first.peer_dependencies.as_ref(), rest, |snapshot| {
                &snapshot.peer_dependencies
            }),
            transitive_peer_dependencies: common_entries(
                first.transitive_peer_dependencies.as_ref(),
                rest,
                |snapshot| &snapshot.transitive_peer_dependencies,
            ),
            optional: first.optional && rest.iter().all(|snapshot| snapshot.optional),
        }
    };
    for snapshot in &mut snapshots {
        snapshot.subtract(&shared);
    }
    (shared, Some(snapshots))
}

impl NextPackage {
    /// The metadata and the snapshots this entry was built from.
    #[must_use]
    pub fn split(mut self) -> (Option<PackageMetadata>, Vec<NextSnapshotEntry>) {
        let shared = self.take_shared();
        let snapshots = self.snapshots.take();
        let metadata = self.into_metadata();
        let snapshots = match snapshots {
            None => vec![shared],
            Some(snapshots) => snapshots
                .into_iter()
                .map(|snapshot| snapshot.over(&shared))
                .collect(),
        };
        (metadata, snapshots)
    }

    /// `None` when the entry records no resolution.
    fn into_metadata(self) -> Option<PackageMetadata> {
        let resolution = match (self.resolution, self.integrity) {
            (None, Some(integrity)) => {
                LockfileResolution::Registry(RegistryResolution { integrity, revision: None })
            }
            (Some(LockfileResolution::Tarball(tarball)), Some(integrity)) => {
                LockfileResolution::Tarball(TarballResolution {
                    integrity: Some(tarball.integrity.unwrap_or(integrity)),
                    ..tarball
                })
            }
            (Some(resolution), _) => resolution,
            (None, None) => return None,
        };
        let (peer_dependencies, peer_dependencies_meta) =
            split_peer_dependencies(self.peer_dependencies);
        Some(PackageMetadata {
            resolution,
            version: self.version,
            engines: self.engines,
            cpu: self.cpu,
            os: self.os,
            libc: self.libc,
            deprecated: None,
            has_bin: self.has_bin,
            prepare: self.prepare,
            bundled_dependencies: self.bundled_dependencies,
            peer_dependencies,
            peer_dependencies_meta,
        })
    }
}

/// The resolution as the entry writes it, with the integrity apart.
fn split_resolution(
    resolution: &LockfileResolution,
) -> (Option<std::borrow::Cow<'_, LockfileResolution>>, Option<&Integrity>) {
    match resolution {
        LockfileResolution::Registry(RegistryResolution { integrity, revision: None }) => {
            (None, Some(integrity))
        }
        LockfileResolution::Tarball(tarball) if tarball.integrity.is_some() => (
            Some(std::borrow::Cow::Owned(LockfileResolution::Tarball(TarballResolution {
                integrity: None,
                ..tarball.clone()
            }))),
            tarball.integrity.as_ref(),
        ),
        other => (Some(std::borrow::Cow::Borrowed(other)), None),
    }
}

impl NextSnapshotEntry {
    /// This snapshot without what `shared` already says.
    fn subtract(&mut self, shared: &NextSnapshotEntry) {
        for (map, shared_map) in [
            (&mut self.dependencies, &shared.dependencies),
            (&mut self.optional_dependencies, &shared.optional_dependencies),
            (&mut self.peer_dependencies, &shared.peer_dependencies),
            (&mut self.transitive_peer_dependencies, &shared.transitive_peer_dependencies),
        ] {
            if let (Some(own), Some(common)) = (map.as_mut(), shared_map) {
                own.retain(|name, _| !common.contains_key(name));
            }
            if map.as_ref().is_some_and(HashMap::is_empty) {
                *map = None;
            }
        }
        if shared.id.is_some() {
            self.id = None;
        }
        if shared.optional {
            self.optional = false;
        }
    }

    /// This snapshot with what `shared` says added back.
    fn over(mut self, shared: &NextSnapshotEntry) -> NextSnapshotEntry {
        for (map, shared_map) in [
            (&mut self.dependencies, &shared.dependencies),
            (&mut self.optional_dependencies, &shared.optional_dependencies),
            (&mut self.peer_dependencies, &shared.peer_dependencies),
            (&mut self.transitive_peer_dependencies, &shared.transitive_peer_dependencies),
        ] {
            if let Some(common) = shared_map {
                map.get_or_insert_default()
                    .extend(
                        common
                            .iter()
                            .map(|(name, reference)| (name.clone(), reference.clone())),
                    );
            }
        }
        self.id = self.id.or_else(|| shared.id.clone());
        self.optional |= shared.optional;
        self
    }
}

/// `first` when every other snapshot has the same value.
fn common_value<'a, Value: PartialEq + Clone + 'a>(
    first: Option<&Value>,
    others: impl Iterator<Item = Option<&'a Value>>,
) -> Option<Value> {
    let first = first?;
    let mut others = others;
    others
        .all(|other| other == Some(first))
        .then(|| first.clone())
}

/// The entries of `first` that every other snapshot has with the same value.
fn common_entries(
    first: Option<&HashMap<PkgName, NextRef>>,
    others: &[NextSnapshotEntry],
    map: impl Fn(&NextSnapshotEntry) -> &Option<HashMap<PkgName, NextRef>>,
) -> Option<HashMap<PkgName, NextRef>> {
    let common: HashMap<PkgName, NextRef> = first
        .into_iter()
        .flatten()
        .filter(|(name, reference)| {
            others
                .iter()
                .all(|other| {
                    map(other)
                        .as_ref()
                        .and_then(|entries| entries.get(*name))
                        == Some(*reference)
                })
        })
        .map(|(name, reference)| (name.clone(), reference.clone()))
        .collect();
    (!common.is_empty()).then_some(common)
}
