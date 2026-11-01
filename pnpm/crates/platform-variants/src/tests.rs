use super::{
    AmbiguousSelection, Floor, Host, HostFloors, Libc, NodeAbi, Platform, select,
    variant_manifests, variant_tag,
};
use pretty_assertions::assert_eq;

fn tag(value: &str) -> Platform {
    Platform::parse_tag(value).unwrap()
}

fn linux_host(libc: Option<Libc>, glibc: Option<(u32, u32)>, napi: Option<u32>) -> Host {
    Host {
        os: "linux".into(),
        cpu: "x64".into(),
        libc,
        floors: HostFloors {
            libc_version: glibc.map(|(major, minor)| Floor { major, minor }),
            napi,
            ..HostFloors::default()
        },
    }
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

#[test]
fn variant_tags_sort_platform_tags() {
    let platforms = [tag("linux-x64-musl"), tag("linux-x64")];
    assert_eq!(variant_tag(&platforms).unwrap(), "linux-x64+linux-x64-musl");
}

#[test]
fn selection_prefers_more_fields_then_higher_floors_then_wasm() {
    let plain = [tag("linux-x64")];
    let old = [tag("linux-x64-manylinux_2_17")];
    let new = [tag("linux-x64-manylinux_2_28")];
    let wasm = [tag("wasm32")];
    let variants: [&[Platform]; 4] = [&wasm, &plain, &old, &new];
    let host = linux_host(None, Some((2, 31)), None);
    assert_eq!(select(variants, &host), Ok(Some(3)));
    let old_host = linux_host(None, Some((2, 20)), None);
    assert_eq!(select(variants, &old_host), Ok(Some(2)));
    let musl = linux_host(Some(Libc::Musl), None, None);
    assert_eq!(select(variants, &musl), Ok(Some(0)));
}

#[test]
fn an_unknown_abi_never_matches() {
    let variants: [&[Platform]; 1] = [&[tag("linux-x64-node127")]];
    assert_eq!(select(variants, &linux_host(None, None, None)), Ok(None));
}

#[test]
fn equal_fits_are_ambiguous() {
    let first = [tag("linux-x64")];
    let second = [tag("linux-x64")];
    let variants: [&[Platform]; 2] = [&first, &second];
    assert_eq!(
        select(variants, &linux_host(None, None, None)),
        Err(AmbiguousSelection { first: 0, second: 1 }),
    );
}

#[test]
fn variant_manifests_apply_each_difference() {
    let manifest = serde_json::json!({
        "name": "fast",
        "dependencies": { "a": "1" },
        "engines": { "node": ">=18" },
        "dist": { "pnpr": { "variants": [
            {
                "platforms": [{ "os": "linux", "cpu": "x64", "libc": "glibc" }],
                "manifest": { "dependencies": { "a": "1", "b": "2" }, "engines": null },
            },
            { "platforms": [{ "os": "linux", "cpu": "x64", "cuda": "12" }], "manifest": {} },
        ] } },
    });
    let variants = variant_manifests(&manifest);
    assert_eq!(variants.len(), 1, "a variant with an unknown field is left out");
    assert_eq!(variants[0].0, "linux-x64");
    assert_eq!(variants[0].1["dependencies"], serde_json::json!({ "a": "1", "b": "2" }));
    assert!(variants[0].1.get("engines").is_none());
}
