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
        package.versions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
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

#[test]
fn policy_field_walk_reuses_hydrated_manifests_and_hydrates_no_others() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "_npmUser": {"trustedPublisher": {"id": "github"}}, "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}},
                "9.9.9": {"this is": "not a version manifest"}
            }
        }"#,
    );
    let hydrated = package.versions.get("1.0.0").expect("hydrate 1.0.0");

    let walked: HashMap<_, _> = package.versions
        .iter_policy_fields()
        .map(|(version, fields)| (version.as_str(), fields))
        .collect();

    assert_eq!(walked.len(), 2);
    assert_eq!(walked["1.0.0"], crate::VersionPolicyFields::from(hydrated.as_ref()));
    let decoded = &walked["2.0.0"];
    assert_eq!(decoded.dist.tarball, "https://r/foo-2.0.0.tgz");
    assert!(
        decoded.npm_user
            .as_ref()
            .and_then(|user| user.trusted_publisher.as_ref())
            .is_some(),
    );
    assert!(!package.versions.is_hydrated("2.0.0"));
}

#[test]
fn policy_field_walk_reports_a_damaged_mirror_fragment() {
    let versions = mirror_versions();

    assert_eq!(versions.iter_policy_fields().count(), 1);
    assert!(versions.has_corrupt_mirror_fragment());
}

/// The walk must keep exactly the versions a full decode keeps, so a policy
/// check sees the same version set either way.
#[test]
fn policy_fields_decode_exactly_where_a_manifest_does() {
    let fragments = [
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"readme":"x","scripts":{"test":"t"}}"#,
        r#"{"version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"}}"#,
        r#"{"name":"foo","version":"not semver","dist":{"tarball":"https://r/foo.tgz"}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz","integrity":"not integrity"}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"dependencies":"bar"}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"devDependencies":{"bar":{"nested":true}}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"optionalDependencies":3}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"peerDependencies":null}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"deprecated":5}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"deprecated":false}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"_npmUser":"someone"}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"peerDependenciesMeta":3}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz","fileCount":"12"}}"#,
        r#"{"name":"foo","name":"bar","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"}}"#,
        r#"{"name":7,"version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"peerDependenciesMeta":{},"peerDependenciesMeta":{}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"_npmUser":{},"_npmUser":{}}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"deprecated":"x","deprecated":"y"}"#,
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"},"readme":"a","readme":"b"}"#,
    ];
    for fragment in fragments {
        let manifest = serde_json::from_str::<PackageVersion>(fragment);
        let probe = serde_json::from_str::<crate::package_version::PolicyFieldsProbe>(fragment);
        assert_eq!(manifest.is_ok(), probe.is_ok(), "{fragment}");
        if let (Ok(manifest), Ok(probe)) = (manifest, probe) {
            assert_eq!(
                crate::VersionPolicyFields::from(&manifest),
                crate::VersionPolicyFields::from(probe),
                "{fragment}",
            );
        }
    }
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
    assert_eq!(
        versions
            .get("1.0.0")
            .unwrap()
            .version
            .to_string(),
        "1.0.0",
    );
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
fn deprecation_probe_cache_survives_filtered_copies_without_hydration() {
    let package = parse_package(
        r#"{"name":"foo","dist-tags":{},"versions":{
        "1.0.0":{"deprecated":"use 2.x"},"2.0.0":{}
    }}"#,
    );
    for (version, expected) in [("1.0.0", true), ("2.0.0", false)] {
        assert_eq!(package.versions.is_deprecated(version), expected);
        let filtered = package.versions.filtered(|candidate| candidate == version);
        let slot = filtered.slot(version).unwrap();
        assert_eq!(slot.deprecated.get(), Some(&expected));
        assert!(slot.parsed.get().is_none());
        assert_eq!(filtered.is_deprecated(version), expected);
    }
}

#[test]
fn failed_hydration_takes_precedence_over_cached_deprecation_probe() {
    let package = parse_package(
        r#"{"name":"foo","dist-tags":{},"versions":{
        "1.0.0":{"deprecated":"use 2.x","version":false}
    }}"#,
    );
    assert!(package.versions.is_deprecated("1.0.0"));
    assert!(package.versions.get("1.0.0").is_none());
    assert!(!package.versions.is_deprecated("1.0.0"));
}

#[test]
fn file_backed_deprecation_probe_does_not_hydrate() {
    use crate::{MirrorFile, PackageVersions};
    use std::io::Write;

    let json = r#"{"name":"foo","version":"1.0.0","deprecated":true,"dist":{"tarball":"https://r/foo.tgz"}}"#;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(json.as_bytes()).unwrap();
    let mirror = MirrorFile::try_hold(file, usize::MAX).unwrap();
    let versions =
        PackageVersions::from_file_spans(&mirror, [("1.0.0".into(), 0, json.len() as u32)]);
    for _ in 0..3 {
        assert!(versions.is_deprecated("1.0.0"));
        assert!(
            versions
                .slot("1.0.0")
                .unwrap()
                .parsed
                .get()
                .is_none(),
        );
    }
    assert!(
        versions
            .get("1.0.0")
            .unwrap()
            .deprecated
            .is_some(),
    );
}

#[test]
fn unreadable_mirror_probe_does_not_cache_a_false_result() {
    use crate::{MirrorFile, PackageVersions};

    let file = tempfile::NamedTempFile::new().unwrap();
    let write_only = std::fs::OpenOptions::new()
        .write(true)
        .open(file.path())
        .unwrap();
    let mirror = MirrorFile::try_hold(write_only, usize::MAX).unwrap();
    let versions = PackageVersions::from_file_spans(&mirror, [("1.0.0".into(), 0, 8)]);
    assert!(!versions.is_deprecated("1.0.0"));
    assert!(
        versions
            .slot("1.0.0")
            .unwrap()
            .deprecated
            .get()
            .is_none(),
    );
}

#[test]
fn trust_metadata_reads_only_trust_fields_from_a_version_fragment() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {
                    "_npmUser": {"approver": {"name": "a", "email": "a@example.com"}},
                    "dist": {"attestations": {"provenance": {"predicateType": "https://slsa.dev/provenance/v1"}}}
                }
            }
        }"#,
    );

    assert!(package.versions.get("1.0.0").is_none());
    let trust = package.versions.trust_metadata("1.0.0").expect("decode trust fields");
    assert!(
        trust.npm_user
            .as_ref()
            .and_then(|user| user.approver.as_ref())
            .is_some(),
    );
    assert!(
        trust.dist
            .as_ref()
            .and_then(|dist| dist.attestations.as_ref())
            .and_then(|attestations| attestations.provenance.as_ref())
            .is_some(),
    );
}

#[test]
fn compact_trust_matches_full_hydration_for_non_record_containers() {
    for value in [
        serde_json::json!(false),
        serde_json::json!(1),
        serde_json::json!("maintainer"),
        serde_json::json!([]),
        serde_json::Value::Null,
    ] {
        for field in ["_npmUser", "_npm_user", "attestations"] {
            let mut manifest = serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "_npmUser": {"approver": true},
                "dist": {
                    "tarball": "https://r/foo.tgz",
                    "attestations": {"provenance": true}
                }
            });
            if field == "attestations" {
                manifest["dist"][field] = value.clone();
            } else {
                manifest
                    .as_object_mut()
                    .unwrap()
                    .remove("_npmUser");
                manifest[field] = value.clone();
            }
            let package: Package = serde_json::from_value(serde_json::json!({
                "name": "foo", "dist-tags": {}, "versions": {"1.0.0": manifest}
            }))
            .unwrap();
            let hydrated_first = package.clone();
            let full = hydrated_first.versions.get("1.0.0").expect("tolerant full hydration");
            let compact =
                package.versions.trust_metadata("1.0.0").expect("tolerant compact decoding");
            dbg!(&compact, &full);
            assert_eq!(compact.npm_user, full.npm_user, "{field}: {value}");
            assert_eq!(
                compact.dist.as_ref().unwrap().attestations,
                full.dist.attestations,
                "{field}: {value}",
            );
            let after_hydration = hydrated_first.versions.trust_metadata("1.0.0").unwrap();
            assert_eq!(after_hydration.npm_user, full.npm_user);
            assert_eq!(after_hydration.dist.as_ref().unwrap().attestations, full.dist.attestations);
            assert!(
                package.versions
                    .slot("1.0.0")
                    .unwrap()
                    .parsed
                    .get()
                    .is_none(),
            );
        }
    }
}

#[test]
fn compact_trust_cache_preserves_successes_and_failures_in_copies() {
    let package = parse_package(
        r#"{
        "name":"foo", "dist-tags":{}, "versions": {
            "1.0.0":{"_npmUser":{"approver":true}},
            "2.0.0":{"dist":"not-an-object"}
        }
    }"#,
    );
    for (version, expected) in [("1.0.0", true), ("2.0.0", false)] {
        assert_eq!(package.versions.trust_metadata(version).is_some(), expected);
        for mut copy in
            [package.versions.clone(), package.versions.filtered(|candidate| candidate == version)]
        {
            let slot = copy.slots
                .iter_mut()
                .find(|(candidate, _)| candidate == version)
                .unwrap();
            assert_eq!(slot.1.trust.get().unwrap().is_some(), expected);
            dbg!(&slot.1);
            assert!(slot.1.parsed.get().is_none());
            slot.1.source = super::FragmentSource::None;
            assert_eq!(copy.trust_metadata(version).is_some(), expected);
        }
    }
}

#[test]
fn compact_trust_reads_typed_manifests_without_fragments() {
    let manifest: PackageVersion = serde_json::from_value(serde_json::json!({
        "name":"foo", "version":"1.0.0", "_npmUser":{"approver":true},
        "dist":{"tarball":"https://r/foo.tgz", "attestations":{"provenance":true}}
    }))
    .unwrap();
    let versions: crate::PackageVersions = HashMap::from([("1.0.0".to_string(), manifest)]).into();
    let trust = versions.trust_metadata("1.0.0").expect("trust from typed manifest");
    dbg!(&trust);
    assert!(
        trust.npm_user
            .as_ref()
            .unwrap()
            .approver
            .is_some(),
    );
    assert!(
        trust.dist
            .as_ref()
            .unwrap()
            .attestations
            .as_ref()
            .unwrap()
            .provenance
            .is_some(),
    );
}

/// The mirror stores registry fragments verbatim, so a version the registry
/// served in the wrong shape reads back the same way and stays absent.
#[test]
fn a_well_formed_mirror_fragment_of_the_wrong_shape_is_not_damage() {
    const ODD: &str = r#"{"name":"acme","version":"not semver"}"#;
    let versions = mirror_spans(ODD, [("1.0.0".to_string(), 0, u32::try_from(ODD.len()).unwrap())]);

    assert!(versions.get("1.0.0").is_none());
    assert!(!versions.is_deprecated("1.0.0"));
    assert!(!versions.has_corrupt_mirror_fragment());
}

#[test]
fn checking_mirror_fragments_reports_damage_without_hydrating() {
    let versions = mirror_versions();

    assert!(versions.check_mirror_fragments());
    assert!(!versions.is_hydrated("1.0.0"));
}

#[test]
fn checking_intact_mirror_fragments_reports_nothing() {
    const VALID: &str =
        r#"{"name":"foo","version":"1.0.0","dist":{"tarball":"https://r/foo.tgz"}}"#;
    let versions =
        mirror_spans(VALID, [("1.0.0".to_string(), 0, u32::try_from(VALID.len()).unwrap())]);

    assert!(!versions.check_mirror_fragments());
    assert!(!versions.is_hydrated("1.0.0"));
}
