use super::{Command, CommandExtra, TempDir, assert_eq};
use crate::cargo_install::crate_archive;
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};
use pnpm_testing_utils::git_repo::GitRepoFixture;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn init_git_crate(parent: &Path) -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::init(parent, "git_dep");
    repo.write_file(
        "Cargo.toml",
        "[package]\nname = \"git_dep\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    repo.write_file("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
    let commit = repo.commit("init");
    (repo, commit)
}

fn write_empty_lib_crate(dir: &Path, manifest: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.rs"), "").unwrap();
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
}

#[test]
fn add_crate_detaching_workspace_path_and_git_sources_keeps_non_registry_resolution() {
    let parent = TempDir::new().expect("create git parent");
    let (repo, commit) = init_git_crate(parent.path());
    let git_url = repo.file_url();

    let root = TempDir::new().expect("create Cargo workspace");
    fs::create_dir_all(root.path().join(".cargo")).unwrap();
    fs::write(root.path().join(".cargo/config.toml"), "[net]\ngit-fetch-with-cli = true\n")
        .unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - crates/app\ncargo:\n  enabled: true\n",
    )
    .unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n\n[workspace.dependencies]\nshared = {{ path = \"crates/shared\" }}\ngit_dep = {{ git = {git_url:?}, rev = {commit:?} }}\n",
        ),
    )
    .unwrap();
    write_empty_lib_crate(
        &root.path().join("crates/shared"),
        "[package]\nname = \"shared\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    let app = root.path().join("crates/app");
    write_empty_lib_crate(
        &app,
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nshared = { workspace = true }\ngit_dep = { workspace = true }\n",
    );

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&app)
        .with_args(["add", "crate:shared@0.1.0", "crate:git_dep@1.0.0", "--lockfile-only"])
        .assert()
        .success();

    let updated = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    assert_eq!(parsed["dependencies"]["shared"]["path"].as_str(), Some("../shared"));
    assert_eq!(parsed["dependencies"]["shared"]["version"].as_str(), Some("0.1.0"));
    assert_eq!(parsed["dependencies"]["git_dep"]["git"].as_str(), Some(git_url.as_str()));
    assert_eq!(parsed["dependencies"]["git_dep"]["rev"].as_str(), Some(commit.as_str()));
    assert_eq!(parsed["dependencies"]["git_dep"]["version"].as_str(), Some("1.0.0"));

    let lockfile = fs::read_to_string(root.path().join("Cargo.lock")).unwrap();
    assert!(lockfile.contains(r#"source = "git+file://"#), "{lockfile}");
    assert!(!lockfile.contains("crates.io-index"), "{lockfile}");
}

fn mock_demo_registry() -> (mockito::ServerGuard, Vec<mockito::Mock>, String) {
    let mut registry = mockito::Server::new();
    let base_url = registry.url();
    let demo = crate_archive("demo", "1.0.0");
    let digest = Sha256::digest(&demo);
    let cksum = format!("{digest:x}");
    let config_body = serde_json::json!({
        "dl": format!("{base_url}/dl/{{crate}}/{{version}}"),
        "api": &base_url,
    })
    .to_string();
    let index_line = serde_json::json!({
        "name": "demo",
        "vers": "1.0.0",
        "deps": [],
        "cksum": cksum,
        "features": {},
        "yanked": false,
        "v": 1,
    });
    let config_mock = registry
        .mock("GET", "/config.json")
        .with_body(config_body)
        .create();
    let index_mock = registry
        .mock("GET", "/de/mo/demo")
        .with_body(format!("{index_line}\n"))
        .create();
    (registry, vec![config_mock, index_mock], base_url)
}

#[test]
fn add_crate_detaching_workspace_registry_keeps_private_registry_source() {
    let (_registry, _mocks, base_url) = mock_demo_registry();
    let root = TempDir::new().expect("create Cargo workspace");
    fs::create_dir_all(root.path().join(".cargo")).unwrap();
    fs::write(
        root.path().join(".cargo/config.toml"),
        format!("[registries.mock]\nindex = \"sparse+{base_url}/\"\n"),
    )
    .unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - member\ncargo:\n  enabled: true\nregistries:\n  {base_url}:\n    ecosystem: cargo\n",
        ),
    )
    .unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n\n[workspace.dependencies]\ndemo = { version = \"0.9.0\", registry = \"mock\" }\n",
    )
    .unwrap();
    let member = root.path().join("member");
    write_empty_lib_crate(
        &member,
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndemo = { workspace = true }\n",
    );

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_args(["add", "crate:demo@1.0.0", "--lockfile-only"])
        .assert()
        .success();

    let updated = fs::read_to_string(member.join("Cargo.toml")).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    assert_eq!(parsed["dependencies"]["demo"]["registry"].as_str(), Some("mock"));
    assert_eq!(parsed["dependencies"]["demo"]["version"].as_str(), Some("1.0.0"));

    let lockfile = fs::read_to_string(root.path().join("Cargo.lock")).unwrap();
    let expected_source = format!(r#"source = "sparse+{base_url}/""#);
    assert!(lockfile.contains(&expected_source), "{lockfile}");
    assert!(!lockfile.contains("crates.io-index"), "{lockfile}");
}

fn read_cargo_dep_identity(manifest_path: &Path) -> (String, Option<String>, Option<String>) {
    let output = std::process::Command::new("cargo")
        .current_dir(manifest_path.parent().unwrap())
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(manifest_path)
        .output()
        .expect("run cargo metadata");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let dep = &json["packages"][0]["dependencies"][0];
    (
        dep["name"]
            .as_str()
            .unwrap()
            .to_string(),
        dep["rename"].as_str().map(str::to_string),
        dep["source"].as_str().map(str::to_string),
    )
}

#[test]
fn add_crate_detaching_workspace_dep_discards_ignored_member_git_and_package_fields() {
    let (_registry, _mocks, base_url) = mock_demo_registry();
    let root = TempDir::new().expect("create Cargo workspace");
    fs::create_dir_all(root.path().join(".cargo")).unwrap();
    fs::write(
        root.path().join(".cargo/config.toml"),
        format!("[registries.mock]\nindex = \"sparse+{base_url}/\"\n"),
    )
    .unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - member\ncargo:\n  enabled: true\nregistries:\n  {base_url}:\n    ecosystem: cargo\n",
        ),
    )
    .unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n\n[workspace.dependencies]\ndemo = { version = \"0.9.0\", registry = \"mock\" }\n",
    )
    .unwrap();
    let member = root.path().join("member");
    write_empty_lib_crate(
        &member,
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndemo = { workspace = true, git = \"https://untrusted.example/evil.git\", branch = \"main\", package = \"evil\" }\n",
    );
    let member_manifest = member.join("Cargo.toml");
    let before = read_cargo_dep_identity(&member_manifest);

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_args(["add", "crate:demo@1.0.0", "--lockfile-only"])
        .assert()
        .success();

    let updated = fs::read_to_string(&member_manifest).unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let after = read_cargo_dep_identity(&member_manifest);
    assert_eq!(after, before);
    assert!(!updated.contains("untrusted.example"), "{updated}");
    assert!(!updated.contains("evil"), "{updated}");
}
