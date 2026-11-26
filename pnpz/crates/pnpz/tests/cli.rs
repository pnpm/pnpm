use crate::support::{
    Remote,
    Served,
};
use assert_cmd::Command;
use pnpz::{
    binary_cache::{
        CACHE_URL_ENV,
        TRUSTED_KEYS_ENV,
    },
    catalog::CATALOG_URL_ENV,
    store::STORE_ROOT_ENV,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::Path,
};
use tempfile::TempDir;

fn pnpz(remote: &Remote, project: &Path) -> Command {
    let mut command = Command::cargo_bin("pnpz").unwrap();
    command
        .current_dir(project)
        .env(CATALOG_URL_ENV, remote.server.url())
        .env(CACHE_URL_ENV, remote.server.url())
        .env(TRUSTED_KEYS_ENV, remote.public_key())
        .env(STORE_ROOT_ENV, remote.store().root());
    command
}

fn stdout(command: &mut Command) -> String {
    String::from_utf8(
        command
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
}

/// Whether this machine lets an unprivileged process mount a store at `/nix`.
/// Containers and some distributions forbid the user namespace that needs.
fn can_mount_store(remote: &Remote, project: &Path) -> bool {
    let probe = pnpz(remote, project)
        .arg("__enter")
        .arg(remote.store().root())
        .arg("true")
        .ok();
    if let Err(error) = &probe {
        eprintln!("skipping: this machine cannot mount a store at /nix: {error}");
    }
    probe.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn add_writes_the_manifest_and_lockfile_and_run_finds_the_package() {
    let mut remote = Remote::start().await;
    let (library, _) = remote.add_path("library", "library", &[], Served::Zstd).await;
    let (tool, _) = remote.add_path("tool", "ran from the store", &[&library], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let project = TempDir::new().unwrap();

    pnpz(&remote, project.path())
        .args(["add", "tool@1"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(project.path().join("pnpz.yaml")).unwrap(),
        "packages:\n- tool@1\n"
    );
    let lockfile = fs::read_to_string(project.path().join("pnpz-lock.yaml")).unwrap();
    assert!(lockfile.contains(&format!("out: {tool}")), "{lockfile}");
    assert!(lockfile.contains(&format!("{library}:")), "{lockfile}");

    if !cfg!(target_os = "linux") || !can_mount_store(&remote, project.path()) {
        return;
    }
    assert_eq!(stdout(pnpz(&remote, project.path()).args(["run", "tool"])), "ran from the store\n");
    let resolved =
        stdout(pnpz(&remote, project.path()).args(["run", "sh", "-c", "command -v tool"]));
    assert_eq!(resolved, format!("{tool}/bin/tool\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_installs_what_the_lockfile_names_but_the_store_lacks() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let project = TempDir::new().unwrap();
    pnpz(&remote, project.path())
        .args(["add", "tool"])
        .assert()
        .success();
    if !cfg!(target_os = "linux") || !can_mount_store(&remote, project.path()) {
        return;
    }
    let fresh_store = TempDir::new().unwrap();

    let mut run = pnpz(&remote, project.path());
    run.env(STORE_ROOT_ENV, fresh_store.path().join("nix"))
        .args(["run", "tool"]);

    assert_eq!(stdout(&mut run), "ran\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn frozen_install_fails_when_the_manifest_changed() {
    let mut remote = Remote::start().await;
    let (tool, _) = remote.add_path("tool", "ran", &[], Served::Zstd).await;
    remote.add_package("tool", "1.0.0", &tool).await;
    let project = TempDir::new().unwrap();
    pnpz(&remote, project.path())
        .args(["add", "tool"])
        .assert()
        .success();
    pnpz(&remote, project.path())
        .args(["install", "--frozen"])
        .assert()
        .success();

    fs::write(project.path().join("pnpz.yaml"), "packages:\n- tool@2\n").unwrap();

    let failure = pnpz(&remote, project.path())
        .args(["install", "--frozen"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&failure.get_output().stderr).into_owned();
    assert!(stderr.contains("ERR_PNPZ_OUTDATED_LOCKFILE"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread")]
async fn add_leaves_the_manifest_alone_when_a_package_does_not_resolve() {
    let mut remote = Remote::start().await;
    remote.server
        .mock("GET", mockito::Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let project = TempDir::new().unwrap();

    pnpz(&remote, project.path())
        .args(["add", "missing"])
        .assert()
        .failure();

    assert!(
        !project
            .path()
            .join("pnpz.yaml")
            .exists()
    );
    assert!(
        !project
            .path()
            .join("pnpz-lock.yaml")
            .exists()
    );
}
