use super::{
    AsyncWriteExt, BlobWrite, CanonicalPackageName, DocumentWrite, ErrorKind,
    HostedRevisionRefIndex, HostedRevisionRefWrite, HostedStoreConfig, MAX_HOSTED_REVISION_REFS,
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
async fn hosted_revision_refs_roundtrip_in_the_org_namespace() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp).for_hosted("acme");
    let digest = "A".repeat(86);
    let ref_id = "b".repeat(64);

    assert_eq!(storage.read_hosted_revision_refs(&digest).await.unwrap(), Vec::<Vec<u8>>::new());
    storage
        .write_hosted_revision_ref(
            &digest,
            &ref_id,
            "owner-a",
            br#"{"package":"foo","version":"1.0.0"}"#,
        )
        .await
        .unwrap();
    assert_eq!(
        storage.read_hosted_revision_refs(&digest).await.unwrap(),
        vec![br#"{"package":"foo","version":"1.0.0"}"#.to_vec()],
    );
    assert_eq!(
        storage_in(&tmp).read_hosted_revision_refs(&digest).await.unwrap(),
        Vec::<Vec<u8>>::new(),
    );
}

#[tokio::test]
async fn hosted_revision_ref_paths_reject_noncanonical_segments() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);

    let invalid_digest = storage.read_hosted_revision_refs("../escape").await;
    assert!(invalid_digest.is_err());
    let invalid_ref =
        storage.write_hosted_revision_ref(&digest, "../escape", "owner-a", b"{}").await;
    assert!(invalid_ref.is_err());
}

#[tokio::test]
async fn hosted_revision_ref_writes_enforce_the_read_bound() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);
    for index in 0..MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}")
            .await
            .unwrap();
    }

    let overflow = MAX_HOSTED_REVISION_REFS;
    let err = storage
        .write_hosted_revision_ref(&digest, &format!("{overflow:064x}"), "owner-a", b"{}")
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(
        err,
        RegistryError::RevisionReferenceLimit { limit } if limit == MAX_HOSTED_REVISION_REFS
    ));

    assert_eq!(
        storage
            .write_hosted_revision_ref(&digest, &"0".repeat(64), "owner-a", b"{}")
            .await
            .unwrap(),
        HostedRevisionRefWrite::AlreadyClaimed,
    );
    let mut stray_dir = tmp.path().join("storage");
    stray_dir.extend(crate::layout::namespace_segments(""));
    let stray_dir = stray_dir.join(super::HOSTED_REVISION_REFS_DIR).join(&digest);
    fs::write(stray_dir.join("not-a-reference.json"), b"stray").await.unwrap();
    fs::write(stray_dir.join("interrupted.tmp"), b"stray").await.unwrap();
    let refs = storage.read_hosted_revision_refs(&digest).await.unwrap();
    assert_eq!(refs.len(), MAX_HOSTED_REVISION_REFS);
    assert!(refs.iter().all(|bytes| bytes == b"{}"));
}

#[tokio::test]
async fn concurrent_hosted_revision_ref_writes_cannot_exceed_the_limit() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);
    let mut writes = Vec::new();
    for index in 0..MAX_HOSTED_REVISION_REFS * 2 {
        let storage = storage.clone();
        let digest = digest.clone();
        writes.push(tokio::spawn(async move {
            storage.write_hosted_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}")
                .await
        }));
    }

    let mut written = 0;
    let mut rejected = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(HostedRevisionRefWrite::Claimed) => written += 1,
            Ok(outcome) => panic!("unexpected revision-reference write outcome: {outcome:?}"),
            Err(RegistryError::RevisionReferenceLimit { .. }) => rejected += 1,
            Err(err) => panic!("unexpected revision-reference write error: {err}"),
        }
    }
    assert_eq!(written, MAX_HOSTED_REVISION_REFS);
    assert_eq!(rejected, MAX_HOSTED_REVISION_REFS);
    assert_eq!(
        storage
            .read_hosted_revision_refs(&digest)
            .await
            .unwrap()
            .len(),
        MAX_HOSTED_REVISION_REFS,
    );
}

#[tokio::test]
async fn hosted_blob_under_non_directory_package_path_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let package_dir = crate::layout::local_package_dir(&tmp.path().join("storage"), "", "foo");
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
    let storage = storage_in(&tmp);
    let parent =
        CanonicalPackageName::parse("acme/app", pnpr_package_name::Ecosystem::Oci).unwrap();
    let child =
        CanonicalPackageName::parse("acme/app/tool", pnpr_package_name::Ecosystem::Oci).unwrap();
    for name in [&parent, &child] {
        storage.write_hosted_document_if_current(name, b"{}", None).await.unwrap();
    }
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app", "acme/app/tool"]);
    storage.remove_package(&parent).await.unwrap();
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app/tool"]);
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
        .for_hosted("npm~internal")
        .write_hosted_document_if_current(&name, b"{}", None)
        .await
        .unwrap();

    let package = tmp.path().join("storage/default/packages/!40scope!2f!41pp");
    assert!(package.join("document.json").is_file());
    assert!(package.join("blobs/!41pp-1.0.0.tgz").is_file());
    assert!(
        tmp.path()
            .join("storage/orgs/npm!7einternal/packages/!40scope!2f!41pp/document.json")
            .is_file(),
    );
}

#[tokio::test]
async fn an_org_named_like_a_package_is_its_own_namespace() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let org = storage.for_hosted("acme");
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
    fs::create_dir_all(cache.join("pnpr-store")).await.unwrap();
    storage.reset_cache_from_other_layout().await.unwrap();
    assert!(!cache.join("~public").exists());
    assert!(cache.join("pnpr-store").exists());

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

#[tokio::test]
async fn an_upstream_revision_blob_named_by_a_tgz_digest_is_not_served() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);
    let tgz_keyed = tmp.path().join("cache/upstream/.revisions/sha512");
    fs::create_dir_all(&tgz_keyed).await.unwrap();
    fs::write(tgz_keyed.join(&digest), b"tgz").await.unwrap();
    assert!(
        storage
            .open_upstream_revision_blob("upstream", &digest)
            .await
            .unwrap()
            .is_none(),
    );

    let mut write = storage.open_upstream_revision_blob_tmp("upstream", &digest).await.unwrap();
    write.write_all(b"tar").await.unwrap();
    write.finalize().await.unwrap();
    let (_file, len) = storage
        .open_upstream_revision_blob("upstream", &digest)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(len, 3);
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
    let packages = tmp.path().join("storage/default/packages");
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

#[test]
fn revision_index_preserves_byte_for_byte_fidelity_across_formatting() {
    let json_dense = b"{\"key\":\"value\",\"count\":1}";
    let json_spaced = b"{\n  \"key\": \"value\",\n  \"count\": 1\n}";

    let mut index = HostedRevisionRefIndex::default();
    index
        .insert(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "owner1",
            json_dense,
        )
        .unwrap();
    index
        .insert(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "owner2",
            json_spaced,
        )
        .unwrap();

    let bytes = index.to_bytes();
    let loaded = HostedRevisionRefIndex::from_bytes(&bytes).unwrap();

    let bodies: Vec<&[u8]> = loaded.bodies().collect();
    assert_eq!(bodies[0], json_dense);
    assert_eq!(bodies[1], json_spaced);
    assert_ne!(bodies[0], bodies[1]);
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
async fn replace_registry_record_migrates_legacy_record_atomically() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    assert!(
        storage
            .create_registry_record(RegistryRecord::TeamRoster, "shared", b"legacy-val")
            .await
            .unwrap(),
    );

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            b"legacy-val",
            b"migrated-val",
        )
        .await
        .unwrap();
    assert_eq!(write_result, DocumentWrite::Written);

    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(b"migrated-val".to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "shared").await.unwrap(),
        Some(b"legacy-val".to_vec()),
    );
}

#[tokio::test]
async fn replace_registry_record_returns_conflict_if_namespaced_creation_conflicts() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    assert!(
        storage
            .create_registry_record(RegistryRecord::TeamRoster, "npm/shared", b"namespaced-val")
            .await
            .unwrap(),
    );
    assert!(
        storage.root.create_record(".team-rosters/v0", "shared.json", b"legacy-val").await.unwrap(),
    );

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            b"legacy-val",
            b"new-val",
        )
        .await
        .unwrap();
    assert_eq!(write_result, DocumentWrite::Conflict);

    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(b"namespaced-val".to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "shared").await.unwrap(),
        Some(b"legacy-val".to_vec()),
    );
}

#[tokio::test]
async fn replace_registry_record_returns_conflict_if_legacy_was_concurrently_modified() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    assert!(
        storage.root
            .create_record(".team-rosters/v0", "shared.json", b"concurrent-legacy")
            .await
            .unwrap(),
    );

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            b"stale-expected",
            b"new-val",
        )
        .await
        .unwrap();
    assert_eq!(write_result, DocumentWrite::Conflict);

    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "shared").await.unwrap(),
        Some(b"concurrent-legacy".to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(b"concurrent-legacy".to_vec()),
    );
}

#[tokio::test]
async fn replace_registry_record_returns_conflict_if_both_records_are_absent() {
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
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "shared").await.unwrap(),
        None,
    );
}

#[tokio::test]
async fn editing_one_grouped_registry_preserves_other_effective_rules() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    assert!(
        storage
            .create_registry_record(
                RegistryRecord::RuleOverrides,
                "shared",
                b"initial-shared-rules"
            )
            .await
            .unwrap(),
    );

    assert_eq!(
        storage.read_registry_record(RegistryRecord::RuleOverrides, "npm/shared").await.unwrap(),
        Some(b"initial-shared-rules".to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::RuleOverrides, "pypi/shared").await.unwrap(),
        Some(b"initial-shared-rules".to_vec()),
    );

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::RuleOverrides,
            "npm/shared",
            b"initial-shared-rules",
            b"updated-npm-rules",
        )
        .await
        .unwrap();
    assert_eq!(write_result, DocumentWrite::Written);

    assert_eq!(
        storage.read_registry_record(RegistryRecord::RuleOverrides, "npm/shared").await.unwrap(),
        Some(b"updated-npm-rules".to_vec()),
    );
    assert_eq!(
        storage.read_registry_record(RegistryRecord::RuleOverrides, "pypi/shared").await.unwrap(),
        Some(b"initial-shared-rules".to_vec()),
    );
}

struct HookedBackend {
    inner: std::sync::Arc<dyn super::backend::HostedBackend>,
    on_create_record: std::sync::Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for HookedBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookedBackend")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl super::backend::HostedBackend for HookedBackend {
    async fn check_readiness(&self) -> pnpr_error::Result<()> {
        self.inner.check_readiness().await
    }
    fn upload_store(&self) -> Option<crate::upload::RemoteUploadStore> {
        self.inner.upload_store()
    }
    async fn read_document(
        &self,
        name: &CanonicalPackageName,
    ) -> pnpr_error::Result<Option<Vec<u8>>> {
        self.inner.read_document(name).await
    }
    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> pnpr_error::Result<Option<super::backend::HostedDocumentForUpdate>> {
        self.inner.read_document_for_update(name).await
    }
    async fn write_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&super::backend::HostedDocumentVersion>,
    ) -> pnpr_error::Result<DocumentWrite> {
        self.inner.write_document_if_current(name, bytes, version).await
    }
    async fn open_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> pnpr_error::Result<Option<(axum::body::Body, Option<u64>)>> {
        self.inner.open_blob(name, filename).await
    }
    async fn open_blob_range(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
        range: &object_store::GetRange,
    ) -> pnpr_error::Result<Option<super::RangedBlob>> {
        self.inner.open_blob_range(name, filename, range).await
    }
    async fn reserve_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> pnpr_error::Result<std::path::PathBuf> {
        self.inner.reserve_blob_tmp(name, filename).await
    }
    async fn finalize_blob(
        &self,
        tmp_path: &std::path::Path,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> pnpr_error::Result<super::BlobFinalize> {
        self.inner.finalize_blob(tmp_path, name, filename).await
    }
    async fn remove_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> pnpr_error::Result<bool> {
        self.inner.remove_blob(name, filename).await
    }
    async fn remove_package(&self, name: &CanonicalPackageName) -> pnpr_error::Result<bool> {
        self.inner.remove_package(name).await
    }
    fn list_blob_files(
        &self,
    ) -> futures_util::stream::BoxStream<'_, pnpr_error::Result<super::HostedBlobFile>> {
        self.inner.list_blob_files()
    }
    async fn list_package_names(&self) -> pnpr_error::Result<Vec<String>> {
        self.inner.list_package_names().await
    }
    async fn list_package_blobs(
        &self,
        name: &CanonicalPackageName,
    ) -> pnpr_error::Result<Vec<String>> {
        self.inner.list_package_blobs(name).await
    }
    async fn list_top_level_dirs(&self) -> pnpr_error::Result<Vec<String>> {
        self.inner.list_top_level_dirs().await
    }
    async fn read_revision_refs(&self, digest: &str) -> pnpr_error::Result<Vec<Vec<u8>>> {
        self.inner.read_revision_refs(digest).await
    }
    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> pnpr_error::Result<HostedRevisionRefWrite> {
        self.inner.write_revision_ref(digest, ref_id, owner, bytes).await
    }
    async fn remove_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> pnpr_error::Result<()> {
        self.inner.remove_revision_ref(digest, ref_id, owner).await
    }
    async fn commit_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> pnpr_error::Result<()> {
        self.inner.commit_revision_ref(digest, ref_id, owner).await
    }
    fn namespaced(&self, segment: &str) -> std::sync::Arc<dyn super::backend::HostedBackend> {
        self.inner.namespaced(segment)
    }
    fn namespace(&self) -> String {
        self.inner.namespace()
    }
    fn local_scratch_root(&self) -> &std::path::Path {
        self.inner.local_scratch_root()
    }
    async fn read_record(&self, namespace: &str, key: &str) -> pnpr_error::Result<Option<Vec<u8>>> {
        self.inner.read_record(namespace, key).await
    }
    async fn create_record(
        &self,
        namespace: &str,
        key: &str,
        bytes: &[u8],
    ) -> pnpr_error::Result<bool> {
        let res = self.inner.create_record(namespace, key, bytes).await;
        (self.on_create_record)();
        res
    }
    async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> pnpr_error::Result<DocumentWrite> {
        self.inner.replace_record_if_current(namespace, key, expected, bytes).await
    }
    async fn remove_record(&self, namespace: &str, key: &str) -> pnpr_error::Result<bool> {
        self.inner.remove_record(namespace, key).await
    }
    async fn remove_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
    ) -> pnpr_error::Result<bool> {
        self.inner.remove_record_if_current(namespace, key, expected).await
    }
    async fn list_record_keys(&self, namespace: &str) -> pnpr_error::Result<Vec<String>> {
        self.inner.list_record_keys(namespace).await
    }
}

#[tokio::test]
async fn replace_registry_record_returns_conflict_if_legacy_updated_between_verification_and_creation()
 {
    let tmp = TempDir::new().unwrap();
    let mut storage = storage_in(&tmp);

    storage
        .create_registry_record(RegistryRecord::TeamRoster, "shared", b"initial-legacy")
        .await
        .unwrap();

    let legacy_file = tmp.path().join("storage/.team-rosters/v0/shared.json");
    let hook_file = legacy_file.clone();
    let hooked = std::sync::Arc::new(HookedBackend {
        inner: std::sync::Arc::clone(&storage.root),
        on_create_record: std::sync::Arc::new(move || {
            std::fs::write(&hook_file, b"concurrent-modified-legacy").unwrap();
        }),
    });
    storage.root = hooked;

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            b"initial-legacy",
            b"migrated-val",
        )
        .await
        .unwrap();

    assert_eq!(write_result, DocumentWrite::Conflict);
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(b"concurrent-modified-legacy".to_vec()),
    );
    assert!(
        !tmp.path()
            .join("storage/.team-rosters/v0/npm/shared.json")
            .exists(),
    );
}

#[tokio::test]
async fn concurrent_registry_record_migrations_race_safely() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    storage
        .create_registry_record(RegistryRecord::TeamRoster, "shared", b"initial-legacy")
        .await
        .unwrap();

    let storage1 = storage.clone();
    let storage2 = storage.clone();

    let task1 = tokio::spawn(async move {
        storage1
            .replace_registry_record_if_current(
                RegistryRecord::TeamRoster,
                "npm/shared",
                b"initial-legacy",
                b"migrated-1",
            )
            .await
            .unwrap()
    });

    let task2 = tokio::spawn(async move {
        storage2
            .replace_registry_record_if_current(
                RegistryRecord::TeamRoster,
                "npm/shared",
                b"initial-legacy",
                b"migrated-2",
            )
            .await
            .unwrap()
    });

    let (res1, res2) = tokio::join!(task1, task2);
    let r1 = res1.unwrap();
    let r2 = res2.unwrap();

    assert!(
        (r1 == DocumentWrite::Written && r2 == DocumentWrite::Conflict)
            || (r1 == DocumentWrite::Conflict && r2 == DocumentWrite::Written),
        "exactly one write must win: r1={r1:?}, r2={r2:?}",
    );
}

#[tokio::test]
async fn replace_registry_record_rollback_preserves_concurrent_namespaced_update() {
    let tmp = TempDir::new().unwrap();
    let mut storage = storage_in(&tmp);

    storage
        .create_registry_record(RegistryRecord::TeamRoster, "shared", b"initial-legacy")
        .await
        .unwrap();

    let legacy_file = tmp.path().join("storage/.team-rosters/v0/shared.json");
    let namespaced_file = tmp.path().join("storage/.team-rosters/v0/npm/shared.json");
    let hook_legacy = legacy_file.clone();
    let hook_namespaced = namespaced_file.clone();
    let hooked = std::sync::Arc::new(HookedBackend {
        inner: std::sync::Arc::clone(&storage.root),
        on_create_record: std::sync::Arc::new(move || {
            std::fs::write(&hook_legacy, b"concurrent-modified-legacy").unwrap();
            std::fs::write(&hook_namespaced, b"concurrent-namespaced-val").unwrap();
        }),
    });
    storage.root = hooked;

    let write_result = storage
        .replace_registry_record_if_current(
            RegistryRecord::TeamRoster,
            "npm/shared",
            b"initial-legacy",
            b"migrated-val",
        )
        .await
        .unwrap();

    assert_eq!(write_result, DocumentWrite::Conflict);
    assert_eq!(
        storage.read_registry_record(RegistryRecord::TeamRoster, "npm/shared").await.unwrap(),
        Some(b"concurrent-namespaced-val".to_vec()),
    );
}

#[test]
fn iso_from_unix_millis_formats_negative_millis_correctly() {
    assert_eq!(super::publish::iso_from_unix_millis(-1), "1969-12-31T23:59:59.999Z");
    assert_eq!(super::publish::iso_from_unix_millis(0), "1970-01-01T00:00:00.000Z");
}
