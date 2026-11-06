use crate::server::{
    AuthedCaller,
    batch::serve_ecosystem_publish,
    publishing::{atomic::publication_request, publish_package},
    routing::state_and_router,
};
use axum::{
    body::{Bytes, to_bytes},
    extract::State,
    http::{HeaderMap, StatusCode},
};
use pnpr_auth::AuthState;
use pnpr_fixtures::MockConfigPresets;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use pnpr_storage::metadata::{BatchDocument, PublishNotes, ReleaseRecords};
use serde_json::{Value, json};
use tempfile::TempDir;

#[tokio::test]
async fn legacy_name_retries_return_the_recorded_outcome() {
    let name = CanonicalPackageName::parse("Upper", Ecosystem::Npm).unwrap();
    for target in [Some(&name), None] {
        let temp = TempDir::new().unwrap();
        let config =
            crate::Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().join("store"));
        let (state, _) = state_and_router(config, AuthState::in_memory(), None).unwrap();
        let identity = Identity::user("publisher");
        let document = json!({
            "name": "Upper",
            "versions": { "1.0.0": { "name": "Upper", "version": "1.0.0" } },
            "dist-tags": { "latest": "1.0.0" },
        });
        let body = match target {
            Some(_) => document.clone(),
            None => json!({ "packages": [document] }),
        };
        let request = publication_request(&identity, &body, None, None, target);
        let committed = state.inner.storage
            .sql_metadata()
            .commit_publication(
                "",
                vec![BatchDocument {
                    ecosystem: Ecosystem::Npm,
                    name: "Upper".to_string(),
                    bytes: serde_json::to_vec(&document).unwrap(),
                    expected: None,
                    principal: Some("publisher".to_string()),
                    records: ReleaseRecords::new(),
                }],
                Vec::new(),
                PublishNotes::default(),
                request,
            )
            .await
            .unwrap();
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
        let response = match target {
            Some(_) => publish_package(&state, &identity, None, "Upper", bytes).await,
            None => {
                serve_ecosystem_publish(
                    State(state),
                    AuthedCaller(identity),
                    None,
                    HeaderMap::new(),
                    bytes,
                )
                .await
            }
        };
        assert_eq!(
            response.headers()["pnpr-generation"].to_str().unwrap(),
            format!(":{}", committed.generation.value),
        );
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&bytes));
        let outcome: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(outcome["publication"], json!(committed.publication));
    }
}
