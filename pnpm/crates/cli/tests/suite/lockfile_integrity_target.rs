//! `lockfile.integrityTarget: tar` pins the hash of every package's tar in
//! the lockfile, and the store answers for it by either hash.

use crate::{
    _utils::read_lockfile,
    auto_dedupe::{pnpm_at, write_settings},
};
use command_extra::CommandExtra;
use pnpm_lockfile::IntegrityTarget;
use pnpm_store_dir::{StoreDir, StoreIndex, store_index_key};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

const PACKAGE: &str = "is-odd@3.0.1";
const TAR_TARGET: &str = "lockfile:\n  integrityTarget: tar\n";

fn write_manifest(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-odd": "3.0.1" } }).to_string(),
    )
    .unwrap();
}

fn install(workspace: &Path, args: &[&str]) -> std::process::Output {
    pnpm_at(workspace)
        .with_env("PNPM_CONFIG_CI", "false")
        .with_args(args)
        .output()
        .unwrap()
}

fn assert_success(output: &std::process::Output) {
    assert!(output.status.success(), "{output:?}");
}

/// The lockfile's recorded target and the integrity of [`PACKAGE`].
fn locked(workspace: &Path) -> (IntegrityTarget, String) {
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let integrity = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&PACKAGE.parse().unwrap()))
        .and_then(|package| package.resolution.integrity())
        .expect("the package has an integrity")
        .to_string();
    (lockfile.integrity_target(), integrity)
}

#[test]
fn switching_the_target_rewrites_every_hash_and_back() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    assert_success(&install(&workspace, &["install"]));
    let (target, resource) = locked(&workspace);
    assert_eq!(target, IntegrityTarget::Resource);

    write_settings(&workspace, TAR_TARGET).unwrap();
    assert_success(&install(&workspace, &["install"]));
    let (target, tar) = locked(&workspace);
    assert_eq!(target, IntegrityTarget::Tar);
    assert_ne!(tar, resource);
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(lockfile.contains("integrityTarget: tar"), "{lockfile}");

    // The row the resource's install wrote records the tar it holds, and is
    // reachable under the tar's integrity too.
    let index = StoreIndex::open_readonly_in(&StoreDir::from(store_dir)).unwrap();
    let row = index
        .get(&store_index_key(&resource, PACKAGE))
        .unwrap()
        .expect("resource row");
    assert_eq!(row.tar_integrity.as_deref(), Some(tar.as_str()));
    assert_eq!(
        index
            .get(&store_index_key(&tar, PACKAGE))
            .unwrap(),
        Some(row),
    );
    drop(index);

    write_settings(&workspace, "lockfile: {}\n").unwrap();
    assert_success(&install(&workspace, &["install"]));
    assert_eq!(locked(&workspace), (IntegrityTarget::Resource, resource));
    drop((root, mock_instance));
}

#[test]
fn a_frozen_install_rejects_a_switched_target() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    assert_success(&install(&workspace, &["install"]));

    write_settings(&workspace, TAR_TARGET).unwrap();
    let output = install(&workspace, &["install", "--frozen-lockfile"]);
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(combined.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"), "{combined}");
    assert!(combined.contains("settings.integrityTarget"), "{combined}");
    drop((root, mock_instance));
}

/// A store a `.tgz` install populated serves a tar-pinned frozen install
/// with no download.
#[test]
fn a_tar_pinned_frozen_install_is_served_offline_by_a_tgz_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    assert_success(&install(&workspace, &["install"]));
    write_settings(&workspace, TAR_TARGET).unwrap();
    assert_success(&install(&workspace, &["install"]));

    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    assert_success(&install(&workspace, &["install", "--frozen-lockfile", "--offline"]));
    assert!(workspace.join("node_modules/is-odd/package.json").exists());
    drop((root, mock_instance));
}

/// A resolution with no store pays one download per version to learn its
/// tar, and records the same hash a store would have given it.
#[test]
fn a_lockfile_only_resolution_derives_the_tar_hash_without_a_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    write_settings(&workspace, TAR_TARGET).unwrap();
    assert_success(&install(&workspace, &["install", "--lockfile-only"]));
    let derived = locked(&workspace);

    fs::remove_file(workspace.join("pnpm-lock.yaml")).unwrap();
    fs::remove_dir_all(&store_dir).unwrap();
    assert_success(&install(&workspace, &["install"]));
    assert_eq!(locked(&workspace), derived);
    drop((root, mock_instance));
}
