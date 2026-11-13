//! A publish authorized before the package lock is checked again against
//! the document it is staged onto.

use super::{package_tar, sha512};
use crate::server::{
    publishing::{stage_publish, validate_publish_doc},
    routing::state_and_router,
};
use base64::Engine;
use pnpr_auth::AuthState;
use pnpr_error::RegistryError;
use pnpr_fixtures::MockConfigPresets;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::{Identity, PackageRules};
use pnpr_storage::database::{BatchDocument, PublishNotes, ReleaseRecords};
use serde_json::{Value, json};
use std::io::Write;
use tempfile::TempDir;

fn document(version: &str, maintainer: &str) -> Value {
    json!({
        "name": "race-pkg",
        "versions": { version: { "name": "race-pkg", "version": version } },
        "dist-tags": { "latest": version },
        "maintainers": [{ "name": maintainer, "userId": maintainer }],
    })
}

/// An npm publish body for `version` of `race-pkg`, its tarball attached.
fn publish_body(version: &str) -> Value {
    let manifest = json!({ "name": "race-pkg", "version": version });
    let tar = package_tar(manifest.to_string().as_bytes());
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    let tgz = encoder.finish().unwrap();
    let mut version_entry = manifest;
    version_entry["dist"] = json!({ "integrity": sha512(&tgz).to_string() });
    json!({
        "name": "race-pkg",
        "versions": { version: version_entry },
        "dist-tags": { "latest": version },
        "_attachments": {
            format!("race-pkg-{version}.tgz"): {
                "content_type": "application/octet-stream",
                "data": base64::engine::general_purpose::STANDARD.encode(&tgz),
                "length": tgz.len(),
            },
        },
    })
}

#[tokio::test]
async fn a_publish_admitted_for_a_new_name_is_refused_once_another_account_claims_it() {
    let temp = TempDir::new().unwrap();
    let mut config =
        crate::Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().join("store"));
    config.routing.hosted.get_mut("local").unwrap().rules = PackageRules::new(Vec::new(), None);
    let (state, _) = state_and_router(config, AuthState::in_memory(), None).unwrap();
    let name = CanonicalPackageName::parse("race-pkg", Ecosystem::Npm).unwrap();

    let (bob, target) =
        validate_publish_doc(&state, &Identity::user("bob"), None, name, publish_body("1.0.1"))
            .await
            .unwrap();
    state.inner.storage
        .sql_database()
        .commit_publication(
            &target.namespace,
            vec![BatchDocument {
                ecosystem: Ecosystem::Npm,
                name: "race-pkg".to_string(),
                bytes: serde_json::to_vec(&document("1.0.0", "alice")).unwrap(),
                expected: None,
                principal: Some("alice".to_string()),
                records: ReleaseRecords::new(),
            }],
            Vec::new(),
            PublishNotes::default(),
            None,
        )
        .await
        .unwrap();

    let err = stage_publish(&state, bob, "2026-01-01T00:00:00.000Z", Some(&target.namespace))
        .await
        .err()
        .expect("bob does not maintain the package alice claimed");
    assert!(matches!(err, RegistryError::Forbidden { .. }), "{err:?}");
}
