//! `pnpm install` against a Cargo workspace, resolving and downloading
//! through the registry `cargo.indexUrl` selects.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use sha2::{Digest, Sha256};
use std::process::Command;
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
