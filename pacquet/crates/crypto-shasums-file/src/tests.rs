use pretty_assertions::assert_eq;

use super::{
    PickFileChecksumError, ShasumsFileItem, parse_shasums_file,
    pick_file_checksum_from_shasums_file,
};

/// Two valid rows are parsed into SRI-encoded integrities.
///
/// Mirrors upstream's first
/// [`pickFileChecksumFromShasumsFile` test](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L5-L8)
/// — same input body, same expected integrity for the first row.
#[test]
fn parses_rows_into_sri_encoded_integrities() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi
";
    let items = parse_shasums_file(body);
    assert_eq!(
        items,
        vec![
            ShasumsFileItem {
                integrity: "sha256-7VIjkpStUX++kaJoFG1dKqihfS1i1khz5DIZB4unHE4=".to_string(),
                file_name: "foo.tar.gz".to_string(),
            },
            ShasumsFileItem {
                integrity: "sha256-vhJ74dmMrZTFb0YkXQ8t6Jk00wAChpRFaGGm1axVi/M=".to_string(),
                file_name: "foo.msi".to_string(),
            },
        ],
    );
}

/// Empty lines anywhere in the body are dropped.
#[test]
fn skips_empty_lines() {
    let body = "\n\nabc def\n";
    // The hash isn't valid hex, so this is just exercising the line
    // split. We rely on the upstream contract that the body has a
    // proper hash; the picker is the validator.
    let items = parse_shasums_file(body);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].file_name, "def");
}

/// Picking the integrity for an existing filename succeeds.
///
/// Mirrors the
/// [`picks the right checksum for a file`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L5-L8)
/// upstream test verbatim.
#[test]
fn picks_the_right_checksum_for_a_file() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let integrity = pick_file_checksum_from_shasums_file(body, "foo.tar.gz").unwrap();
    assert_eq!(integrity, "sha256-7VIjkpStUX++kaJoFG1dKqihfS1i1khz5DIZB4unHE4=");
}

/// Picking a filename that isn't in the body raises `NODE_INTEGRITY_HASH_NOT_FOUND`.
///
/// Mirrors the
/// [`throws an error if no integrity found`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L9-L12)
/// upstream test.
#[test]
fn missing_file_name_raises_not_found() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let err = pick_file_checksum_from_shasums_file(body, "bar.zip").unwrap_err();
    assert!(matches!(
        err,
        PickFileChecksumError::NotFound { ref file_name } if file_name == "bar.zip",
    ));
}

/// A malformed (too-short) hash in an otherwise well-formed row raises
/// `NODE_MALFORMED_INTEGRITY_HASH` instead of silently truncating the
/// integrity.
///
/// Mirrors the
/// [`throws an error if a malformed integrity is found`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L13-L16)
/// upstream test.
#[test]
fn malformed_hash_raises_malformed() {
    let body = "\
ed52239294ad517fbe91  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let err = pick_file_checksum_from_shasums_file(body, "foo.tar.gz").unwrap_err();
    match err {
        PickFileChecksumError::Malformed { file_name, sha256 } => {
            assert_eq!(file_name, "foo.tar.gz");
            assert_eq!(sha256, "ed52239294ad517fbe91");
        }
        other @ PickFileChecksumError::NotFound { .. } => {
            panic!("expected Malformed, got {other:?}")
        }
    }
}
