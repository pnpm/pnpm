use super::PrefetchDownloads;
use pnpm_tarball::TarballError;

#[tokio::test]
async fn wait_reports_the_failed_downloads_by_url() {
    let downloads = PrefetchDownloads::default();
    downloads.track("https://registry.test/ok.tgz".to_string(), tokio::spawn(async { Ok(()) }));
    downloads.track(
        "https://registry.test/failed.tgz".to_string(),
        tokio::spawn(async {
            Err(TarballError::SiblingFetchFailed {
                url: "https://registry.test/failed.tgz".to_string(),
            })
        }),
    );

    let failed = downloads.wait().await;

    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, "https://registry.test/failed.tgz");
    assert!(downloads.wait().await.is_empty(), "a second wait has nothing left");
}

#[tokio::test]
async fn a_failed_download_is_reported_without_the_url_credentials() {
    let downloads = PrefetchDownloads::default();
    let url = "https://user:secret@registry.test/pkg.tgz?token=secret".to_string();
    downloads.track(
        url.clone(),
        tokio::spawn(
            async move { Err(TarballError::SiblingFetchFailed { url: "pkg".to_string() }) },
        ),
    );

    let error = crate::store_fetch::wait_for_prefetched_downloads(&downloads)
        .await
        .expect_err("the download failed");

    assert!(!error.to_string().contains("secret"), "{error}");
}
