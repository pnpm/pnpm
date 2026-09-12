use super::{
    AuthState, Body, Ecosystem, Request, ServiceExt, StatusCode, TempDir, Value, basic, body_bytes,
    digest_of, get, header, json, oci_config, router_with_auth, strip_referrer_metadata, token,
};

#[tokio::test]
async fn referrer_pages_bound_migration_and_keep_filter_and_registry() {
    for hosted_store in [
        pnpr::HostedStoreConfig::Fs,
        pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: "pages/".into(),
        },
    ] {
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.hosted_store = hosted_store;
        let storage = pnpr_storage::Storage::new(
            &config.hosted_store,
            config.storage.clone(),
            config.cache_storage.clone(),
        )
        .unwrap()
        .for_hosted("images");
        let app = router_with_auth(config, AuthState::in_memory());
        let auth = basic(&token(&app).await);
        let subject = digest_of(b"subject");
        let mut expected = Vec::new();
        for index in 0..35 {
            let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
                "manifests": [], "subject": { "digest": subject, "size": 7 },
                "artifactType": "application/example+json", "annotations": { "index": index.to_string() } }).to_string();
            expected.push(digest_of(manifest.as_bytes()));
            let response = app
                .clone()
                .oneshot(
                    Request::put(format!("/v2/acme/paged/manifests/referrer-{index}"))
                        .header(header::AUTHORIZATION, &auth)
                        .body(Body::from(manifest))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        let key =
            pnpr_package_name::CanonicalPackageName::parse("acme/paged", Ecosystem::Oci).unwrap();
        let mut document: Value =
            serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
                .unwrap();
        strip_referrer_metadata(&mut document, 35);
        storage
            .update_hosted_document_with_retry(&key, 1, |_| {
                Ok(Some(serde_json::to_vec(&document).unwrap()))
            })
            .await
            .unwrap();
        let path = format!("/oci/~images/v2/acme/paged/referrers/{subject}?artifactType=absent");
        let response = get(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(header::LINK));
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["manifests"], json!([]));
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(
            document.manifests().iter().filter(|entry| entry.referrer.is_some()).count(),
            32,
        );
        let first = get(&app, &path);
        let second = get(&app, &path);
        let responses: [_; 2] = tokio::join!(first, second).into();
        for response in responses {
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key(header::LINK));
        }
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert!(document.manifests().iter().all(|entry| entry.referrer.is_some()));
        let path = format!(
            "/oci/~images/v2/acme/paged/referrers/{subject}?artifactType=application%2Fexample%2Bjson",
        );
        let (received, pages) = collect_filtered_referrer_pages(&app, path).await;
        expected.sort();
        assert_eq!(received, expected);
        assert_eq!(pages, 2);
    }
}

#[tokio::test]
async fn large_referrer_annotations_stay_out_of_repository_documents() {
    let tmp = TempDir::new().unwrap();
    let config = oci_config(tmp.path().to_path_buf(), "$all");
    let storage = pnpr_storage::Storage::new(
        &config.hosted_store,
        config.storage.clone(),
        config.cache_storage.clone(),
    )
    .unwrap()
    .for_hosted("images");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let subject = digest_of(b"subject");
    for index in 0..3 {
        let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
            "manifests": [], "subject": { "digest": subject, "size": 7 },
            "annotations": { "large": "a".repeat(2 * 1024 * 1024), "index": index.to_string() } });
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/v2/acme/large/manifests/referrer-{index}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::from(manifest.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let key = pnpr_package_name::CanonicalPackageName::parse("acme/large", Ecosystem::Oci).unwrap();
    let document = storage.read_hosted_document(&key).await.unwrap().unwrap();
    assert!(document.len() < 2048, "annotations must not inflate ordinary repository reads");
    let mut path = format!("/v2/acme/large/referrers/{subject}");
    let mut count = 0;
    loop {
        let response = get(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        let next = response.headers().get(header::LINK).map(|link| {
            link.to_str().unwrap().strip_prefix('<').unwrap().split_once('>').unwrap().0.to_string()
        });
        let bytes = body_bytes(response.into_body()).await;
        assert!(bytes.len() <= 4 * 1024 * 1024);
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["manifests"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload["manifests"][0]["annotations"]["large"].as_str().unwrap().len(),
            2 * 1024 * 1024,
        );
        count += 1;
        assert!(count <= 3);
        let Some(next) = next else { break };
        path = next;
    }
    assert_eq!(count, 3);
}

async fn collect_filtered_referrer_pages(
    app: &axum::Router,
    mut path: String,
) -> (Vec<String>, usize) {
    let mut received = Vec::new();
    let mut pages = 0;
    loop {
        let response = get(app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["oci-filters-applied"], "artifactType");
        let next = response.headers().get(header::LINK).map(|link| {
            let link = link.to_str().unwrap();
            assert!(link.contains("artifactType=application%2Fexample%2Bjson"));
            assert!(link.starts_with("</oci/~images/v2/"));
            link.strip_prefix('<').unwrap().split_once('>').unwrap().0.to_string()
        });
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        received.extend(
            payload["manifests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["digest"].as_str().unwrap().to_string()),
        );
        pages += 1;
        assert!(pages <= 2);
        let Some(next) = next else { break };
        path = next;
    }
    (received, pages)
}
