use super::{build_zip, fast_retry_opts, gzipped_tar, tempdir_with_leaked_path};
use crate::{ArchiveStoreProjection, IngestTarballToStore, IngestZipArchiveToStore, TarballError};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter};
use pnpm_store_dir::{StoreIndex, StoreIndexWriter};
use ssri::Integrity;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[tokio::test]
async fn archive_requests_preserve_the_deployments_redirect_guard() {
    let mut source = mockito::Server::new_async().await;
    let mut target = mockito::Server::new_async().await;
    let blocked = target.mock("GET", "/private").expect(0).create_async().await;
    let redirect = source
        .mock("GET", "/artifact")
        .with_status(302)
        .with_header("location", &format!("{}/private", target.url()))
        .expect(1)
        .create_async()
        .await;
    let allowed = source.url();
    let client = ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
        url.as_str().starts_with(&allowed)
    });
    let result = crate::archive_request::request_archive::<SilentReporter>(
        &client,
        &format!("{}/artifact", source.url()),
        "fixture",
        &AuthHeaders::default(),
        0,
        0,
        false,
    )
    .await;
    let error = result.err().expect("off-allowlist redirect must fail");
    eprintln!("error={error}");
    assert!(matches!(error, TarballError::FetchTarball(_)));
    redirect.assert_async().await;
    blocked.assert_async().await;
}

#[tokio::test]
async fn archive_retry_redacts_secrets_and_accepts_the_maximum_retry_budget() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }
    let mut server = mockito::Server::new_async().await;
    let url = format!(
        "{}/artifact?token=secret#fragment",
        server.url().replacen("http://", "http://user:password@", 1),
    );
    let failed = server
        .mock("GET", "/artifact?token=secret")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("GET", "/artifact?token=secret")
        .with_body("done")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth = AuthHeaders::default();
    crate::archive_retry::retry_archive::<RecordingReporter, _, _>(
        &url,
        "fixture",
        "test",
        None,
        RetryOpts { retries: u32::MAX, ..fast_retry_opts() },
        |attempt| {
            crate::archive_request::request_archive::<RecordingReporter>(
                &client, &url, "fixture", &auth, 0, attempt, false,
            )
        },
    )
    .await
    .unwrap();
    let events = EVENTS.lock().unwrap().clone();
    eprintln!("events={events:?}");
    let retry = events
        .iter()
        .find_map(|event| match event {
            LogEvent::RequestRetry(retry) => Some(retry),
            _ => None,
        })
        .unwrap();
    assert_eq!(retry.max_retries, u32::MAX);
    assert_eq!(retry.url, format!("{}/artifact", server.url()));
    for secret in ["password", "token", "secret", "fragment"] {
        assert!(!retry.error.message.contains(secret));
    }
    failed.assert_async().await;
    success.assert_async().await;
}

#[tokio::test]
async fn archive_network_errors_remove_urls_from_the_source_chain() {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let url = format!("http://user:password@{address}/artifact?token=secret#fragment");
    let client = ThrottledClient::default();
    let result = crate::archive_request::request_archive::<SilentReporter>(
        &client,
        &url,
        "fixture",
        &AuthHeaders::default(),
        0,
        u32::MAX,
        false,
    )
    .await;
    let error = result.err().expect("closed port must fail");
    eprintln!("error={error:?}");
    let mut source: Option<&dyn std::error::Error> = Some(&error);
    while let Some(error) = source {
        for secret in ["password", "token", "secret", "fragment"] {
            assert!(!error.to_string().contains(secret), "exposed {secret}");
        }
        source = error.source();
    }
}

#[derive(Debug, Clone, Copy)]
enum Container {
    TarGz,
    Zip,
}

#[tokio::test]
async fn archive_requests_do_not_downgrade_secure_credentials_to_plain_http() {
    let mut server = mockito::Server::new_async().await;
    let address: std::net::SocketAddr = server.host_with_port().parse().unwrap();
    let request = server
        .mock("GET", "/simple/pkg.whl")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_body("wheel")
        .expect(1)
        .create_async()
        .await;
    let client =
        reqwest::Client::builder().no_proxy().resolve("registry.example", address).build().unwrap();
    let no_redirects = reqwest::Client::builder()
        .no_proxy()
        .resolve("registry.example", address)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let client = ThrottledClient::from_clients(client, no_redirects);
    let mut auth = AuthHeaders::default().with_secure_transport();
    auth.insert_url_header(
        &format!("https://registry.example:{}/simple/", address.port()),
        "Basic secret".to_string(),
    );
    let url = format!("http://registry.example:{}/simple/pkg.whl", address.port());
    let (_guard, response) = crate::archive_request::request_archive::<SilentReporter>(
        &client,
        &url,
        "python:alpha",
        &auth,
        0,
        0,
        false,
    )
    .await
    .unwrap();
    eprintln!("response={response:?}");
    assert_eq!(response.bytes().await.unwrap(), "wheel");
    request.assert_async().await;
}

impl Container {
    const ALL: [Self; 2] = [Self::TarGz, Self::Zip];

    fn body(self) -> Vec<u8> {
        let entries: &[(&str, &[u8])] = &[("artifact/data.txt", b"native artifact")];
        match self {
            Self::TarGz => gzipped_tar(entries),
            Self::Zip => build_zip(entries),
        }
    }

    async fn ingest(
        self,
        input: &IngestTarballToStore<'_>,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        match self {
            Self::TarGz => input.run_without_mem_cache::<SilentReporter>().await,
            Self::Zip => {
                IngestZipArchiveToStore {
                    http_client: input.http_client,
                    store_dir: input.store_dir,
                    store_index: input.store_index.clone(),
                    store_index_writer: input.store_index_writer.clone(),
                    verify_store_integrity: input.verify_store_integrity,
                    strict_store_pkg_content_check: input.strict_store_pkg_content_check,
                    verified_files_cache: Arc::clone(&input.verified_files_cache),
                    package_integrity: input.package_integrity.unwrap(),
                    package_url: input.package_url,
                    package_id: input.package_id,
                    requester: input.requester,
                    prefetched_cas_paths: input.prefetched_cas_paths,
                    retry_opts: input.retry_opts,
                    auth_headers: input.auth_headers,
                    archive_prefix: Some("artifact"),
                    ignore_file_pattern: input.ignore_file_pattern.clone(),
                    offline: input.offline,
                    store_projection: input.store_projection,
                }
                .run_without_mem_cache::<SilentReporter>()
                .await
            }
        }
    }
}

#[tokio::test]
async fn formats_share_projection_offline_replay_and_missing_blob_validation() {
    for container in Container::ALL {
        for projection in [
            ArchiveStoreProjection::RawArchive,
            ArchiveStoreProjection::Package {
                append_manifest: Some(br#"{"name":"fixture","version":"1.0.0"}"#),
            },
        ] {
            eprintln!("container={container:?}, projection={projection:?}");
            let mut registry = mockito::Server::new_async().await;
            let body = container.body();
            let integrity = Integrity::from(body.as_slice());
            let request =
                registry.mock("GET", "/artifact").with_body(body).expect(1).create_async().await;
            let (_directory, store) = tempdir_with_leaked_path();
            store.init().unwrap();
            let (writer, task) = StoreIndexWriter::spawn(store);
            let client = ThrottledClient::default();
            let auth = AuthHeaders::default();
            let url = format!("{}/artifact", registry.url());
            let mut input = IngestTarballToStore {
                http_client: &client,
                store_dir: store,
                store_index: None,
                store_index_writer: Some(Arc::clone(&writer)),
                verify_store_integrity: true,
                strict_store_pkg_content_check: true,
                verified_files_cache: Arc::default(),
                package_integrity: Some(&integrity),
                package_unpacked_size: None,
                package_file_count: None,
                package_url: &url,
                package_id: "fixture@1.0.0",
                requester: "contract test",
                prefetched_cas_paths: None,
                retry_opts: fast_retry_opts(),
                auth_headers: &auth,
                ignore_file_pattern: None,
                offline: false,
                progress_reported: None,
                store_projection: projection,
            };
            let paths = container.ingest(&input).await.unwrap();
            dbg!(&paths);
            assert_eq!(std::fs::read(&paths["data.txt"]).unwrap(), b"native artifact");
            assert_eq!(
                paths.contains_key("package.json"),
                matches!(projection, ArchiveStoreProjection::Package { .. }),
            );
            input.store_index_writer = None;
            drop(writer);
            StoreIndexWriter::drain(task, "contract test").await;
            input.store_index = StoreIndex::shared_readonly_in(store);
            input.offline = true;
            assert_eq!(container.ingest(&input).await.unwrap(), paths);

            std::fs::remove_file(&paths["data.txt"]).unwrap();
            input.verified_files_cache = Arc::default();
            let error = container.ingest(&input).await.unwrap_err();
            eprintln!("missing blob: {error}");
            assert!(matches!(error, TarballError::NoOfflineTarball { .. }));
            request.assert_async().await;
        }
    }
}

#[tokio::test]
async fn formats_share_retry_classification_and_never_publish_failed_integrity() {
    for container in Container::ALL {
        for (status, attempts) in [(401, 1), (503, 3), (200, 3)] {
            eprintln!("container={container:?}, status={status}");
            let mut registry = mockito::Server::new_async().await;
            let body = container.body();
            let integrity = Integrity::from(body.as_slice());
            let request = registry
                .mock("GET", "/artifact")
                .with_status(status)
                .with_body("corrupted transfer")
                .expect(attempts)
                .create_async()
                .await;
            let (_directory, store) = tempdir_with_leaked_path();
            store.init().unwrap();
            let (writer, task) = StoreIndexWriter::spawn(store);
            let client = ThrottledClient::default();
            let auth = AuthHeaders::default();
            let url = format!("{}/artifact", registry.url());
            let mut input = IngestTarballToStore {
                http_client: &client,
                store_dir: store,
                store_index: None,
                store_index_writer: Some(Arc::clone(&writer)),
                verify_store_integrity: true,
                strict_store_pkg_content_check: true,
                verified_files_cache: Arc::default(),
                package_integrity: Some(&integrity),
                package_unpacked_size: None,
                package_file_count: None,
                package_url: &url,
                package_id: "fixture@1.0.0",
                requester: "contract test",
                prefetched_cas_paths: None,
                retry_opts: fast_retry_opts(),
                auth_headers: &auth,
                ignore_file_pattern: None,
                offline: false,
                progress_reported: None,
                store_projection: ArchiveStoreProjection::RawArchive,
            };
            let error = container.ingest(&input).await.unwrap_err();
            eprintln!("failed fetch: {error}");
            if status == 200 {
                assert!(matches!(error, TarballError::Checksum(_)));
            } else {
                assert!(matches!(error, TarballError::HttpStatus(_)));
            }
            input.store_index_writer = None;
            drop(writer);
            StoreIndexWriter::drain(task, "contract test").await;
            input.store_index = StoreIndex::shared_readonly_in(store);
            input.offline = true;
            let error = container.ingest(&input).await.unwrap_err();
            eprintln!("failed fetch was not published: {error}");
            assert!(matches!(error, TarballError::NoOfflineTarball { .. }));
            request.assert_async().await;
        }
    }
}
