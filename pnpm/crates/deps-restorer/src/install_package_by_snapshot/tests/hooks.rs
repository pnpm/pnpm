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

#[tokio::test]
async fn completed_fetch_under_tarball_url_is_reused_by_snapshot_install() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let mut metadata = registry_metadata();
    let integrity: ssri::Integrity = "sha512-9876543210abcdef".parse().unwrap();
    let tarball_url = "https://registry.test/foo-1.0.0.tgz";
    metadata.resolution =
        pnpm_lockfile::LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
            tarball: tarball_url.to_string(),
            integrity: Some(integrity.clone()),
            path: None,
            git_hosted: None,
            revision: None,
        });

    let session = scripted_session(
        true,
        Err(pnpm_hooks::HookError::Execution {
            pnpmfile: ".pnpmfile.cjs".to_string(),
            message: "fetch was called again".to_string(),
        }),
    );

    let fetched = std::sync::Arc::new(pnpm_tarball::FetchedTarball {
        files_map: std::collections::HashMap::new(),
        integrity: integrity.clone(),
        manifest: None,
        requires_build: false,
    });
    session.register_completed_for_test(tarball_url, &integrity, fetched);

    let installed =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect("should reuse completed tarball from resolution without calling hook again");

    assert!(!installed.source_is_mutable);
    drop(store_tmp);
}

#[tokio::test]
async fn completed_fetch_under_name_ver_is_reused_when_installed_by_snapshot() {
    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let mut metadata = registry_metadata();
    let integrity: ssri::Integrity = "sha512-1234567890abcdef".parse().unwrap();
    let tarball_url = "https://registry.test/different-url.tgz";
    metadata.resolution =
        pnpm_lockfile::LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
            tarball: tarball_url.to_string(),
            integrity: Some(integrity.clone()),
            path: None,
            git_hosted: None,
            revision: None,
        });

    let session = scripted_session(
        true,
        Err(pnpm_hooks::HookError::Execution {
            pnpmfile: ".pnpmfile.cjs".to_string(),
            message: "fetch was called again".to_string(),
        }),
    );

    let fetched = std::sync::Arc::new(pnpm_tarball::FetchedTarball {
        files_map: std::collections::HashMap::new(),
        integrity: integrity.clone(),
        manifest: None,
        requires_build: false,
    });
    session.register_completed_for_test("foo@1.0.0", &integrity, fetched);

    let installed =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect("should reuse completed tarball keyed by name@version");

    assert!(!installed.source_is_mutable);
    drop(store_tmp);
}
