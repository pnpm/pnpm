use super::{
    Arc, AuthHeaders, FetchTarballForResolution, SilentReporter, StoreIndex, StoreIndexWriter,
    ThrottledClient, fast_retry_opts, gzipped_archive, store_index_key, tempdir_with_leaked_path,
};

/// The row would be keyed by the subpackage (read from
/// `<subdir>/package.json`) while carrying an index built from the
/// whole archive — the repo's manifest and every repo file. Writing
/// that would hand any consumer trusting the key a payload describing
/// the repo, so no row is written at all.
#[tokio::test]
async fn fetch_for_resolution_writes_no_index_row_for_a_subdirectory_package() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/package.json", r#"{"name":"foo","version":"1.2.3"}"#),
    ]);
    let _mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();
    let (writer, writer_task) = StoreIndexWriter::spawn(store_path);

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: Some(Arc::clone(&writer)),
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("subdirectory fetch should succeed");

    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");

    // The key the row *would* have taken, had one been written.
    let key = store_index_key(&resolved.integrity.to_string(), "foo@1.2.3");
    let index = StoreIndex::open_in(store_path).expect("open store index");
    let rows = index.get_many(std::slice::from_ref(&key)).expect("read index");
    assert!(rows.is_empty(), "a subpackage key must not carry the whole repo's index: {rows:?}");
    drop(store_dir_keep);
}
