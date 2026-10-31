use super::{likely_picks, split_above};
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
fn a_small_document_stays_whole() {
    let doc = packument();
    let body = serde_json::to_vec(&doc).expect("serialize");
    assert_eq!(split_above(doc, body.clone(), 64 * 1024, false).expect("split"), body);
}

#[test]
fn a_large_document_leaves_other_versions_in_the_version_document() {
    // Only the body's length is compared with the threshold.
    let split = split_above(packument(), vec![b' '; 1024 * 1024], 64 * 1024, false).expect("split");
    let split: Value = serde_json::from_slice(&split).expect("parse");
    assert_eq!(split["versions"]["2.1.0"], version("sha512-g"));
    assert_eq!(
        split["versions"]["1.0.0"],
        json!({
            "name": "acme",
            "version": "x",
            "deprecated": "use 2",
            "_npmUser": { "trustedPublisher": true },
            "dist": {
                "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } },
                "pnpr": { "details": "external" },
            },
        }),
    );
}

#[test]
fn a_dist_only_split_keeps_dependencies_and_variant_platforms() {
    let split = split_above(packument(), vec![b' '; 1024 * 1024], 64 * 1024, true).expect("split");
    let split: Value = serde_json::from_slice(&split).expect("parse");
    assert_eq!(split["versions"]["1.0.0"]["dependencies"], json!({ "dep": "^1.0.0" }));
    assert_eq!(
        split["versions"]["1.0.0"]["dist"],
        json!({
            "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } },
            "pnpr": {
                "details": "external",
                "variants": [{ "tag": "linux-x64", "platforms": [{ "os": "linux", "cpu": "x64" }] }],
            },
        }),
    );
}
