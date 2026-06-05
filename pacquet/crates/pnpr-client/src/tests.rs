use std::collections::HashSet;

use bytes::Bytes;
use pacquet_store_dir::StoreDir;
use tempfile::TempDir;

use super::{PnprClientError, VerifyError, consume_stream, hex_encode, parse_inline_response};

/// Frame a JSON header into a complete inline install payload with an
/// empty file section (the `{}` prefix plus the end-of-stream marker),
/// matching what the server sends when there are no files to inline.
fn inline_payload(header_json: &str) -> Vec<u8> {
    let header = header_json.as_bytes();
    let mut payload = Vec::new();
    payload.extend_from_slice(&(header.len() as u32).to_be_bytes());
    payload.extend_from_slice(header);
    payload.extend_from_slice(&2u32.to_be_bytes());
    payload.extend_from_slice(b"{}");
    payload.extend_from_slice(&[0u8; 64]);
    payload
}

/// A header carrying verification violations is rebuilt into the same
/// `VerifyError` the local gate raises, so the CLI aborts with an
/// identical diagnostic code + breakdown.
#[test]
fn header_with_violations_rebuilds_a_verify_error() {
    let payload = inline_payload(
        r#"{"violations":[{"name":"@foo/no-deps","version":"1.0.0","code":"MINIMUM_RELEASE_AGE_VIOLATION","reason":"was published yesterday"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_inline_response(&payload) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::MinimumReleaseAgeViolation { .. }),
        "got {verify_err:?}",
    );
    assert!(verify_err.to_string().contains("@foo/no-deps@1.0.0"), "got {verify_err}");
}

/// A lone `TARBALL_URL_MISMATCH` maps to the generic envelope — matching
/// `VerifyError::from_rendered`'s handling of a code with no dedicated
/// variant.
#[test]
fn tarball_mismatch_maps_to_the_generic_envelope() {
    let payload = inline_payload(
        r#"{"violations":[{"name":"acme","version":"1.0.0","code":"TARBALL_URL_MISMATCH","reason":"url mismatch"}]}"#,
    );
    let Err(PnprClientError::Verification(verify_err)) = parse_inline_response(&payload) else {
        panic!("expected a Verification error");
    };
    assert!(
        matches!(verify_err, VerifyError::LockfileResolutionVerification { .. }),
        "got {verify_err:?}",
    );
}

/// A header with no lockfile and no violations is a malformed response,
/// not a silent success.
#[test]
fn header_without_a_lockfile_is_a_protocol_error() {
    let payload = inline_payload("{}");
    let Err(PnprClientError::Protocol(_)) = parse_inline_response(&payload) else {
        panic!("expected a Protocol error");
    };
}

/// One `[64-byte digest][u32 size][1-byte exec][content]` file frame, the
/// shape the server emits per missing file.
fn file_frame(digest: &[u8; 64], executable: bool, content: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(digest);
    frame.extend_from_slice(&(content.len() as u32).to_be_bytes());
    frame.push(u8::from(executable));
    frame.extend_from_slice(content);
    frame
}

/// Frame a header plus file frames the way the server streams them: the
/// length-prefixed header, the `{}` files-payload prefix, the frames, and
/// the 64-zero end marker.
fn streaming_payload(header_json: &str, frames: &[Vec<u8>]) -> Vec<u8> {
    let header = header_json.as_bytes();
    let mut payload = Vec::new();
    payload.extend_from_slice(&(header.len() as u32).to_be_bytes());
    payload.extend_from_slice(header);
    payload.extend_from_slice(&2u32.to_be_bytes());
    payload.extend_from_slice(b"{}");
    for frame in frames {
        payload.extend_from_slice(frame);
    }
    payload.extend_from_slice(&[0u8; 64]);
    payload
}

/// The streaming consumer reassembles frames split across arbitrary chunk
/// boundaries and writes each file into the CAFS by digest. Feeding the
/// body 3 bytes at a time stresses the cross-chunk reassembly that a
/// single-buffer parse never exercises.
#[tokio::test]
async fn consume_stream_reassembles_frames_split_across_chunks() {
    let store = TempDir::new().expect("temp store");
    let store_dir = StoreDir::new(store.path().to_path_buf());

    let digest_a = [0x11u8; 64];
    let digest_b = [0x22u8; 64];
    let content_a = b"console.log('a')".to_vec();
    let content_b = b"module.exports = 42".to_vec();
    let payload = streaming_payload(
        r#"{"lockfile":{"lockfileVersion":"9.0"},"stats":{},"indexEntries":[]}"#,
        &[file_frame(&digest_a, false, &content_a), file_frame(&digest_b, true, &content_b)],
    );

    let chunks: Vec<Result<Bytes, PnprClientError>> =
        payload.chunks(3).map(|chunk| Ok(Bytes::copy_from_slice(chunk))).collect();
    let present = HashSet::new();
    let outcome = consume_stream(futures_util::stream::iter(chunks), &store_dir, &present)
        .await
        .expect("streaming consume should succeed");

    assert_eq!(outcome.files_written, 2);

    let path_a =
        store_dir.cas_file_path_by_mode(&hex_encode(&digest_a), 0o644).expect("digest a path");
    let path_b =
        store_dir.cas_file_path_by_mode(&hex_encode(&digest_b), 0o755).expect("digest b path");
    assert_eq!(std::fs::read(&path_a).expect("read a"), content_a);
    assert_eq!(std::fs::read(&path_b).expect("read b"), content_b);
}
