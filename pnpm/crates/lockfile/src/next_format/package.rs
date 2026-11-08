//! One entry of the next format's `packages`: the package's metadata, its
//! resolved dependencies, and its peer bindings, with a `variants` list for a
//! package installed in several peer variants.

use super::NextSnapshotEntry;
use crate::{
    BundledDependencies, LockfileResolution, PackageMetadata, PeerDependencyMeta, PkgName,
    RegistryResolution, SnapshotDepRef, StringOrList, TarballResolution,
};
use serde::{Deserialize, Serialize};
use ssri::Integrity;
use std::collections::HashMap;

/// The metadata fields mirror [`PackageMetadata`], the graph fields
/// [`NextSnapshotEntry`]. The graph fields hold what every variant shares;
/// `variants` holds, per variant, what differs.
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
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<BundledDependencies>,
    /// The peers the package declares, as ranges.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peer_dependencies_meta: Option<HashMap<String, PeerDependencyMeta>>,

    // The graph fields, as in [`NextSnapshotEntry`]. What every variant
    // shares, or the one variant.
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub peers: Option<HashMap<PkgName, SnapshotDepRef>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serialize_yaml::sorted_map_opt"
    )]
    pub transitive_peers: Option<HashMap<PkgName, SnapshotDepRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_peers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patched: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,

    /// What each variant adds to the shared graph fields, when the package
    /// has several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<NextSnapshotEntry>>,
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
            missing_peer_dependencies: self.missing_peers.take(),
            patched: self.patched.take(),
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
    pub deprecated: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_dependencies: Option<&'a BundledDependencies>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<SortedMap<'a, String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies_meta: Option<SortedMap<'a, String, PeerDependencyMeta>>,
    #[serde(flatten)]
    pub shared: NextSnapshotEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<NextSnapshotEntry>>,
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
    /// The entry for `metadata` installed as `variants`, sorted. A map entry
    /// every variant has with the same value is written once, in `shared`.
    #[must_use]
    pub fn new(metadata: Option<&'a PackageMetadata>, variants: Vec<NextSnapshotEntry>) -> Self {
        let (resolution, integrity) = match metadata {
            Some(metadata) => split_resolution(&metadata.resolution),
            None => (None, None),
        };
        let (shared, variants) = hoist(variants);
        NextPackageOut {
            resolution,
            integrity,
            version: metadata.and_then(|metadata| metadata.version.as_ref()),
            engines: metadata.and_then(|metadata| metadata.engines.as_ref()).map(SortedMap),
            cpu: metadata.and_then(|metadata| metadata.cpu.as_ref()),
            os: metadata.and_then(|metadata| metadata.os.as_ref()),
            libc: metadata.and_then(|metadata| metadata.libc.as_ref()),
            deprecated: metadata.and_then(|metadata| metadata.deprecated.as_ref()),
            has_bin: metadata.and_then(|metadata| metadata.has_bin),
            prepare: metadata.and_then(|metadata| metadata.prepare),
            bundled_dependencies: metadata.and_then(|metadata| {
                metadata.bundled_dependencies.as_ref()
            }),
            peer_dependencies: metadata
                .and_then(|metadata| metadata.peer_dependencies.as_ref())
                .map(SortedMap),
            peer_dependencies_meta: metadata
                .and_then(|metadata| metadata.peer_dependencies_meta.as_ref())
                .map(SortedMap),
            shared,
            variants,
        }
    }
}

/// What `variants` share, and what each adds, when there are several.
fn hoist(
    mut variants: Vec<NextSnapshotEntry>,
) -> (NextSnapshotEntry, Option<Vec<NextSnapshotEntry>>) {
    if variants.len() <= 1 {
        return (variants.pop().unwrap_or_default(), None);
    }
    let shared = {
        let (first, rest) = variants.split_first_mut().expect("several variants");
        NextSnapshotEntry {
            id: common_value(first.id.as_ref(), rest.iter().map(|variant| variant.id.as_ref())),
            dependencies: common_entries(first.dependencies.as_ref(), rest, |variant| {
                &variant.dependencies
            }),
            optional_dependencies: common_entries(
                first.optional_dependencies.as_ref(),
                rest,
                |variant| &variant.optional_dependencies,
            ),
            peer_dependencies: common_entries(first.peer_dependencies.as_ref(), rest, |variant| {
                &variant.peer_dependencies
            }),
            transitive_peer_dependencies: common_entries(
                first.transitive_peer_dependencies.as_ref(),
                rest,
                |variant| &variant.transitive_peer_dependencies,
            ),
            missing_peer_dependencies: common_value(
                first.missing_peer_dependencies.as_ref(),
                rest.iter().map(|variant| variant.missing_peer_dependencies.as_ref()),
            ),
            patched: common_value(
                first.patched.as_ref(),
                rest.iter().map(|variant| variant.patched.as_ref()),
            ),
            optional: first.optional && rest.iter().all(|variant| variant.optional),
        }
    };
    for variant in &mut variants {
        variant.subtract(&shared);
    }
    (shared, Some(variants))
}

impl NextPackage {
    /// The metadata and the variants this entry was built from.
    #[must_use]
    pub fn split(mut self) -> (Option<PackageMetadata>, Vec<NextSnapshotEntry>) {
        let shared = self.take_shared();
        let variants = self.variants.take();
        let metadata = self.into_metadata();
        let variants = match variants {
            None => vec![shared],
            Some(variants) => variants
                .into_iter()
                .map(|variant| variant.over(&shared))
                .collect(),
        };
        (metadata, variants)
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
        Some(PackageMetadata {
            resolution,
            version: self.version,
            engines: self.engines,
            cpu: self.cpu,
            os: self.os,
            libc: self.libc,
            deprecated: self.deprecated,
            has_bin: self.has_bin,
            prepare: self.prepare,
            bundled_dependencies: self.bundled_dependencies,
            peer_dependencies: self.peer_dependencies,
            peer_dependencies_meta: self.peer_dependencies_meta,
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
    /// This variant without what `shared` already says.
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
        if shared.missing_peer_dependencies.is_some() {
            self.missing_peer_dependencies = None;
        }
        if shared.patched.is_some() {
            self.patched = None;
        }
        if shared.optional {
            self.optional = false;
        }
    }

    /// This variant with what `shared` says added back.
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
        self.missing_peer_dependencies = self.missing_peer_dependencies.or_else(|| {
            shared.missing_peer_dependencies.clone()
        });
        self.patched = self.patched.or(shared.patched);
        self.optional |= shared.optional;
        self
    }
}

/// `first` when every other variant has the same value.
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

/// The entries of `first` that every other variant has with the same value.
fn common_entries(
    first: Option<&HashMap<PkgName, SnapshotDepRef>>,
    others: &[NextSnapshotEntry],
    map: impl Fn(&NextSnapshotEntry) -> &Option<HashMap<PkgName, SnapshotDepRef>>,
) -> Option<HashMap<PkgName, SnapshotDepRef>> {
    let common: HashMap<PkgName, SnapshotDepRef> = first
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
