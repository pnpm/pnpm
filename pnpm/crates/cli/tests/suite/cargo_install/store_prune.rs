//! `pnpm store prune` and the crates Cargo workspaces link from the store.

use super::{
    cargo_workspace,
    crate_archive,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use sha2::{
    Digest,
    Sha256,
};
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};
use tempfile::TempDir;

/// A registry serving version 1.0.0 of each of `names`, and the slot each
/// one takes in the store at `store`.
fn serve_crates(
    registry: &mut mockito::ServerGuard,
    store: &Path,
    names: &[&str],
) -> (Vec<mockito::Mock>, Vec<PathBuf>) {
    let mut mocks = vec![
        registry
            .mock("GET", "/config.json")
            .with_body(
                serde_json::json!({"dl": format!("{}/dl/{{crate}}/{{version}}", registry.url())})
                    .to_string(),
            )
            .expect_at_least(0)
            .create(),
    ];
    let mut slots = Vec::new();
    for name in names {
        let archive = crate_archive(name, "1.0.0");
        let checksum = format!("{:x}", Sha256::digest(&archive));
        let entry = serde_json::json!({
            "name": name,
            "vers": "1.0.0",
            "deps": [],
            "cksum": checksum,
            "features": {},
            "yanked": false,
            "v": 1,
        });
        mocks.push(
            registry
                .mock("GET", format!("/{}/{}/{name}", &name[..2], &name[2..4]).as_str())
                .with_body(format!("{entry}\n"))
                .expect_at_least(0)
                .create(),
        );
        mocks.push(
            registry
                .mock("GET", format!("/dl/{name}/1.0.0").as_str())
                .with_body(&archive)
                .expect_at_least(0)
                .create(),
        );
        slots.push(
            store
                .join("v11/crates")
                .join(name)
                .join("1.0.0")
                .join(checksum),
        );
    }
    (mocks, slots)
}

fn pnpm_in(dir: &Path, store: &Path, args: &[&str]) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .with_env("CARGO_HOME", store.join("cargo-home"))
        .with_env("PNPM_CONFIG_CACHE_DIR", store.join("cache"))
        .with_env("PNPM_CONFIG_STORE_DIR", store)
        .with_args(args)
}

fn workspace(registry: &mockito::ServerGuard, dependencies: &str) -> TempDir {
    cargo_workspace(&registry.url(), dependencies, "")
}

#[test]
fn store_prune_removes_the_crates_only_a_removed_workspace_links() {
    let store = TempDir::new().unwrap();
    let mut registry = mockito::Server::new();
    let (_mocks, slots) = serve_crates(&mut registry, store.path(), &["demo", "other"]);
    let kept = workspace(&registry, "demo = \"1\"\n");
    let removed = workspace(&registry, "demo = \"1\"\nother = \"1\"\n");
    for root in [&kept, &removed] {
        pnpm_in(root.path(), store.path(), &["install"]).assert().success();
    }
    assert!(slots.iter().all(|slot| slot.is_dir()));
    fs::remove_dir_all(removed.path()).unwrap();

    let output = pnpm_in(kept.path(), store.path(), &["store", "prune"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(output.status.success());
    assert!(stderr.contains("Removed 1 unused crate\n"), "{stderr}");
    assert!(slots[0].is_dir(), "a crate the remaining workspace links was removed");
    assert!(!slots[1].exists(), "a crate no workspace links was kept");
    Command::new("cargo")
        .with_current_dir(kept.path())
        .with_env("CARGO_HOME", store.path().join("cargo-home"))
        .with_args(["check", "--locked", "--offline"])
        .assert()
        .success();
}

/// An older pnpm linked crates without registering the workspace, so a
/// crate it linked may be in use by a workspace the prune cannot find.
#[test]
fn store_prune_keeps_a_crate_no_registered_workspace_has_linked() {
    let store = TempDir::new().unwrap();
    let mut registry = mockito::Server::new();
    let (_mocks, slots) = serve_crates(&mut registry, store.path(), &["demo"]);
    let root = workspace(&registry, "demo = \"1\"\n");
    pnpm_in(root.path(), store.path(), &["install"]).assert().success();
    let slot = &slots[0];
    fs::remove_file(linked_record(slot)).unwrap();
    fs::remove_dir_all(root.path()).unwrap();

    pnpm_in(store.path(), store.path(), &["store", "prune"]).assert().success();
    assert!(slot.is_dir());
}

/// The first install by this pnpm of a workspace an older pnpm installed
/// records the crates the workspace already links.
#[test]
fn store_prune_removes_a_crate_an_older_pnpm_linked_once_the_workspace_is_reinstalled() {
    let store = TempDir::new().unwrap();
    let mut registry = mockito::Server::new();
    let (_mocks, slots) = serve_crates(&mut registry, store.path(), &["demo"]);
    let root = workspace(&registry, "demo = \"1\"\n");
    pnpm_in(root.path(), store.path(), &["install"]).assert().success();
    let slot = &slots[0];
    fs::remove_file(linked_record(slot)).unwrap();
    fs::remove_dir_all(store.path().join("v11/cargo-workspaces")).unwrap();

    pnpm_in(root.path(), store.path(), &["install"]).assert().success();
    assert!(linked_record(slot).is_file());
    fs::remove_dir_all(root.path()).unwrap();
    pnpm_in(store.path(), store.path(), &["store", "prune"]).assert().success();
    assert!(!slot.exists());
}

fn linked_record(slot: &Path) -> PathBuf {
    slot.with_file_name(format!(
        "{}.linked",
        slot.file_name()
            .unwrap()
            .to_string_lossy(),
    ))
}
