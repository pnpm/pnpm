use std::{collections::HashMap, io::Write};

use crate::{Package, PackageVersion};

fn parse_package(json: &str) -> Package {
    serde_json::from_str(json).expect("parse package")
}

#[test]
fn hydrates_only_requested_versions_and_caches_them() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {"latest": "2.0.0"},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}}
            }
        }"#,
    );

    assert_eq!(package.versions.len(), 2);
    assert!(package.versions.contains_key("1.0.0"));
    let picked = package.versions.get("2.0.0").expect("hydrate 2.0.0");
    assert_eq!(picked.version.to_string(), "2.0.0");
    let again = package.versions.get("2.0.0").expect("cached 2.0.0");
    assert!(std::sync::Arc::ptr_eq(&picked, &again));
}

#[test]
fn sorts_version_slots_for_lookup() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "10.0.0": {"name": "foo", "version": "10.0.0", "dist": {"integrity": "sha512-c", "tarball": "https://r/foo-10.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}},
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}
            }
        }"#,
    );

    assert_eq!(
        package.versions.keys().map(String::as_str).collect::<Vec<_>>(),
        ["1.0.0", "10.0.0", "2.0.0"],
    );
    assert!(package.versions.get("2.0.0").is_some());
    assert!(package.versions.get("3.0.0").is_none());
}

#[test]
fn undecodable_fragment_behaves_as_absent() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "9.9.9": {"this is": "not a version manifest"}
            }
        }"#,
    );

    assert!(package.versions.contains_key("9.9.9"));
    assert!(package.versions.get("9.9.9").is_none());
    assert!(package.versions.get("1.0.0").is_some());
    assert_eq!(package.versions.iter().count(), 1);
    assert!(!package.versions.has_corrupt_mirror_fragment());
}

/// A mirror-backed packument whose valid fragment sits before a
/// damaged one, plus the two spans that address them.
fn mirror_versions() -> crate::PackageVersions {
    const VALID: &str = r#"{"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}"#;
    const DAMAGED: &str = "{not json at all}";
    let valid_len = u32::try_from(VALID.len()).unwrap();
    mirror_spans(
        &format!("{VALID}{DAMAGED}"),
        [
            ("1.0.0".to_string(), 0, valid_len),
            ("2.0.0".to_string(), u64::from(valid_len), u32::try_from(DAMAGED.len()).unwrap()),
        ],
    )
}

/// Spans over `fragments`, served from a held-open file the way an
/// indexed mirror load serves them.
fn mirror_spans(
    fragments: &str,
    spans: impl IntoIterator<Item = (String, u64, u32)>,
) -> crate::PackageVersions {
    let mut file = tempfile::tempfile().expect("create the mirror file");
    file.write_all(fragments.as_bytes()).expect("write the fragments");
    let held = super::MirrorFile::try_hold(file, usize::MAX).expect("hold the mirror file");
    crate::PackageVersions::from_file_spans(&held, spans)
}

#[test]
fn a_damaged_mirror_fragment_is_reported_once_hydrated() {
    let versions = mirror_versions();

    assert!(versions.get("1.0.0").is_some());
    assert!(!versions.has_corrupt_mirror_fragment());

    assert!(versions.get("2.0.0").is_none());
    assert!(versions.has_corrupt_mirror_fragment());
}

/// The publish-date filter hands out a filtered view of the same
/// fragments, so damage found through either handle has to be visible
/// from the one the resolver checks.
#[test]
fn a_filtered_view_shares_the_damage_report() {
    let versions = mirror_versions();
    let filtered = versions.filtered(|version| version == "2.0.0");

    assert!(filtered.get("2.0.0").is_none());
    assert!(filtered.has_corrupt_mirror_fragment());
    assert!(versions.has_corrupt_mirror_fragment());
}

#[test]
fn probing_a_damaged_mirror_fragment_for_deprecation_reports_it() {
    const DAMAGED: &str = r#"{"deprecated": "use 2.x",,}"#;
    let versions =
        mirror_spans(DAMAGED, [("1.0.0".to_string(), 0, u32::try_from(DAMAGED.len()).unwrap())]);

    assert!(!versions.is_deprecated("1.0.0"));
    assert!(versions.has_corrupt_mirror_fragment());
}

#[test]
fn serializes_raw_fragments_verbatim() {
    let json = r#"{
        "name": "foo",
        "dist-tags": {},
        "versions": {
            "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}, "extraKeyTheStructDoesNotType": [1, 2, {"deep": true}]}
        }
    }"#;
    let package = parse_package(json);
    let round_tripped = serde_json::to_string(&package).expect("serialize package");
    let reparsed: serde_json::Value = serde_json::from_str(&round_tripped).unwrap();
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(reparsed["versions"], original["versions"]);
}

#[test]
fn eager_construction_from_typed_manifests_round_trips() {
    let manifest: PackageVersion = serde_json::from_str(
        r#"{"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}"#,
    )
    .unwrap();
    let versions: crate::PackageVersions = HashMap::from([("1.0.0".to_string(), manifest)]).into();
    assert_eq!(versions.get("1.0.0").unwrap().version.to_string(), "1.0.0");
    let json = serde_json::to_string(&versions).unwrap();
    assert!(json.contains(r#""1.0.0""#));
}

#[test]
fn filtered_keeps_slots_without_hydration() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}}
            }
        }"#,
    );
    let filtered = package.versions.filtered(|version| version == "1.0.0");
    assert_eq!(filtered.len(), 1);
    assert!(filtered.get("1.0.0").is_some());
}

#[test]
fn pinned_version_falls_back_past_undecodable_highest() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "1.9.0": {"corrupt": "fragment"}
            }
        }"#,
    );
    let pinned = package.pinned_version("^1.0.0").expect("fall back to 1.0.0");
    assert_eq!(pinned.version.to_string(), "1.0.0");
}

#[test]
fn latest_returns_none_for_dangling_or_undecodable_tag() {
    let undecodable = parse_package(
        r#"{"name": "foo", "dist-tags": {"latest": "2.0.0"}, "versions": {"2.0.0": {"corrupt": true}}}"#,
    );
    assert!(undecodable.latest().is_none());
    let dangling =
        parse_package(r#"{"name": "foo", "dist-tags": {"latest": "9.9.9"}, "versions": {}}"#);
    assert!(dangling.latest().is_none());
}

#[test]
fn is_deprecated_probes_without_hydrating() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "1.1.0": {"name": "foo", "version": "1.1.0", "deprecated": "use 2.x", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-1.1.0.tgz"}},
                "1.2.0": {"name": "foo", "version": "1.2.0", "deprecated": false, "dist": {"integrity": "sha512-c", "tarball": "https://r/foo-1.2.0.tgz"}},
                "1.3.0": {"name": "foo", "version": "1.3.0", "deprecated": true, "dist": {"integrity": "sha512-d", "tarball": "https://r/foo-1.3.0.tgz"}},
                "1.4.0": {"name": "foo", "version": "1.4.0", "deprecated": "", "dist": {"integrity": "sha512-e", "tarball": "https://r/foo-1.4.0.tgz"}},
                "1.9.0": {"corrupt": "fragment", "deprecated": 1}
            }
        }"#,
    );

    assert!(!package.versions.is_deprecated("1.0.0"));
    assert!(package.versions.is_deprecated("1.1.0"));
    assert!(!package.versions.is_deprecated("1.2.0"));
    assert!(package.versions.is_deprecated("1.3.0"));
    assert!(package.versions.is_deprecated("1.4.0"));
    assert!(!package.versions.is_deprecated("9.9.9"));
    assert!(!package.versions.is_deprecated("1.9.0"));

    // The probe must agree with the hydrated field on every slot it
    // can hydrate, and must not have hydrated anything itself: `get`
    // still parses fresh (no cached Arc identity from the probe).
    for version in ["1.0.0", "1.1.0", "1.2.0", "1.3.0", "1.4.0"] {
        let manifest = package.versions.get(version).expect("hydrate");
        assert_eq!(
            package.versions.is_deprecated(version),
            manifest.deprecated.is_some(),
            "probe vs hydrated disagree for {version}",
        );
    }
}

#[test]
fn is_deprecated_ignores_unrelated_key_text() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dependencies": {"deprecated": "^0.0.2"}, "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}
            }
        }"#,
    );
    assert!(!package.versions.is_deprecated("1.0.0"));
}

#[test]
fn trust_metadata_reads_npm_user_and_attestations_from_a_raw_fragment() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {
                    "name": "foo",
                    "version": "1.0.0",
                    "dist": {
                        "integrity": "sha512-a",
                        "tarball": "https://r/foo-1.0.0.tgz",
                        "attestations": {"provenance": {"predicateType": "https://slsa.dev/provenance/v1"}}
                    },
                    "_npmUser": {"trustedPublisher": {"id": "github", "oidcConfigId": "release"}}
                }
            }
        }"#,
    );
    let trust = package.versions.trust_metadata("1.0.0").expect("decode trust metadata");
    assert!(
        trust.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref()).is_some(),
        "trusted_publisher missing",
    );
    assert!(
        trust
            .dist
            .as_ref()
            .and_then(|dist| dist.attestations.as_ref())
            .and_then(|a| a.provenance.as_ref())
            .is_some(),
        "provenance missing",
    );
}

#[test]
fn trust_metadata_decodes_a_manifest_missing_fields_full_package_version_requires() {
    // A `PackageVersion` needs `name` / `version` / `dist.tarball`; the
    // compact shape doesn't, so a fragment carrying only trust fields
    // still decodes even though `get()` on the same fragment would not.
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"_npmUser": {"approver": {"name": "a", "email": "a@example.com"}}}
            }
        }"#,
    );
    assert!(package.versions.get("1.0.0").is_none(), "full decode should fail on this fixture");
    let trust = package.versions.trust_metadata("1.0.0").expect("compact decode should succeed");
    assert!(trust.npm_user.as_ref().and_then(|user| user.approver.as_ref()).is_some());
}

#[test]
fn trust_metadata_reads_an_already_hydrated_slot_without_a_raw_fragment() {
    // Constructed from typed manifests (`From<HashMap<..>>`), so the slot
    // carries `FragmentSource::None` — no raw JSON to parse. `trust_metadata`
    // has to read the already-hydrated `Arc<PackageVersion>` instead of
    // treating the missing fragment as a decode failure.
    let mut manifest: PackageVersion = serde_json::from_str(
        r#"{"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}"#,
    )
    .unwrap();
    manifest.npm_user = Some(crate::NpmUser {
        name: None,
        email: None,
        approver: Some(crate::Approver { name: None, email: None }),
        trusted_publisher: None,
    });
    let versions: crate::PackageVersions = HashMap::from([("1.0.0".to_string(), manifest)]).into();

    let trust = versions.trust_metadata("1.0.0").expect("read the hydrated slot");
    assert!(trust.npm_user.as_ref().and_then(|user| user.approver.as_ref()).is_some());
}

#[test]
fn trust_metadata_returns_none_for_an_absent_version() {
    let package = parse_package(r#"{"name": "foo", "dist-tags": {}, "versions": {}}"#);
    assert!(package.versions.trust_metadata("9.9.9").is_none());
}

#[test]
fn trust_metadata_reports_a_damaged_mirror_fragment() {
    let versions = mirror_versions();

    assert!(!versions.has_corrupt_mirror_fragment());
    assert!(versions.trust_metadata("2.0.0").is_none());
    assert!(versions.has_corrupt_mirror_fragment());
}
