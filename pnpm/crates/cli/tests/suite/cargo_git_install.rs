use super::cargo_install::{cargo_workspace, crate_archive, install_in};
use assert_cmd::prelude::*;
use pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use sha2::{Digest, Sha256};
use std::{fs, process::Command};
use tempfile::TempDir;

fn git_workspace() -> (TempDir, TempDir, GitRepoFixture) {
    let parent = TempDir::new().unwrap();
    let repository = GitRepoFixture::init(parent.path(), "dependency");
    repository.write_file(
        "Cargo.toml",
        "[workspace]\nmembers = [\"demo\", \"sibling\"]\nresolver = \"2\"\n[workspace.package]\nedition = \"2024\"\nlicense = \"MIT\"\n[workspace.dependencies]\nsibling = { path = \"sibling\" }\n",
    );
    repository.write_file(
        "demo/Cargo.toml",
        "[package]\nname = \"demo\"\npublish = false\nedition.workspace = true\nlicense.workspace = true\n[dependencies]\nsibling.workspace = true\n",
    );
    repository.write_file("demo/src/lib.rs", "pub use sibling::answer;\n");
    repository.write_file(
        "sibling/Cargo.toml",
        "[package]\nname = \"sibling\"\npublish = false\nedition.workspace = true\n",
    );
    repository.write_file("sibling/src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
    let commit = repository.commit("init");
    let root = cargo_workspace(
        CRATES_IO_SPARSE_INDEX,
        &format!("demo = {{ git = {:?}, rev = {commit:?} }}\n", repository.file_url()),
        "pub use demo::answer;\n",
    );
    fs::create_dir(root.path().join(".cargo")).unwrap();
    fs::write(
        root.path().join(".cargo/config.toml"),
        "[env]\nWORKSPACE_DIR = { value = \"workspace\", relative = true }\n[net]\ngit-fetch-with-cli = true\n",
    )
    .unwrap();
    (root, parent, repository)
}

fn cargo_check(root: &TempDir) {
    let cargo_home = TempDir::new().unwrap();
    Command::new("cargo")
        .current_dir(root.path())
        .env("CARGO_HOME", cargo_home.path())
        .args(["check", "--locked", "--offline"])
        .assert()
        .success();
}

fn pnpm(root: &TempDir) -> Command {
    let mut command = Command::cargo_bin("pnpm").unwrap();
    command
        .current_dir(root.path())
        .env("PNPM_CONFIG_CACHE_DIR", root.path().join("cache"))
        .env("PNPM_CONFIG_STORE_DIR", root.path().join("store"));
    command
}

fn registry_cargo_home(registry: &mockito::ServerGuard) -> TempDir {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("config.toml"),
        format!("[source.crates-io]\nregistry = {:?}\n", format!("sparse+{}/", registry.url())),
    )
    .unwrap();
    home
}

#[test]
fn frozen_install_vendors_versionless_git_members_and_path_dependencies() {
    let (root, _parent, _repository) = git_workspace();
    Command::new("cargo")
        .current_dir(root.path())
        .arg("generate-lockfile")
        .assert()
        .success();
    let lock = fs::read(root.path().join("Cargo.lock")).unwrap();
    let original_config = fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap();

    install_in(&root, &["install", "--frozen-lockfile"]);

    assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lock);
    for name in ["demo", "sibling"] {
        let source = root
            .path()
            .join(format!(".pnpm/crates/git/{name}-0.0.0/src/lib.rs"));
        eprintln!("The versionless git crate must be available at {}", source.display());
        assert!(source.is_file());
    }
    let config = fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap();
    eprintln!("The original Cargo configuration must remain: {config}");
    assert!(config.starts_with(&original_config));
    cargo_check(&root);
    install_in(&root, &["install", "--offline", "--frozen-lockfile"]);
    assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lock);
    assert_eq!(fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap(), config);
}

#[test]
fn missing_lockfile_is_resolved_with_git_sources_before_materialization() {
    let (root, _parent, repository) = git_workspace();

    install_in(&root, &["install", "--no-frozen-lockfile"]);

    let lock: cargo_lock::Lockfile = fs::read_to_string(root.path().join("Cargo.lock"))
        .unwrap()
        .parse()
        .unwrap();
    for name in ["demo", "sibling"] {
        let package = lock.packages
            .iter()
            .find(|package| package.name.as_str() == name)
            .unwrap();
        assert_eq!(package.version.to_string(), "0.0.0");
        assert_eq!(
            package.source
                .as_ref()
                .unwrap()
                .precise(),
            Some(repository.head().as_str()),
        );
    }
    cargo_check(&root);
}

#[test]
fn frozen_install_does_not_generate_a_missing_cargo_lockfile() {
    let (root, _parent, _repository) = git_workspace();
    let config = fs::read(root.path().join(".cargo/config.toml")).unwrap();

    let output = pnpm(&root)
        .args(["install", "--frozen-lockfile"])
        .output()
        .unwrap();

    eprintln!("Frozen installation must fail without resolving: {output:?}");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("forbids generating"));
    assert!(!root.path().join("Cargo.lock").exists());
    assert_eq!(fs::read(root.path().join(".cargo/config.toml")).unwrap(), config);
}

#[test]
fn lockfile_generation_can_change_a_git_revision_with_managed_sources_present() {
    let (root, _parent, repository) = git_workspace();
    install_in(&root, &["install", "--no-frozen-lockfile"]);
    let old_commit = repository.head();
    let config = fs::read(root.path().join(".cargo/config.toml")).unwrap();
    repository.write_file("sibling/src/lib.rs", "pub fn answer() -> u8 { 43 }\n");
    let new_commit = repository.commit("update");
    let manifest = root.path().join("Cargo.toml");
    fs::write(&manifest, fs::read_to_string(&manifest).unwrap().replace(&old_commit, &new_commit))
        .unwrap();
    fs::remove_file(root.path().join("Cargo.lock")).unwrap();

    install_in(&root, &["install", "--lockfile-only", "--no-frozen-lockfile"]);

    let lock = fs::read_to_string(root.path().join("Cargo.lock")).unwrap();
    eprintln!("The new revision must be recorded: {lock}");
    assert!(lock.contains(&new_commit));
    assert!(!lock.contains(&old_commit));
    assert_eq!(fs::read(root.path().join(".cargo/config.toml")).unwrap(), config);
    install_in(&root, &["install", "--frozen-lockfile"]);
    cargo_check(&root);
}

#[test]
fn adding_a_registry_crate_to_a_git_workspace_resolves_both_sources() {
    let (root, _parent, _repository) = git_workspace();
    install_in(&root, &["install", "--no-frozen-lockfile"]);
    let config = fs::read(root.path().join(".cargo/config.toml")).unwrap();
    let mut registry = mockito::Server::new();
    let cargo_home = registry_cargo_home(&registry);
    let checksum = format!("{:x}", Sha256::digest(crate_archive("extra", "1.0.0")));
    let _config = registry
        .mock("GET", "/config.json")
        .with_body(serde_json::json!({ "dl": format!("{}/dl", registry.url()) }).to_string())
        .create();
    let index = registry
        .mock("GET", "/ex/tr/extra")
        .with_body(format!(
            "{}\n",
            serde_json::json!({
                "name": "extra", "vers": "1.0.0", "deps": [], "cksum": checksum,
                "features": {}, "yanked": false,
            }),
        ))
        .create();

    pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .args(["add", "crate:extra@1", "--lockfile-only"])
        .assert()
        .success();

    let lock: cargo_lock::Lockfile = fs::read_to_string(root.path().join("Cargo.lock"))
        .unwrap()
        .parse()
        .unwrap();
    let extra = lock.packages
        .iter()
        .find(|package| package.name.as_str() == "extra")
        .unwrap();
    assert_eq!(
        extra.checksum
            .as_ref()
            .unwrap()
            .to_string(),
        checksum,
    );
    eprintln!("The git dependencies must remain in the resolved lockfile: {lock:?}");
    assert!(
        lock.packages
            .iter()
            .any(|package| package.name.as_str() == "demo"),
    );
    assert!(
        lock.packages
            .iter()
            .any(|package| package.name.as_str() == "sibling"),
    );
    assert_eq!(fs::read(root.path().join(".cargo/config.toml")).unwrap(), config);
    index.assert();
}

#[test]
fn failed_git_workspace_resolution_restores_manifest_lockfile_and_sources() {
    let (root, _parent, _repository) = git_workspace();
    install_in(&root, &["install", "--no-frozen-lockfile"]);
    let paths = ["Cargo.toml", "Cargo.lock", ".cargo/config.toml"];
    let original = paths.map(|path| fs::read(root.path().join(path)).unwrap());
    let mut registry = mockito::Server::new();
    let cargo_home = registry_cargo_home(&registry);
    let _config = registry
        .mock("GET", "/config.json")
        .with_body(serde_json::json!({ "dl": format!("{}/dl", registry.url()) }).to_string())
        .create();
    let missing = registry
        .mock("GET", "/mi/ss/missing-crate")
        .with_status(404)
        .create();

    let output = pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .args(["add", "crate:missing-crate@1", "--lockfile-only"])
        .output()
        .unwrap();

    eprintln!("Failed resolution must roll back the native metadata: {output:?}");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("generate-lockfile failed"));
    for (path, contents) in paths.into_iter().zip(original) {
        assert_eq!(fs::read(root.path().join(path)).unwrap(), contents, "{path}");
    }
    cargo_check(&root);
    missing.assert();
}

#[cfg(unix)]
#[test]
fn lockfile_resolution_does_not_execute_checkout_configured_helpers() {
    use std::os::unix::fs::PermissionsExt;

    let (root, _parent, _repository) = git_workspace();
    let helper = root.path().join("checkout-rustc");
    fs::write(&helper, "#!/bin/sh\ntouch \"$0.executed\"\nexit 1\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let config = format!(
        "[env]\nRUSTC = {{ value = {:?}, force = true }}\n[registry]\nglobal-credential-providers = [{:?}]\n[net]\ngit-fetch-with-cli = true\n",
        helper.to_string_lossy(),
        helper.to_string_lossy(),
    );
    fs::write(root.path().join(".cargo/config.toml"), &config).unwrap();

    install_in(&root, &["install", "--lockfile-only", "--no-frozen-lockfile"]);

    let marker = root.path().join("checkout-rustc.executed");
    eprintln!("Checkout helpers must not execute during resolution: {}", marker.display());
    assert!(!marker.exists());
    assert_eq!(fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap(), config);
    let lock: cargo_lock::Lockfile = fs::read_to_string(root.path().join("Cargo.lock"))
        .unwrap()
        .parse()
        .unwrap();
    eprintln!("Resolution must still preserve the git dependency: {lock:?}");
    assert!(
        lock.packages
            .iter()
            .any(|package| package.name.as_str() == "demo"),
    );
}
