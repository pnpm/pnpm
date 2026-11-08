//! The next format's document, borrowed from a [`Lockfile`] and its converted
//! sections, so writing it serializes nothing twice.

use super::{NextSections, NextSnapshots};
use crate::{
    CatalogSnapshots, Lockfile, LockfileExtra, LockfileSettings, LockfileVersion, PackageKey,
    PackageMetadata, ProjectSnapshot,
};
use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

/// The serde attributes mirror those of [`crate::LockfileDocument`].
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the pnpm-lock.yaml format, as LockfileDocument's do."
    )
)]
pub(super) struct NextDocument<'a> {
    lockfile_version: &'a LockfileVersion<9>,
    #[serde(skip_serializing_if = "is_none")]
    settings: &'a Option<LockfileSettings>,
    #[serde(skip_serializing_if = "is_none")]
    catalogs: &'a Option<CatalogSnapshots>,
    #[serde(skip_serializing_if = "is_none")]
    overrides: &'a Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "is_none")]
    package_extensions_checksum: &'a Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pnpmfile_checksum: &'a Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    ignored_optional_dependencies: &'a Option<Vec<String>>,
    #[serde(skip_serializing_if = "is_none")]
    patched_dependencies: &'a Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "HashMap::is_empty", serialize_with = "sorted_map")]
    importers: &'a HashMap<String, ProjectSnapshot>,
    #[serde(skip_serializing_if = "is_none", serialize_with = "sorted_map_opt")]
    packages: &'a Option<HashMap<PackageKey, PackageMetadata>>,
    #[serde(skip_serializing_if = "is_none", serialize_with = "sorted_map_opt")]
    snapshots: &'a Option<NextSnapshots>,
    #[serde(skip_serializing_if = "is_none")]
    time: &'a Option<BTreeMap<String, String>>,
    #[serde(flatten, skip_serializing_if = "LockfileExtra::is_empty")]
    extra: &'a LockfileExtra,
}

impl<'a> NextDocument<'a> {
    pub(super) fn new(lockfile: &'a Lockfile, sections: &'a NextSections) -> Self {
        NextDocument {
            lockfile_version: &lockfile.lockfile_version,
            settings: &lockfile.settings,
            catalogs: &lockfile.catalogs,
            overrides: &lockfile.overrides,
            package_extensions_checksum: &lockfile.package_extensions_checksum,
            pnpmfile_checksum: &lockfile.pnpmfile_checksum,
            ignored_optional_dependencies: &lockfile.ignored_optional_dependencies,
            patched_dependencies: &lockfile.patched_dependencies,
            importers: &sections.importers,
            packages: &lockfile.packages,
            snapshots: &sections.snapshots,
            time: &lockfile.time,
            extra: &lockfile.extra,
        }
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference to the field, which is itself a reference"
)]
fn is_none<Value>(value: &&Option<Value>) -> bool {
    value.is_none()
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference to the field, which is itself a reference"
)]
fn sorted_map<Key, Value, Ser>(
    map: &&HashMap<Key, Value>,
    serializer: Ser,
) -> Result<Ser::Ok, Ser::Error>
where
    Key: Serialize + std::fmt::Display,
    Value: Serialize + Sync,
    Ser: Serializer,
{
    crate::serialize_yaml::sorted_map(map, serializer)
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference to the field, which is itself a reference"
)]
fn sorted_map_opt<Key, Value, Ser>(
    map: &&Option<HashMap<Key, Value>>,
    serializer: Ser,
) -> Result<Ser::Ok, Ser::Error>
where
    Key: Serialize + std::fmt::Display,
    Value: Serialize + Sync,
    Ser: Serializer,
{
    crate::serialize_yaml::sorted_map_opt(map, serializer)
}
