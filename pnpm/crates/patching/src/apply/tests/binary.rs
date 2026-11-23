use crate::apply::{PatchApplyError, apply_patch_to_dir, preview_patch};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;

fn write_patch(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("patch.patch");
    fs::write(&path, body).expect("write patch");
    path
}

/// A `git diff --binary` modify record over a small binary file, taken
/// verbatim from git's output. The forward block is a `literal` holding the
/// full 17-byte post-image; the reverse block a `literal` holding the full
/// 13-byte pre-image.
const BINARY_MODIFY_PATCH: &str = "\
diff --git a/bin.dat b/bin.dat
index 9a20e1af1471b1378e27f2c97c3047811aea78ce..2ba8392e9cff316d90d283b8635b340073fd704c 100644
GIT binary patch
literal 17
Ycma!ujACGBVa-b|PtMOPNzE$(04SmbT>t<8

literal 13
UcmZ>Ca&~cbV_;-*_we)r026ouy8r+H

";

const BINARY_PREIMAGE: &[u8] = b"ABCDEF\x00\x01\x02GHIJ";
const BINARY_POSTIMAGE: &[u8] = b"XYZ\x00\x03\x04\x05newcontent";

/// A `git diff --binary` create record: the forward block carries the full
/// 7-byte content, the reverse block is `literal 0`.
const BINARY_CREATE_PATCH: &str = "\
diff --git a/added.bin b/added.bin
new file mode 100644
index 0000000000000000000000000000000000000000..0ae8040411865fbc997e77a0a3ccce72e6e91cd8
GIT binary patch
literal 7
Ocmc~xEoVr|%mV-lAOeH{

literal 0
HcmV?d00001

";

const BINARY_CREATED: &[u8] = b"new\x00bin";

/// What `git diff --irreversible-delete --binary` leaves for a deleted
/// binary file: the header alone, without a `GIT binary patch` section.
const BINARY_DELETE_HEADER_ONLY_PATCH: &str = "\
diff --git a/bin.dat b/bin.dat
deleted file mode 100644
index 9a20e1af1471b1378e27f2c97c3047811aea78ce..0000000000000000000000000000000000000000
";

/// A `git diff --binary` modify record whose forward and reverse blocks are
/// both `delta` — git picks this encoding when the two versions are similar.
const BINARY_DELTA_MODIFY_PATCH: &str = "\
diff --git a/big.bin b/big.bin
index d88d75086be4e95d2edadda5a4361e3e64e8532f..20177a995f60ed80f133d437da00c8db3d2ab43e 100644
GIT binary patch
delta 23
fcmZo*>t@?9fsy;(+I{Z>97CKvTwOL!C}0EtaDoX(

delta 13
UcmeBXYhc?jfpOsk#)%UO03oym8~^|S

";

/// 768 bytes of the byte pattern the delta patch was authored against.
const BIG_BIN_PREIMAGE: &[u8] = &{
    let mut bytes = [0u8; 768];
    let mut i = 0;
    while i < 768 {
        bytes[i] = (i % 256) as u8;
        i += 1;
    }
    bytes
};

/// The post-image: 400 untouched bytes, then the injected marker, then the
/// remaining 368 bytes.
const BIG_BIN_INJECTED: &[u8] = b"\xde\xad\xbe\xefPATCHED";

const BIG_BIN_POSTIMAGE: &[u8] = &{
    let mut bytes = [0u8; 779];
    let mut i = 0;
    while i < 400 {
        bytes[i] = (i % 256) as u8;
        i += 1;
    }
    let mut j = 0;
    while j < 11 {
        bytes[400 + j] = BIG_BIN_INJECTED[j];
        j += 1;
    }
    let mut k = 0;
    while k < 368 {
        bytes[411 + k] = ((400 + k) % 256) as u8;
        k += 1;
    }
    bytes
};

/// A rename record over a binary file that also carries a content change as
/// a `GIT binary patch` delta section.
const BINARY_RENAME_PATCH: &str = "\
diff --git a/old.bin b/new.bin
similarity index 92%
rename from old.bin
rename to new.bin
index d88d75086be4e95d2edadda5a4361e3e64e8532f..4b802595e1f2ab0feb61fb2d757bba69b382856a 100644
GIT binary patch
delta 11
ScmZo*Yhhc!%u<q=nF9b5QUhWD

delta 6
NcmZo+YhYWz3;+g^0ki-B
";

const RENAME_PREIMAGE: &[u8] = &{
    let mut bytes = [0u8; 768];
    let mut i = 0;
    while i < 768 {
        bytes[i] = (i % 256) as u8;
        i += 1;
    }
    bytes
};

const RENAME_POSTIMAGE: &[u8] = &{
    let mut bytes = [0u8; 772];
    let mut i = 0;
    while i < 768 {
        bytes[i] = (i % 256) as u8;
        i += 1;
    }
    bytes[768] = b't';
    bytes[769] = b'a';
    bytes[770] = b'i';
    bytes[771] = b'l';
    bytes
};

#[test]
fn binary_modify_patch_applies_and_is_idempotent() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("bin.dat");
    fs::write(&target, BINARY_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), BINARY_MODIFY_PATCH);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert_eq!(fs::read(&target).expect("read patched"), BINARY_POSTIMAGE);

    // A re-apply is a no-op, like the text path.
    apply_patch_to_dir(dir.path(), &patch).expect("reapply");
    assert_eq!(fs::read(&target).expect("read patched"), BINARY_POSTIMAGE);
}

#[test]
fn binary_modify_patch_rejects_wrong_preimage() {
    // The delta section validates the base's size (and content), so a file
    // that is neither the pre-image nor the post-image is rejected. A
    // `literal` section carries the full post-image and decodes regardless
    // of the base, so it cannot detect this from content alone.
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("big.bin");
    fs::write(&target, b"some other bytes entirely").expect("write wrong preimage");
    let patch = write_patch(dir.path(), BINARY_DELTA_MODIFY_PATCH);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(matches!(error, PatchApplyError::PatchFailed { .. }), "error: {error}");
    assert_eq!(fs::read(&target).expect("read target"), b"some other bytes entirely");
}

#[test]
fn binary_delta_modify_patch_applies_and_is_idempotent() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("big.bin");
    fs::write(&target, BIG_BIN_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), BINARY_DELTA_MODIFY_PATCH);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert_eq!(fs::read(&target).expect("read patched"), BIG_BIN_POSTIMAGE);

    // A re-apply is a no-op: the forward delta fails against the post-image
    // and the reverse delta decodes cleanly.
    apply_patch_to_dir(dir.path(), &patch).expect("reapply");
    assert_eq!(fs::read(&target).expect("read patched"), BIG_BIN_POSTIMAGE);
}

#[test]
fn binary_create_patch_creates_file_without_clobbering() {
    let dir = tempdir().expect("temp dir");
    let patch = write_patch(dir.path(), BINARY_CREATE_PATCH);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    let target = dir.path().join("added.bin");
    assert_eq!(fs::read(&target).expect("read created"), BINARY_CREATED);

    // Re-apply on the created file is a no-op.
    apply_patch_to_dir(dir.path(), &patch).expect("reapply");
    assert_eq!(fs::read(&target).expect("read created"), BINARY_CREATED);

    // A different existing file is not clobbered.
    let dir2 = tempdir().expect("temp dir");
    let patch2 = write_patch(dir2.path(), BINARY_CREATE_PATCH);
    let target2 = dir2.path().join("added.bin");
    fs::write(&target2, b"precious existing data").expect("write existing");
    let error = apply_patch_to_dir(dir2.path(), &patch2).expect_err("should refuse to overwrite");
    assert!(matches!(error, PatchApplyError::PatchFailed { .. }), "error: {error}");
    assert_eq!(fs::read(&target2).expect("read existing"), b"precious existing data");
}

#[test]
fn binary_delete_header_only_removes_file_and_is_idempotent() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("bin.dat");
    fs::write(&target, BINARY_PREIMAGE).expect("write file");
    let patch = write_patch(dir.path(), BINARY_DELETE_HEADER_ONLY_PATCH);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert!(!target.exists(), "file should be deleted");

    // A re-apply finds nothing to delete and still succeeds.
    apply_patch_to_dir(dir.path(), &patch).expect("reapply");
}

#[test]
fn binary_rename_patch_moves_and_rewrites_content() {
    let dir = tempdir().expect("temp dir");
    let from = dir.path().join("old.bin");
    let to = dir.path().join("new.bin");
    fs::write(&from, RENAME_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), BINARY_RENAME_PATCH);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert!(!from.exists(), "old entry should be gone");
    assert_eq!(fs::read(&to).expect("read renamed"), RENAME_POSTIMAGE);
}

#[test]
fn binary_patch_preview_reports_written_paths() {
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("bin.dat"), BINARY_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), BINARY_CREATE_PATCH);

    // Preview does not write anything.
    let preview = preview_patch(dir.path(), &patch).expect("preview");
    assert_eq!(preview.written_paths, vec!["added.bin".to_string()]);
    assert!(!dir.path().join("added.bin").exists(), "preview must not write");
}
