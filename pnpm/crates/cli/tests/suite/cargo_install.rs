//! `pnpm install` against a Cargo workspace, resolving and downloading
//! through the registry `cargo.indexUrl` selects.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

/// A `.crate` archive holding the one source file a dependent needs, laid
/// out under the `<name>-<version>` root `cargo` expects.
pub(crate) fn crate_archive(name: &str, version: &str) -> Vec<u8> {
    let root = format!("{name}-{version}");
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in [
        ("Cargo.toml", format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")),
        ("src/lib.rs", "pub fn answer() -> u8 { 42 }\n".to_string()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, format!("{root}/{path}"), contents.as_bytes()).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn cargo_workspace(index_url: &str, dependencies: &str, source: &str) -> TempDir {
    let root = TempDir::new().expect("create Cargo workspace");
    std::fs::create_dir(root.path().join("src")).expect("create Cargo source directory");
    std::fs::write(root.path().join("src/lib.rs"), source).expect("write Cargo source");
    std::fs::write(
        root.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}",
        ),
    )
    .expect("write Cargo manifest");
    std::fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("cargo:\n  enabled: true\n  indexUrl: {index_url}\n"),
    )
    .expect("enable Cargo dependency management");
    root
}

fn install_in(root: &TempDir, args: &[&str]) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", root.path().join("cache"))
        .with_env("PNPM_CONFIG_STORE_DIR", root.path().join("store"))
        .with_args(args)
        .assert()
        .success();
}

#[test]
fn install_resolves_and_downloads_through_the_configured_registry() {
    let mut registry = mockito::Server::new();
    let archive = crate_archive("demo", "1.0.0");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let _config_mock = registry
        .mock("GET", "/config.json")
        .with_body(
            serde_json::json!({
                "dl": format!("{}/dl/{{crate}}/{{version}}", registry.url()),
                "api": registry.url(),
            })
            .to_string(),
        )
        .create();
    let index_mock = registry
        .mock("GET", "/de/mo/demo")
        .with_body(format!(
            "{}\n",
            serde_json::json!({
                "name": "demo",
                "vers": "1.0.0",
                "deps": [],
                "cksum": checksum,
                "features": {},
                "yanked": false,
                "v": 1,
            }),
        ))
        .expect(1)
        .create();
    let download_mock =
        registry.mock("GET", "/dl/demo/1.0.0").with_body(&archive).expect(1).create();
    let root = cargo_workspace(&registry.url(), "demo = \"1\"\n", "pub use demo::answer;\n");

    install_in(&root, &["install"]);

    let lockfile =
        std::fs::read_to_string(root.path().join("Cargo.lock")).expect("read Cargo.lock");
    assert!(lockfile.contains(&format!(r#"source = "sparse+{}/""#, registry.url())), "{lockfile}");
    assert!(root.path().join(".pnpm/crates/crates-io/demo-1.0.0/src/lib.rs").is_file());
    Command::new("cargo")
        .with_current_dir(root.path())
        .with_args(["check", "--offline"])
        .assert()
        .success();

    index_mock.assert();
    download_mock.assert();
}

#[test]
fn offline_install_without_registry_crates_never_reads_the_registry_config() {
    let root = cargo_workspace("https://registry.example.test/index/", "", "");

    install_in(&root, &["install", "--offline"]);

    assert!(root.path().join("Cargo.lock").is_file());
    let config = std::fs::read_to_string(root.path().join(".cargo/config.toml"))
        .expect("read managed Cargo configuration");
    assert!(config.contains(r#"registry = "sparse+https://registry.example.test/index/""#));
}

fn append_manifest_section(manifest: &Path, section: &str) {
    let mut contents = fs::read_to_string(manifest).expect("read Cargo manifest");
    contents.push_str(section);
    fs::write(manifest, contents).expect("write Cargo manifest");
}

/// A repository holding a Cargo workspace of two crates, `patched` and
/// the `sibling` it depends on by path. Both take their version from the
/// workspace, which the vendored copies must carry on their own.
fn patched_repository(root: &Path) -> GitRepoFixture {
    let repository = GitRepoFixture::init(root, "patched");
    repository.write_file(
        "Cargo.toml",
        "[workspace]\nmembers = [\"patched\", \"sibling\"]\n\n[workspace.package]\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    repository.write_file(
        "patched/Cargo.toml",
        "[package]\nname = \"patched\"\nversion.workspace = true\nedition.workspace = true\n\n[dependencies]\nsibling = { path = \"../sibling\", version = \"1.0.0\" }\n",
    );
    repository.write_file("patched/src/lib.rs", "pub fn patched() -> u8 { sibling::seven() }\n");
    repository.write_file(
        "sibling/Cargo.toml",
        "[package]\nname = \"sibling\"\nversion.workspace = true\nedition.workspace = true\n",
    );
    repository.write_file("sibling/src/lib.rs", "pub fn seven() -> u8 { 7 }\n");
    repository
}

#[test]
fn install_vendors_a_git_patched_crate_beside_the_registry_crates() {
    let mut registry = mockito::Server::new();
    let archive = crate_archive("demo", "1.0.0");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let _config_mock = registry
        .mock("GET", "/config.json")
        .with_body(
            serde_json::json!({
                "dl": format!("{}/dl/{{crate}}/{{version}}", registry.url()),
                "api": registry.url(),
            })
            .to_string(),
        )
        .create();
    let _download_mock = registry.mock("GET", "/dl/demo/1.0.0").with_body(&archive).create();
    let root = cargo_workspace(
        &registry.url(),
        "demo = \"1\"\npatched = \"1\"\n",
        "pub use demo::answer;\npub use patched::patched;\n",
    );
    // Outside the workspace: a manifest under it would be discovered as a
    // Cargo workspace of its own.
    let outside = TempDir::new().expect("create a repository directory");
    let repository = patched_repository(outside.path());
    let repository_url = repository.file_url();
    let commit = repository.commit("init");
    append_manifest_section(
        &root.path().join("Cargo.toml"),
        &format!(
            "\n[patch.crates-io]\npatched = {{ git = \"{repository_url}\", rev = \"{commit}\" }}\n",
        ),
    );
    let lockfile = format!(
        concat!(
            "version = 4\n\n",
            "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
            "dependencies = [\n \"demo\",\n \"patched\",\n]\n\n",
            "[[package]]\nname = \"demo\"\nversion = \"1.0.0\"\n",
            "source = \"sparse+{registry}/\"\nchecksum = \"{checksum}\"\n\n",
            "[[package]]\nname = \"patched\"\nversion = \"1.0.0\"\n",
            "source = \"git+{repository_url}?rev={commit}#{commit}\"\n",
            "dependencies = [\n \"sibling\",\n]\n\n",
            "[[package]]\nname = \"sibling\"\nversion = \"1.0.0\"\n",
            "source = \"git+{repository_url}?rev={commit}#{commit}\"\n",
        ),
        registry = registry.url(),
        checksum = checksum,
        repository_url = repository_url,
        commit = commit,
    );
    std::fs::write(root.path().join("Cargo.lock"), &lockfile).unwrap();

    install_in(&root, &["install", "--frozen-lockfile"]);

    assert_eq!(fs::read_to_string(root.path().join("Cargo.lock")).unwrap(), lockfile);
    assert!(root.path().join(".pnpm/crates/crates-io/demo-1.0.0/src/lib.rs").is_file());
    assert!(root.path().join(".pnpm/crates/git/patched-1.0.0/src/lib.rs").is_file());
    assert!(root.path().join(".pnpm/crates/git/sibling-1.0.0/src/lib.rs").is_file());
    let config = fs::read_to_string(root.path().join(".cargo/config.toml")).unwrap();
    assert!(
        config.contains(&format!(r#"[source."git+{repository_url}?rev={commit}"]"#)),
        "{config}",
    );
    assert!(config.contains(r#"replace-with = "pnpm-git""#), "{config}");
    // Offline, so the revision has to come from what the install vendored.
    Command::new("cargo")
        .with_current_dir(root.path())
        .with_args(["check", "--offline"])
        .assert()
        .success();
}

#[test]
fn a_workspace_with_a_cargo_patch_is_not_resolved_from_the_registry() {
    let root = cargo_workspace("https://registry.example.test/index/", "demo = \"1\"\n", "");
    append_manifest_section(
        &root.path().join("Cargo.toml"),
        "\n[patch.crates-io]\ndemo = { git = \"https://example.test/demo\" }\n",
    );

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_env("PNPM_CONFIG_CACHE_DIR", root.path().join("cache"))
        .with_env("PNPM_CONFIG_STORE_DIR", root.path().join("store"))
        .with_args(["install", "--offline"])
        .output()
        .expect("run pnpm install");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    // One word: a diagnostic wraps at a width the path length decides.
    assert!(stderr.contains("[patch]"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn reuses_workspace_metadata_with_aliased_paths_and_nested_workspaces() {
    use std::os::unix::fs::PermissionsExt;

    let repository = TempDir::new().unwrap();
    let root = repository.path().join("workspace");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("pnpm-workspace.yaml"), "packages: []\ncargo:\n  enabled: true\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    for (name, workspace) in [("a", ""), ("b", ""), ("c", ""), ("a/nested", "[workspace]\n")] {
        let project = root.join(name);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/lib.rs"), "").unwrap();
        let package = name.replace('/', "-");
        fs::write(project.join("Cargo.toml"), format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{workspace}")).unwrap();
    }
    for project in [&root, &root.join("a/nested")] {
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(project)
            .assert()
            .success();
    }
    let original_path = std::env::var_os("PATH").unwrap();
    let cargo = std::env::split_paths(&original_path)
        .map(|directory| directory.join("cargo"))
        .find(|path| path.is_file())
        .expect("cargo on PATH");
    let bin = repository.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("cargo");
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$CARGO_CALL_LOG\"\nexec \"$REAL_CARGO\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&original_path)))
            .unwrap();
    let log = repository.path().join("cargo-calls");
    for alias in [root.clone(), root.join("../workspace"), std::path::PathBuf::from(".")] {
        fs::write(&log, "").unwrap();
        Command::cargo_bin("pnpm")
            .unwrap()
            .current_dir(&root)
            .args(["install", "--frozen-lockfile"])
            .env("PATH", &path)
            .env("REAL_CARGO", &cargo)
            .env("CARGO_CALL_LOG", &log)
            .env("NPM_CONFIG_WORKSPACE_DIR", &alias)
            .env("PNPM_CONFIG_CACHE_DIR", repository.path().join("cache"))
            .env("PNPM_CONFIG_STORE_DIR", repository.path().join("store"))
            .assert()
            .success();
        assert_eq!(fs::read_to_string(&log).unwrap(), "metadata\nmetadata\n", "{alias:?}");
    }
}
