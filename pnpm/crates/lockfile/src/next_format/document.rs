//! The next format's document, borrowed from a [`Lockfile`] and its converted
//! sections, so writing it serializes nothing twice.

use super::{NextImporter, NextPackageOut, settings::NextSettingsOut};
use crate::{Lockfile, LockfileExtra, LockfileVersion, PackageKey};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

/// The serde attributes mirror those of [`crate::LockfileDocument`].
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NextDocument<'a> {
    lockfile_version: &'a LockfileVersion<9>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<NextSettingsOut<'a>>,
    #[serde(skip_serializing_if = "HashMap::is_empty", serialize_with = "sorted_map")]
    importers: &'a HashMap<String, NextImporter>,
    #[serde(skip_serializing_if = "HashMap::is_empty", serialize_with = "sorted_map")]
    packages: &'a HashMap<PackageKey, NextPackageOut<'a>>,
    #[serde(skip_serializing_if = "is_none")]
    time: &'a Option<BTreeMap<String, String>>,
    #[serde(flatten, skip_serializing_if = "LockfileExtra::is_empty")]
    extra: &'a LockfileExtra,
}

impl<'a> NextDocument<'a> {
    pub(super) fn new(
        lockfile: &'a Lockfile,
        importers: &'a HashMap<String, NextImporter>,
        packages: &'a HashMap<PackageKey, NextPackageOut<'a>>,
    ) -> Self {
        NextDocument {
            lockfile_version: &lockfile.lockfile_version,
            settings: NextSettingsOut::of(lockfile),
            importers,
            packages,
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
