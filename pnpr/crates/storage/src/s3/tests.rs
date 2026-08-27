use super::{Body, ObjectStore, S3Store};
use crate::HostedRevisionRefWrite;
use object_store::{ObjectStoreExt, PutPayload, memory::InMemory, path::Path as ObjectPath};
use pnpr_config::S3Settings;
use pnpr_package_name::PackageName;
use std::sync::Arc;
use tempfile::tempdir;

fn store_with_prefix(prefix: &str) -> (S3Store, tempfile::TempDir) {
    let staging = tempdir().expect("tempdir");
    let inner: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    // `S3Store::new` takes the already-normalized prefix, exactly as
    // `config.rs` feeds it from `S3Settings::normalized_prefix`.
    let normalized = S3Settings {
        bucket: "b".to_string(),
        region: None,
        endpoint: None,
        prefix: Some(prefix.to_string()),
        access_key_id: None,
        secret_access_key: None,
        force_path_style: None,
        allow_http: None,
    }
    .normalized_prefix();
    let store = S3Store::new(inner, normalized, staging.path().to_path_buf());
    (store, staging)
}

fn pkg(name: &str) -> PackageName {
    PackageName::parse(name).expect("valid package name")
}

async fn collect(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

/// Stage a tarball through the same path the publish flow uses: write
/// the decoded bytes to the reserved local tmp file, then upload.
/// (Cleaning up the staging file belongs to the `HostedBackend` impl, not
/// to `upload_tarball`, so it stays behind here.)
async fn upload(store: &S3Store, name: &PackageName, filename: &str, bytes: &[u8]) {
    let tmp = store.staging_tmp_path(name, filename).await.expect("reserve staging path");
    tokio::fs::write(&tmp, bytes).await.expect("write staging file");
    store.upload_tarball(&tmp, name, filename).await.expect("upload");
}

async fn write_packument(store: &S3Store, name: &PackageName, bytes: &[u8]) {
    assert!(store.write_packument_if_current(name, bytes, None).await.unwrap());
}

#[tokio::test]
async fn packument_roundtrips_and_missing_is_none() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert_eq!(store.read_packument(&name).await.unwrap(), None);
    write_packument(&store, &name, br#"{"name":"is-positive"}"#).await;
    assert_eq!(
        store.read_packument(&name).await.unwrap().as_deref(),
        Some(&br#"{"name":"is-positive"}"#[..]),
    );
}

#[tokio::test]
async fn stale_packument_update_is_rejected() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("racer");
    store.write_packument_if_current(&name, br#"{"name":"racer"}"#, None).await.unwrap();

    let first_read = store.read_packument_for_update(&name).await.unwrap().unwrap();
    let second_read = store.read_packument_for_update(&name).await.unwrap().unwrap();

    let first_written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#,
            Some(&first_read.version),
        )
        .await
        .unwrap();
    assert!(first_written);

    let second_written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"racer","versions":{"2.0.0":{"version":"2.0.0"}}}"#,
            Some(&second_read.version),
        )
        .await
        .unwrap();
    assert!(!second_written);
    assert_eq!(
        store.read_packument(&name).await.unwrap().as_deref(),
        Some(&br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#[..]),
    );
}

#[tokio::test]
async fn deleted_packument_update_is_rejected() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("removed-racer");
    write_packument(&store, &name, br#"{"name":"removed-racer"}"#).await;

    let read = store.read_packument_for_update(&name).await.unwrap().unwrap();
    store.remove_package(&name).await.unwrap();

    let written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"removed-racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#,
            Some(&read.version),
        )
        .await
        .unwrap();
    assert!(!written);
    assert!(store.read_packument(&name).await.unwrap().is_none());
}

#[tokio::test]
async fn concurrent_tarball_finalize_does_not_overwrite() {
    use crate::TarballFinalize;
    let (store, _staging) = store_with_prefix("");
    let name = pkg("racer");
    let file = "racer-1.0.0.tgz";

    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball A").await.unwrap();
    assert_eq!(store.upload_tarball(&tmp, &name, file).await.unwrap(), TarballFinalize::Written);

    // Re-promoting byte-identical content is a tolerated no-op, so idempotent
    // journal roll-forward and concurrent identical publishes don't conflict.
    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball A").await.unwrap();
    assert_eq!(
        store.upload_tarball(&tmp, &name, file).await.unwrap(),
        TarballFinalize::AlreadyIdentical,
    );

    // Different bytes for the same version's key are rejected without
    // overwriting the first writer's tarball.
    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball B").await.unwrap();
    assert_eq!(store.upload_tarball(&tmp, &name, file).await.unwrap(), TarballFinalize::Conflict);

    let (body, _len) = store.open_tarball(&name, file).await.unwrap().unwrap();
    assert_eq!(collect(body).await, b"tarball A");
}

#[tokio::test]
async fn tarball_uploads_streams_and_reports_length() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert!(store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().is_none());

    let payload = b"a fake tarball payload";
    upload(&store, &name, "is-positive-1.0.0.tgz", payload).await;

    let (body, len) = store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().unwrap();
    assert_eq!(len, Some(payload.len() as u64));
    assert_eq!(collect(body).await, payload);
}

#[tokio::test]
async fn scoped_keys_and_prefix_are_honored() {
    let (store, _staging) = store_with_prefix("packages");
    let name = pkg("@scope/thing");
    write_packument(&store, &name, br#"{"name":"@scope/thing"}"#).await;
    upload(&store, &name, "thing-1.0.0.tgz", b"scoped tarball").await;

    let (body, _len) = store.open_tarball(&name, "thing-1.0.0.tgz").await.unwrap().unwrap();
    assert_eq!(collect(body).await, b"scoped tarball");
    assert!(store.read_packument(&name).await.unwrap().is_some());
}

#[tokio::test]
async fn remove_tarball_then_package() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    write_packument(&store, &name, b"{}").await;
    upload(&store, &name, "is-positive-1.0.0.tgz", b"payload").await;

    assert!(store.remove_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap());
    // S3 (and the in-memory store) deletes are idempotent and don't
    // report whether the key existed, so a second delete still succeeds.
    store.remove_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap();
    assert!(store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().is_none());

    store.remove_package(&name).await.unwrap();
    assert!(store.read_packument(&name).await.unwrap().is_none());
}

#[tokio::test]
async fn lists_hosted_package_names() {
    for prefix in ["", "packages"] {
        let (store, _staging) = store_with_prefix(prefix);
        write_packument(&store, &pkg("is-positive"), b"{}").await;
        write_packument(&store, &pkg("@scope/thing"), b"{}").await;
        // A stray tarball-only key must not be mistaken for a package.
        upload(&store, &pkg("is-positive"), "is-positive-1.0.0.tgz", b"x").await;

        let mut names = store.list_package_names().await.unwrap();
        names.sort();
        assert_eq!(names, vec!["@scope/thing".to_string(), "is-positive".to_string()]);
    }
}

#[tokio::test]
async fn revision_refs_roundtrip_under_the_configured_prefix() {
    for prefix in ["", "packages"] {
        let (store, _staging) = store_with_prefix(prefix);
        let digest = "A".repeat(86);
        assert_eq!(store.read_revision_refs(&digest).await.unwrap(), Vec::<Vec<u8>>::new());

        store.write_revision_ref(&digest, &"a".repeat(64), "owner-a", b"first").await.unwrap();
        store.write_revision_ref(&digest, &"b".repeat(64), "owner-a", b"second").await.unwrap();
        let mut refs = store.read_revision_refs(&digest).await.unwrap();
        refs.sort();
        assert_eq!(refs, vec![b"first".to_vec(), b"second".to_vec()]);
    }
}

#[tokio::test]
async fn revision_ref_removal_is_scoped_to_its_owner() {
    let (store, _staging) = store_with_prefix("packages");
    let digest = "A".repeat(86);
    let ref_id = "a".repeat(64);
    assert_eq!(
        store.write_revision_ref(&digest, &ref_id, "owner-a", b"record").await.unwrap(),
        HostedRevisionRefWrite::Claimed,
    );
    assert_eq!(
        store.write_revision_ref(&digest, &ref_id, "owner-b", b"record").await.unwrap(),
        HostedRevisionRefWrite::Claimed,
    );

    store.remove_revision_ref(&digest, &ref_id, "owner-a").await.unwrap();
    assert_eq!(store.read_revision_refs(&digest).await.unwrap(), vec![b"record".to_vec()]);

    store.remove_revision_ref(&digest, &ref_id, "owner-a").await.unwrap();
    assert_eq!(store.read_revision_refs(&digest).await.unwrap(), vec![b"record".to_vec()]);

    store.commit_revision_ref(&digest, &ref_id, "owner-b").await.unwrap();
    store.remove_revision_ref(&digest, &ref_id, "owner-b").await.unwrap();
    assert_eq!(store.read_revision_refs(&digest).await.unwrap(), vec![b"record".to_vec()]);

    assert_eq!(
        store.write_revision_ref(&digest, &ref_id, "owner-a", b"record").await.unwrap(),
        HostedRevisionRefWrite::Committed,
    );
}

#[tokio::test]
async fn concurrent_revision_ref_claims_survive_other_owner_removal() {
    let (store, _staging) = store_with_prefix("packages");
    let digest = "A".repeat(86);
    let ref_id = "a".repeat(64);
    let first = {
        let store = store.clone();
        let digest = digest.clone();
        let ref_id = ref_id.clone();
        tokio::spawn(async move {
            store.write_revision_ref(&digest, &ref_id, "owner-a", b"record").await
        })
    };
    let second = {
        let store = store.clone();
        let digest = digest.clone();
        let ref_id = ref_id.clone();
        tokio::spawn(async move {
            store.write_revision_ref(&digest, &ref_id, "owner-b", b"record").await
        })
    };

    assert_eq!(first.await.unwrap().unwrap(), HostedRevisionRefWrite::Claimed);
    assert_eq!(second.await.unwrap().unwrap(), HostedRevisionRefWrite::Claimed);
    store.remove_revision_ref(&digest, &ref_id, "owner-a").await.unwrap();
    assert_eq!(store.read_revision_refs(&digest).await.unwrap(), vec![b"record".to_vec()]);
    store.remove_revision_ref(&digest, &ref_id, "owner-b").await.unwrap();
    assert_eq!(store.read_revision_refs(&digest).await.unwrap(), Vec::<Vec<u8>>::new());
}

#[tokio::test]
async fn revision_ref_writes_enforce_the_read_bound() {
    let (store, _staging) = store_with_prefix("packages");
    let digest = "A".repeat(86);
    for index in 0..crate::MAX_HOSTED_REVISION_REFS {
        store
            .write_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}")
            .await
            .unwrap();
    }

    let overflow = crate::MAX_HOSTED_REVISION_REFS;
    let err = store
        .write_revision_ref(&digest, &format!("{overflow:064x}"), "owner-a", b"{}")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        pnpr_error::RegistryError::RevisionReferenceLimit { limit }
            if limit == crate::MAX_HOSTED_REVISION_REFS
    ));

    assert_eq!(
        store.write_revision_ref(&digest, &"0".repeat(64), "owner-a", b"{}").await.unwrap(),
        HostedRevisionRefWrite::AlreadyClaimed,
    );
    store
        .store
        .put(
            &ObjectPath::from(format!("packages/.revisions/sha512/{digest}/not-a-reference.json")),
            PutPayload::from_static(b"stray"),
        )
        .await
        .unwrap();
    let refs = store.read_revision_refs(&digest).await.unwrap();
    assert_eq!(refs.len(), crate::MAX_HOSTED_REVISION_REFS);
    assert!(refs.iter().all(|bytes| bytes == b"{}"));
}

#[tokio::test]
async fn concurrent_revision_ref_writes_cannot_exceed_the_limit() {
    let (store, _staging) = store_with_prefix("packages");
    let digest = "A".repeat(86);
    let mut writes = Vec::new();
    for index in 0..crate::MAX_HOSTED_REVISION_REFS * 2 {
        let store = store.clone();
        let digest = digest.clone();
        writes.push(tokio::spawn(async move {
            store.write_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}").await
        }));
    }

    let mut written = 0;
    let mut rejected = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(HostedRevisionRefWrite::Claimed) => written += 1,
            Ok(outcome) => panic!("unexpected revision-reference write outcome: {outcome:?}"),
            Err(pnpr_error::RegistryError::RevisionReferenceLimit { .. }) => rejected += 1,
            Err(err) => panic!("unexpected revision-reference write error: {err}"),
        }
    }
    assert_eq!(written, crate::MAX_HOSTED_REVISION_REFS);
    assert_eq!(rejected, crate::MAX_HOSTED_REVISION_REFS);
    assert_eq!(
        store.read_revision_refs(&digest).await.unwrap().len(),
        crate::MAX_HOSTED_REVISION_REFS,
    );
}

#[test]
fn prefix_normalizes() {
    let normalized = |prefix: Option<&str>| {
        S3Settings {
            bucket: "b".to_string(),
            region: None,
            endpoint: None,
            prefix: prefix.map(str::to_string),
            access_key_id: None,
            secret_access_key: None,
            force_path_style: None,
            allow_http: None,
        }
        .normalized_prefix()
    };
    assert_eq!(normalized(None), "");
    assert_eq!(normalized(Some("")), "");
    assert_eq!(normalized(Some("  ")), "");
    assert_eq!(normalized(Some("packages")), "packages/");
    assert_eq!(normalized(Some("/packages/")), "packages/");
}
