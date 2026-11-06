use super::{
    PackedPkg, PackedVariant, PublishPackedPkgError, StagedLengths, StagedReference,
    VariantArchive, check_registry, provenance_subjects, reference_staged_archives, sri_sha512,
    staged_archives,
};
use crate::registry_config_keys::parse_supported_registry_url;
use serde_json::json;
use std::collections::HashSet;

fn pkg<'a>(
    manifest: &'a serde_json::Value,
    any: &'a [u8],
    variants: &'a [PackedVariant],
) -> PackedPkg<'a> {
    PackedPkg {
        published_manifest: manifest,
        tarball_data: any,
        tarball_path: "example-cli-4.0.0.tgz",
        contents: &[],
        unpacked_size: 0,
        notes: None,
        variants,
        any_staged: None,
    }
}

fn document() -> serde_json::Value {
    json!({
        "name": "example-cli",
        "versions": {
            "4.0.0": { "name": "example-cli", "version": "4.0.0", "dist": { "integrity": "x" } },
        },
        "_attachments": {
            "example-cli-4.0.0.tgz": { "data": "AAAA", "length": 3 },
        },
    })
}

#[test]
fn references_every_archive_by_integrity_under_its_tag() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0" });
    let variants = [PackedVariant {
        tag: "linux-x64+linux-x64-musl".into(),
        archive: VariantArchive::Packed(b"linux".to_vec()),
    }];
    let pkg = pkg(&manifest, b"any", &variants);
    let mut document = document();
    reference_staged_archives(&mut document, &pkg, ("4.0.0", &StagedLengths::new()));
    assert_eq!(
        document["_attachments"],
        json!({
            "example-cli-4.0.0.tgz": {
                "content_type": "application/octet-stream",
                "integrity": sri_sha512(b"any"),
                "length": 3,
            },
            "example-cli-4.0.0~linux-x64+linux-x64-musl.tgz": {
                "content_type": "application/octet-stream",
                "integrity": sri_sha512(b"linux"),
                "length": 5,
            },
        }),
    );
    assert_eq!(document["versions"]["4.0.0"]["dist"]["integrity"], "x");
    let staged: Vec<String> = staged_archives(&pkg, &sri_sha512(b"any"))
        .into_iter()
        .map(|archive| archive.integrity)
        .collect();
    assert_eq!(staged, [sri_sha512(b"any"), sri_sha512(b"linux")]);
}

#[test]
fn a_version_without_an_any_archive_has_no_dist_and_no_plain_attachment() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0" });
    let variants = [PackedVariant {
        tag: "win32-x64".into(),
        archive: VariantArchive::Packed(b"win".to_vec()),
    }];
    let pkg = pkg(&manifest, b"", &variants);
    let mut document = document();
    reference_staged_archives(&mut document, &pkg, ("4.0.0", &StagedLengths::new()));
    assert!(document["versions"]["4.0.0"].get("dist").is_none(), "{document}");
    let attachments: Vec<&String> = document["_attachments"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(attachments, ["example-cli-4.0.0~win32-x64.tgz"]);
    assert_eq!(staged_archives(&pkg, "unused").len(), 1);
}

#[test]
fn variants_are_published_only_to_a_pnpr_registry() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0" });
    let variants = [PackedVariant {
        tag: "win32-x64".into(),
        archive: VariantArchive::Packed(b"win".to_vec()),
    }];
    let registry =
        parse_supported_registry_url("https://registry.example/").unwrap().normalized_url;
    let refused = check_registry(&pkg(&manifest, b"any", &variants), &HashSet::new(), &registry);
    assert!(matches!(refused, Err(PublishPackedPkgError::VariantsNeedPnpr { .. })));
    let pnpr = HashSet::from([registry.as_str().to_string()]);
    check_registry(&pkg(&manifest, b"any", &variants), &pnpr, &registry).unwrap();
    check_registry(&pkg(&manifest, b"any", &[]), &HashSet::new(), &registry).unwrap();
}

/// The document keys a version without build metadata, as npm does, so a
/// version published without an `any` archive loses its `dist` whatever
/// build metadata its manifest's version carries.
#[test]
fn a_version_with_build_metadata_without_an_any_archive_has_no_dist() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0+build" });
    let variants = [PackedVariant {
        tag: "win32-x64".into(),
        archive: VariantArchive::Packed(b"win".to_vec()),
    }];
    let pkg = pkg(&manifest, b"", &variants);
    let mut document = document();
    reference_staged_archives(&mut document, &pkg, ("4.0.0+build", &StagedLengths::new()));
    assert!(document["versions"]["4.0.0"].get("dist").is_none(), "{document}");
}

#[test]
fn archives_another_machine_staged_are_referenced_by_their_integrity() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0" });
    let staged =
        |integrity: &str| StagedReference { integrity: integrity.into(), label: "x".into() };
    let any = staged("sha512-any");
    let variants = [PackedVariant {
        tag: "win32-x64".into(),
        archive: VariantArchive::Staged(staged("sha512-win")),
    }];
    let mut pkg = pkg(&manifest, b"", &variants);
    pkg.any_staged = Some(&any);
    let lengths = StagedLengths::from([("sha512-any".into(), 7), ("sha512-win".into(), 9)]);
    let mut document = document();
    reference_staged_archives(&mut document, &pkg, ("4.0.0", &lengths));
    assert_eq!(document["versions"]["4.0.0"]["dist"], json!({ "integrity": "sha512-any" }));
    assert_eq!(
        document["_attachments"],
        json!({
            "example-cli-4.0.0.tgz": {
                "content_type": "application/octet-stream",
                "integrity": "sha512-any",
                "length": 7,
            },
            "example-cli-4.0.0~win32-x64.tgz": {
                "content_type": "application/octet-stream",
                "integrity": "sha512-win",
                "length": 9,
            },
        }),
    );
    assert!(staged_archives(&pkg, "sha512-any").is_empty(), "nothing to upload");
}

#[test]
fn a_provenance_statement_names_every_archive_by_its_purl() {
    use sha2::Digest as _;
    use std::fmt::Write as _;

    let hex = |bytes: &[u8]| {
        sha2::Sha512::digest(bytes)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    };
    let manifest = json!({ "name": "@example/cli", "version": "4.0.0" });
    let staged = StagedReference { integrity: sri_sha512(b"staged"), label: "x".into() };
    let variants = [
        PackedVariant {
            tag: "linux-x64+linux-x64-musl".into(),
            archive: VariantArchive::Packed(b"linux".to_vec()),
        },
        PackedVariant { tag: "win32-x64".into(), archive: VariantArchive::Staged(staged) },
    ];
    let with_any = pkg(&manifest, b"any", &variants);
    let subjects: Vec<(String, String)> = provenance_subjects(&with_any, ("@example/cli", "4.0.0"))
        .unwrap()
        .into_iter()
        .map(|subject| (subject.name, subject.sha512_hex))
        .collect();
    assert_eq!(
        subjects,
        [
            ("pkg:npm/%40example/cli@4.0.0".to_string(), hex(b"any")),
            (
                "pkg:npm/%40example/cli@4.0.0?variant=linux-x64%2Blinux-x64-musl".to_string(),
                hex(b"linux"),
            ),
            ("pkg:npm/%40example/cli@4.0.0?variant=win32-x64".to_string(), hex(b"staged")),
        ],
    );
    let without_any = pkg(&manifest, b"", &variants);
    assert_eq!(provenance_subjects(&without_any, ("@example/cli", "4.0.0")).unwrap().len(), 2);
}
