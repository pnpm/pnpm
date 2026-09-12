use super::{
    super::tarball_url_and_integrity, DUMMY_SHA512, custom_resolution_metadata,
    leaked_offline_config, run_snapshot_install_with_session, scripted_session,
};
use crate::install_package_by_snapshot::fetch::emit_progress_resolved;
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, PackageKey, RegistryResolution, TarballRevision};
use pnpm_reporter::{LogEvent, ProgressMessage, Reporter};
use pretty_assertions::assert_eq;
use std::sync::Mutex;

/// The (`package_id`, `requester`) pair pins pnpm's per-package
/// counter to the right row.
#[test]
fn emits_resolved_with_supplied_identifiers() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_progress_resolved::<RecordingReporter>("react@18.0.0", "/proj");

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(log)] if matches!(
                &log.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "expected a single Resolved event with matching identifiers; got {captured:?}",
    );
}
#[test]
fn registry_resolution_uses_scoped_registry_tarball_base() {
    let mut config = Config::new();
    config.registry = "https://default.example/npm/".to_string();
    config
        .registries_by_scope
        .insert("@private".to_string(), "https://private.example/npm/".to_string());

    let integrity = DUMMY_SHA512.parse().expect("parse integrity");
    let resolution = LockfileResolution::Registry(RegistryResolution { integrity, revision: None });
    let package_key: PackageKey = "@private/foo@1.0.0".parse().expect("parse package key");

    let (tarball_url, _) = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect("a registry resolution is always fetchable");

    assert_eq!(tarball_url.as_ref(), "https://private.example/npm/@private/foo/-/foo-1.0.0.tgz");
}
#[test]
fn registry_revision_uses_the_scoped_registry_digest_route() {
    let mut config = Config::new();
    config.registry = "https://default.example/npm/".to_string();
    config
        .registries_by_scope
        .insert("@private".to_string(), "https://private.example/npm/".to_string());
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: DUMMY_SHA512.parse().expect("parse integrity"),
        revision: Some(TarballRevision::try_from(2).unwrap()),
    });
    let package_key: PackageKey = "@private/foo@1.0.0".parse().expect("parse package key");

    let (tarball_url, _) = tarball_url_and_integrity(&resolution, &package_key, &config)
        .expect("a revision with complete integrity is fetchable");

    assert_eq!(
        tarball_url.as_ref(),
        format!("https://private.example/npm/-/tarballs/sha512/{}", "A".repeat(86)),
    );
}
/// A custom fetcher may delegate to a directory resolution, in which
/// case the file map points at mutable local source even though the
/// lockfile entry does not say so. Reporting that from the effective
/// resolution is what lets the slot be re-copied on the next install.
#[tokio::test]
async fn a_delegated_directory_resolution_reports_mutable_source() {
    let store_tmp = tempfile::tempdir().expect("create the store dir");
    let source = store_tmp.path().join("local-src");
    std::fs::create_dir_all(&source).expect("create the source dir");
    std::fs::write(
        source.join("package.json"),
        serde_json::json!({ "name": "foo", "version": "1.0.0" }).to_string(),
    )
    .expect("write the source manifest");

    let config = leaked_offline_config("https://registry.test", store_tmp.path());
    let metadata = custom_resolution_metadata("custom:cdn");

    let session = scripted_session(
        true,
        Ok(serde_json::json!({
            "delegate": { "type": "directory", "directory": "local-src" },
        })),
    );

    let installed =
        run_snapshot_install_with_session(config, &metadata, &session, None, store_tmp.path())
            .await
            .expect("the delegated directory resolution must drive the fetch");

    assert!(
        installed.source_is_mutable,
        "a delegated directory resolution must report mutable source",
    );

    drop(store_tmp);
}
