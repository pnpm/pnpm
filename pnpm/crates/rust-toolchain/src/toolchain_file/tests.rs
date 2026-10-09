use super::{
    Profile, ToolchainRequest, Unmanaged, find_toolchain_file, parse_toolchain_file, with_channel,
};
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
    assert_eq!(
        parse_toolchain_file("[toolchain]\nchannel = \"stable\"\nprofile = \"complete\"\n")
            .unwrap(),
        Err(Unmanaged::UnsupportedProfile("complete".to_string())),
    );
}

#[test]
fn refuses_what_rustup_would_refuse() {
    for contents in [
        "[toolchain]\nchannel = \"stable\"\npath = \"/opt/rust\"\n",
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

#[cfg(unix)]
#[test]
fn a_toolchain_file_linked_outside_the_checkout_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("checkout");
    fs::create_dir_all(&checkout).unwrap();
    fs::write(root.path().join("secret"), "stable").unwrap();
    std::os::unix::fs::symlink(root.path().join("secret"), checkout.join("rust-toolchain"))
        .unwrap();

    assert_eq!(find_toolchain_file(&checkout, &checkout), None);
}

#[test]
fn sets_the_channel_and_keeps_the_rest_of_the_file() {
    let channel = Channel::parse("1.96.0").unwrap();
    assert_eq!(with_channel(None, &channel).unwrap(), "[toolchain]\nchannel = \"1.96.0\"\n");
    assert_eq!(with_channel(Some("1.95.0\n"), &channel).unwrap(), "1.96.0\n");
    assert_eq!(
        with_channel(
            Some(
                "# pinned for CI\n[toolchain]\n  channel = \"1.95.0\" # bump with care\ncomponents = [\"rust-src\"]\n"
            ),
            &channel,
        )
        .unwrap(),
        "# pinned for CI\n[toolchain]\n  channel = \"1.96.0\" # bump with care\ncomponents = [\"rust-src\"]\n",
    );
    assert_eq!(
        with_channel(Some("[toolchain]\nprofile = \"minimal\"\n"), &channel).unwrap(),
        "[toolchain]\nchannel = \"1.96.0\"\nprofile = \"minimal\"\n",
    );
}

#[test]
fn keeps_a_comment_only_file() {
    let channel = Channel::parse("1.96.0").unwrap();
    assert_eq!(
        with_channel(Some("# pinned by CI\n"), &channel).unwrap(),
        "# pinned by CI\n\n[toolchain]\nchannel = \"1.96.0\"\n",
    );
    assert_eq!(
        with_channel(Some("# the toolchain CI uses\n"), &channel).unwrap(),
        "# the toolchain CI uses\n\n[toolchain]\nchannel = \"1.96.0\"\n",
    );
}

#[test]
fn refuses_a_file_it_cannot_update_faithfully() {
    let channel = Channel::parse("1.96.0").unwrap();
    for contents in [
        "[toolchain]\npath = \"/opt/rust\"\n",
        "toolchain = { channel = \"1.95.0\" }\n",
        "toolchain.channel = \"1.95.0\"\n",
    ] {
        let error = with_channel(Some(contents), &channel).expect_err(contents);
        eprintln!("{contents:?}: {error}");
    }
}

#[test]
fn sets_the_channel_of_a_toolchain_table_before_another_table() {
    let channel = Channel::parse("1.96.0").unwrap();
    assert_eq!(
        with_channel(
            Some("[toolchain]\nprofile = \"minimal\"\n\n[other]\nchannel = \"x\"\n"),
            &channel
        )
        .unwrap(),
        "[toolchain]\nchannel = \"1.96.0\"\nprofile = \"minimal\"\n\n[other]\nchannel = \"x\"\n",
    );
}

#[test]
fn adds_targets_but_not_target_specification_files() {
    let request = ToolchainRequest::for_channel(Channel::parse("nightly").unwrap())
        .with_targets([
            "wasm32-wasip1-threads",
            "./custom.json",
            "custom.json",
            "host-tuple",
            "thumbv8m.main-none-eabi",
            "aarch64-apple-darwin",
        ])
        .with_targets(["wasm32-wasip1-threads"]);
    assert_eq!(request.profile, Profile::Default);
    assert_eq!(
        request.targets,
        ["aarch64-apple-darwin", "thumbv8m.main-none-eabi", "wasm32-wasip1-threads"],
    );
}
