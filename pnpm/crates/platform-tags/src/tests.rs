use super::{
    Floor,
    Libc,
    NodeAbi,
    Platform,
};
use pretty_assertions::assert_eq;

fn tag(value: &str) -> Platform {
    Platform::parse_tag(value).unwrap()
}

#[test]
fn canonical_tags_round_trip() {
    for value in [
        "linux-x64",
        "linux-x64-musl",
        "linux-x64-manylinux_2_28-napi8",
        "linux-arm64-node127",
        "darwin-arm64-macos_11_0",
        "win32-x64-windows_10_0-electron130",
        "linux-x64-napi8",
        "freebsd-x64",
        "wasm32",
    ] {
        assert_eq!(tag(value).tag().unwrap(), value);
    }
}

#[test]
fn parses_the_structured_fields() {
    assert_eq!(
        tag("linux-x64-manylinux_2_17-napi8"),
        Platform {
            os: Some("linux".into()),
            cpu: Some("x64".into()),
            libc: Some(Libc::Glibc),
            libc_version: Some(Floor { major: 2, minor: 17 }),
            napi: Some(8),
            ..Platform::default()
        },
    );
    assert_eq!(tag("linux-x64-node127").node_abi, Some(NodeAbi::Node(127)));
}

#[test]
fn rejects_non_canonical_tags() {
    for value in [
        "linux",
        "linux-x64-glibc",
        "darwin-x64-musl",
        "linux-x64-musl-manylinux_2_17",
        "linux-x64-macos_11_0",
        "darwin-arm64-macos_11",
        "darwin-arm64-macos_011_0",
        "linux-x64-napi8-musl",
        "linux-X64",
        "linux-x64-",
        "linux-x64-napi8-node127",
    ] {
        assert!(Platform::parse_tag(value).is_err(), "{value} parsed");
    }
}

#[test]
fn floors_accept_one_or_two_numbers() {
    assert_eq!("11".parse::<Floor>().unwrap(), Floor { major: 11, minor: 0 });
    assert_eq!("2.28".parse::<Floor>().unwrap(), Floor { major: 2, minor: 28 });
}

#[test]
fn the_structured_form_serializes_as_the_document_writes_it() {
    let platform = tag("linux-x64-manylinux_2_17-napi8");
    assert_eq!(
        serde_json::to_value(&platform).unwrap(),
        serde_json::json!({
            "os": "linux", "cpu": "x64", "libc": "glibc", "libcVersion": "2.17", "napi": 8,
        }),
    );
    assert!(
        serde_json::from_value::<Platform>(serde_json::json!({
            "os": "linux", "cpu": "x64", "cuda": "12",
        }))
        .is_err(),
    );
}
