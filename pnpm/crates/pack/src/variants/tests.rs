use super::{
    VariantArchiveError, VariantDeclaration, VariantDeclarationError, archive_file_name,
    check_archive,
};
use serde_json::{Value, json};

fn base(variants: &Value) -> Value {
    json!({
        "name": "example-native-addon",
        "version": "2.3.0",
        "main": "index.js",
        "peerDependencies": { "react": "*" },
        "publishConfig": { "access": "public", "variants": variants },
    })
}

fn read(variants: &Value) -> Result<VariantDeclaration, VariantDeclarationError> {
    VariantDeclaration::read(&base(variants)).map(Option::unwrap)
}

#[test]
fn derives_each_platform_variants_tag_from_its_targets() {
    let declaration = read(&json!([
        { "targets": ["any"], "files": ["lib/**"] },
        { "targets": ["linux-x64-musl", "linux-x64"], "files": ["native/linux"] },
        { "targets": ["darwin-x64", "darwin-arm64"] },
    ]))
    .unwrap();
    let tags: Vec<Option<&str>> = declaration.entries
        .iter()
        .map(|entry| entry.tag.as_deref())
        .collect();
    assert_eq!(tags, [None, Some("linux-x64+linux-x64-musl"), Some("darwin-arm64+darwin-x64")]);
    assert!(declaration.primary().unwrap().is_any());
    let found = declaration.find("linux-x64-musl").unwrap();
    assert_eq!(found.tag.as_deref(), Some("linux-x64+linux-x64-musl"));
    assert_eq!(declaration.find("linux-x64").unwrap(), found, "any target names the entry");
    assert!(
        declaration
            .find("any")
            .unwrap()
            .is_any(),
    );
    assert!(matches!(
        declaration.find("win32-x64"),
        Err(VariantDeclarationError::NoSuchEntry { .. }),
    ));
}

#[test]
fn a_declaration_without_an_any_entry_has_no_primary() {
    let declaration = read(&json!([{ "targets": ["linux-x64"] }])).unwrap();
    assert!(declaration.primary().is_none());
    assert_eq!(
        VariantDeclaration::read(&json!({ "name": "a", "version": "1.0.0" })).unwrap(),
        None,
    );
}

#[test]
fn rejects_targets_that_are_not_canonical_tags_with_their_spelling() {
    for (target, canonical) in
        [("linux-x64-gnu", "linux-x64"), ("linux-x64-napi8-musl", "linux-x64-musl-napi8")]
    {
        match read(&json!([{ "targets": [target] }])) {
            Err(VariantDeclarationError::NonCanonicalTarget { canonical: spelled, .. }) => {
                assert_eq!(spelled, canonical);
            }
            other => panic!("{target}: {other:?}"),
        }
    }
    assert!(matches!(
        read(&json!([{ "targets": ["linux-x64-riscvfloat9"] }])),
        Err(VariantDeclarationError::UnknownTarget { .. }),
    ));
}

#[test]
fn rejects_targets_declared_twice_and_any_beside_a_platform() {
    for variants in [
        json!([{ "targets": ["linux-x64", "linux-x64"] }]),
        json!([{ "targets": ["linux-x64"] }, { "targets": ["linux-x64"] }]),
        json!([{ "targets": ["any"] }, { "targets": ["any"] }]),
    ] {
        assert!(
            matches!(read(&variants), Err(VariantDeclarationError::DuplicateTarget { .. })),
            "{variants}",
        );
    }
    assert!(matches!(
        read(&json!([{ "targets": ["any", "linux-x64"] }])),
        Err(VariantDeclarationError::AnyWithPlatforms { .. }),
    ));
}

#[test]
fn platforms_that_could_tie_are_the_same_target() {
    let tie = read(&json!([
        { "targets": ["linux-x64-musl"] },
        { "targets": ["linux-arm64", "wasm32"] },
        { "targets": ["linux-x64", "wasm32"] },
    ]));
    assert!(matches!(tie, Err(VariantDeclarationError::DuplicateTarget { .. })), "{tie:?}");
    let floors = read(&json!([
        { "targets": ["linux-x64-manylinux_2_17"] },
        { "targets": ["linux-x64-manylinux_2_28"] },
        { "targets": ["linux-x64-napi8"] },
    ]));
    assert!(floors.is_ok(), "a host ranks every one of them differently");
}

#[test]
fn rejects_a_tag_over_the_limit() {
    let targets: Vec<String> = (100..112)
        .map(|abi| format!("linux-x64-manylinux_2_17-node{abi}"))
        .collect();
    assert!(matches!(
        read(&json!([{ "targets": targets }])),
        Err(VariantDeclarationError::TagTooLong { .. }),
    ));
}

#[test]
fn rejects_manifest_fields_every_archive_shares_or_a_variant_cannot_set() {
    for (variants, field) in [
        (json!([{ "targets": ["any"], "manifest": { "name": "other" } }]), "name"),
        (
            json!([{ "targets": ["linux-x64"], "manifest": { "peerDependencies": {} } }]),
            "peerDependencies",
        ),
        (json!([{ "targets": ["linux-x64"], "manifest": { "variant": {} } }]), "variant"),
        (json!([{ "targets": ["linux-x64"], "manifest": { "os": ["linux"] } }]), "os"),
    ] {
        match read(&variants) {
            Err(VariantDeclarationError::ForbiddenField { field: found, .. }) => {
                assert_eq!(found, field);
            }
            other => panic!("{field}: {other:?}"),
        }
    }
    assert!(read(&json!([{ "targets": ["any"], "manifest": { "os": ["linux"] } }])).is_ok());
    let mut inherited = base(&json!([{ "targets": ["linux-x64"] }]));
    inherited["cpu"] = json!(["x64"]);
    assert!(matches!(
        VariantDeclaration::read(&inherited),
        Err(VariantDeclarationError::BaseFieldInherited { .. }),
    ));
    let mut marked = base(&json!([{ "targets": ["any"] }]));
    marked["variant"] = json!({ "platforms": ["linux-x64"] });
    let read_marked = VariantDeclaration::read(&marked);
    assert!(matches!(read_marked, Err(VariantDeclarationError::BaseVariant)), "{read_marked:?}");
}

#[test]
fn rejects_a_declaration_without_entries() {
    match VariantDeclaration::read(&base(&json!([]))) {
        Err(VariantDeclarationError::Shape { reason }) => {
            assert!(reason.contains("at least one entry"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_archive_manifest_applies_its_entry_to_the_base_manifest() {
    let variants = json!([
        { "targets": ["any"], "manifest": { "scripts": { "install": "node-gyp rebuild" } } },
        {
            "targets": ["linux-x64-musl", "linux-x64"],
            "manifest": { "main": null, "bin": { "example": "native/example" } },
        },
    ]);
    let manifest = base(&variants);
    let declaration = read(&variants).unwrap();
    assert_eq!(
        VariantDeclaration::archive_manifest(&manifest, &declaration.entries[1]),
        json!({
            "name": "example-native-addon",
            "version": "2.3.0",
            "peerDependencies": { "react": "*" },
            "publishConfig": { "access": "public" },
            "bin": { "example": "native/example" },
            "variant": { "platforms": ["linux-x64", "linux-x64-musl"] },
        }),
    );
    let primary = VariantDeclaration::archive_manifest(&manifest, &declaration.entries[0]);
    assert!(primary.get("variant").is_none());
    assert_eq!(primary["scripts"]["install"], "node-gyp rebuild");
}

#[test]
fn checks_a_packed_archive_as_pnpr_does() {
    let variants = json!([{ "targets": ["any"] }, { "targets": ["linux-x64"] }]);
    let manifest = base(&variants);
    let declaration = read(&variants).unwrap();
    let (any, linux) = (&declaration.entries[0], &declaration.entries[1]);
    let archive = VariantDeclaration::archive_manifest(&manifest, linux);
    check_archive(&manifest, linux, &archive, &["index.js"]).unwrap();
    assert!(matches!(
        check_archive(&manifest, linux, &archive, &["binding.gyp"]),
        Err(VariantArchiveError::RequiresBuild { .. }),
    ));
    let any_archive = VariantDeclaration::archive_manifest(&manifest, any);
    check_archive(&manifest, any, &any_archive, &["binding.gyp"]).unwrap();
    let mut other_peers = archive;
    other_peers["peerDependencies"] = json!({});
    assert!(matches!(
        check_archive(&manifest, linux, &other_peers, &["index.js"]),
        Err(VariantArchiveError::Peers { .. }),
    ));
}

#[test]
fn names_a_platform_variants_archive_after_the_primarys() {
    let declaration =
        read(&json!([{ "targets": ["any"] }, { "targets": ["linux-x64", "linux-x64-musl"] }]))
            .unwrap();
    let name = "example-native-addon-2.3.0.tgz";
    assert_eq!(
        archive_file_name(name, "example-native-addon", &declaration.entries[0]).unwrap(),
        name,
    );
    assert_eq!(
        archive_file_name(name, "example-native-addon", &declaration.entries[1]).unwrap(),
        "example-native-addon-2.3.0~linux-x64+linux-x64-musl.tgz",
    );
    let long = format!("{}-2.3.0.tgz", "a".repeat(230));
    assert!(matches!(
        archive_file_name(&long, "long", &declaration.entries[1]),
        Err(VariantArchiveError::FileNameTooLong { .. }),
    ));
}
