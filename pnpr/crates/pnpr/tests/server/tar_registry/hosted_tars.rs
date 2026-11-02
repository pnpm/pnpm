use super::{
    AuthState, Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt,
    StatusCode, TempDir, body_bytes, body_json, config_for, get, gzip, header,
    hosted_publish_request, hosted_with_access, issue_token, router_with_auth, sha512_integrity,
    widget_tar,
};

/// A hosted `acme` registry that declares `tarballs: tar`, behind the
/// path-less `zstd` router, which serves tars too.
fn tar_acme_config(storage: std::path::PathBuf) -> Config {
    let mut config = config_for("http://127.0.0.1:1", storage);
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("zstd".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("zstd".to_string()));
    config.routing.tar_registries.insert("acme".to_string());
    config.routing.tar_registries.insert("zstd".to_string());
    config
}

/// The path of the file named `name` under `dir`.
fn find_file(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            if let Some(found) = find_file(&entry.path(), name) {
                return Some(found);
            }
        } else if entry.file_name() == name {
            return Some(entry.path());
        }
    }
    None
}

/// The names of every file under `dir`.
fn file_names(dir: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            names.extend(file_names(&entry.path()));
        } else {
            names.push(
                entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    names
}

async fn publish(app: &axum::Router, token: &str, version: &str, tgz: &[u8]) -> StatusCode {
    app.clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            version,
            tgz,
            token,
        ))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_hosted_tar_registry_stores_and_serves_only_the_tar() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(tar_acme_config(tmp.path().to_path_buf()), auth);
    let tar = widget_tar();
    assert_eq!(publish(&app, &token, "1.0.0", &gzip(&tar)).await, StatusCode::CREATED);

    let files = file_names(tmp.path());
    assert!(files.contains(&"widget-1.0.0.tar.zst".to_string()), "{files:?}");
    assert!(
        !files
            .iter()
            .any(|file| file.ends_with(".tgz")),
        "{files:?}",
    );

    let packument = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(&tar));

    let zstd = get(&app, "/npm/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(zstd.status(), StatusCode::OK);
    assert_eq!(zstd.headers()[header::CONTENT_ENCODING], "zstd");
    assert_eq!(zstd.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert!(zstd.headers().contains_key(header::CONTENT_LENGTH));
    let decoded = zstd::decode_all(body_bytes(zstd.into_body()).await.as_slice()).unwrap();
    assert_eq!(decoded, tar);

    let identity = get(&app, "/npm/@acme/widget@1.0.0", None).await;
    assert_eq!(identity.status(), StatusCode::OK);
    assert!(
        identity
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_none(),
    );
    assert_eq!(body_bytes(identity.into_body()).await, tar);

    let gzip_client = get(&app, "/npm/@acme/widget@1.0.0", Some("gzip")).await;
    assert_eq!(gzip_client.status(), StatusCode::OK);
    let gzipped = gzip_client
        .headers()
        .get(header::CONTENT_ENCODING)
        .cloned();
    let body = body_bytes(gzip_client.into_body()).await;
    let body = if gzipped.is_some_and(|coding| coding == "gzip") {
        let mut decoded = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(body.as_slice()),
            &mut decoded,
        )
        .unwrap();
        decoded
    } else {
        body.clone()
    };
    assert_eq!(body, tar);

    let through_hosted = get(&app, "/npm/~acme/@acme/widget@1.0.0", None).await;
    assert_eq!(body_bytes(through_hosted.into_body()).await, tar);
}

#[tokio::test]
async fn a_hosted_tar_registry_refuses_a_tgz_that_does_not_decode() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(tar_acme_config(tmp.path().to_path_buf()), auth);

    let mut truncated = gzip(&widget_tar());
    truncated.truncate(truncated.len() - 8);
    assert_eq!(publish(&app, &token, "1.0.0", &truncated).await, StatusCode::BAD_REQUEST);
    let files = file_names(tmp.path());
    assert!(
        !files
            .iter()
            .any(|file| file.contains("widget-1.0.0")),
        "{files:?}",
    );
}

#[tokio::test]
async fn deleting_a_version_of_a_hosted_tar_registry_removes_its_tar() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(tar_acme_config(tmp.path().to_path_buf()), auth);
    let tar = widget_tar();
    assert_eq!(publish(&app, &token, "1.0.0", &gzip(&tar)).await, StatusCode::CREATED);

    let delete = app
        .clone()
        .oneshot(
            Request::delete("/npm/~acme/@acme/widget/-/widget-1.0.0.tgz")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::CREATED);
    let files = file_names(tmp.path());
    assert!(!files.contains(&"widget-1.0.0.tar.zst".to_string()), "{files:?}");
    let served = get(&app, "/npm/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(served.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn verify_checks_a_stored_tar_against_its_tar_integrity() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(tar_acme_config(tmp.path().to_path_buf()), auth);
    assert_eq!(publish(&app, &token, "1.0.0", &gzip(&widget_tar())).await, StatusCode::CREATED);

    let config = tar_acme_config(tmp.path().to_path_buf());
    pnpr_storage::layout::write_local_layout_marker(&config.storage.hosted_dir).unwrap();
    let options =
        pnpr::verify::VerifyOptions { check_hashes: true, prune_orphans: false, min_age_secs: 0 };
    let report = pnpr::verify::verify_storage(&config, &options).await.unwrap();
    assert_eq!(report.blobs_checked, 1);
    assert!(report.corrupted_blobs.is_empty(), "{:?}", report.corrupted_blobs);
    assert!(report.orphaned_blobs.is_empty(), "{:?}", report.orphaned_blobs);

    let stored = find_file(tmp.path(), "widget-1.0.0.tar.zst").unwrap();
    std::fs::write(&stored, zstd::encode_all(&b"another tar"[..], 3).unwrap()).unwrap();
    let report = pnpr::verify::verify_storage(&config, &options).await.unwrap();
    assert_eq!(report.corrupted_blobs.len(), 1, "{:?}", report.corrupted_blobs);
    assert!(
        report.corrupted_blobs[0].contains("tar integrity mismatch"),
        "{:?}",
        report.corrupted_blobs,
    );
}
