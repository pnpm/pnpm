use super::{calc_patch_hashes, create_hex_hash_from_file};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;

/// SHA-256 of `"hello\n"` (Node's `crypto.hash('sha256', s, 'hex')`
/// gives `5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03`).
/// Cross-checked with `printf 'hello\n' | shasum -a 256`.
const HELLO_SHA256_HEX: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

#[test]
fn hashes_a_known_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("hello.patch");
    fs::write(&path, b"hello\n").unwrap();
    assert_eq!(create_hex_hash_from_file(&path).unwrap(), HELLO_SHA256_HEX);
}

/// CRLF must normalize to LF — otherwise the same logical patch
/// hashes differently on Windows vs POSIX. Mirrors upstream's
/// [`readNormalizedFile`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/crypto/hash/src/index.ts#L36-L39).
#[test]
fn crlf_normalizes_to_lf() {
    let dir = tempdir().unwrap();
    let crlf = dir.path().join("crlf.patch");
    let lf = dir.path().join("lf.patch");
    fs::write(&crlf, b"hello\r\n").unwrap();
    fs::write(&lf, b"hello\n").unwrap();

    let crlf_hash = create_hex_hash_from_file(&crlf).unwrap();
    let lf_hash = create_hex_hash_from_file(&lf).unwrap();
    assert_eq!(crlf_hash, lf_hash);
    assert_eq!(lf_hash, HELLO_SHA256_HEX);
}

#[test]
fn maps_keys_to_hashes() {
    let dir = tempdir().unwrap();
    let patch_a = dir.path().join("a.patch");
    let patch_b = dir.path().join("b.patch");
    fs::write(&patch_a, b"hello\n").unwrap();
    fs::write(&patch_b, b"world\n").unwrap();

    let hashes =
        calc_patch_hashes(vec![("foo@1.0.0".to_string(), patch_a), ("bar".to_string(), patch_b)])
            .unwrap();

    assert_eq!(hashes.get("foo@1.0.0").unwrap(), HELLO_SHA256_HEX);
    assert_eq!(
        hashes.get("bar").unwrap(),
        // sha256 of "world\n"
        "e258d248fda94c63753607f7c4494ee0fcbe92f1a76bfdac795c9d84101eb317",
    );
}

#[test]
fn missing_file_errors() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("nope.patch");
    let err = create_hex_hash_from_file(&missing).unwrap_err();
    // Just confirm the error variant; `io::Error` formatting is
    // platform-specific.
    assert!(matches!(err, super::CalcPatchHashError::ReadFile { .. }), "got: {err:?}");
}

/// Invalid UTF-8 bytes are replaced with U+FFFD rather than
/// erroring, matching Node.js `fs.readFile(path, 'utf8')` which
/// upstream uses in
/// [`readNormalizedFile`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/crypto/hash/src/index.ts#L36-L39).
/// Three stray invalid bytes hash as three U+FFFD chars (each
/// encoded as `0xEF 0xBF 0xBD` in UTF-8). The expected digest is
/// the SHA-256 of those nine bytes, cross-checked against
/// `printf '\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd' | shasum -a 256`.
#[test]
fn non_utf8_uses_replacement_char_and_does_not_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("invalid.patch");
    fs::write(&path, [0xffu8, 0xfeu8, 0xfdu8]).unwrap();
    let hash = create_hex_hash_from_file(&path).expect("lossy decoding must not error");
    assert_eq!(hash, "a73f4cb996ceb6ee097888d897ae1004c9b1faab6c97629214139b9639aaf1af");
}
