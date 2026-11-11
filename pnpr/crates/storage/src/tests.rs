use super::{
    AsyncWriteExt, BlobWrite, CanonicalPackageName, DocumentWrite, HostedStoreConfig,
    RegistryRecord, Storage, atomic_write::create_tmp_file_with, fs,
};
use crate::archives::testing::{served, store_file};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(
        &HostedStoreConfig::Fs,
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
    .unwrap()
}

fn pkg(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm).unwrap()
}

#[test]
fn document_write_conflict_delay_caps_growth() {
    assert_eq!(super::document_write_conflict_delay(0).as_millis(), 5);
    assert_eq!(super::document_write_conflict_delay(1).as_millis(), 10);
    assert_eq!(super::document_write_conflict_delay(6).as_millis(), 250);
    assert_eq!(super::document_write_conflict_delay(32).as_millis(), 250);
}

#[tokio::test]
async fn temp_file_creation_retries_existing_candidate_without_overwriting() {
    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let occupied = tmp.path().join("foo-1.0.0.tgz.tmp.occupied");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&occupied, b"occupied").await.unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { occupied.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&occupied).await.unwrap(), b"occupied");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
}

#[cfg(unix)]
#[tokio::test]
async fn temp_file_creation_does_not_follow_symlink_candidate() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let victim = tmp.path().join("victim");
    let symlink_path = tmp.path().join("foo-1.0.0.tgz.tmp.symlink");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&victim, b"victim").await.unwrap();
    symlink(&victim, &symlink_path).unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { symlink_path.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&victim).await.unwrap(), b"victim");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
    assert!(
        std::fs::symlink_metadata(&symlink_path)
            .unwrap()
            .file_type()
            .is_symlink(),
    );
}

#[tokio::test]
async fn failed_blob_finalize_removes_tmp_file() {
    let tmp = TempDir::new().unwrap();
    let tmp_path = tmp.path().join("foo-1.0.0.tgz.tmp.test");
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    fs::create_dir(&final_path).await.unwrap();
    fs::write(final_path.join("block-rename"), b"occupied").await.unwrap();

    let file = fs::File::create(&tmp_path).await.unwrap();
    let mut write = BlobWrite { file: Some(file), tmp_path: Some(tmp_path.clone()), final_path };
    write.write_all(b"tarball").await.unwrap();

    assert!(write.finalize().await.is_err());
    assert!(!tmp_path.exists(), "failed finalization must remove its temporary file");
}

#[tokio::test]
async fn nested_names_are_stored_as_siblings() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp).for_hosted("", pnpr_package_name::Ecosystem::Oci);
    let parent =
        CanonicalPackageName::parse("acme/app", pnpr_package_name::Ecosystem::Oci).unwrap();
    let child =
        CanonicalPackageName::parse("acme/app/tool", pnpr_package_name::Ecosystem::Oci).unwrap();
    let top = CanonicalPackageName::parse("acme", pnpr_package_name::Ecosystem::Oci).unwrap();
    let blobs_repo =
        CanonicalPackageName::parse("acme/blobs", pnpr_package_name::Ecosystem::Oci).unwrap();
    for name in [&parent, &child, &top, &blobs_repo] {
        storage.write_hosted_document_if_current(name, b"{}", None, None).await.unwrap();
    }
    store_file(&storage, &top, "sha256-top", b"top-layer").await;
    store_file(&storage, &blobs_repo, "sha256-repo", b"repo-layer").await;
    assert_eq!(
        storage.hosted_package_names().await.unwrap(),
        ["acme", "acme/app", "acme/app/tool", "acme/blobs"],
    );
    assert_eq!(served(&storage, &top, "sha256-top").await.as_deref(), Some(&b"top-layer"[..]));
    assert_eq!(served(&storage, &top, "sha256-repo").await, None);
    assert_eq!(
        served(&storage, &blobs_repo, "sha256-repo").await.as_deref(),
        Some(&b"repo-layer"[..]),
    );
    storage.remove_package(&parent, None).await.unwrap();
    assert_eq!(
        storage.hosted_package_names().await.unwrap(),
        ["acme", "acme/app/tool", "acme/blobs"],
    );
}

#[tokio::test]
async fn each_namespace_stores_an_archive_under_its_key() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("@scope/App");
    let mut keys = Vec::new();
    for namespace in
        [storage.clone(), storage.for_hosted("npm~internal", pnpr_package_name::Ecosystem::Npm)]
    {
        let promoted = store_file(&namespace, &name, "App-1.0.0.tgz", b"tarball").await;
        keys.push(promoted.artifact.blob.key());
    }
    assert_eq!(keys[0], keys[1], "the same bytes have one key");
    for org in ["", "npm~internal"] {
        let path = crate::layout::local_archive_path(
            &tmp.path().join("storage"),
            org,
            pnpr_package_name::Ecosystem::Npm,
            &keys[0],
        );
        assert!(path.is_file(), "{}", path.display());
    }
}

#[tokio::test]
async fn an_org_named_like_a_package_is_its_own_namespace() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let org = storage.for_hosted("acme", pnpr_package_name::Ecosystem::Npm);
    storage
        .write_hosted_document_if_current(&pkg("acme"), b"{}", None, None)
        .await
        .unwrap();
    org.write_hosted_document_if_current(&pkg("tool"), b"{}", None, None).await
        .unwrap();
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme"]);
    storage
        .remove_package(&pkg("acme"), None)
        .await
        .unwrap();
    assert_eq!(org.hosted_package_names().await.unwrap(), ["tool"]);
}

#[tokio::test]
async fn a_store_without_packages_gets_the_layout_marker() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    fs::create_dir_all(tmp.path().join("storage/.team-rosters")).await.unwrap();
    storage.ensure_hosted_layout().await.unwrap();
    let marker = fs::read(tmp.path().join("storage/layout.json")).await.unwrap();
    let parsed = serde_json::from_slice::<crate::layout::LayoutMarker>(&marker).unwrap();
    assert_eq!(parsed.layout, crate::layout::LAYOUT_VERSION);
    let store_id = parsed.store_id.expect("store_id written to layout.json");
    assert_eq!(store_id.len(), 32);
    assert!(
        store_id
            .chars()
            .all(|ch| matches!(ch, '0'..='9' | 'a'..='f')),
    );
    assert_eq!(
        storage.database.store_binding().await.unwrap(),
        crate::metadata::sql::StoreBinding::Bound(store_id.clone()),
    );
    storage.ensure_hosted_layout().await.unwrap();
    let marker_after = fs::read(tmp.path().join("storage/layout.json")).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<crate::layout::LayoutMarker>(&marker_after).unwrap().store_id,
        Some(store_id),
    );
}

#[tokio::test]
async fn mismatched_or_half_bound_store_and_database_are_refused() {
    let tmp_a = TempDir::new().unwrap();
    let storage_a = storage_in(&tmp_a);
    storage_a.ensure_hosted_layout().await.unwrap();

    let tmp_b = TempDir::new().unwrap();
    let storage_b = storage_in(&tmp_b);
    storage_b.ensure_hosted_layout().await.unwrap();

    // Store A paired with database B: differing store_ids.
    let cross = Storage::new(
        &HostedStoreConfig::Fs,
        tmp_a.path().join("storage"),
        tmp_a.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp_b.path().join("metadata.sqlite")),
    )
    .unwrap();
    let err = cross
        .check_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("bound to store"), "{err}");
    assert!(cross.ensure_hosted_layout().await.is_err());

    // Populated store paired with an empty database.
    let empty_db = TempDir::new().unwrap();
    let store_only = Storage::new(
        &HostedStoreConfig::Fs,
        tmp_a.path().join("storage"),
        tmp_a.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(empty_db.path().join("fresh.sqlite")),
    )
    .unwrap();
    let err = store_only
        .check_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no store_id in hosted_store"), "{err}");
    assert!(store_only.ensure_hosted_layout().await.is_err());

    // Bound database paired with an unmarked empty store.
    let empty_store = TempDir::new().unwrap();
    let db_only = Storage::new(
        &HostedStoreConfig::Fs,
        empty_store.path().join("storage"),
        empty_store.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp_a.path().join("metadata.sqlite")),
    )
    .unwrap();
    let err = db_only
        .check_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("has no layout.json"), "{err}");
    assert!(db_only.ensure_hosted_layout().await.is_err());

    // Store with `{"layout": 2}` and no `store_id`.
    let no_id = TempDir::new().unwrap();
    fs::create_dir_all(no_id.path().join("storage")).await.unwrap();
    fs::write(
        no_id.path().join("storage/layout.json"),
        format!(r#"{{"layout":{}}}"#, crate::layout::LAYOUT_VERSION),
    )
    .await
    .unwrap();
    let legacy_marker = storage_in(&no_id);
    let err = legacy_marker
        .check_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("has no store_id"), "{err}");
    assert!(legacy_marker.ensure_hosted_layout().await.is_err());
}

#[tokio::test]
async fn a_store_in_the_earlier_layout_is_refused() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    fs::create_dir_all(tmp.path().join("storage/left-pad")).await.unwrap();
    fs::write(tmp.path().join("storage/left-pad/package.json"), b"{}").await.unwrap();
    let error = storage
        .ensure_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("earlier layout"), "{error}");
    assert!(
        !tmp.path()
            .join("storage/layout.json")
            .exists(),
        "no marker is written",
    );
}

#[tokio::test]
async fn a_store_from_a_newer_pnpr_is_refused() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    fs::create_dir_all(tmp.path().join("storage")).await.unwrap();
    let newer = crate::layout::LAYOUT_VERSION + 1;
    fs::write(tmp.path().join("storage/layout.json"), format!(r#"{{"layout":{newer}}}"#))
        .await
        .unwrap();
    let error = storage
        .ensure_hosted_layout()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("newer pnpr"), "{error}");
}

#[tokio::test]
async fn a_cache_from_another_layout_is_emptied_once() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let cache = tmp.path().join("cache");
    let stale = cache.join("~public/abc/left-pad/package.json");
    fs::create_dir_all(stale.parent().unwrap()).await.unwrap();
    fs::write(&stale, b"{}").await.unwrap();
    fs::create_dir_all(cache.join("resolver-store")).await.unwrap();
    storage.reset_cache_from_other_layout().await.unwrap();
    assert!(!cache.join("~public").exists());
    assert!(cache.join("resolver-store").exists());

    fs::create_dir_all(stale.parent().unwrap()).await.unwrap();
    storage.reset_cache_from_other_layout().await.unwrap();
    assert!(cache.join("~public").exists());
}

#[tokio::test]
async fn a_staged_record_is_replaced_only_while_it_is_unchanged() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let stage_id = "11111111-2222-3333-4444-555555555555";
    storage.create_staged_meta(stage_id, br#"{"id":"stage"}"#).await.unwrap();

    let original = storage
        .read_staged_meta(stage_id)
        .await
        .unwrap()
        .unwrap();
    let first = storage
        .replace_staged_meta_if_current(stage_id, &original, br#"{"id":"stage","a":1}"#)
        .await
        .unwrap();
    assert!(first.is_some());

    let second = storage
        .replace_staged_meta_if_current(stage_id, &original, br#"{"id":"stage","b":2}"#)
        .await
        .unwrap();
    assert!(second.is_none());
    assert_eq!(
        storage
            .read_staged_meta(stage_id)
            .await
            .unwrap()
            .as_ref()
            .map(|record| record.bytes.as_slice()),
        Some(&br#"{"id":"stage","a":1}"#[..]),
    );

    assert!(storage.remove_staged(stage_id).await.unwrap());
    let removed = storage
        .replace_staged_meta_if_current(
            stage_id,
            first.as_ref().unwrap(),
            br#"{"id":"stage","c":3}"#,
        )
        .await
        .unwrap();
    assert!(removed.is_none());
    assert!(
        storage
            .read_staged_meta(stage_id)
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn a_blob_being_staged_is_not_listed() {
    use futures_util::TryStreamExt as _;
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("left-pad");
    let slot = storage.reserve_hosted_blob(&name, "left-pad-1.0.0.tgz").await.unwrap();
    fs::write(&slot.tmp_path, b"tarball").await.unwrap();
    let files: Vec<_> = storage
        .hosted_archives()
        .try_collect()
        .await
        .unwrap();
    assert!(files.is_empty(), "{files:?}");
    let identity = storage.identify_blob_slot(&slot).await.unwrap();
    storage.promote_blob_slot(slot, identity, None).await.unwrap();
    let files: Vec<_> = storage
        .hosted_archives()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
}

#[tokio::test]
async fn grouped_registries_with_same_local_name_keep_records_isolated() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let npm_record = b"npm-team-roster";
    let pypi_record = b"pypi-team-roster";

    assert!(
        storage
            .create_registry_record(RegistryRecord::TeamRoster, "npm/shared", npm_record)
            .await
            .unwrap(),
    );
    assert!(
        storage
            .create_registry_record(RegistryRecord::TeamRoster, "pypi/shared", pypi_record)
            .await
            .unwrap(),
    );

    assert_eq!(
        storage
            .read_registry_record(RegistryRecord::TeamRoster, "npm/shared")
            .await
            .unwrap()
            .unwrap()
            .bytes,
        npm_record.to_vec(),
    );
    assert_eq!(
        storage
            .read_registry_record(RegistryRecord::TeamRoster, "pypi/shared")
            .await
            .unwrap()
            .unwrap()
            .bytes,
        pypi_record.to_vec(),
    );
    assert!(
        storage.remove_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        None,
    );
}

#[tokio::test]
async fn replace_registry_record_returns_conflict_if_record_is_absent() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            &super::MetadataRecord { bytes: b"expected-val".to_vec(), generation: 1 },
            b"new-val",
        )
        .await
        .unwrap();
    assert!(write_result.is_none());

    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        None,
    );
}

#[test]
fn iso_from_unix_millis_formats_negative_millis_correctly() {
    assert_eq!(super::publish::iso_from_unix_millis(-1), "1969-12-31T23:59:59.999Z");
    assert_eq!(super::publish::iso_from_unix_millis(0), "1970-01-01T00:00:00.000Z");
}

#[tokio::test]
async fn multi_component_and_max_length_oci_repositories_round_trip_on_local_storage() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp).for_hosted("images", pnpr_package_name::Ecosystem::Oci);

    let max_len_repo = ["a"; 128].join("/");
    assert_eq!(max_len_repo.len(), 255);
    let repos = ["acme", "acme/blobs", "acme/app", "acme/app/tool", max_len_repo.as_str()];

    for repo in repos {
        let name = CanonicalPackageName::parse(repo, pnpr_package_name::Ecosystem::Oci).unwrap();
        let doc = format!(r#"{{"repository":"{repo}"}}"#);
        assert_eq!(
            storage
                .write_hosted_document_if_current(&name, doc.as_bytes(), None, None)
                .await
                .unwrap(),
            DocumentWrite::Written,
        );
        store_file(&storage, &name, "sha256-deadbeef", repo.as_bytes()).await;
    }

    for repo in repos {
        let name = CanonicalPackageName::parse(repo, pnpr_package_name::Ecosystem::Oci).unwrap();
        let expected_doc = format!(r#"{{"repository":"{repo}"}}"#);
        assert_eq!(
            storage
                .read_hosted_document(&name)
                .await
                .unwrap()
                .as_deref(),
            Some(expected_doc.as_bytes()),
        );
    }

    let mut listed = storage.hosted_package_names().await.unwrap();
    listed.sort();
    let mut expected: Vec<String> = repos
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    expected.sort();
    assert_eq!(listed, expected);
}

/// An archive staging file a process left behind is swept once it is old
/// enough; anything else in the archives directory stays.
#[tokio::test]
async fn abandoned_archive_staging_files_are_swept() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = storage.hosted.reserve_archive_tmp().await.unwrap();
    tokio::fs::write(&staged, b"left behind").await.unwrap();
    let other = staged.with_file_name("not-a-staging-file");
    tokio::fs::write(&other, b"keep").await.unwrap();

    let young = std::time::Duration::from_hours(1);
    assert_eq!(storage.sweep_archive_temps(young).await.unwrap(), 0);
    assert!(staged.exists());
    assert_eq!(storage.sweep_archive_temps(std::time::Duration::ZERO).await.unwrap(), 1);
    assert!(!staged.exists());
    assert!(other.exists());
}
