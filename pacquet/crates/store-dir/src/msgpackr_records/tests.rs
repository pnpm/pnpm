use super::{
    DecodeError, EncodeError, EncodeState, FIRST_INNER_SLOT, PKG_FILES_INDEX_SLOT,
    RECORD_DEF_EXT_TYPE, SLOT_HI, encode_package_files_index, transcode_to_plain_msgpack,
};
use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// Decoding fixture bytes produced by msgpackr yields the same
/// `PackageFilesIndex` we'd get from a vanilla msgpack round-trip.
fn decode(bytes: &[u8]) -> PackageFilesIndex {
    let plain = transcode_to_plain_msgpack(bytes).expect("transcode succeeds");
    rmp_serde::from_slice::<PackageFilesIndex>(&plain)
        .expect("transcoded output deserializes as PackageFilesIndex")
}

/// Fixture: `node /tmp/msgpackr_fixture.mjs`, "one-file index" case.
/// Source object:
/// ```js
/// { algo: 'sha512', files: new Map([['package.json',
///   { digest: 'abc', mode: 0o644, size: 17, checkedAt: 1700000000000 }]]) }
/// ```
#[test]
fn decodes_one_file_fixture_from_msgpackr() {
    let bytes: [u8; 84] = [
        0xd4, 0x72, 0x40, 0x92, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73,
        0xa6, 0x73, 0x68, 0x61, 0x35, 0x31, 0x32, 0x81, 0xac, 0x70, 0x61, 0x63, 0x6b, 0x61, 0x67,
        0x65, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0xd4, 0x72, 0x41, 0x94, 0xa6, 0x64, 0x69, 0x67, 0x65,
        0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4, 0x73, 0x69, 0x7a, 0x65, 0xa9, 0x63, 0x68,
        0x65, 0x63, 0x6b, 0x65, 0x64, 0x41, 0x74, 0xa3, 0x61, 0x62, 0x63, 0xcd, 0x01, 0xa4, 0x11,
        0xcb, 0x42, 0x78, 0xbc, 0xfe, 0x56, 0x80, 0x00, 0x00,
    ];
    let decoded = decode(&bytes);

    let mut expected_files = HashMap::new();
    expected_files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: "abc".to_string(),
            mode: 0o644,
            size: 17,
            checked_at: Some(1_700_000_000_000),
        },
    );
    assert_eq!(decoded.algo, "sha512");
    assert_eq!(decoded.files, expected_files);
    assert_eq!(decoded.manifest, None);
    assert_eq!(decoded.requires_build, None);
}

/// Fixture: "two-file index" — exercises record **reuse** (the second
/// `CafsFileInfo` starts with a bare slot byte 0x41).
#[test]
fn decodes_two_file_fixture_with_record_reuse() {
    let bytes: [u8; 103] = [
        0xd4, 0x72, 0x40, 0x92, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73,
        0xa6, 0x73, 0x68, 0x61, 0x35, 0x31, 0x32, 0x82, 0xac, 0x70, 0x61, 0x63, 0x6b, 0x61, 0x67,
        0x65, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0xd4, 0x72, 0x41, 0x94, 0xa6, 0x64, 0x69, 0x67, 0x65,
        0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4, 0x73, 0x69, 0x7a, 0x65, 0xa9, 0x63, 0x68,
        0x65, 0x63, 0x6b, 0x65, 0x64, 0x41, 0x74, 0xa3, 0x61, 0x62, 0x63, 0xcd, 0x01, 0xa4, 0x11,
        0xcb, 0x42, 0x78, 0xbc, 0xfe, 0x56, 0x80, 0x00, 0x00, 0xa8, 0x69, 0x6e, 0x64, 0x65, 0x78,
        0x2e, 0x6a, 0x73, 0x41, 0xa3, 0x64, 0x65, 0x66, 0xcd, 0x01, 0xed, 0x2a, 0xc0,
    ];
    let decoded = decode(&bytes);
    assert_eq!(decoded.files.len(), 2);

    let pkg_json = decoded.files.get("package.json").unwrap();
    assert_eq!(pkg_json.digest, "abc");
    assert_eq!(pkg_json.mode, 0o644);
    assert_eq!(pkg_json.size, 17);
    assert_eq!(pkg_json.checked_at, Some(1_700_000_000_000));

    let index_js = decoded.files.get("index.js").unwrap();
    assert_eq!(index_js.digest, "def");
    assert_eq!(index_js.mode, 0o755);
    assert_eq!(index_js.size, 42);
    assert_eq!(index_js.checked_at, None);
}

/// Fixture: "with requiresBuild" — boolean top-level field.
#[test]
fn decodes_requires_build_true() {
    let bytes: [u8; 83] = [
        0xd4, 0x72, 0x40, 0x93, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xad, 0x72, 0x65, 0x71, 0x75, 0x69,
        0x72, 0x65, 0x73, 0x42, 0x75, 0x69, 0x6c, 0x64, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73, 0xa6,
        0x73, 0x68, 0x61, 0x35, 0x31, 0x32, 0xc3, 0x81, 0xa4, 0x61, 0x2e, 0x6a, 0x73, 0xd4, 0x72,
        0x41, 0x94, 0xa6, 0x64, 0x69, 0x67, 0x65, 0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4,
        0x73, 0x69, 0x7a, 0x65, 0xa9, 0x63, 0x68, 0x65, 0x63, 0x6b, 0x65, 0x64, 0x41, 0x74, 0xa3,
        0x61, 0x61, 0x61, 0xcd, 0x01, 0xa4, 0x01, 0x0a,
    ];
    let decoded = decode(&bytes);
    assert_eq!(decoded.requires_build, Some(true));
}

/// Fixture: "no checkedAt" — proves msgpackr emits a *different* record
/// shape (3 fields instead of 4) when an optional field is absent, and
/// our `Option<u64>` deserializer copes.
#[test]
fn decodes_file_without_checked_at() {
    let bytes: [u8; 57] = [
        0xd4, 0x72, 0x40, 0x92, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73,
        0xa6, 0x73, 0x68, 0x61, 0x35, 0x31, 0x32, 0x81, 0xa4, 0x61, 0x2e, 0x6a, 0x73, 0xd4, 0x72,
        0x41, 0x93, 0xa6, 0x64, 0x69, 0x67, 0x65, 0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4,
        0x73, 0x69, 0x7a, 0x65, 0xa3, 0x61, 0x61, 0x61, 0xcd, 0x01, 0xa4, 0x01,
    ];
    let decoded = decode(&bytes);
    let info = decoded.files.get("a.js").unwrap();
    assert_eq!(info.checked_at, None);
}

/// Fixture: "with sideEffects" — nested map inside a record field,
/// plus a second record slot for the inner struct.
#[test]
fn decodes_side_effects() {
    let bytes: [u8; 113] = [
        0xd4, 0x72, 0x40, 0x93, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73,
        0xab, 0x73, 0x69, 0x64, 0x65, 0x45, 0x66, 0x66, 0x65, 0x63, 0x74, 0x73, 0xa6, 0x73, 0x68,
        0x61, 0x35, 0x31, 0x32, 0x81, 0xa4, 0x61, 0x2e, 0x6a, 0x73, 0xd4, 0x72, 0x41, 0x94, 0xa6,
        0x64, 0x69, 0x67, 0x65, 0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4, 0x73, 0x69, 0x7a,
        0x65, 0xa9, 0x63, 0x68, 0x65, 0x63, 0x6b, 0x65, 0x64, 0x41, 0x74, 0xa3, 0x61, 0x61, 0x61,
        0xcd, 0x01, 0xa4, 0x01, 0x0a, 0x81, 0xa5, 0x6c, 0x69, 0x6e, 0x75, 0x78, 0xd4, 0x72, 0x42,
        0x91, 0xa5, 0x61, 0x64, 0x64, 0x65, 0x64, 0x81, 0xa4, 0x62, 0x2e, 0x73, 0x6f, 0x41, 0xa3,
        0x62, 0x62, 0x62, 0xcd, 0x01, 0xa4, 0x02, 0x14,
    ];
    let decoded = decode(&bytes);
    let side = decoded.side_effects.expect("side_effects present");
    let linux = side.get("linux").expect("linux entry");
    let added = linux.added.as_ref().expect("added map");
    let b_so = added.get("b.so").expect("b.so entry");
    assert_eq!(b_so.digest, "bbb");
    assert_eq!(b_so.mode, 0o644);
    assert_eq!(b_so.size, 2);
    assert_eq!(b_so.checked_at, Some(20));
}

/// A row pacquet wrote itself — vanilla msgpack via `rmp_serde::to_vec_named`
/// — must decode to the same struct after passing through the
/// transcoder. The bytes are *not* guaranteed to be byte-for-byte
/// identical post-transcode: `CafsFileInfo::checked_at` is written
/// as `float 64` for msgpackr/pnpm interop, and the transcoder's
/// integer-valued-float narrowing rewrites it back to `uint 64`.
/// What matters is that the decoded `PackageFilesIndex` round-trips.
#[test]
fn round_trips_plain_msgpack_through_transcoder() {
    let mut files = HashMap::new();
    files.insert(
        "README.md".to_string(),
        CafsFileInfo { digest: "x".repeat(128), mode: 0o644, size: 42, checked_at: Some(1) },
    );
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    let bytes = rmp_serde::to_vec_named(&original).unwrap();
    let transcoded = transcode_to_plain_msgpack(&bytes).unwrap();
    let decoded: PackageFilesIndex = rmp_serde::from_slice(&transcoded).unwrap();
    assert_eq!(decoded, original);
}

/// Plain msgpack bytes that contain no `float`-encoded integers should
/// still pass through the transcoder byte-for-byte — the narrowing
/// rule must not touch anything that isn't a float header.
#[test]
fn plain_msgpack_without_floats_passes_through_unchanged() {
    // { "size": 17, "mode": 420 } — purely integer values, no
    // checked_at, so the encoded bytes have no float headers.
    let bytes = rmp_serde::to_vec_named(&serde_json::json!({
        "size": 17,
        "mode": 420,
    }))
    .unwrap();
    let transcoded = transcode_to_plain_msgpack(&bytes).unwrap();
    assert_eq!(transcoded, bytes);
}

/// A genuine non-integer float (π) must survive the transcoder as a
/// float. We don't have `PackageFilesIndex` fields that carry such
/// a value today, but the transcoder itself is a general utility —
/// the narrowing should only fire for integer-valued floats.
#[test]
fn non_integer_floats_pass_through() {
    // [3.14] as fixarray(1) + float64
    let mut input = vec![0x91, 0xcb];
    input.extend_from_slice(&std::f64::consts::PI.to_be_bytes());

    let out = transcode_to_plain_msgpack(&input).unwrap();
    assert_eq!(out, input, "π must stay as float 64, not be narrowed");
}

/// A `float 64` whose value is exactly `2^64` must NOT narrow —
/// `u64::MAX as f64` rounds up to 2^64, so a naive
/// `v <= u64::MAX as f64` bound would admit the value and silently
/// cast it to `u64::MAX`. Must pass through unchanged instead.
#[test]
fn float64_equal_to_2_pow_64_passes_through() {
    let mut input = vec![0x91, 0xcb];
    input.extend_from_slice(&18_446_744_073_709_551_616.0_f64.to_be_bytes());
    let out = transcode_to_plain_msgpack(&input).unwrap();
    assert_eq!(out, input, "2^64 must not be narrowed to u64::MAX");
}

/// An integer-valued float 32 must be narrowed too. Pnpm doesn't
/// emit `float 32`, but a hand-crafted payload could, and the rule
/// should be consistent.
#[test]
fn integer_valued_float32_is_narrowed_to_uint64() {
    // [42.0] as fixarray(1) + float32
    let mut input = vec![0x91, 0xca];
    input.extend_from_slice(&42.0_f32.to_be_bytes());

    let out = transcode_to_plain_msgpack(&input).unwrap();
    // Expect fixarray(1) + uint 64 (cf) + 42 as 8 big-endian bytes.
    let mut expected = vec![0x91, 0xcf];
    expected.extend_from_slice(&42u64.to_be_bytes());
    assert_eq!(out, expected);
}

#[test]
fn rejects_reference_to_unknown_slot() {
    // fixarray(2):
    //   [0] def slot 0x40 (fields ["x"]) + inline first instance (nil)
    //   [1] bare reference to slot 0x41 — never defined
    let bytes: &[u8] = &[
        0x92, // fixarray(2)
        0xd4, 0x72, 0x40, // def slot 0x40
        0x91, 0xa1, b'x', // fields: ["x"]
        0xc0, // first instance: nil
        0x41, // ref to slot 0x41 — undefined
    ];
    let err = transcode_to_plain_msgpack(bytes).unwrap_err();
    assert!(matches!(err, DecodeError::UnknownSlot { slot: 0x41, .. }), "got {err:?}");
}

/// In plain `MessagePack`, a bare 0x40..=0x7f byte is a positive
/// fixint (64..=127) — not a record slot reference. The transcoder
/// must not touch it until a record definition has actually
/// appeared in the stream.
#[test]
fn plain_positive_fixint_in_slot_range_passes_through() {
    // [65, 127] — both bytes would be "slot refs" under the old
    // always-records interpretation and would blow up as
    // `UnknownSlot`. Under records-mode tracking they're legitimate
    // positive fixints.
    let input = &[0x92, 0x41, 0x7f][..];
    let out = transcode_to_plain_msgpack(input).unwrap();
    assert_eq!(out, input);
}

#[test]
fn rejects_truncated_buffer() {
    // Record def claims 2 field names but only one is present.
    let err = transcode_to_plain_msgpack(&[0xd4, 0x72, 0x40, 0x92, 0xa1, b'k']).unwrap_err();
    assert!(matches!(err, DecodeError::UnexpectedEof { .. }), "got {err:?}");
}

// ===== Encoder tests =====
//
// The round-trip pattern is: `encode` → `transcode_to_plain_msgpack`
// → `rmp_serde::from_slice`. The transcoder is the Rust
// implementation of msgpackr's records wire format, so if bytes
// round-trip through it cleanly, msgpackr 1.11.8 will too. pnpm's
// store uses exactly that version, pinned in its `catalog:`.

fn roundtrip(original: &PackageFilesIndex) -> PackageFilesIndex {
    let bytes = encode_package_files_index(original).expect("encode succeeds");
    let plain = transcode_to_plain_msgpack(&bytes).expect("transcode succeeds");
    rmp_serde::from_slice(&plain).expect("deserialize")
}

fn sample_cafs(size: u64, with_checked_at: bool) -> CafsFileInfo {
    CafsFileInfo {
        digest: "a".repeat(128),
        mode: 0o644,
        size,
        checked_at: with_checked_at.then_some(1_700_000_000_000),
    }
}

#[test]
fn encode_emits_record_header_for_top_level_struct() {
    // The whole point: outer struct is a record (fixext1 `d4 72 40`),
    // not a plain msgpack map. Without this, pnpm's msgpackr would
    // decode the row as a top-level JS `Map`, and `pkgIndex.files`
    // (a property access) would be `undefined`.
    let idx = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    let bytes = encode_package_files_index(&idx).unwrap();
    assert_eq!(&bytes[0..3], &[0xd4, RECORD_DEF_EXT_TYPE, PKG_FILES_INDEX_SLOT]);
}

#[test]
fn encode_roundtrips_single_file() {
    let mut files = HashMap::new();
    files.insert("index.js".to_string(), sample_cafs(10, true));
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn encode_roundtrips_many_files_sharing_one_slot() {
    // Second and subsequent `CafsFileInfo` instances must be
    // emitted as bare slot references (one byte), not re-defined.
    // A tarball with N files collapses N × ~34 bytes of field
    // names into N × 1 byte — that's the whole point of records.
    let mut files = HashMap::new();
    for i in 0..5 {
        files.insert(format!("file{i}.js"), sample_cafs(1000 + i as u64, true));
    }
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    let bytes = encode_package_files_index(&original).unwrap();
    let record_def_headers =
        bytes.windows(2).filter(|window| *window == [0xd4, RECORD_DEF_EXT_TYPE]).count();
    // Exactly two record defs: one for `PackageFilesIndex`, one
    // for the first `CafsFileInfo` instance. The other four
    // `CafsFileInfo` instances must reference that slot.
    assert_eq!(
        record_def_headers, 2,
        "expected one def per distinct shape, got bytes {bytes:02x?}",
    );
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn encode_handles_fixint_in_slot_range_safely() {
    // `size: 0x7b` (= 123) falls inside the slot-reference range
    // 0x40..=0x7f. A naive encoder that emits it as a positive
    // fixint would produce a byte stream the decoder then
    // interprets as a reference to slot 0x7b, which is never
    // defined — the classic "UnknownSlot" blow-up. msgpackr
    // promotes all integers in this range to `uint 8` for exactly
    // this reason.
    let mut files = HashMap::new();
    files.insert("f".to_string(), sample_cafs(0x7b, true));
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    assert_eq!(roundtrip(&original).files.get("f").unwrap().size, 0x7b);
}

#[test]
fn encode_omits_checked_at_when_none() {
    // `None` `checkedAt` is *omitted* from the record schema
    // rather than encoded as `nil` — matches msgpackr's
    // field-omit-when-absent behaviour, so pnpm's reader sees the
    // same object shape it would produce on its own output (the
    // `checkedAt` property is missing, not `null`). Round-trip
    // through our transcoder still recovers `None` because
    // `Option<u64>` deserializes a missing field to `None`.
    let mut files = HashMap::new();
    files.insert("f".to_string(), sample_cafs(100, false));
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    let bytes = encode_package_files_index(&original).unwrap();
    // No `checkedAt` string should appear — the schema for this
    // `CafsFileInfo` only has `[digest, mode, size]`.
    let needle = b"checkedAt";
    assert!(
        bytes.windows(needle.len()).all(|window| window != needle),
        "checkedAt leaked into output when the field was None: {bytes:02x?}",
    );
    assert_eq!(roundtrip(&original).files.get("f").unwrap().checked_at, None);
}

#[test]
fn encode_allocates_separate_slots_for_distinct_cafs_shapes() {
    // Two `CafsFileInfo` instances with different shapes — one
    // carries `checkedAt`, the other doesn't — must land in
    // different slots. Same shape reuses its slot, which is the
    // whole point of records. msgpackr does the same: slot 0x41
    // for the first shape seen, 0x42 for the next new one, etc.
    let mut files = HashMap::new();
    files.insert("with_ts.js".to_string(), sample_cafs(10, true));
    files.insert("no_ts_a.js".to_string(), sample_cafs(20, false));
    files.insert("no_ts_b.js".to_string(), sample_cafs(30, false));
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    let bytes = encode_package_files_index(&original).unwrap();
    let record_def_headers =
        bytes.windows(2).filter(|window| *window == [0xd4, RECORD_DEF_EXT_TYPE]).count();
    // Exactly three defs: `PackageFilesIndex`, `CafsFileInfo` with
    // checkedAt, `CafsFileInfo` without checkedAt. The third file
    // (second no-ts instance) shares the no-ts slot, so no fourth
    // def.
    assert_eq!(
        record_def_headers, 3,
        "expected three defs (outer + two CafsFileInfo shapes), got bytes {bytes:02x?}",
    );
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn encode_requires_build_when_set() {
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    let roundtripped = roundtrip(&original);
    assert_eq!(roundtripped.requires_build, Some(true));
}

#[test]
fn encode_outer_field_order_matches_msgpackr() {
    // Field order in the outer record must match what pnpm's
    // msgpackr emits, because wire-level diffing against pnpm-
    // written rows is a routine debugging exercise. Ground truth
    // comes from the `decodes_requires_build_true` fixture
    // captured from msgpackr 1.11.8, where the schema reads
    // `[algo, requiresBuild, files]` — i.e. optional fields slot
    // in at pnpm's TypeScript-declared position, not tacked onto
    // the end.
    let mut files = HashMap::new();
    files.insert("a.js".to_string(), sample_cafs(10, true));
    let idx = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    let bytes = encode_package_files_index(&idx).unwrap();
    // Find the outer schema bytes: after `d4 72 40` (fixext1 +
    // ext-type + slot) comes `93` (fixarray of 3 fields), then
    // the field-name fixstrs.
    assert_eq!(&bytes[0..4], &[0xd4, RECORD_DEF_EXT_TYPE, PKG_FILES_INDEX_SLOT, 0x93]);
    // Re-decode the field names from offset 4 onwards.
    let mut pos = 4;
    let mut names = Vec::new();
    for _ in 0..3 {
        let hdr = bytes[pos];
        assert!(matches!(hdr, 0xa0..=0xbf), "expected fixstr at {pos}, got {hdr:02x}");
        let len = (hdr & 0x1f) as usize;
        pos += 1;
        names.push(std::str::from_utf8(&bytes[pos..pos + len]).unwrap().to_string());
        pos += len;
    }
    assert_eq!(names, vec!["algo", "requiresBuild", "files"]);
}

#[test]
fn encode_omits_requires_build_when_none() {
    // When `requires_build` is `None`, it must not appear in the
    // record schema at all — matching msgpackr's own behaviour of
    // field-omit-when-absent for plain JS objects with missing
    // properties. This keeps the byte output minimal for the
    // common case (pacquet rarely populates requires_build).
    let idx = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    let bytes = encode_package_files_index(&idx).unwrap();
    // Scan for the bytes that spell "requiresBuild" — must not
    // appear anywhere in the output.
    let needle = b"requiresBuild";
    assert!(
        bytes.windows(needle.len()).all(|window| window != needle),
        "requiresBuild leaked into output when the field was None: {bytes:02x?}",
    );
}

#[test]
fn encode_side_effects_roundtrip() {
    let mut added = HashMap::new();
    added.insert("foo.so".to_string(), sample_cafs(42, true));
    let mut side_effects = HashMap::new();
    side_effects.insert(
        "linux".to_string(),
        SideEffectsDiff { added: Some(added), deleted: Some(vec!["bar.o".to_string()]) },
    );
    let mut files = HashMap::new();
    files.insert("main.js".to_string(), sample_cafs(10, true));
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: Some(side_effects),
    };
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn encode_side_effects_with_only_added_omits_deleted_field() {
    // A `SideEffectsDiff` with `deleted: None` must not emit a
    // `deleted` field name in the record schema. This is the case
    // Copilot flagged: the fixed-schema encoder used to write
    // `deleted: nil` here, producing a JS shape (`{ added, deleted:
    // null }`) different from what msgpackr itself produces for
    // the same Rust input (`{ added }`).
    let mut added = HashMap::new();
    added.insert("foo.so".to_string(), sample_cafs(42, true));
    let mut side_effects = HashMap::new();
    side_effects.insert("linux".to_string(), SideEffectsDiff { added: Some(added), deleted: None });
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: Some(side_effects),
    };
    let bytes = encode_package_files_index(&original).unwrap();
    assert!(
        bytes.windows(7).all(|window| window != b"deleted"),
        "`deleted` field name appeared in output when the field was None: {bytes:02x?}",
    );
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn encode_allocates_separate_slots_for_distinct_side_effects_shapes() {
    // Two `SideEffectsDiff` instances with distinct shapes (one
    // with only `added`, one with only `deleted`) must land in
    // different slots, mirroring msgpackr's behaviour on the same
    // input.
    let mut linux_added = HashMap::new();
    linux_added.insert("foo.so".to_string(), sample_cafs(42, true));
    let mut side_effects = HashMap::new();
    side_effects
        .insert("linux".to_string(), SideEffectsDiff { added: Some(linux_added), deleted: None });
    side_effects.insert(
        "darwin".to_string(),
        SideEffectsDiff { added: None, deleted: Some(vec!["bar.o".to_string()]) },
    );
    let original = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: Some(side_effects),
    };
    let bytes = encode_package_files_index(&original).unwrap();
    let record_def_headers =
        bytes.windows(2).filter(|window| *window == [0xd4, RECORD_DEF_EXT_TYPE]).count();
    // Three defs: outer `PackageFilesIndex`, `SideEffectsDiff`
    // shape-`added`, `SideEffectsDiff` shape-`deleted`. The inner
    // `CafsFileInfo` adds a fourth.
    assert_eq!(
        record_def_headers, 4,
        "expected defs for outer + two distinct side-effects shapes + CafsFileInfo, got bytes {bytes:02x?}",
    );
    assert_eq!(roundtrip(&original), original);
}

#[test]
fn allocate_slot_returns_error_past_0x7f() {
    // Exhaust the inner-slot range (0x41..=0x7f, 63 slots) and
    // verify the next call returns `OutOfRecordSlots` rather than
    // panicking. Not reachable through the public encoder for
    // current pacquet payloads — `CafsFileInfo` has 2 shapes,
    // `SideEffectsDiff` has 4 — but the error path must exist in
    // case a future record type is added without bumping the
    // shape accounting.
    let mut state = EncodeState::default();
    for _ in FIRST_INNER_SLOT..=SLOT_HI {
        state.allocate_slot().expect("should succeed within the slot range");
    }
    let err = state.allocate_slot().expect_err("64th allocation must fail");
    assert!(matches!(err, EncodeError::OutOfRecordSlots { max: 63 }), "got {err:?}");
}

/// A `manifest: Some(_)` must round-trip through encode →
/// transcode → `rmp_serde::from_slice` unchanged. This is the basic
/// "the encoder doesn't drop or mangle JSON values" smoke test.
#[test]
fn encode_roundtrips_simple_manifest() {
    let manifest = serde_json::json!({
        "name": "pkg",
        "version": "1.0.0",
        "bin": "cli.js",
    });
    let original = PackageFilesIndex {
        manifest: Some(manifest),
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    assert_eq!(roundtrip(&original), original);
}

/// Nested objects inside the manifest must be **record-encoded**, not
/// emitted as plain msgpack maps — otherwise msgpackr in
/// `useRecords: true` mode would decode them as JS `Map` and
/// `manifest.bin.tsc` (the property access pnpm's bin linker uses
/// for object-form `bin` fields) would be `undefined`. The encoder
/// signals this by emitting one `d4 72 <slot>` def per distinct
/// nested-object shape.
#[test]
fn encode_record_encodes_nested_objects_in_manifest() {
    let manifest = serde_json::json!({
        "name": "tsc",
        "version": "5.0.0",
        "bin": { "tsc": "bin/tsc.js", "tsserver": "bin/tsserver.js" },
        "directories": { "bin": "bin" },
    });
    let idx = PackageFilesIndex {
        manifest: Some(manifest),
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    let bytes = encode_package_files_index(&idx).unwrap();

    // Outer PackageFilesIndex def + nested manifest object def + nested
    // `bin` object def + nested `directories` object def = 4 defs.
    let record_defs =
        bytes.windows(2).filter(|window| *window == [0xd4, RECORD_DEF_EXT_TYPE]).count();
    assert_eq!(
        record_defs, 4,
        "expected 4 record defs (outer + manifest + bin + directories), got bytes {bytes:02x?}",
    );

    // Round-trip the manifest through the transcoder to verify the
    // bytes a msgpackr 1.11.8 reader would consume parse cleanly.
    assert_eq!(roundtrip(&idx), idx);
}

/// Two nested objects with the **same** key set within the same
/// manifest must share a slot. Verifies record-reuse — the whole
/// point of records. Shape here is the *key set*, not just the
/// arity: `{left-pad}` and `{right-pad}` are different shapes
/// (different keys), so the test uses two objects that genuinely
/// share keys.
#[test]
fn encode_shares_slot_for_same_shaped_nested_objects() {
    let manifest = serde_json::json!({
        "bin": { "cli": "bin/cli.js" },
        "directories": { "cli": "src" },
    });
    let idx = PackageFilesIndex {
        manifest: Some(manifest),
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    let bytes = encode_package_files_index(&idx).unwrap();
    let record_defs =
        bytes.windows(2).filter(|window| *window == [0xd4, RECORD_DEF_EXT_TYPE]).count();
    // Outer + manifest + ONE shape `{ cli }` shared by both nested
    // maps = 3 defs. If the encoder allocated a new slot per
    // instance instead of sharing, this would be 4.
    assert_eq!(
        record_defs, 3,
        "expected slot reuse for same-shape objects, got bytes {bytes:02x?}",
    );
    assert_eq!(roundtrip(&idx), idx);
}

/// All JSON value kinds (null, bool, number, string, array, object)
/// must round-trip. Numbers cover the slot-byte fixint range so the
/// `0x40..=0x7f` → `uint 8` promotion in [`write_uint`] is exercised
/// from inside the manifest encoding path too.
#[test]
fn encode_roundtrips_all_json_value_kinds() {
    let manifest = serde_json::json!({
        "string": "hello",
        "null": null,
        "true": true,
        "false": false,
        "small_int": 0,
        "slot_range_int": 0x42,
        "big_int": 1_000_000_u64,
        "negative_int": -5,
        "float": 3.5,
        "array": ["a", 1, null, true, [1, 2], { "k": "v" }],
        "empty_array": [],
        "empty_object": {},
    });
    let idx = PackageFilesIndex {
        manifest: Some(manifest),
        requires_build: None,
        algo: "sha512".to_string(),
        files: HashMap::new(),
        side_effects: None,
    };
    assert_eq!(roundtrip(&idx), idx);
}

/// Manifest must round-trip alongside `requiresBuild`, `files`, and
/// `sideEffects` — the encoder has to emit all five fields in the
/// expected order and the decoder still needs to recover each.
#[test]
fn encode_roundtrips_manifest_with_other_fields() {
    let mut files = HashMap::new();
    files.insert("package.json".to_string(), sample_cafs(42, false));
    let original = PackageFilesIndex {
        manifest: Some(serde_json::json!({ "name": "x", "bin": "cli.js" })),
        requires_build: Some(true),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    assert_eq!(roundtrip(&original), original);
}
