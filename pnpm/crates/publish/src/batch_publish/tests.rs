use super::{BatchGroup, BatchPublishError, DocumentArchives};
use crate::{
    PackedPkg, StagedReference, parse_supported_registry_url,
    staged_archives::{MAX_STAGED_ARCHIVES_PER_PUBLISH, StagedArchive},
};
use serde_json::{Value, json};

fn staging_group(packages: usize) -> BatchGroup<'static> {
    BatchGroup {
        registry: parse_supported_registry_url("https://registry.test/").unwrap().normalized_url,
        package_names: vec![String::new(); packages],
        summary_indexes: (0..packages).collect(),
        documents: vec![json!({ "_attachments": { "pkg.tgz": { "data": "AAAA" } } }); packages],
        archives: (0..packages)
            .map(|_| DocumentArchives::Single("sha512-AAAA".to_owned()))
            .collect(),
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
    group.reference_staged_archives().unwrap();
    assert!(group.staged.is_some());
    assert!(
        group.documents
            .iter()
            .all(|document| {
                attachment(document).get("data").is_none()
                    && attachment(document)["integrity"] == "sha512-AAAA"
            }),
    );
}

#[test]
fn a_group_over_the_commit_limit_sends_its_archives_inline() {
    let mut group = staging_group(MAX_STAGED_ARCHIVES_PER_PUBLISH + 1);
    group.reference_staged_archives().unwrap();
    assert!(group.staged.is_none());
    assert!(
        group.documents
            .iter()
            .all(|document| {
                attachment(document)["data"] == "AAAA"
                    && attachment(document).get("integrity").is_none()
            }),
    );
}

/// A version with variants is published only from staged archives, so a
/// group over the commit limit that holds one fails instead of sending its
/// archives inline.
#[test]
fn a_group_with_variants_over_the_commit_limit_is_refused() {
    let manifest = json!({ "name": "example-cli", "version": "4.0.0" });
    let staged = StagedReference { integrity: "sha512-any".into(), label: "any".into() };
    let package = PackedPkg {
        published_manifest: &manifest,
        tarball_data: b"",
        tarball_path: "example-cli-4.0.0.tgz",
        contents: &[],
        unpacked_size: 0,
        notes: None,
        variants: &[],
        any_staged: Some(&staged),
    };
    let mut group = staging_group(MAX_STAGED_ARCHIVES_PER_PUBLISH);
    group.documents.push(json!({}));
    group.archives.push(DocumentArchives::Variants(&package));
    assert!(matches!(
        group.reference_staged_archives(),
        Err(BatchPublishError::TooManyArchives { count, .. })
            if count == MAX_STAGED_ARCHIVES_PER_PUBLISH + 1,
    ));
}
