use super::{
    install_v2, likely_picks, record_address, release_records::release_record,
    without_pnpr_facts_in,
};
use pnpr_config::PackageDocumentsConfig;
use serde_json::{Value, json};
use std::time::Duration;

fn version(integrity: &str) -> Value {
    json!({
        "name": "acme",
        "version": "x",
        "deprecated": "use 2",
        "dependencies": { "dep": "^1.0.0" },
        "_npmUser": { "trustedPublisher": true },
        "dist": {
            "integrity": integrity,
            "tarball": "https://registry.example/acme@x",
            "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } },
            "pnpr": { "requiresBuild": false },
        },
    })
}

fn packument() -> Value {
    json!({
        "name": "acme",
        "dist-tags": { "latest": "2.1.0", "next": "3.0.0-rc.1" },
        "time": {
            "modified": "2026-01-01T00:00:00.000Z",
            "1.0.0": "2025-01-01T00:00:00.000Z",
            "1.4.2": "2025-12-15T00:00:00.000Z",
            "2.0.0": "2025-12-20T00:00:00.000Z",
            "2.1.0": "2026-01-01T00:00:00.000Z",
        },
        "versions": {
            "0.1.0": version("sha512-a"),
            "0.1.1": version("sha512-b"),
            "0.2.0": version("sha512-c"),
            "1.0.0": version("sha512-d"),
            "1.4.2": version("sha512-e"),
            "2.0.0": version("sha512-f"),
            "2.1.0": version("sha512-g"),
            "3.0.0-rc.1": version("sha512-h"),
        },
    })
}

fn document(packument: &Value, split: bool) -> Value {
    install_v2(packument, split, &PackageDocumentsConfig::default())
}

#[test]
fn likely_picks_are_the_tag_targets_and_the_highest_of_each_line() {
    let mut picks: Vec<String> = likely_picks(&packument()).into_iter().collect();
    picks.sort();
    assert_eq!(picks, ["0.1.1", "0.2.0", "1.4.2", "2.1.0", "3.0.0-rc.1"]);
}

#[test]
fn a_small_document_keeps_every_manifest() {
    let doc = document(&packument(), false);
    assert_eq!(doc["name"], json!("acme"));
    assert_eq!(doc["distTags"], json!({ "latest": "2.1.0", "next": "3.0.0-rc.1" }));
    let versions = doc["versions"].as_object().expect("versions");
    assert_eq!(versions.len(), 8);
    assert!(
        versions
            .values()
            .all(|entry| entry.get("manifest").is_some()),
    );
}

#[test]
fn a_large_document_keeps_the_manifests_of_the_likely_picks() {
    let doc = document(&packument(), true);
    assert_eq!(
        doc["versions"]["2.1.0"],
        json!({
            "time": "2026-01-01T00:00:00.000Z",
            "deprecated": "use 2",
            "trust": "trustedPublisher",
            "integrity": "sha512-g",
            "requiresBuild": false,
            "manifest": { "dependencies": { "dep": "^1.0.0" } },
        }),
    );
    assert_eq!(
        doc["versions"]["1.0.0"],
        json!({ "deprecated": "use 2", "trust": "trustedPublisher" }),
    );
}

#[test]
fn only_versions_inside_the_window_keep_their_publish_time() {
    let doc = document(&packument(), true);
    assert_eq!(doc["timesSince"], json!("2025-12-02T00:00:00.000Z"));
    assert_eq!(doc["versions"]["2.0.0"]["time"], json!("2025-12-20T00:00:00.000Z"));
    assert!(doc["versions"]["1.0.0"].get("time").is_none());
    assert_eq!(doc["versions"]["0.1.0"]["time"], Value::Null);
}

#[test]
fn a_longer_window_keeps_more_publish_times() {
    let config = PackageDocumentsConfig {
        publish_time_window: Duration::from_hours(400 * 24),
        ..PackageDocumentsConfig::default()
    };
    let doc = install_v2(&packument(), true, &config);
    assert_eq!(doc["timesSince"], json!("2024-11-27T00:00:00.000Z"));
    assert_eq!(doc["versions"]["1.0.0"]["time"], json!("2025-01-01T00:00:00.000Z"));
}

#[test]
fn coarsened_publish_times_are_read() {
    let mut packument = packument();
    packument["time"]["2.1.0"] = json!("2026-01-01T00:00Z");
    packument["time"]["2.0.0"] = json!("2025-12-20");
    packument["time"]["1.0.0"] = json!("2025-01-01");
    let doc = document(&packument, true);
    assert_eq!(doc["timesSince"], json!("2025-12-02T00:00:00.000Z"));
    assert_eq!(doc["versions"]["2.0.0"]["time"], json!("2025-12-20"));
    assert!(doc["versions"]["1.0.0"].get("time").is_none());
}

#[test]
fn a_version_without_flags_is_an_empty_entry() {
    let mut packument = packument();
    let manifest = packument["versions"]["1.0.0"].as_object_mut().expect("manifest");
    manifest.remove("deprecated");
    manifest.remove("_npmUser");
    manifest["dist"]
        .as_object_mut()
        .expect("dist")
        .remove("attestations");
    assert_eq!(document(&packument, true)["versions"]["1.0.0"], json!({}));
}

#[test]
fn a_selected_revision_is_in_the_entry_even_when_it_is_the_original() {
    let mut packument = packument();
    packument["versions"]["1.0.0"]["dist"]["revision"] = json!(2);
    packument["versions"]["1.4.2"]["dist"]["revision"] = json!(0);
    let doc = document(&packument, true);
    assert_eq!(doc["versions"]["1.0.0"]["revision"], json!(2));
    assert_eq!(doc["versions"]["1.4.2"]["revision"], json!(0));
    assert!(doc["versions"]["2.0.0"].get("revision").is_none());
}

#[test]
fn a_record_address_names_a_canonical_revision() {
    assert_eq!(record_address("1.0.0+r2"), ("1.0.0", Some(2)));
    assert_eq!(record_address("1.0.0+r0"), ("1.0.0", Some(0)));
    assert_eq!(record_address("1.0.0"), ("1.0.0", None));
    assert_eq!(record_address("latest"), ("latest", None));
    assert_eq!(record_address("1.0.0+rc"), ("1.0.0+rc", None));
    assert_eq!(record_address("1.0.0+r02"), ("1.0.0+r02", None));
    assert_eq!(record_address("1.0.0+r"), ("1.0.0+r", None));
}

#[test]
fn a_release_record_leaves_out_what_may_change() {
    let record = Value::Object(release_record(&version("sha512-a")));
    assert_eq!(
        record,
        json!({
            "integrity": "sha512-a",
            "requiresBuild": false,
            "manifest": { "dependencies": { "dep": "^1.0.0" } },
        }),
    );
}

#[test]
fn npm_documents_carry_no_pnpr_facts() {
    let mut doc = packument();
    doc["versions"]["1.0.0"]["dist"]["revision"] = json!(1);
    doc["versions"]["1.0.0"]["dist"]["revisions"] = json!([]);
    without_pnpr_facts_in(&mut doc);
    let dist = &doc["versions"]["1.0.0"]["dist"];
    for omitted in ["pnpr", "revision", "revisions"] {
        assert!(dist.get(omitted).is_none(), "{omitted}");
    }
    assert_eq!(dist["integrity"], json!("sha512-d"));
}
