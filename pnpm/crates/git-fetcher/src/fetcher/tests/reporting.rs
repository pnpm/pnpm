#[cfg(unix)]
use super::{
    GitFetcherError, SilentReporter, StoreDir, failing_fetcher, tempdir, write_failing_git_shim,
};

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_shallow_fetch_is_reported_like_a_failed_clone() {
    let tmp = tempdir().unwrap();
    let shim_path = write_failing_git_shim(&tmp.path().join("shim"));
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let shallow_hosts = vec!["github.com".to_string()];

    let source_cache = crate::GitSourceCache::default();
    let mut fetcher = failing_fetcher(
        &source_cache,
        "ssh://git@github.com/acme/widget.git",
        &store_dir,
        &shim_path,
    );
    fetcher.git_shallow_hosts = &shallow_hosts;
    let err = fetcher.run::<SilentReporter>().await.expect_err("the shim fails every fetch");

    let GitFetcherError::FetchOverSsh { package, host, .. } = &err else {
        panic!("expected FetchOverSsh; got {err:?}");
    };
    assert_eq!(package, "@scope/pkg");
    assert_eq!(host, "github.com");
}
