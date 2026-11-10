use super::{Body, ObjectStore, S3Store};
use crate::DocumentWrite;
use futures_util::TryStreamExt;
use object_store::{ObjectStoreExt as _, PutPayload, memory::InMemory};
use pnpr_config::S3Settings;
use pnpr_package_name::CanonicalPackageName;
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

fn pkg(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm)
        .expect("valid package name")
}

async fn collect(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

/// Stage an archive through the same path the publish flow uses: write
/// the decoded bytes to the reserved local tmp file, then upload it under
/// `key`. (Cleaning up the staging file belongs to the `HostedBackend` impl,
/// not to `upload_archive`, so it stays behind here.)
async fn upload(store: &S3Store, key: &str, bytes: &[u8]) {
    let tmp = store.staging_tmp_path().await.expect("reserve staging path");
    tokio::fs::write(&tmp, bytes).await.expect("write staging file");
    store.upload_archive(&tmp, key).await.expect("upload");
}

/// A key of the bytes `bytes`, at incarnation 1.
fn key_of(bytes: &[u8]) -> String {
    crate::BlobId::of_bytes(pnpr_package_name::Ecosystem::Npm, bytes).key(1)
}

async fn write_document(store: &S3Store, name: &CanonicalPackageName, bytes: &[u8]) {
    assert!(store.write_document_if_current(name, bytes, None).await.unwrap());
}

#[tokio::test]
async fn document_roundtrips_and_missing_is_none() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert_eq!(store.read_document(&name).await.unwrap(), None);
    write_document(&store, &name, br#"{"name":"is-positive"}"#).await;
    assert_eq!(
        store
            .read_document(&name)
            .await
            .unwrap()
            .as_deref(),
        Some(&br#"{"name":"is-positive"}"#[..]),
    );
}

#[tokio::test]
async fn stale_document_update_is_rejected() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("racer");
    store.write_document_if_current(&name, br#"{"name":"racer"}"#, None).await.unwrap();

    let first_read = store
        .read_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let second_read = store
        .read_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();

    let first_written = store
        .write_document_if_current(
            &name,
            br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#,
            Some(&first_read.version),
        )
        .await
        .unwrap();
    assert!(first_written);

    let second_written = store
        .write_document_if_current(
            &name,
            br#"{"name":"racer","versions":{"2.0.0":{"version":"2.0.0"}}}"#,
            Some(&second_read.version),
        )
        .await
        .unwrap();
    assert!(!second_written);
    assert_eq!(
        store
            .read_document(&name)
            .await
            .unwrap()
            .as_deref(),
        Some(&br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#[..]),
    );
}

#[tokio::test]
async fn archive_uploads_stream_and_report_length() {
    let (store, _staging) = store_with_prefix("");
    let payload = b"a fake tarball payload";
    let key = key_of(payload);
    assert!(
        store
            .open_archive(&key)
            .await
            .unwrap()
            .is_none(),
    );
    upload(&store, &key, payload).await;
    let (body, len) = store
        .open_archive(&key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(len, Some(payload.len() as u64));
    assert_eq!(collect(body).await, payload);
}

/// Two writers of one key write the bytes its identity names, so the
/// second write leaves the same archive in place.
#[tokio::test]
async fn writing_a_key_again_keeps_its_archive() {
    let (store, _staging) = store_with_prefix("");
    let key = key_of(b"tarball A");
    upload(&store, &key, b"tarball A").await;
    upload(&store, &key, b"tarball A").await;
    let (body, _) = store
        .open_archive(&key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(collect(body).await, b"tarball A");
}

#[tokio::test]
async fn archive_keys_are_sharded_below_the_prefix() {
    let (store, _staging) = store_with_prefix("packages");
    let key = key_of(b"scoped tarball");
    upload(&store, &key, b"scoped tarball").await;
    let path = format!("packages/{}", crate::layout::archive_object_path(&key));
    let stored = store.store
        .get(&object_store::path::Path::from(path))
        .await
        .unwrap();
    assert_eq!(stored.bytes().await.unwrap().as_ref(), b"scoped tarball");
}

#[tokio::test]
async fn removing_an_archive_is_idempotent() {
    let (store, _staging) = store_with_prefix("");
    let key = key_of(b"payload");
    upload(&store, &key, b"payload").await;
    assert!(store.remove_archive(&key).await.unwrap());
    // S3 (and the in-memory store) deletes are idempotent and don't
    // report whether the key existed, so a second delete still succeeds.
    store.remove_archive(&key).await.unwrap();
    assert!(
        store
            .open_archive(&key)
            .await
            .unwrap()
            .is_none(),
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

#[derive(Debug, Default)]
struct RecordedMultipart {
    sizes: Vec<usize>,
    fail_part: bool,
    completed: bool,
    aborted: bool,
}

#[async_trait::async_trait]
impl object_store::MultipartUpload for RecordedMultipart {
    fn put_part(&mut self, data: PutPayload) -> object_store::UploadPart {
        self.sizes.push(data.content_length());
        let fail = self.fail_part;
        Box::pin(async move {
            if fail {
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: std::io::Error::other("part failed").into(),
                });
            }
            Ok(())
        })
    }

    async fn complete(&mut self) -> object_store::Result<object_store::PutResult> {
        self.completed = true;
        Ok(object_store::PutResult {
            e_tag: None,
            version: None,
            extensions: axum::http::Extensions::default(),
        })
    }

    async fn abort(&mut self) -> object_store::Result<()> {
        self.aborted = true;
        Ok(())
    }
}

#[tokio::test]
async fn multipart_streams_large_files_in_bounded_parts_and_aborts_failures() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let size = super::MAX_SINGLE_PUT_BYTES + 1;
    temp.as_file().set_len(size).unwrap();
    let mut upload = RecordedMultipart::default();
    super::send_parts(temp.path(), &mut upload).await.unwrap();
    assert!(upload.completed);
    assert!(!upload.aborted);
    assert_eq!(upload.sizes.iter().sum::<usize>() as u64, size);
    assert_eq!(upload.sizes.last(), Some(&1));
    assert!(
        upload.sizes[..upload.sizes.len() - 1]
            .iter()
            .all(|size| *size == super::MULTIPART_PART_BYTES),
    );
    let mut failed = RecordedMultipart { fail_part: true, ..Default::default() };
    assert!(super::send_parts(temp.path(), &mut failed).await.is_err());
    assert!(failed.aborted);
    assert!(!failed.completed);
}

#[tokio::test]
async fn an_archive_listing_holds_its_namespace_only() {
    let (store, _staging) = store_with_prefix("images");
    let keys = [key_of(b"parent"), key_of(b"child")];
    for (key, bytes) in keys
        .iter()
        .zip([&b"parent"[..], b"child"])
    {
        upload(&store, key, bytes).await;
    }
    let other = store.namespaced("org");
    upload(&other, &key_of(b"elsewhere"), b"elsewhere").await;
    let mut listed: Vec<String> = crate::HostedBackend::list_archives(&store)
        .map_ok(|stored| stored.name)
        .try_collect()
        .await
        .unwrap();
    listed.sort();
    let mut expected = keys.to_vec();
    expected.sort();
    // The other namespace's key sits below this one's prefix, but not where
    // this namespace keeps its archives.
    assert_eq!(listed, expected);
}

/// A staged record is claimed by rewriting it. Two replicas that read the same
/// record both compute a claim, and only the one whose read is still current
/// gets to write it.
#[tokio::test]
async fn a_staged_record_is_replaced_only_while_it_is_unchanged() {
    let (store, _staging) = store_with_prefix("");
    store.create_record(".staged", "stage.json", br#"{"id":"stage"}"#).await.unwrap();

    let first = store
        .replace_record_if_current(
            ".staged",
            "stage.json",
            br#"{"id":"stage"}"#,
            br#"{"id":"stage","a":1}"#,
        )
        .await
        .unwrap();
    assert_eq!(first, DocumentWrite::Written);

    let second = store
        .replace_record_if_current(
            ".staged",
            "stage.json",
            br#"{"id":"stage"}"#,
            br#"{"id":"stage","b":2}"#,
        )
        .await
        .unwrap();
    assert_eq!(second, DocumentWrite::Conflict);
    assert_eq!(
        store
            .read_record(".staged", "stage.json")
            .await
            .unwrap()
            .as_deref(),
        Some(&br#"{"id":"stage","a":1}"#[..]),
    );
}

/// A record another replica removed — a rejection, or an approval that
/// finished — is not one to write back.
#[tokio::test]
async fn a_removed_staged_record_is_not_replaced() {
    let (store, _staging) = store_with_prefix("");
    store.create_record(".staged", "stage.json", br#"{"id":"stage"}"#).await.unwrap();
    assert!(store.remove_record(".staged", "stage.json").await.unwrap());

    let replaced = store
        .replace_record_if_current(
            ".staged",
            "stage.json",
            br#"{"id":"stage"}"#,
            br#"{"id":"stage","a":1}"#,
        )
        .await
        .unwrap();
    assert_eq!(replaced, DocumentWrite::Conflict);
    assert!(
        store
            .read_record(".staged", "stage.json")
            .await
            .unwrap()
            .is_none(),
    );
}

/// A record that must not be rewritten is created, not put: the second
/// writer is told the key is taken and the first record stands.
#[tokio::test]
async fn creating_a_record_twice_leaves_the_first_one() {
    let (store, _staging) = store_with_prefix("");
    assert!(store.create_record(".staged", "stage.json", br#"{"id":"stage"}"#).await.unwrap());
    assert!(!store.create_record(".staged", "stage.json", br#"{"id":"other"}"#).await.unwrap());
    assert_eq!(
        store
            .read_record(".staged", "stage.json")
            .await
            .unwrap()
            .as_deref(),
        Some(&br#"{"id":"stage"}"#[..]),
    );
}

/// Keys come back relative to their namespace, nested ones included: a run
/// record is addressed `<workspace>/<run id>`.
#[tokio::test]
async fn record_keys_are_listed_relative_to_their_namespace() {
    let (store, _staging) = store_with_prefix("bucket-prefix/");
    store.create_record(".pipeline-runs", "demo/100.json", b"{}").await.unwrap();
    store.create_record(".pipeline-runs", "other/200.json", b"{}").await.unwrap();
    store.create_record(".staged", "stage.json", b"{}").await.unwrap();

    let mut keys = store.list_record_keys(".pipeline-runs").await.unwrap();
    keys.sort();
    assert_eq!(keys, ["demo/100.json", "other/200.json"]);
    assert_eq!(store.list_record_keys(".staged").await.unwrap(), ["stage.json"]);
}

#[tokio::test]
async fn empty_records_can_be_created_read_replaced_and_listed() {
    let (store, _staging) = store_with_prefix("");
    assert!(store.create_record(".test-records", "empty.json", b"").await.unwrap());
    assert_eq!(store.read_record(".test-records", "empty.json").await.unwrap(), Some(Vec::new()));
    assert_eq!(store.list_record_keys(".test-records").await.unwrap(), ["empty.json"]);

    let replaced = store
        .replace_record_if_current(".test-records", "empty.json", b"", b"non-empty")
        .await
        .unwrap();
    assert_eq!(replaced, DocumentWrite::Written);
    assert_eq!(
        store
            .read_record(".test-records", "empty.json")
            .await
            .unwrap()
            .as_deref(),
        Some(&b"non-empty"[..]),
    );
}

fn s3_storage_in(tmp: &tempfile::TempDir) -> crate::Storage {
    crate::Storage::new(
        &pnpr_config::HostedStoreConfig::ObjectStore {
            store: Arc::new(InMemory::new()),
            prefix: String::new(),
        },
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
    .unwrap()
}

fn backdate_file(path: &std::path::Path, age: std::time::Duration) {
    let modified =
        std::time::SystemTime::now().checked_sub(age).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[tokio::test]
async fn s3_staging_temps_sweep_once_past_max_age_and_count_bytes() {
    let tmp = tempdir().unwrap();
    let storage = s3_storage_in(&tmp);
    let name = pkg("large-pkg");
    let mut slot = storage.reserve_hosted_blob(&name, "large-pkg-1.0.0.tgz").await.unwrap();
    let staged_path = slot.tmp_path.clone();

    let payload = vec![b'x'; 4096];
    tokio::fs::write(&staged_path, &payload).await.unwrap();
    let unrelated = staged_path.with_file_name("keep-me.dat");
    tokio::fs::write(&unrelated, b"keep").await.unwrap();

    let timeout = std::time::Duration::from_mins(1);
    let base = super::cache_staging_tmp_base(&tmp.path().join("cache"));
    backdate_file(&staged_path, std::time::Duration::from_mins(2));
    assert_eq!(
        crate::count_stale_temps(&base, timeout).await.unwrap(),
        crate::TempSweep::default(),
        "an active in-process slot must not be counted as stale even past max_age",
    );
    assert_eq!(storage.sweep_archive_temps(timeout).await.unwrap(), 0);
    assert!(staged_path.exists());

    slot.guard.disarm();
    drop(slot);
    backdate_file(&staged_path, std::time::Duration::from_mins(2));
    assert_eq!(
        storage
            .sweep_archive_temps(std::time::Duration::from_mins(5))
            .await
            .unwrap(),
        0,
    );
    assert!(staged_path.exists());

    let expected = crate::TempSweep { files: 1, bytes: payload.len() as u64 };
    assert_eq!(crate::count_stale_temps(&base, timeout).await.unwrap(), expected);
    assert!(staged_path.exists());

    assert_eq!(crate::sweep_stale_temps(&base, timeout).await.unwrap(), expected);
    assert!(!staged_path.exists());
    assert!(unrelated.exists());
}

#[tokio::test]
async fn cancelling_delayed_blob_slot_write_removes_file_after_creation() {
    let tmp = tempdir().unwrap();
    let storage = s3_storage_in(&tmp);
    let name = pkg("timed-out-pkg");
    let slot = storage.reserve_hosted_blob(&name, "timed-out-pkg-1.0.0.tgz").await.unwrap();
    let staged_path = slot.tmp_path.clone();

    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (proceed_tx, proceed_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();

    let mut write_fut = Box::pin(slot.run_blocking_write(move |path| {
        let _ = entered_tx.send(());
        let _ = proceed_rx.recv();
        let res = std::fs::write(path, b"late-bytes");
        let _ = finished_tx.send(());
        res
    }));

    tokio::select! {
        _ = &mut write_fut => panic!("write should still be waiting"),
        _ = entered_rx => {}
    }
    drop(write_fut);
    assert!(!staged_path.exists());

    proceed_tx.send(()).unwrap();
    finished_rx.await.unwrap();
    for _ in 0..50 {
        if !staged_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(!staged_path.exists());
}

#[tokio::test]
async fn dropping_unpromoted_s3_blob_slot_removes_staging_file() {
    let tmp = tempdir().unwrap();
    let storage = s3_storage_in(&tmp);
    let name = pkg("cancelled-pkg");

    let cancelled_slot =
        storage.reserve_hosted_blob(&name, "cancelled-pkg-1.0.0.tgz").await.unwrap();
    let cancelled_path = cancelled_slot.tmp_path.clone();
    tokio::fs::write(&cancelled_path, b"partial-publish").await.unwrap();
    assert!(cancelled_path.exists());
    drop(cancelled_slot);
    assert!(!cancelled_path.exists());

    let promoted_slot =
        storage.reserve_hosted_blob(&name, "cancelled-pkg-1.0.1.tgz").await.unwrap();
    let promoted_path = promoted_slot.tmp_path.clone();
    tokio::fs::write(&promoted_path, b"complete-publish").await.unwrap();
    let identified = storage.identify_blob_slot(&promoted_slot).await.unwrap();
    let promoted = storage.promote_blob_slot(promoted_slot, identified, None).await.unwrap();
    assert!(promoted.stored);
    assert!(!promoted_path.exists());
    let served = storage.hosted
        .open_archive(&promoted.artifact.blob.key())
        .await
        .unwrap();
    assert!(served.is_some());
}
