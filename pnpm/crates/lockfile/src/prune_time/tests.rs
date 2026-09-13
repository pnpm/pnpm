use super::prune_time;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

fn pruned(document: Value) -> Value {
    let mut document = document;
    prune_time(&mut document);
    document
}

#[test]
fn transitive_entries_are_dropped() {
    let document = pruned(json!({
        "importers": {
            ".": {
                "dependencies": {
                    "is-positive": { "specifier": "1.0.0", "version": "1.0.0" },
                },
            },
        },
        "time": {
            "is-negative@1.0.0": "2016-01-01T00:00:00.000Z",
            "is-positive@1.0.0": "2016-02-02T00:00:00.000Z",
        },
    }));
    assert_eq!(document["time"], json!({ "is-positive@1.0.0": "2016-02-02T00:00:00.000Z" }));
}

#[test]
fn every_dependency_group_of_every_importer_is_direct() {
    let document = pruned(json!({
        "importers": {
            ".": {
                "devDependencies": {
                    "typescript": { "specifier": "^5.1.6", "version": "5.1.6" },
                },
            },
            "packages/app": {
                "optionalDependencies": {
                    "fsevents": { "specifier": "^2.3.3", "version": "2.3.3" },
                },
            },
        },
        "time": {
            "fsevents@2.3.3": "2024-01-01T00:00:00.000Z",
            "typescript@5.1.6": "2023-06-27T18:00:00.000Z",
        },
    }));
    assert_eq!(
        document["time"],
        json!({
            "fsevents@2.3.3": "2024-01-01T00:00:00.000Z",
            "typescript@5.1.6": "2023-06-27T18:00:00.000Z",
        }),
    );
}

#[test]
fn a_peer_qualified_dependency_matches_its_peer_stripped_key() {
    let document = pruned(json!({
        "importers": {
            ".": {
                "dependencies": {
                    "react-dom": { "specifier": "^17.0.2", "version": "17.0.2(react@17.0.2)" },
                },
            },
        },
        "time": { "react-dom@17.0.2": "2021-03-22T15:00:00.000Z" },
    }));
    assert_eq!(document["time"], json!({ "react-dom@17.0.2": "2021-03-22T15:00:00.000Z" }));
}

#[test]
fn an_aliased_dependency_matches_its_target_key() {
    let document = pruned(json!({
        "importers": {
            ".": {
                "dependencies": {
                    "positive": { "specifier": "npm:is-positive@1.0.0", "version": "is-positive@1.0.0" },
                },
            },
        },
        "time": {
            "is-positive@1.0.0": "2016-02-02T00:00:00.000Z",
            "positive@1.0.0": "2016-02-02T00:00:00.000Z",
        },
    }));
    assert_eq!(document["time"], json!({ "is-positive@1.0.0": "2016-02-02T00:00:00.000Z" }));
}

#[test]
fn a_linked_dependency_keeps_nothing() {
    let document = pruned(json!({
        "importers": {
            ".": {
                "dependencies": {
                    "shared": { "specifier": "workspace:*", "version": "link:packages/shared" },
                },
            },
        },
        "time": { "shared@1.0.0": "2024-01-01T00:00:00.000Z" },
    }));
    assert_eq!(document["time"], json!({}));
}

#[test]
fn a_document_without_a_time_section_is_untouched() {
    let document = json!({
        "importers": { ".": { "dependencies": {} } },
        "packages": { "is-positive@1.0.0": {} },
    });
    assert_eq!(pruned(document.clone()), document);
}
