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

fn patched_git_workspace() -> (TempDir, TempDir, GitRepoFixture) {
    let (root, parent, repository) = git_workspace();
    fs::write(
        root.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\ndemo = \"0\"\n[patch.crates-io]\ndemo = {{ git = {:?}, rev = {:?} }}\n",
            repository.file_url(), repository.head(),
        ),
    ).unwrap();
    (root, parent, repository)
}

fn patched_path_workspace() -> (TempDir, TempDir, GitRepoFixture) {
    let (root, parent, repository) = patched_git_workspace();
    fs::create_dir_all(root.path().join("dep/src")).unwrap();
    fs::write(root.path().join("dep/src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    fs::write(
        root.path().join("dep/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let manifest = root.path().join("Cargo.toml");
    let contents = fs::read_to_string(&manifest).unwrap();
    let (package, _) = contents.split_once("[patch.crates-io]").unwrap();
    fs::write(manifest, format!("{package}[patch.crates-io]\ndemo = {{ path = \"dep\" }}\n"))
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
fn missing_lockfile_is_resolved_with_git_dependencies_and_source_overrides() {
    for workspace in [git_workspace, patched_git_workspace, patched_path_workspace] {
        let (root, _parent, repository) = workspace();

        install_in(&root, &["install", "--no-frozen-lockfile"]);

        let lock: cargo_lock::Lockfile = fs::read_to_string(root.path().join("Cargo.lock"))
            .unwrap()
            .parse()
            .unwrap();
        let names =
            if root.path().join("dep").exists() { vec!["demo"] } else { vec!["demo", "sibling"] };
        for name in names {
            let package = lock.packages
                .iter()
                .find(|package| package.name.as_str() == name)
                .unwrap();
            assert_eq!(package.version.to_string(), "0.0.0");
            if root.path().join("dep").exists() {
                assert_eq!(package.source, None);
                continue;
            }
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
}

#[test]
fn frozen_install_does_not_generate_a_missing_cargo_lockfile() {
    for workspace in [git_workspace, patched_git_workspace, patched_path_workspace] {
        let (root, _parent, _repository) = workspace();
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
}

#[test]
fn lockfile_generation_can_change_a_git_revision_with_managed_sources_present() {
    for workspace in [git_workspace, patched_git_workspace] {
        let (root, _parent, repository) = workspace();
        install_in(&root, &["install", "--no-frozen-lockfile"]);
        let old_commit = repository.head();
        let config = fs::read(root.path().join(".cargo/config.toml")).unwrap();
        repository.write_file("sibling/src/lib.rs", "pub fn answer() -> u8 { 43 }\n");
        let new_commit = repository.commit("update");
        let manifest = root.path().join("Cargo.toml");
        fs::write(
            &manifest,
            fs::read_to_string(&manifest).unwrap().replace(&old_commit, &new_commit),
        )
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
}

#[test]
fn adding_a_registry_crate_preserves_git_dependencies_and_source_overrides() {
    for workspace in [git_workspace, patched_git_workspace, patched_path_workspace] {
        let (root, _parent, _repository) = workspace();
        install_in(&root, &["install", "--no-frozen-lockfile"]);
        let config = fs::read(root.path().join(".cargo/config.toml")).unwrap();
        let mut registry = mockito::Server::new();
        let cargo_home = registry_cargo_home(&registry);
        let _demo = registry
            .mock("GET", "/de/mo/demo")
            .with_status(404)
            .create();
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
        if !root.path().join("dep").exists() {
            assert!(
                lock.packages
                    .iter()
                    .any(|package| package.name.as_str() == "sibling"),
            );
        }
        assert_eq!(fs::read(root.path().join(".cargo/config.toml")).unwrap(), config);
        index.assert();
    }
}

#[test]
fn failed_resolution_restores_manifest_lockfile_and_sources() {
    for workspace in [git_workspace, patched_git_workspace, patched_path_workspace] {
        let (root, _parent, _repository) = workspace();
        install_in(&root, &["install", "--no-frozen-lockfile"]);
        let paths = ["Cargo.toml", "Cargo.lock", ".cargo/config.toml"];
        let original = paths.map(|path| fs::read(root.path().join(path)).unwrap());
        let mut registry = mockito::Server::new();
        let cargo_home = registry_cargo_home(&registry);
        let _demo = registry
            .mock("GET", "/de/mo/demo")
            .with_status(404)
            .create();
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
}

#[cfg(unix)]
#[test]
fn lockfile_resolution_does_not_execute_checkout_configured_helpers() {
    for workspace in [git_workspace, patched_git_workspace, patched_path_workspace] {
        use std::os::unix::fs::PermissionsExt;

        let (root, _parent, _repository) = workspace();
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
        let config_after = fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap();
        eprintln!("Config after resolution:\n{config_after}\n");
        assert_eq!(config_after, config);
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
}

#[test]
fn lockfile_generation_preserves_legacy_path_replacements() {
    let (root, _parent, _repository) = patched_path_workspace();
    let manifest = root.path().join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("[patch.crates-io]\ndemo", "[replace]\n\"demo:0.0.0\""),
    )
    .unwrap();
    let mut registry = mockito::Server::new();
    let cargo_home = registry_cargo_home(&registry);
    let checksum = format!("{:x}", Sha256::digest(crate_archive("demo", "0.0.0")));
    let _config = registry
        .mock("GET", "/config.json")
        .with_body(serde_json::json!({ "dl": format!("{}/dl", registry.url()) }).to_string())
        .create();
    let index = registry
        .mock("GET", "/de/mo/demo")
        .with_body(format!(
            "{}\n",
            serde_json::json!({
                "name": "demo", "vers": "0.0.0", "deps": [], "cksum": checksum,
                "features": {}, "yanked": false,
            }),
        ))
        .create();

    pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .args(["install", "--lockfile-only", "--no-frozen-lockfile"])
        .assert()
        .success();

    let lock: cargo_lock::Lockfile = fs::read_to_string(root.path().join("Cargo.lock"))
        .unwrap()
        .parse()
        .unwrap();
    let original = lock.packages
        .iter()
        .find(|package| package.name.as_str() == "demo" && package.source.is_some())
        .unwrap();
    assert_eq!(
        original.replace
            .as_ref()
            .unwrap()
            .name
            .as_str(),
        "demo",
    );
    assert_eq!(original.replace.as_ref().unwrap().source, None);
    eprintln!("Cargo must retain the replacement's distinct identity: {lock:?}");
    assert!(
        lock.packages
            .iter()
            .any(|package| package.name.as_str() == "demo" && package.source.is_none()),
    );
    Command::new("cargo")
        .current_dir(root.path())
        .env("CARGO_HOME", cargo_home.path())
        .args(["check", "--offline", "--locked"])
        .assert()
        .success();
    index.assert();
}
#[cfg(unix)]
#[test]
fn path_patched_dependencies_cannot_execute_git_transport_helpers() {
    use std::os::unix::fs::PermissionsExt;

    let (root, _parent, _repository) = patched_path_workspace();
    let patched = TempDir::new().unwrap();
    fs::rename(root.path().join("dep"), patched.path().join("dep")).unwrap();
    let workspace_manifest = root.path().join("Cargo.toml");
    let patched_path = patched.path().join("dep");
    let path_override = format!("path = {:?}", patched_path.to_string_lossy());
    fs::write(
        &workspace_manifest,
        fs::read_to_string(&workspace_manifest).unwrap().replace(r#"path = "dep""#, &path_override),
    )
    .unwrap();
    let manifest = patched.path().join("dep/Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[dependencies]\nhelper = {{ git = \"pnpm-test://invalid.example/repository\" }}\n",
            fs::read_to_string(&manifest).unwrap(),
        ),
    )
    .unwrap();
    let helpers = TempDir::new().unwrap();
    let helper = helpers.path().join("git-remote-pnpm-test");
    fs::write(&helper, "#!/bin/sh\n: > \"$HELPER_MARKER\"\nexit 1\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let marker = helpers.path().join("executed");
    let mut registry = mockito::Server::new();
    let cargo_home = registry_cargo_home(&registry);
    let _config = registry
        .mock("GET", "/config.json")
        .with_body(serde_json::json!({ "dl": format!("{}/dl", registry.url()) }).to_string())
        .create();
    let _demo = registry
        .mock("GET", "/de/mo/demo")
        .with_status(404)
        .create();

    let output = pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
        .env("CARGO_NET_RETRY", "0")
        .env("GIT_EXEC_PATH", helpers.path())
        .env("HELPER_MARKER", &marker)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.pnpm-test.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .env("GIT_ALLOW_PROTOCOL", "file:pnpm-test")
        .args(["install", "--lockfile-only", "--no-frozen-lockfile"])
        .output()
        .unwrap();

    eprintln!("Transitive helper transports must be rejected before execution: {output:?}");
    assert!(!output.status.success());
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("transport 'pnpm-test' not allowed"));
}

#[test]
fn path_patched_lockfile_resolution_does_not_require_git() {
    let (root, _parent, _repository) = patched_path_workspace();
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(sysroot.status.success());
    let sysroot = String::from_utf8(sysroot.stdout).unwrap();
    let mut registry = mockito::Server::new();
    let cargo_home = registry_cargo_home(&registry);
    let _config = registry
        .mock("GET", "/config.json")
        .with_body(serde_json::json!({ "dl": format!("{}/dl", registry.url()) }).to_string())
        .create();
    let _demo = registry
        .mock("GET", "/de/mo/demo")
        .with_status(404)
        .create();

    pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .env("PATH", std::path::Path::new(sysroot.trim()).join("bin"))
        // The PATH above carries `cargo` and `rustc` and nothing else. A
        // `build.rustc-wrapper` in the developer's own Cargo configuration
        // reaches this far as `RUSTC_WRAPPER`, and its binary is not on that
        // PATH; an empty value turns it off.
        .env("RUSTC_WRAPPER", "")
        .args(["install", "--lockfile-only", "--no-frozen-lockfile"])
        .assert()
        .success();
    cargo_check(&root);
}

#[test]
fn cargo_resolution_preserves_workspace_local_protocol_bans() {
    let (root, _parent, _repository) = git_workspace();
    let setup: [&[&str]; 2] = [&["init", "--quiet"], &["config", "protocol.file.allow", "never"]];
    for args in setup {
        Command::new("git")
            .current_dir(root.path())
            .args(args)
            .assert()
            .success();
    }
    let cargo_home = TempDir::new().unwrap();

    let output = pnpm(&root)
        .env("CARGO_HOME", cargo_home.path())
        .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
        .env("CARGO_NET_RETRY", "0")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .args(["install", "--lockfile-only", "--no-frozen-lockfile"])
        .output()
        .unwrap();

    eprintln!("Workspace-local protocol bans must constrain Cargo Git fetching: {output:?}");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("transport 'file' not allowed"));
}

fn add_submodule(parent: &TempDir, name: &str, url: &str, path: &str) {
    Command::new("git")
        .current_dir(
            parent
                .path()
                .join(format!("{name}-src")),
        )
        .args(["-c", "protocol.file.allow=always", "submodule", "add", "--", url, path])
        .assert()
        .success();
}

#[test]
fn recursive_submodules_are_pinned_checksummed_and_reused_offline() {
    let (root, parent, repository) = git_workspace();
    let leaf = GitRepoFixture::init(parent.path(), "leaf");
    leaf.write_file("answer.rs", "pub fn answer() -> u8 { 42 }\n");
    let _leaf_commit = leaf.commit("init");
    let nested = GitRepoFixture::init(parent.path(), "nested");
    nested.write_file("README", "nested sources\n");
    let _nested_commit = nested.commit("init");
    add_submodule(&parent, "nested", "../leaf.git", "leaf");
    let _nested_commit = nested.commit("pin leaf");
    add_submodule(&parent, "dependency", "../nested.git", "demo/native");
    repository.write_file("demo/src/lib.rs", "include!(\"../native/leaf/answer.rs\");\n");
    let commit = repository.commit("pin nested sources");
    leaf.write_file("answer.rs", "compile_error!(\"unpinned source\");\n");
    let _new_leaf_commit = leaf.commit("move branch");
    let manifest = root.path().join("Cargo.toml");
    fs::write(&manifest, format!(
        "[package]\nname = \"survey\"\nversion = \"1.0.0\"\nedition = \"2024\"\n[dependencies]\ndemo = {{ git = {:?}, rev = {commit:?} }}\n",
        repository.file_url(),
    )).unwrap();

    pnpm(&root)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .args(["install", "--no-frozen-lockfile"])
        .assert()
        .success();
    let slot = root.path().join(".pnpm/crates/git/demo-0.0.0");
    let checksum: serde_json::Value =
        serde_json::from_slice(&fs::read(slot.join(".cargo-checksum.json")).unwrap()).unwrap();
    assert_eq!(
        checksum["files"]["native/leaf/answer.rs"],
        format!("{:x}", Sha256::digest(fs::read(slot.join("native/leaf/answer.rs")).unwrap()),),
    );
    eprintln!("Submodule Git metadata must not be vendored: {}", slot.display());
    assert!(!slot.join("native/.git").exists());
    assert!(!slot.join("native/leaf/.git").exists());
    cargo_check(&root);
    fs::remove_dir_all(parent.path().join("nested.git")).unwrap();
    fs::remove_dir_all(parent.path().join("leaf.git")).unwrap();
    install_in(&root, &["install", "--offline", "--frozen-lockfile"]);
    cargo_check(&root);
    fs::write(slot.join("native/leaf/answer.rs"), "tampered").unwrap();
    let cargo_home = TempDir::new().unwrap();
    Command::new("cargo")
        .current_dir(root.path())
        .env("CARGO_HOME", cargo_home.path())
        .env("CARGO_TARGET_DIR", root.path().join("tampered-target"))
        .args(["check", "--locked", "--offline"])
        .assert()
        .failure();
}

#[test]
fn submodule_fetching_preserves_the_callers_transport_allowlist() {
    let (root, parent, repository) = git_workspace();
    let cargo_home = TempDir::new().unwrap();
    Command::new("cargo")
        .current_dir(root.path())
        .env("CARGO_HOME", cargo_home.path())
        .arg("generate-lockfile")
        .assert()
        .success();
    let old_commit = repository.head();
    let mut server = mockito::Server::new();
    let requests = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create();
    add_submodule(&parent, "dependency", &repository.file_url(), "demo/native");
    repository.write_file(
        ".gitmodules",
        &format!("[submodule \"native\"]\npath = demo/native\nurl = {}/native.git\n", server.url()),
    );
    let commit = repository.commit("add HTTP submodule");
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = root.path().join(name);
        fs::write(&path, fs::read_to_string(&path).unwrap().replace(&old_commit, &commit)).unwrap();
    }

    let output = pnpm(&root)
        .env("GIT_ALLOW_PROTOCOL", "file")
        .args(["install", "--frozen-lockfile"])
        .output()
        .unwrap();

    eprintln!("A file-only caller must reject an HTTP submodule: {output:?}");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("transport 'http' not allowed"));
    assert!(
        !root
            .path()
            .join(".pnpm/crates/git/demo-0.0.0/.cargo-checksum.json")
            .exists(),
    );
    requests.assert();
}
