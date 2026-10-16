use super::{
    Cursor, MAX_UNTRUSTED_PREALLOC_BYTES, TarballError, assert_eq, build_zip,
    extract_gzipped_tarball, extract_tarball_entries, extract_zip_entries, gzip_bomb_tarball,
    gzip_bytes, is_transient_error, stream_extract_gzipped_tarball, tar_with_entries,
    tempdir_with_leaked_path,
};

const LIMIT: u64 = 1000;

/// A tar of two regular files whose declared sizes add up to `first + second`.
fn two_files(first: usize, second: usize) -> Vec<u8> {
    let first = vec![b'a'; first];
    let second = vec![b'b'; second];
    tar_with_entries(&[("package/a.bin", first.as_slice()), ("package/b.bin", second.as_slice())])
}

fn assert_limit_exceeded(error: &TarballError) {
    match error {
        TarballError::UnpackedSizeExceeded { limit } => assert_eq!(*limit, LIMIT),
        other => panic!("expected UnpackedSizeExceeded, got {other:?}"),
    }
}

#[test]
fn eager_extract_rejects_an_archive_past_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let error = extract_tarball_entries(&two_files(600, 600), store_path, None, LIMIT)
        .expect_err("1200 declared bytes exceed a 1000-byte limit");
    assert_limit_exceeded(&error);
}

#[test]
fn eager_extract_accepts_an_archive_at_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let (cas_paths, _) = extract_tarball_entries(&two_files(500, 500), store_path, None, LIMIT)
        .expect("1000 declared bytes fit a 1000-byte limit");
    assert_eq!(cas_paths.len(), 2);
}

#[test]
fn streaming_extract_rejects_an_archive_past_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let error =
        stream_extract_gzipped_tarball(&gzip_bytes(&two_files(600, 600)), store_path, None, LIMIT)
            .expect_err("1200 declared bytes exceed a 1000-byte limit");
    assert_limit_exceeded(&error);
}

/// The limit measures the archive, not the files kept from it: a filter
/// that drops an entry does not make room for the next one.
#[test]
fn ignored_entries_count_toward_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let drop_first = |path: &str| path == "a.bin";
    let error =
        extract_tarball_entries(&two_files(600, 600), store_path, Some(&drop_first), LIMIT)
            .expect_err("the ignored 600 bytes still count");
    assert_limit_exceeded(&error);
}

/// A gzip bomb routes from the eager path to the streaming path once it
/// inflates past the eager ceiling; the unpacked-size limit stops it there.
#[test]
fn gzip_bomb_is_rejected_by_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let bomb = gzip_bomb_tarball(&[("package/bomb.bin", MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)], &[]);
    let limit = 1024 * 1024;
    let error = extract_gzipped_tarball(&bomb, None, store_path, None, limit)
        .expect_err("a 64 MiB file exceeds a 1 MiB limit");
    match error {
        TarballError::UnpackedSizeExceeded { limit: reported } => assert_eq!(reported, limit),
        other => panic!("expected UnpackedSizeExceeded, got {other:?}"),
    }
}

#[test]
fn zip_extract_rejects_an_archive_past_the_unpacked_size_limit() {
    let (_tempdir, store_path) = tempdir_with_leaked_path();
    let first = vec![b'a'; 600];
    let second = vec![b'b'; 600];
    let bytes = build_zip(&[("pkg/a.bin", first.as_slice()), ("pkg/b.bin", second.as_slice())]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
    let error = extract_zip_entries(
        &mut archive,
        "https://example.test/pkg.zip",
        store_path,
        None,
        None,
        LIMIT,
    )
    .expect_err("1200 declared bytes exceed a 1000-byte limit");
    assert_limit_exceeded(&error);
}

#[test]
fn an_archive_past_the_unpacked_size_limit_is_not_retried() {
    let error = TarballError::UnpackedSizeExceeded { limit: LIMIT };
    assert!(!is_transient_error(&error), "a re-fetch yields the same archive");
}
