use super::{
    LockfileSettingsCheck, PnpmfileChecksumCheck, StalenessReason, check_lockfile_settings,
    satisfies_package_manifest,
};
use crate::Lockfile;
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::PackageManifest;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::tempdir;
use text_block_macros::text_block;

/// Build a `PackageManifest` from inline JSON. Writes it to a temp
/// file so [`PackageManifest::from_path`] can parse it back through
/// the normal load path — exercises the actual deserialize, not a
/// `serde_json::Value` shortcut.
fn manifest_from_json(json: &str) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempdir().expect("create tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, json).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

fn settings_check(catalogs: &Catalogs) -> LockfileSettingsCheck<'_> {
    LockfileSettingsCheck {
        catalogs,
        overrides: None,
        package_extensions_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        auto_install_peers: true,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
        pnpmfile_checksum: PnpmfileChecksumCheck::Current(None),
    }
}

mod manifests;

mod behavior;

mod lockfile;

mod files;

mod dependencies;

mod security;

mod workspace_settings;

mod configuration_check_settings_passes_when;

mod configuration_check_settings_reports_pnpmfile;
