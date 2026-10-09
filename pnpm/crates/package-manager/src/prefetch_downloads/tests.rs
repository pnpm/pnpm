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
