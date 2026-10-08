use super::{Archive, Manifest, parse_signed};
use crate::{Channel, Profile, RustToolchainError, ToolchainRequest};
use pretty_assertions::assert_eq;
use ssri::{Algorithm, Integrity};

const SIGNED_MANIFEST: &[u8] = include_bytes!("../fixtures/channel-rust-1.8.0.toml");
const SIGNATURE: &[u8] = include_bytes!("../fixtures/channel-rust-1.8.0.toml.asc");
const URL: &str = "https://static.rust-lang.org/dist/channel-rust-1.8.0.toml";
const SERVER: &str = "https://static.rust-lang.org";
const HOST: &str = "x86_64-unknown-linux-gnu";

fn signed_manifest() -> Manifest {
    parse_signed(URL.to_string(), SIGNED_MANIFEST, SIGNATURE).expect("the fixture is signed")
}

fn request(profile: Profile, components: &[&str], targets: &[&str]) -> ToolchainRequest {
    ToolchainRequest {
        channel: Channel::parse("stable").unwrap(),
        profile,
        components: components
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        targets: targets
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    }
}

fn archive(url: &str, hash: &str) -> Archive {
    Archive {
        url: url.to_string(),
        integrity: Integrity::from_hex(hash, Algorithm::Sha256).unwrap(),
    }
}

#[test]
fn refuses_a_manifest_the_rust_release_key_did_not_sign() {
    let tampered =
        String::from_utf8(SIGNED_MANIFEST.to_vec()).unwrap().replacen("3ca355bd", "00000000", 1);
    let error = parse_signed(URL.to_string(), tampered.as_bytes(), SIGNATURE)
        .expect_err("the signature no longer matches");
    assert!(matches!(error, RustToolchainError::SignatureInvalid { .. }), "{error:?}");
}

#[test]
fn a_moving_channel_resolves_to_the_release_version() {
    let manifest = signed_manifest();
    assert_eq!(
        manifest
            .pinned(&Channel::parse("stable").unwrap())
            .unwrap(),
        Channel::parse("1.8.0").unwrap(),
    );
    assert_eq!(
        manifest
            .pinned(&Channel::parse("nightly").unwrap())
            .unwrap(),
        Channel::parse("nightly-2016-04-12").unwrap(),
    );
}

#[test]
fn selects_the_profile_components_the_release_publishes() {
    let manifest = signed_manifest();
    let pinned = Channel::parse("1.8.0").unwrap();
    let archives = manifest
        .archives(SERVER, &pinned, HOST, &request(Profile::Default, &[], &[]))
        .unwrap();
    // 1.8.0 published no rustfmt or clippy, which the profile does without.
    assert_eq!(
        archives,
        [
            archive(
                "https://static.rust-lang.org/dist/2016-04-12/rustc-1.8.0-x86_64-unknown-linux-gnu.tar.gz",
                "3ca355bdd81332641aef985b6699f0f4a6a7ef778b5ee3079bcd42bdf270f5f1",
            ),
            archive(
                "https://static.rust-lang.org/cargo-dist/2016-03-01/cargo-nightly-x86_64-unknown-linux-gnu.tar.gz",
                "b0681ec71318644fd8602ce836f06bf4e5491c57d4a2dcc47bb838258313c36f",
            ),
            archive(
                "https://static.rust-lang.org/dist/2016-04-12/rust-std-1.8.0-x86_64-unknown-linux-gnu.tar.gz",
                "4a4b4bb06dc6d35a506bd465873ca35bdd14f01227ddcd02523f6cfcb9028435",
            ),
        ],
    );
}

#[test]
fn listed_components_and_targets_are_required() {
    let manifest = signed_manifest();
    let pinned = Channel::parse("1.8.0").unwrap();
    let archives = manifest
        .archives(
            "https://mirror.example.com",
            &pinned,
            HOST,
            &request(Profile::Minimal, &["rust-docs"], &["x86_64-pc-windows-msvc"]),
        )
        .unwrap();
    let urls = archives
        .iter()
        .map(|archive| archive.url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        urls,
        [
            "https://mirror.example.com/dist/2016-04-12/rustc-1.8.0-x86_64-unknown-linux-gnu.tar.gz",
            "https://mirror.example.com/cargo-dist/2016-03-01/cargo-nightly-x86_64-unknown-linux-gnu.tar.gz",
            "https://mirror.example.com/dist/2016-04-12/rust-std-1.8.0-x86_64-unknown-linux-gnu.tar.gz",
            "https://mirror.example.com/dist/2016-04-12/rust-docs-1.8.0-x86_64-unknown-linux-gnu.tar.gz",
            "https://mirror.example.com/dist/2016-04-12/rust-std-1.8.0-x86_64-pc-windows-msvc.tar.gz",
        ],
    );

    let error = manifest
        .archives(SERVER, &pinned, HOST, &request(Profile::Minimal, &["clippy"], &[]))
        .expect_err("1.8.0 published no clippy");
    assert!(matches!(error, RustToolchainError::ComponentUnavailable { .. }), "{error:?}");
}

#[test]
fn a_host_without_a_toolchain_is_refused() {
    let manifest = signed_manifest();
    let error = manifest
        .archives(
            SERVER,
            &Channel::parse("1.8.0").unwrap(),
            "aarch64-unknown-linux-gnu",
            &request(Profile::Default, &[], &[]),
        )
        .expect_err("1.8.0 published no aarch64 Linux toolchain");
    assert!(matches!(error, RustToolchainError::NotPublishedForHost { .. }), "{error:?}");
}
