use super::{
    cargo_workspace,
    crate_archive_with_manifest,
    install_in,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use sha2::{
    Digest,
    Sha256,
};
use std::{
    fs,
    process::Command,
    str::FromStr,
};

#[test]
fn weak_optional_dependencies_match_cargo_without_activating_them_for_builds() {
    let mut registry = mockito::Server::new();
    let index_url = format!("sparse+{}/", registry.url());
    let config = registry
        .mock("GET", "/config.json")
        .with_body(
            serde_json::json!({"dl": format!("{}/dl/{{crate}}/{{version}}", registry.url())})
                .to_string(),
        )
        .expect_at_least(1)
        .create();
    let packages = [
        (
            "demo",
            r#"[package]
name = "demo"
version = "1.0.0"
[features]
default = ["helper?/extra"]
unused = ["dep:unused"]
[target.'cfg(windows)'.dependencies]
helper = { version = "1", optional = true, default-features = false }
[dependencies]
unused = { version = "1", optional = true }
"#,
            serde_json::json!([
                {"name":"helper","req":"^1","features":[],"optional":true,"default_features":false,"target":"cfg(windows)","kind":"normal"},
                {"name":"unused","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal"},
            ]),
            serde_json::json!({"default":["helper?/extra"],"unused":["dep:unused"]}),
        ),
        (
            "helper",
            r#"[package]
name = "helper"
version = "1.0.0"
[features]
extra = ["leaf?/std"]
[target.'cfg(unix)'.dependencies]
leaf = { version = "1", optional = true, default-features = false }
"#,
            serde_json::json!([
                {"name":"leaf","req":"^1","features":[],"optional":true,"default_features":false,"target":"cfg(unix)","kind":"normal"},
            ]),
            serde_json::json!({"extra":["leaf?/std"]}),
        ),
        (
            "leaf",
            r#"[package]
name = "leaf"
version = "1.0.0"
[features]
std = []
"#,
            serde_json::json!([]),
            serde_json::json!({"std":[]}),
        ),
    ];
    let unused = registry
        .mock("GET", "/un/us/unused")
        .expect(0)
        .create();
    let mut mocks = vec![config, unused];
    for (name, manifest, deps, features) in packages {
        let archive = crate_archive_with_manifest(name, "1.0.0", manifest);
        let checksum = format!("{:x}", Sha256::digest(&archive));
        let prefix = pnpm_cargo_resolver::index_prefix(name);
        mocks.push(
            registry
                .mock("GET", format!("/{prefix}/{name}").as_str())
                .with_body(
                    serde_json::json!({
                        "name":name,"vers":"1.0.0","deps":deps,"cksum":checksum,
                        "features":{},"features2":features,"yanked":false,"v":2,
                    })
                    .to_string(),
                )
                .expect_at_least(1)
                .create(),
        );
        mocks.push(
            registry
                .mock("GET", format!("/dl/{name}/1.0.0").as_str())
                .with_body(archive)
                .expect_at_least(1)
                .create(),
        );
    }
    let root = cargo_workspace(&registry.url(), "demo = \"1\"\n", "pub use demo::answer;\n");
    let pristine = cargo_workspace(&registry.url(), "demo = \"1\"\n", "pub use demo::answer;\n");
    let cargo_home = tempfile::TempDir::new().unwrap();
    fs::create_dir(pristine.path().join(".cargo")).unwrap();
    fs::write(pristine.path().join(".cargo/config.toml"), format!(
        "[source.crates-io]\nreplace-with = \"test-registry\"\n[source.test-registry]\nregistry = \"{index_url}\"\n",
    )).unwrap();
    Command::new("cargo")
        .with_current_dir(pristine.path())
        .with_env("CARGO_HOME", cargo_home.path())
        .with_args(["generate-lockfile"])
        .assert()
        .success();
    let expected = fs::read_to_string(pristine.path().join("Cargo.lock")).unwrap();

    install_in(&root, &["install", "--lockfile-only", "--no-frozen-lockfile"]);
    let actual = fs::read_to_string(root.path().join("Cargo.lock")).unwrap();
    eprintln!("Cargo lockfile:\n{expected}\npnpm lockfile:\n{actual}");
    assert_eq!(
        cargo_lock::Lockfile::from_str(&actual).unwrap().packages,
        cargo_lock::Lockfile::from_str(&expected).unwrap().packages,
    );
    assert_eq!(cargo_lock::Lockfile::from_str(&actual).unwrap().packages.len(), 4);
    fs::write(pristine.path().join("Cargo.lock"), &actual).unwrap();
    Command::new("cargo")
        .with_current_dir(pristine.path())
        .with_env("CARGO_HOME", cargo_home.path())
        .with_args(["metadata", "--locked", "--format-version", "1"])
        .assert()
        .success();

    install_in(&root, &["install", "--frozen-lockfile"]);
    let output = Command::new("cargo")
        .with_current_dir(root.path())
        .with_env("CARGO_HOME", cargo_home.path())
        .with_args(["check", "--locked", "--offline", "--message-format=json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut built = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|message| message["reason"] == "compiler-artifact")
        .map(|message| {
            message["target"]["name"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    built.sort();
    assert_eq!(built, ["app", "demo"]);
    assert_eq!(fs::read_to_string(root.path().join("Cargo.lock")).unwrap(), actual);
    for mock in mocks {
        mock.assert();
    }
}
