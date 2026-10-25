use super::{
    AsyncWriteExt, BlobWrite, CanonicalPackageName, DocumentWrite, ErrorKind, HostedStoreConfig,
    PackageTombstone, RegistryError, RegistryRecord, Storage, atomic_write::create_tmp_file_with,
    fs,
};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
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
async fn hosted_blob_under_non_directory_package_path_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let package_dir = crate::layout::local_package_dir(
        &tmp.path().join("storage"),
        "",
        pnpr_package_name::Ecosystem::Npm,
        "foo",
    );
    fs::create_dir_all(package_dir.parent().unwrap()).await.unwrap();
    fs::write(&package_dir, b"not a directory").await.unwrap();

    let Err(err) = storage.open_hosted_blob(&name, "foo-1.0.0.tgz").await else {
        panic!("expected hosted blob open to fail");
    };
    match err {
        RegistryError::Io(err) => assert_eq!(err.kind(), ErrorKind::NotADirectory),
        other => panic!("expected I/O error, got {other:?}"),
    }
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
        storage.write_hosted_document_if_current(name, b"{}", None).await.unwrap();
    }
    let slot = storage.reserve_hosted_blob(&top, "sha256-top").await.unwrap();
    fs::write(&slot.tmp_path, b"top-layer").await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();
    let slot = storage.reserve_hosted_blob(&blobs_repo, "sha256-repo").await.unwrap();
    fs::write(&slot.tmp_path, b"repo-layer").await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();
    assert_eq!(
        storage.hosted_package_names().await.unwrap(),
        ["acme", "acme/app", "acme/app/tool", "acme/blobs"],
    );
    assert_eq!(storage.hosted_package_blobs(&top).await.unwrap(), ["sha256-top"]);
    assert_eq!(storage.hosted_package_blobs(&blobs_repo).await.unwrap(), ["sha256-repo"]);
    storage.remove_package(&parent).await.unwrap();
    assert_eq!(
        storage.hosted_package_names().await.unwrap(),
        ["acme", "acme/app/tool", "acme/blobs"],
    );
}

#[tokio::test]
async fn packages_documents_and_blobs_have_one_path_each() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("@scope/App");
    storage.write_hosted_document_if_current(&name, b"{}", None).await.unwrap();
    let slot = storage.reserve_hosted_blob(&name, "App-1.0.0.tgz").await.unwrap();
    fs::write(&slot.tmp_path, b"tarball").await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();
    storage
        .for_hosted("npm~internal", pnpr_package_name::Ecosystem::Npm)
        .write_hosted_document_if_current(&name, b"{}", None)
        .await
        .unwrap();

    let package = tmp.path().join("storage/default/npm/packages/@scope/!41pp");
    assert!(package.join("document.json").is_file());
    assert!(package.join("blobs/1.0.0.tgz").is_file());
    assert!(
        tmp.path()
            .join("storage/orgs/npm!7einternal/npm/packages/@scope/!41pp/document.json")
            .is_file(),
    );
}

#[tokio::test]
async fn an_org_named_like_a_package_is_its_own_namespace() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let org = storage.for_hosted("acme", pnpr_package_name::Ecosystem::Npm);
    storage
        .write_hosted_document_if_current(&pkg("acme"), b"{}", None)
        .await
        .unwrap();
    org.write_hosted_document_if_current(&pkg("tool"), b"{}", None).await
        .unwrap();
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme"]);
    storage
        .remove_package(&pkg("acme"))
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
    assert_eq!(
        serde_json::from_slice::<crate::layout::LayoutMarker>(&marker).unwrap(),
        crate::layout::LayoutMarker { layout: crate::layout::LAYOUT_VERSION },
    );
    storage.ensure_hosted_layout().await.unwrap();
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
    assert!(error.contains("pnpr upgrade-storage"), "{error}");
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

/// The local backend has no `ETag`, so its claim on a staged record compares
/// the bytes: only the writer whose copy is what the store holds wins.
#[tokio::test]
async fn a_staged_record_is_replaced_only_while_it_is_unchanged() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let stage_id = "11111111-2222-3333-4444-555555555555";
    storage.create_staged_meta(stage_id, br#"{"id":"stage"}"#).await.unwrap();

    let first = storage
        .replace_staged_meta_if_current(stage_id, br#"{"id":"stage"}"#, br#"{"id":"stage","a":1}"#)
        .await
        .unwrap();
    assert_eq!(first, super::DocumentWrite::Written);

    let second = storage
        .replace_staged_meta_if_current(stage_id, br#"{"id":"stage"}"#, br#"{"id":"stage","b":2}"#)
        .await
        .unwrap();
    assert_eq!(second, super::DocumentWrite::Conflict);
    assert_eq!(
        storage
            .read_staged_meta(stage_id)
            .await
            .unwrap()
            .as_deref(),
        Some(&br#"{"id":"stage","a":1}"#[..]),
    );

    assert!(storage.remove_staged(stage_id).await.unwrap());
    let removed = storage
        .replace_staged_meta_if_current(
            stage_id,
            br#"{"id":"stage","a":1}"#,
            br#"{"id":"stage","c":3}"#,
        )
        .await
        .unwrap();
    assert_eq!(removed, super::DocumentWrite::Conflict);
    assert!(
        storage
            .read_staged_meta(stage_id)
            .await
            .unwrap()
            .is_none(),
    );
}

fn add_tombstone_ban(
    existing: Option<PackageTombstone>,
    package: &str,
    maintainer: &str,
    version: &str,
) -> PackageTombstone {
    let mut tombstone = existing.unwrap_or_else(|| PackageTombstone {
        package: package.to_string(),
        unpublished_by: Some(maintainer.to_string()),
        unpublished_at: "2026-01-01T00:00:00Z".to_string(),
        maintainers: vec![maintainer.to_string()],
        unpublished_versions: Vec::new(),
    });
    if !tombstone.maintainers
        .iter()
        .any(|user| user == maintainer)
    {
        tombstone.maintainers.push(maintainer.to_string());
    }
    if !tombstone.unpublished_versions
        .iter()
        .any(|ver| ver == version)
    {
        tombstone.unpublished_versions.push(version.to_string());
    }
    tombstone
}

#[tokio::test]
async fn concurrent_tombstone_writers_retain_all_version_bans() {
    let tmp = TempDir::new().unwrap();
    let storage = std::sync::Arc::new(storage_in(&tmp));
    let name = pkg("concurrent-pkg");

    let mut handles = Vec::new();
    for i in 0..10 {
        let storage = std::sync::Arc::clone(&storage);
        let name = name.clone();
        let version = format!("1.{i}.0");
        let maintainer = format!("user-{i}");
        handles.push(tokio::spawn(async move {
            storage
                .update_tombstone(&name, |existing| {
                    add_tombstone_ban(existing, name.as_str(), &maintainer, &version)
                })
                .await
                .unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let tombstone = storage
        .read_tombstone(&name)
        .await
        .unwrap()
        .expect("tombstone exists");
    assert_eq!(tombstone.package, "concurrent-pkg");
    assert_eq!(tombstone.unpublished_versions.len(), 10);
    for i in 0..10 {
        let version = format!("1.{i}.0");
        assert!(
            tombstone.unpublished_versions.contains(&version),
            "missing unpublished version {version}",
        );
        let maintainer = format!("user-{i}");
        assert!(tombstone.maintainers.contains(&maintainer), "missing maintainer {maintainer}");
    }
}

#[tokio::test]
async fn concurrent_write_tombstone_merges_version_bans() {
    let tmp = TempDir::new().unwrap();
    let storage = std::sync::Arc::new(storage_in(&tmp));
    let name = pkg("concurrent-write-pkg");

    let mut handles = Vec::new();
    for i in 0..5 {
        let storage = std::sync::Arc::clone(&storage);
        let name = name.clone();
        let version = format!("2.{i}.0");
        handles.push(tokio::spawn(async move {
            let tombstone = PackageTombstone {
                package: name.as_str().to_string(),
                unpublished_by: Some("admin".to_string()),
                unpublished_at: "2026-01-01T00:00:00Z".to_string(),
                maintainers: vec!["admin".to_string()],
                unpublished_versions: vec![version],
            };
            storage.write_tombstone(&tombstone).await.unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let tombstone = storage
        .read_tombstone(&name)
        .await
        .unwrap()
        .expect("tombstone exists");
    assert_eq!(tombstone.unpublished_versions.len(), 5);
    for i in 0..5 {
        let version = format!("2.{i}.0");
        assert!(tombstone.unpublished_versions.contains(&version));
    }
}

#[tokio::test]
async fn subsequent_write_tombstone_replaces_maintainers_and_merges_version_bans() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("ownership-reset-pkg");

    let first = PackageTombstone {
        package: name.as_str().to_string(),
        unpublished_by: Some("alice".to_string()),
        unpublished_at: "2026-01-01T00:00:00Z".to_string(),
        maintainers: vec!["alice".to_string()],
        unpublished_versions: vec!["1.0.0".to_string()],
    };
    storage.write_tombstone(&first).await.unwrap();

    let second = PackageTombstone {
        package: name.as_str().to_string(),
        unpublished_by: Some("bob".to_string()),
        unpublished_at: "2026-02-01T00:00:00Z".to_string(),
        maintainers: vec!["bob".to_string()],
        unpublished_versions: vec!["2.0.0".to_string()],
    };
    storage.write_tombstone(&second).await.unwrap();

    let stored = storage
        .read_tombstone(&name)
        .await
        .unwrap()
        .expect("tombstone exists");
    assert_eq!(stored.maintainers, vec!["bob".to_string()]);
    assert_eq!(stored.unpublished_versions, vec!["1.0.0".to_string(), "2.0.0".to_string()]);
    assert_eq!(stored.unpublished_by.as_deref(), Some("bob"));
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_in_the_packages_directory_are_not_followed() {
    use futures_util::TryStreamExt as _;
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let outside = tmp.path().join("outside");
    fs::create_dir_all(outside.join("blobs")).await.unwrap();
    fs::write(outside.join("document.json"), b"{}").await.unwrap();
    fs::write(outside.join("blobs/sha256-aa"), b"keep").await.unwrap();
    storage
        .write_hosted_document_if_current(&pkg("real"), b"{}", None)
        .await
        .unwrap();
    let scopes = tmp.path().join("storage/default/npm/packages");
    fs::create_dir_all(tmp.path().join("outside-scope/pkg")).await.unwrap();
    fs::write(tmp.path().join("outside-scope/pkg/document.json"), b"{}").await.unwrap();
    fs::symlink(tmp.path().join("outside-scope"), scopes.join("@linked")).await.unwrap();
    let packages = scopes.join("@");
    fs::symlink(&outside, packages.join("linked")).await.unwrap();
    fs::create_dir_all(packages.join("real/blobs")).await.unwrap();
    fs::symlink(outside.join("blobs/sha256-aa"), packages.join("real/blobs/sha256-aa"))
        .await
        .unwrap();
    fs::symlink(outside.join("document.json"), packages.join("linked-doc"))
        .await
        .unwrap();
    storage
        .write_hosted_document_if_current(&pkg("other"), b"{}", None)
        .await
        .unwrap();
    fs::symlink(outside.join("blobs"), packages.join("other/blobs")).await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["other", "real"]);
    let files: Vec<_> = storage
        .hosted_blob_files()
        .try_collect()
        .await
        .unwrap();
    assert!(files.iter().all(|file| file.blob.is_none()), "{files:?}");
    assert_eq!(files.len(), 2, "{files:?}");
}

#[tokio::test]
async fn a_blob_being_staged_is_not_listed() {
    use futures_util::TryStreamExt as _;
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("left-pad");
    storage.write_hosted_document_if_current(&name, b"{}", None).await.unwrap();
    let slot = storage.reserve_hosted_blob(&name, "left-pad-1.0.0.tgz").await.unwrap();
    fs::write(&slot.tmp_path, b"tarball").await.unwrap();
    let files: Vec<_> = storage
        .hosted_blob_files()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    storage.finalize_blob_slot(slot).await.unwrap();
    let files: Vec<_> = storage
        .hosted_blob_files()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(files.len(), 2, "{files:?}");
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
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(npm_record.to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "pypi/shared").await.unwrap(),
        Some(pypi_record.to_vec()),
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
            b"expected-val",
            b"new-val",
        )
        .await
        .unwrap();
    assert_eq!(write_result, DocumentWrite::Conflict);

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
            storage.write_hosted_document_if_current(&name, doc.as_bytes(), None).await.unwrap(),
            DocumentWrite::Written,
        );
        let slot = storage.reserve_hosted_blob(&name, "sha256-deadbeef").await.unwrap();
        fs::write(&slot.tmp_path, repo.as_bytes()).await.unwrap();
        storage.finalize_blob_slot(slot).await.unwrap();
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
