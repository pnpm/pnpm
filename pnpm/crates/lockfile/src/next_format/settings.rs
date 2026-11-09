//! The next format's `settings`: what `pnpm-workspace.yaml` said when the
//! lockfile was written. Beside today's settings it holds `catalogs`,
//! `overrides`, `patchedDependencies`, the two checksums and
//! `ignoredOptionalDependencies`, which today are top-level sections.

use crate::{CatalogSnapshots, Lockfile, LockfileSettings};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `settings` as read. `base` is `None` when the block has none of today's
/// settings.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSettings {
    #[serde(flatten)]
    pub base: Option<LockfileSettings>,
    #[serde(default)]
    pub catalogs: Option<CatalogSnapshots>,
    #[serde(default)]
    pub overrides: Option<IndexMap<String, String>>,
    #[serde(default)]
    pub package_extensions_checksum: Option<String>,
    #[serde(default)]
    pub pnpmfile_checksum: Option<String>,
    #[serde(default)]
    pub ignored_optional_dependencies: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::deserialize_patched_dependencies")]
    pub patched_dependencies: Option<BTreeMap<String, String>>,
}

/// `settings` as written, borrowed from the lockfile.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NextSettingsOut<'a> {
    #[serde(flatten)]
    base: Option<&'a LockfileSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalogs: Option<&'a CatalogSnapshots>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overrides: Option<&'a IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_extensions_checksum: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pnpmfile_checksum: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_optional_dependencies: Option<&'a Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    patched_dependencies: Option<&'a BTreeMap<String, String>>,
}

impl<'a> NextSettingsOut<'a> {
    /// `None` when the lockfile records none of it.
    pub(super) fn of(lockfile: &'a Lockfile) -> Option<Self> {
        let out = NextSettingsOut {
            base: lockfile.settings.as_ref(),
            catalogs: lockfile.catalogs.as_ref(),
            overrides: lockfile.overrides.as_ref(),
            package_extensions_checksum: lockfile.package_extensions_checksum.as_ref(),
            pnpmfile_checksum: lockfile.pnpmfile_checksum.as_ref(),
            ignored_optional_dependencies: lockfile
                .ignored_optional_dependencies
                .as_ref(),
            patched_dependencies: lockfile.patched_dependencies.as_ref(),
        };
        let empty = out.base.is_none()
            && out.catalogs.is_none()
            && out.overrides.is_none()
            && out.package_extensions_checksum.is_none()
            && out.pnpmfile_checksum.is_none()
            && out.ignored_optional_dependencies.is_none()
            && out.patched_dependencies.is_none();
        (!empty).then_some(out)
    }
}
