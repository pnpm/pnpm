use super::BatchGroup;
use crate::{
    parse_supported_registry_url,
    staged_archives::{MAX_STAGED_ARCHIVES_PER_PUBLISH, StagedArchive},
};
use serde_json::{Value, json};

fn staging_group(packages: usize) -> BatchGroup<'static> {
    BatchGroup {
        registry: parse_supported_registry_url("https://registry.test/").unwrap().normalized_url,
        package_names: vec![String::new(); packages],
        summary_indexes: (0..packages).collect(),
        documents: vec![json!({ "_attachments": { "pkg.tgz": { "data": "AAAA" } } }); packages],
        staged: Some(
            (0..packages)
                .map(|_| StagedArchive { integrity: "sha512-AAAA".to_owned(), bytes: b"\0\0\0" })
                .collect(),
        ),
    }
}

fn attachment(document: &Value) -> &Value {
    &document["_attachments"]["pkg.tgz"]
}

#[test]
fn a_group_within_the_commit_limit_names_its_archives_by_integrity() {
    let mut group = staging_group(MAX_STAGED_ARCHIVES_PER_PUBLISH);
    group.reference_staged_archives();
    assert!(group.staged.is_some());
    assert!(
        group.documents
            .iter()
            .all(|document| {
                attachment(document).get("data").is_none()
                    && attachment(document)["integrity"] == "sha512-AAAA"
            })
    );
}

#[test]
fn a_group_over_the_commit_limit_sends_its_archives_inline() {
    let mut group = staging_group(MAX_STAGED_ARCHIVES_PER_PUBLISH + 1);
    group.reference_staged_archives();
    assert!(group.staged.is_none());
    assert!(
        group.documents
            .iter()
            .all(|document| {
                attachment(document)["data"] == "AAAA"
                    && attachment(document).get("integrity").is_none()
            })
    );
}
