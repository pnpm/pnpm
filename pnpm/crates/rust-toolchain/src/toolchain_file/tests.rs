use super::{Profile, ToolchainRequest, Unmanaged, find_toolchain_file, parse_toolchain_file};
use crate::Channel;
use pretty_assertions::assert_eq;
use std::fs;

#[test]
fn reads_a_bare_channel_name() {
    let request = parse_toolchain_file("1.90.0\n").unwrap().unwrap();
    assert_eq!(
        request,
        ToolchainRequest {
            channel: Channel::parse("1.90.0").unwrap(),
            profile: Profile::Default,
            components: Vec::new(),
            targets: Vec::new(),
        },
    );
}

#[test]
fn reads_the_toolchain_table() {
    let request = parse_toolchain_file(
        "[toolchain]\nchannel = \"nightly-2025-01-01\"\nprofile = \"minimal\"\ncomponents = [\"rust-src\", \"clippy\", \"rust-src\"]\ntargets = [\"wasm32-unknown-unknown\"]\n",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        request,
        ToolchainRequest {
            channel: Channel::parse("nightly-2025-01-01").unwrap(),
            profile: Profile::Minimal,
            components: vec!["clippy".to_string(), "rust-src".to_string()],
            targets: vec!["wasm32-unknown-unknown".to_string()],
        },
    );
}

#[test]
fn leaves_toolchains_rustup_finds_elsewhere_alone() {
    assert_eq!(
        parse_toolchain_file("[toolchain]\npath = \"/opt/rust\"\n").unwrap(),
        Err(Unmanaged::CustomPath),
    );
    assert_eq!(
        parse_toolchain_file("[toolchain]\ncomponents = [\"rust-src\"]\n").unwrap(),
        Err(Unmanaged::NoChannel),
    );
    assert_eq!(
        parse_toolchain_file("my-toolchain").unwrap(),
        Err(Unmanaged::UnknownChannel("my-toolchain".to_string())),
    );
}

#[test]
fn refuses_what_rustup_would_refuse() {
    for contents in [
        "[toolchain]\nchannel = \"stable\"\npath = \"/opt/rust\"\n",
        "[toolchain]\nchannel = \"stable\"\nprofile = \"complete\"\n",
        "[toolchain]\nchannel = \"stable\"\ncomponents = [\"../x\"]\n",
        "[toolchain\n",
    ] {
        let error = parse_toolchain_file(contents).expect_err(contents);
        eprintln!("{contents:?}: {error}");
    }
}

#[test]
fn finds_the_nearest_file_within_the_boundary() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("checkout");
    let member = checkout.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(root.path().join("rust-toolchain.toml"), "stable").unwrap();
    assert_eq!(find_toolchain_file(&member, &checkout), None);

    fs::write(checkout.join("rust-toolchain.toml"), "stable").unwrap();
    assert_eq!(find_toolchain_file(&member, &checkout), Some(checkout.join("rust-toolchain.toml")));

    fs::write(checkout.join("rust-toolchain"), "stable").unwrap();
    assert_eq!(find_toolchain_file(&member, &checkout), Some(checkout.join("rust-toolchain")));
}
