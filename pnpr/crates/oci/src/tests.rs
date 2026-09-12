use crate::{Digest, ImageDocument, Manifest, ManifestEntry, TagEntry, media_type};
use std::collections::HashSet;

fn digest_of(body: &str) -> Digest {
    Digest::of(body.as_bytes())
}

fn entry(body: &str) -> ManifestEntry {
    ManifestEntry {
        referrer: None,
        digest: digest_of(body),
        media_type: media_type::OCI_IMAGE_MANIFEST.to_string(),
        size: body.len() as u64,
    }
}

fn tag(name: &str, body: &str, updated: u64) -> TagEntry {
    TagEntry { tag: name.to_string(), digest: digest_of(body), updated }
}

#[test]
fn parses_and_rejects_digests() {
    let digest = Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
    assert_eq!(digest.blob_filename(), format!("sha256-{}", "a".repeat(64)));

    assert!(Digest::parse("sha256:short").is_err());
    assert!(Digest::parse(&format!("sha512:{}", "a".repeat(64))).is_err());
    assert!(Digest::parse(&"a".repeat(64)).is_err());
    // Uppercase hex names the same bytes, so admitting it would put one blob
    // at two storage keys.
    assert!(Digest::parse(&format!("sha256:{}", "A".repeat(64))).is_err());
}

#[test]
fn digest_of_matches_the_spec_vector() {
    assert_eq!(
        Digest::of(b"").to_string(),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

#[test]
fn resolves_a_reference_as_tag_or_digest() {
    let mut document = ImageDocument::new("acme/app");
    document.insert_manifest(entry("one"));
    document.set_tag(tag("latest", "one", 1));

    assert_eq!(document.resolve("latest").unwrap().digest, digest_of("one"));
    assert_eq!(document.resolve(&digest_of("one").to_string()).unwrap().digest, digest_of("one"));
    assert!(document.resolve("missing").is_none());
}

#[test]
fn lists_tags_in_lexical_order() {
    let mut document = ImageDocument::new("acme/app");
    document.insert_manifest(entry("one"));
    for name in ["v2", "latest", "v10"] {
        document.set_tag(tag(name, "one", 1));
    }
    assert_eq!(document.tag_names(), ["latest", "v10", "v2"]);
}

#[test]
fn removing_a_manifest_drops_the_tags_that_named_it() {
    let mut document = ImageDocument::new("acme/app");
    document.insert_manifest(entry("one"));
    document.insert_manifest(entry("two"));
    document.set_tag(tag("latest", "one", 1));
    document.set_tag(tag("stable", "two", 1));

    assert!(document.remove_manifest(&digest_of("one")));
    assert_eq!(document.tag_names(), ["stable"]);
    assert!(!document.remove_manifest(&digest_of("one")));
}

#[test]
fn removing_a_tag_keeps_the_manifest() {
    let mut document = ImageDocument::new("acme/app");
    document.insert_manifest(entry("one"));
    document.set_tag(tag("latest", "one", 1));

    assert!(document.remove_tag("latest"));
    assert!(document.manifest(&digest_of("one")).is_some());
    assert!(!document.remove_tag("latest"));
}

#[test]
fn merge_keeps_the_newer_tag_whichever_order_it_arrives_in() {
    fn older(document: &mut ImageDocument) {
        document.insert_manifest(entry("one"));
        document.set_tag(tag("latest", "one", 1));
    }
    fn newer(document: &mut ImageDocument) {
        document.insert_manifest(entry("two"));
        document.set_tag(tag("latest", "two", 2));
    }

    type Write = fn(&mut ImageDocument);
    let orders: [(Write, Write); 2] = [(older, newer), (newer, older)];
    for (first, second) in orders {
        let mut stored = ImageDocument::new("acme/app");
        first(&mut stored);
        let mut addition = ImageDocument::new("acme/app");
        second(&mut addition);

        stored.merge(addition, &HashSet::new());
        assert_eq!(stored.tag("latest").unwrap().digest, digest_of("two"));
    }
}

#[test]
fn merge_skips_entries_whose_blob_was_lost() {
    let mut stored = ImageDocument::new("acme/app");
    let mut addition = ImageDocument::new("acme/app");
    addition.insert_manifest(entry("one"));
    addition.set_tag(tag("latest", "one", 1));

    let lost = HashSet::from([digest_of("one").blob_filename()]);
    assert!(!stored.merge(addition, &lost));
    assert!(stored.manifests().is_empty());
    assert!(stored.tags().is_empty());
}

#[test]
fn merge_refuses_a_tag_with_no_manifest_behind_it() {
    let mut stored = ImageDocument::new("acme/app");
    let mut addition = ImageDocument::new("acme/app");
    addition.set_tag(tag("latest", "absent", 1));

    assert!(!stored.merge(addition, &HashSet::new()));
    assert!(stored.tags().is_empty());
}

#[test]
fn merge_reports_no_change_when_everything_is_already_held() {
    let mut stored = ImageDocument::new("acme/app");
    stored.insert_manifest(entry("one"));
    stored.set_tag(tag("latest", "one", 1));
    let addition = stored.clone();

    assert!(!stored.merge(addition, &HashSet::new()));
}

#[test]
fn document_round_trips() {
    let mut document = ImageDocument::new("acme/app");
    document.insert_manifest(entry("one"));
    document.set_tag(tag("latest", "one", 1));

    assert_eq!(ImageDocument::parse(&document.to_bytes()).unwrap(), document);
}

#[test]
fn parses_an_image_manifest() {
    let body = format!(
        r#"{{"schemaVersion":2,"mediaType":"{}","config":{{"digest":"{}","size":2}},
            "layers":[{{"digest":"{}","size":3}}]}}"#,
        media_type::OCI_IMAGE_MANIFEST,
        digest_of("config"),
        digest_of("layer"),
    );
    let manifest = Manifest::parse(body.as_bytes(), None).unwrap();
    assert_eq!(manifest.media_type(), media_type::OCI_IMAGE_MANIFEST);
    assert_eq!(manifest.references().count(), 2);
}

#[test]
fn takes_the_media_type_from_the_request_when_the_document_omits_it() {
    let body = format!(
        r#"{{"schemaVersion":2,"config":{{"digest":"{}","size":2}}}}"#,
        digest_of("config"),
    );
    let manifest =
        Manifest::parse(body.as_bytes(), Some(media_type::DOCKER_IMAGE_MANIFEST)).unwrap();
    assert_eq!(manifest.media_type(), media_type::DOCKER_IMAGE_MANIFEST);
}

#[test]
fn an_index_needs_no_config_and_references_its_children() {
    let body = format!(
        r#"{{"schemaVersion":2,"mediaType":"{}","manifests":[{{"digest":"{}","size":5}}]}}"#,
        media_type::OCI_IMAGE_INDEX,
        digest_of("child"),
    );
    let manifest = Manifest::parse(body.as_bytes(), None).unwrap();
    assert_eq!(manifest.references().count(), 1);
}

#[test]
fn rejects_unusable_manifests() {
    let config = format!(r#""config":{{"digest":"{}","size":2}}"#, digest_of("config"));
    // Docker's schema 1 is a different document entirely.
    let schema_one = format!(r#"{{"schemaVersion":1,{config}}}"#);
    assert!(Manifest::parse(schema_one.as_bytes(), None).is_err());
    // An image manifest with nothing to run.
    assert!(Manifest::parse(br#"{"schemaVersion":2}"#, None).is_err());
    assert!(Manifest::parse(b"not json", None).is_err());
    let unsupported_type = format!(r#"{{"schemaVersion":2,{config}}}"#);
    assert!(
        Manifest::parse(unsupported_type.as_bytes(), Some("application/octet-stream")).is_err(),
    );
}

#[test]
fn accepts_the_spec_tag_grammar() {
    for tag in ["latest", "1.0", "v1_2-3", "_leading", "A", &"a".repeat(128)] {
        assert!(crate::is_valid_tag(tag), "{tag} should be a valid tag");
    }
}

#[test]
fn refuses_references_that_are_neither_tag_nor_digest() {
    for tag in ["", ".start", "-start", "has/slash", "sha256:short", "has space", &"a".repeat(129)]
    {
        assert!(!crate::is_valid_tag(tag), "{tag} should not be a valid tag");
    }
}

#[test]
fn a_tag_written_in_the_same_millisecond_still_moves() {
    // Live writes to one repository are serialized by its package lock, so a
    // tie is an ordering the clock could not resolve, not a conflict.
    let same_instant = 1;
    let mut stored = ImageDocument::new("acme/app");
    stored.insert_manifest(entry("one"));
    stored.set_tag(tag("latest", "one", same_instant));

    let mut addition = ImageDocument::new("acme/app");
    addition.insert_manifest(entry("two"));
    addition.set_tag(tag("latest", "two", same_instant));

    assert!(stored.merge(addition, &HashSet::new()));
    assert_eq!(stored.tag("latest").unwrap().digest, digest_of("two"));
}

#[test]
fn a_stale_journaled_tag_cannot_move_a_re_pushed_tag_backward() {
    // The tag is pushed to `one`, a push to `two` is journaled but not
    // applied, then `one` is pushed again. Replaying the journal afterwards
    // must not resurrect `two`, which it would if the re-push had left the
    // first push's timestamp in place.
    let mut stored = ImageDocument::new("acme/app");
    stored.insert_manifest(entry("one"));
    stored.insert_manifest(entry("two"));
    stored.set_tag(tag("latest", "one", 1));

    let mut re_push = ImageDocument::new("acme/app");
    re_push.insert_manifest(entry("one"));
    re_push.set_tag(tag("latest", "one", 3));
    stored.merge(re_push, &HashSet::new());

    let mut journaled = ImageDocument::new("acme/app");
    journaled.insert_manifest(entry("two"));
    journaled.set_tag(tag("latest", "two", 2));
    stored.merge(journaled, &HashSet::new());

    assert_eq!(stored.tag("latest").unwrap().digest, digest_of("one"));
}

/// A document whose collections are both in the wrong order, with the
/// disorder asserted rather than assumed: which of two digests sorts first is
/// a fact about their hashes, so a fixture that relies on it silently stops
/// exercising the sort the day the labels change.
fn stored_out_of_order() -> serde_json::Value {
    assert!(digest_of("one").hex() > digest_of("two").hex(), "manifests must be unsorted");
    serde_json::json!({
        "name": "acme/app",
        "manifests": [entry("one"), entry("two")],
        "tags": [tag("zeta", "one", 1), tag("alpha", "two", 1)],
    })
}

#[test]
fn a_document_stored_out_of_order_still_finds_its_entries() {
    let stored = stored_out_of_order();
    let document = ImageDocument::parse(&serde_json::to_vec(&stored).unwrap()).unwrap();

    assert!(document.manifest(&digest_of("one")).is_some());
    assert!(document.manifest(&digest_of("two")).is_some());
    assert_eq!(document.resolve("alpha").unwrap().digest, digest_of("two"));
    assert_eq!(document.resolve("zeta").unwrap().digest, digest_of("one"));
    assert_eq!(document.tag_names(), ["alpha", "zeta"]);
}

#[test]
fn deserializing_directly_sorts_as_parsing_does() {
    let document: ImageDocument = serde_json::from_value(stored_out_of_order()).unwrap();

    assert!(document.manifest(&digest_of("one")).is_some());
    assert_eq!(document.tag_names(), ["alpha", "zeta"]);
}

#[test]
fn an_image_referrer_requires_an_artifact_type_or_config_media_type() {
    for artifact_type in [None, Some("")] {
        for config_media_type in [None, Some("")] {
            let mut manifest = serde_json::json!({
                "schemaVersion": 2,
                "mediaType": media_type::OCI_IMAGE_MANIFEST,
                "config": { "digest": digest_of("config"), "size": 6 },
                "subject": { "digest": digest_of("subject"), "size": 7 },
            });
            if let Some(artifact_type) = artifact_type {
                manifest["artifactType"] = artifact_type.into();
            }
            if let Some(config_media_type) = config_media_type {
                manifest["config"]["mediaType"] = config_media_type.into();
            }
            let result = Manifest::parse(&serde_json::to_vec(&manifest).unwrap(), None);
            assert!(matches!(result, Err(crate::ManifestError::MissingArtifactType)));
            manifest["artifactType"] = "application/example.signature".into();
            let parsed = Manifest::parse(&serde_json::to_vec(&manifest).unwrap(), None).unwrap();
            assert_eq!(parsed.artifact_type(), Some("application/example.signature"));
            assert!(manifest.as_object_mut().unwrap().remove("artifactType").is_some());
            manifest["config"]["mediaType"] = "application/example.config".into();
            let parsed = Manifest::parse(&serde_json::to_vec(&manifest).unwrap(), None).unwrap();
            assert_eq!(parsed.artifact_type(), Some("application/example.config"));
        }
    }
}

#[test]
fn deletion_generations_fence_staged_and_recovered_manifest_writes() {
    let mut stored = ImageDocument::new("acme/app");
    let mut staged = stored.clone();
    staged.insert_manifest(entry("manifest"));
    staged.set_tag(tag("latest", "manifest", 1));
    stored.generation = 1;
    stored.deleting_blob = Some(digest_of("layer"));
    assert!(!stored.merge(staged.clone(), &HashSet::new()));
    stored.deleting_blob = None;
    assert!(!stored.merge(staged.clone(), &HashSet::new()));
    assert!(stored.manifests().is_empty());
    staged.generation = 1;
    assert!(stored.merge(staged, &HashSet::new()));
    assert!(stored.resolve("latest").is_some());
}
