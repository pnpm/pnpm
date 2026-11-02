use super::{RecordError, StoredRecord};
use crate::{
    digest::RecordDigest,
    identity::RecordIdentity,
    test_support::{NPM_RECORD, OCI_RECORD, OCI_TOMBSTONE},
};
use pretty_assertions::assert_eq;

#[test]
fn reads_the_core_of_an_npm_record() {
    let record = StoredRecord::parse(NPM_RECORD.as_bytes().to_vec()).unwrap();
    assert_eq!(record.digest(), &RecordDigest::of(NPM_RECORD.as_bytes()));
    assert_eq!(record.bytes(), NPM_RECORD.as_bytes());
    let key = record.key();
    let RecordIdentity::Package(purl) = &key.identity else { panic!("not a package record") };
    assert_eq!(purl.as_str(), "pkg:npm/%40example/widgets@1.2.0");
    assert_eq!(key.revision, Some(0));
    assert_eq!(key.profile.as_str(), "npm-install-manifest/1");
    assert_eq!(record.image(), None);
    assert_eq!(record.log_entry().unwrap().digest(), record.digest());
}

#[test]
fn reads_oci_tag_records_and_tombstones() {
    let record = StoredRecord::parse(OCI_RECORD.as_bytes().to_vec()).unwrap();
    assert_eq!(
        record.key().identity,
        RecordIdentity::OciTag { repository: "acme/app".to_owned(), tag: "1.0".to_owned() },
    );
    assert_eq!(record.key().revision, Some(3));
    assert!(
        record
            .image()
            .unwrap()
            .as_str()
            .starts_with("pkg:oci/app@sha256:0123"),
    );

    let tombstone = StoredRecord::parse(OCI_TOMBSTONE.as_bytes().to_vec()).unwrap();
    assert_eq!(tombstone.key().revision, Some(4));
    assert_eq!(tombstone.image(), None);
}

#[test]
fn an_oci_tag_record_must_carry_its_purl_even_when_null() {
    let missing = OCI_TOMBSTONE.replace(r#","purl":null"#, "");
    let error = StoredRecord::parse(missing.into_bytes()).unwrap_err();
    assert!(matches!(error, RecordError::MissingField { field: "purl", .. }), "{error}");
    let no_revision = OCI_RECORD.replace(r#""revision":3,"#, "");
    let error = StoredRecord::parse(no_revision.into_bytes()).unwrap_err();
    assert!(matches!(error, RecordError::MissingField { field: "revision", .. }), "{error}");
}

#[test]
fn rejects_records_that_do_not_match_their_profile() {
    let null_purl = NPM_RECORD.replace(r#""pkg:npm/%40example/widgets@1.2.0""#, "null");
    let error = StoredRecord::parse(null_purl.into_bytes()).unwrap_err();
    assert!(matches!(error, RecordError::MissingField { field: "purl", .. }), "{error}");
    let cargo_purl = NPM_RECORD.replace("pkg:npm/%40example/widgets", "pkg:cargo/widgets");
    let error = StoredRecord::parse(cargo_purl.into_bytes()).unwrap_err();
    assert!(
        matches!(&error, RecordError::WrongPurlType { purl_type, .. } if purl_type == "cargo"),
        "{error}",
    );
    let schema = NPM_RECORD.replace("pnpr-release-record/1", "pnpr-release-record/2");
    let error = StoredRecord::parse(schema.into_bytes()).unwrap_err();
    assert!(matches!(error, RecordError::UnsupportedSchema { .. }), "{error}");
}

#[test]
fn accepts_records_of_profiles_it_does_not_know() {
    let future = NPM_RECORD.replace("npm-install-manifest/1", "npm-install-manifest/2");
    let record = StoredRecord::parse(future.into_bytes()).unwrap();
    assert_eq!(record.key().profile.as_str(), "npm-install-manifest/2");
}

#[test]
fn checks_bytes_against_an_expected_digest() {
    let bytes = NPM_RECORD.as_bytes().to_vec();
    let expected = RecordDigest::of(&bytes);
    assert!(StoredRecord::parse_expecting(bytes.clone(), expected).is_ok());
    let reformatted =
        serde_json::to_vec(&serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()).unwrap();
    let error = StoredRecord::parse_expecting(reformatted, expected).unwrap_err();
    assert!(matches!(error, RecordError::DigestMismatch { .. }), "{error}");
}
