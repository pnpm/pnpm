use super::{PublicationNotes, ReleaseNotesTarget};
use crate::registry_config_keys::parse_supported_registry_url;
use pretty_assertions::assert_eq;
use serde_json::json;

fn target() -> ReleaseNotesTarget {
    ReleaseNotesTarget {
        registries: ["https://pnpr.example/npm/".to_owned()].into(),
        publication: PublicationNotes {
            name: Some("v1.0.0".to_owned()),
            notes: Some("Release.".to_owned()),
        },
    }
}

#[test]
fn a_registry_that_stores_notes_gets_them() {
    let registry =
        parse_supported_registry_url("https://pnpr.example/npm/").expect("URL").normalized_url;
    let mut document = json!({ "name": "pkg" });
    target().attach_version_notes(&mut document, Some("Version."), &registry);
    let documents = [document];
    let body = serde_json::to_value(target().batch_body(&registry, &documents)).expect("body");

    assert_eq!(
        body,
        json!({
            "name": "v1.0.0",
            "notes": "Release.",
            "packages": [{ "name": "pkg", "notes": "Version." }],
        }),
    );
}

#[test]
fn any_other_registry_gets_the_documents_without_notes() {
    let registry =
        parse_supported_registry_url("https://registry.example/").expect("URL").normalized_url;
    let mut document = json!({ "name": "pkg" });
    target().attach_version_notes(&mut document, Some("Version."), &registry);
    let documents = [document];
    let body = serde_json::to_value(target().batch_body(&registry, &documents)).expect("body");

    assert_eq!(body, json!({ "packages": [{ "name": "pkg" }] }));
}
