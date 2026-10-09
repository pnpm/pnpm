use super::{rust_release, rust_request};
use pnpm_rust_toolchain::{Channel, Profile, ToolchainRequest};
use std::fs;

#[test]
fn rust_names_a_release_for_its_tools() {
    let package = |spec: &str| vec![spec.to_string()];
    let release = |spec: &str, command: &str| {
        rust_release(&package(spec), command).map(|channel| channel.to_string())
    };
    assert_eq!(release("rust@nightly-2026-08-27", "cargo").as_deref(), Some("nightly-2026-08-27"));
    assert_eq!(release("rust@1.95", "rustfmt").as_deref(), Some("1.95"));
    assert_eq!(release("rust", "rustc").as_deref(), Some("stable"));
    // Not a tool of the toolchain, or not a channel: the registry package.
    assert_eq!(release("rust@1.95", "rust"), None);
    assert_eq!(release("rust@latest", "cargo"), None);
    assert_eq!(release("rusty@1.95", "cargo"), None);
    assert_eq!(rust_release(&[], "cargo"), None);
}

#[test]
fn a_release_keeps_the_project_selection_and_adds_targets() {
    let root = tempfile::tempdir().unwrap();
    let member = root.path().join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(
        root.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.95.0\"\nprofile = \"minimal\"\ncomponents = [\"rust-src\"]\n",
    )
    .unwrap();
    let args = [
        "build",
        "--target",
        "wasm32-wasip1",
        "--target=x86_64-unknown-linux-musl",
        "--",
        "--target",
        "ignored",
    ]
    .map(String::from);

    let request = rust_request(Channel::parse("nightly").unwrap(), &member, &args).unwrap();

    assert_eq!(request.channel.to_string(), "nightly");
    assert_eq!(request.profile, Profile::Minimal);
    assert_eq!(request.components, ["rust-src"]);
    assert_eq!(request.targets, ["wasm32-wasip1", "x86_64-unknown-linux-musl"]);

    let elsewhere = tempfile::tempdir().unwrap();
    let request = rust_request(Channel::parse("nightly").unwrap(), elsewhere.path(), &[]).unwrap();
    assert_eq!(request, ToolchainRequest::for_channel(Channel::parse("nightly").unwrap()));
}

#[test]
fn a_toolchain_file_that_cannot_be_read_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let nightly = || Channel::parse("nightly").unwrap();
    fs::write(root.path().join("rust-toolchain.toml"), "[toolchain\n").unwrap();
    assert!(rust_request(nightly(), root.path(), &[]).is_err());

    // A file rustup handles itself is not one pnpm reads.
    fs::write(root.path().join("rust-toolchain.toml"), "[toolchain]\npath = \"/opt/rust\"\n")
        .unwrap();
    assert_eq!(
        rust_request(nightly(), root.path(), &[]).unwrap(),
        ToolchainRequest::for_channel(nightly()),
    );
}
