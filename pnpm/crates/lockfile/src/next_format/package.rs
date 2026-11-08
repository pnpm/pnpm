//! One entry of the next format's `packages`: the package's metadata, its
//! resolved dependencies, and its peer bindings, with a `variants` list for a
//! package installed in several peer variants.

use super::NextSnapshotEntry;
use crate::{
    BundledDependencies, LockfileResolution, PackageMetadata, PeerDependencyMeta, PkgName,
    SnapshotDepRef, StringOrList,
};
use serde::{Deserialize, Serialize};
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
    /// Absent for a package whose metadata the lockfile does not record, such
    /// as a directory dependency written by an older pnpm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<LockfileResolution>,
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
    /// The shared graph fields as one entry.
    fn shared(&self) -> NextSnapshotEntry {
        NextSnapshotEntry {
            id: self.id.clone(),
            dependencies: self.dependencies.clone(),
            optional_dependencies: self.optional_dependencies.clone(),
            peer_dependencies: self.peers.clone(),
            transitive_peer_dependencies: self.transitive_peers.clone(),
            missing_peer_dependencies: self.missing_peers.clone(),
            patched: self.patched,
            optional: self.optional,
        }
    }

    fn set_shared(&mut self, shared: NextSnapshotEntry) {
        self.id = shared.id;
        self.dependencies = shared.dependencies;
        self.optional_dependencies = shared.optional_dependencies;
        self.peers = shared.peer_dependencies;
        self.transitive_peers = shared.transitive_peer_dependencies;
        self.missing_peers = shared.missing_peer_dependencies;
        self.patched = shared.patched;
        self.optional = shared.optional;
    }
}

impl NextPackage {
    /// The entry for `metadata` installed as `variants`, sorted. A map entry
    /// every variant has with the same value is written once, in `shared`.
    #[must_use]
    pub fn hoisted(
        metadata: Option<&PackageMetadata>,
        mut variants: Vec<NextSnapshotEntry>,
    ) -> Self {
        let mut package = metadata.map(NextPackage::from).unwrap_or_default();
        if variants.len() <= 1 {
            package.set_shared(variants.pop().unwrap_or_default());
            return package;
        }
        let (first, rest) = variants.split_first_mut().expect("several variants");
        let shared = NextSnapshotEntry {
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
        };
        for variant in &mut variants {
            variant.subtract(&shared);
        }
        package.set_shared(shared);
        package.variants = Some(variants);
        package
    }

    /// The metadata and the variants this entry was built from.
    #[must_use]
    pub fn split(self) -> (Option<PackageMetadata>, Vec<NextSnapshotEntry>) {
        let shared = self.shared();
        let variants = self.variants.clone();
        let metadata = self.resolution.is_some().then(|| self.into_metadata());
        let variants = match variants {
            None => vec![shared],
            Some(variants) => variants
                .into_iter()
                .map(|variant| variant.over(&shared))
                .collect(),
        };
        (metadata, variants)
    }

    fn into_metadata(self) -> PackageMetadata {
        PackageMetadata {
            resolution: self.resolution.expect("checked by the caller"),
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
        }
    }
}

impl From<&PackageMetadata> for NextPackage {
    fn from(metadata: &PackageMetadata) -> Self {
        NextPackage {
            resolution: Some(metadata.resolution.clone()),
            version: metadata.version.clone(),
            engines: metadata.engines.clone(),
            cpu: metadata.cpu.clone(),
            os: metadata.os.clone(),
            libc: metadata.libc.clone(),
            deprecated: metadata.deprecated.clone(),
            has_bin: metadata.has_bin,
            prepare: metadata.prepare,
            bundled_dependencies: metadata.bundled_dependencies.clone(),
            peer_dependencies: metadata.peer_dependencies.clone(),
            peer_dependencies_meta: metadata.peer_dependencies_meta.clone(),
            id: None,
            dependencies: None,
            optional_dependencies: None,
            peers: None,
            transitive_peers: None,
            missing_peers: None,
            patched: None,
            optional: false,
            variants: None,
        }
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
