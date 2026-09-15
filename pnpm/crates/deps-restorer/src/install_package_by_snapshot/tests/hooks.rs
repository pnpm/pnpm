use super::{
    super::InstallPackageBySnapshotError, leaked_offline_config, registry_metadata,
    run_snapshot_install_with_session, scripted_session,
};

/// A throwing fetcher hook surfaces as
/// [`InstallPackageBySnapshotError::CustomFetcher`] with the hook's
/// message, so an optional snapshot can swallow it via
/// `is_fetch_side_failure` and a required one aborts with context.
#[tokio::test]
async fn custom_fetcher_hook_error_propagates() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = registry_metadata();

    let session = scripted_session(
        true,
        Err(pnpm_hooks::HookError::Execution {
            pnpmfile: ".pnpmfile.cjs".to_string(),
            message: "fetch crashed".to_string(),
        }),
    );

    let err =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect_err("a throwing fetcher must fail the install");

    assert!(
        matches!(
            &err,
            InstallPackageBySnapshotError::CustomFetcher(message)
                if message.contains("fetch crashed"),
        ),
        "expected the hook error to propagate, got {err:?}",
    );

    drop(store_tmp);
}
