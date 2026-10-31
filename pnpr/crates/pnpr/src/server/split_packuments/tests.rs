use super::{install_v2, likely_picks, record_address, release_record};
use serde_json::{Value, json};

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
            "pnpr": {
                "requiresBuild": false,
                "variants": [{
                    "tag": "linux-x64",
                    "platforms": [{ "os": "linux", "cpu": "x64" }],
                    "integrity": "sha512-variant",
                }],
            },
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

#[test]
fn likely_picks_are_the_tag_targets_and_the_highest_of_each_line() {
    let mut picks: Vec<String> = likely_picks(&packument()).into_iter().collect();
    picks.sort();
    assert_eq!(picks, ["0.1.1", "0.2.0", "1.4.2", "2.1.0", "3.0.0-rc.1"]);
}

#[test]
fn a_small_document_keeps_every_manifest() {
    let doc = install_v2(&packument(), false);
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
    let doc = install_v2(&packument(), true);
    assert_eq!(
        doc["versions"]["2.1.0"],
        json!({
            "time": "2026-01-01T00:00:00.000Z",
            "deprecated": "use 2",
            "trust": "trustedPublisher",
            "manifest": {
                "dependencies": { "dep": "^1.0.0" },
                "dist": {
                    "integrity": "sha512-g",
                    "tarball": "https://registry.example/acme@x",
                    "pnpr": {
                        "requiresBuild": false,
                        "variants": [{
                            "tag": "linux-x64",
                            "platforms": [{ "os": "linux", "cpu": "x64" }],
                            "integrity": "sha512-variant",
                        }],
                    },
                },
            },
        }),
    );
    assert_eq!(
        doc["versions"]["1.0.0"],
        json!({ "deprecated": "use 2", "trust": "trustedPublisher" }),
    );
}

#[test]
fn only_versions_inside_the_window_keep_their_publish_time() {
    let doc = install_v2(&packument(), true);
    assert_eq!(doc["timesSince"], json!("2025-12-02T00:00:00.000Z"));
    assert_eq!(doc["versions"]["2.0.0"]["time"], json!("2025-12-20T00:00:00.000Z"));
    assert!(doc["versions"]["1.0.0"].get("time").is_none());
    assert_eq!(doc["versions"]["0.1.0"]["time"], Value::Null);
}

#[test]
fn coarsened_publish_times_are_read() {
    let mut packument = packument();
    packument["time"]["2.1.0"] = json!("2026-01-01T00:00Z");
    packument["time"]["2.0.0"] = json!("2025-12-20");
    packument["time"]["1.0.0"] = json!("2025-01-01");
    let doc = install_v2(&packument, true);
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
    assert_eq!(install_v2(&packument, true)["versions"]["1.0.0"], json!({}));
}

#[test]
fn a_selected_replacement_is_in_the_entry() {
    let mut packument = packument();
    packument["versions"]["1.0.0"]["dist"]["revision"] = json!(2);
    assert_eq!(install_v2(&packument, true)["versions"]["1.0.0"]["revision"], json!(2));
}

#[test]
fn a_record_address_names_a_revision() {
    assert_eq!(record_address("1.0.0+r2"), ("1.0.0", Some(2)));
    assert_eq!(record_address("1.0.0"), ("1.0.0", None));
    assert_eq!(record_address("latest"), ("latest", None));
    assert_eq!(record_address("1.0.0+rc"), ("1.0.0+rc", None));
}

#[test]
fn a_release_record_leaves_out_what_may_change() {
    let record = release_record(&version("sha512-a"));
    assert!(record.get("deprecated").is_none());
    assert!(record.get("_npmUser").is_none());
    assert!(record["dist"].get("attestations").is_none());
    assert_eq!(record["dependencies"], json!({ "dep": "^1.0.0" }));
    assert_eq!(record["dist"]["integrity"], json!("sha512-a"));
}
