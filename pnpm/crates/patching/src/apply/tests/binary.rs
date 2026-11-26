use crate::apply::{
    PatchApplyError,
    apply_patch_to_dir,
    preview_patch,
};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;

use super::write_patch;

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
/// both `delta`. Git picks this encoding when the two versions are similar.
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
    let mut index = 0;
    while index < 768 {
        bytes[index] = (index % 256) as u8;
        index += 1;
    }
    bytes
};

/// The post-image: 400 untouched bytes, then the injected marker, then the
/// remaining 368 bytes.
const BIG_BIN_INJECTED: &[u8] = b"\xde\xad\xbe\xefPATCHED";

const BIG_BIN_POSTIMAGE: &[u8] = &{
    let mut bytes = [0u8; 779];
    let mut index = 0;
    while index < 400 {
        bytes[index] = (index % 256) as u8;
        index += 1;
    }
    let mut index = 0;
    while index < 11 {
        bytes[400 + index] = BIG_BIN_INJECTED[index];
        index += 1;
    }
    let mut index = 0;
    while index < 368 {
        bytes[411 + index] = ((400 + index) % 256) as u8;
        index += 1;
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
    let mut index = 0;
    while index < 768 {
        bytes[index] = (index % 256) as u8;
        index += 1;
    }
    bytes
};

const RENAME_POSTIMAGE: &[u8] = &{
    let mut bytes = [0u8; 772];
    let mut index = 0;
    while index < 768 {
        bytes[index] = (index % 256) as u8;
        index += 1;
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
    // A file that matches neither the recorded pre-image nor the
    // post-image is refused, whatever the block encoding.
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

    // A re-apply is a no-op, like the text path.
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

    let preview = preview_patch(dir.path(), &patch).expect("preview");
    assert_eq!(preview.written_paths, vec!["added.bin".to_string()]);
    assert!(!dir.path().join("added.bin").exists(), "preview must not write");
}

#[test]
fn binary_literal_modify_rejects_file_matching_neither_side() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("bin.dat");
    fs::write(&target, b"some other bytes entirely").expect("write unrelated content");
    let patch = write_patch(dir.path(), BINARY_MODIFY_PATCH);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(matches!(error, PatchApplyError::PatchFailed { .. }), "error: {error}");
    assert_eq!(fs::read(&target).expect("read target"), b"some other bytes entirely");
}

/// Pre- and post-image with equal length, built by rearranging the same
/// bytes: the case a length-only base check would confuse.
fn equal_length_pair() -> (Vec<u8>, Vec<u8>) {
    let mut pre: Vec<u8> = (0..512u32)
        .map(|i| (i % 256) as u8)
        .collect();
    pre.extend_from_slice(b"HEAD");
    let mut post = pre[256..].to_vec();
    post.extend_from_slice(&pre[..256]);
    (pre, post)
}

#[test]
fn binary_delta_equal_length_rearrange_replay_is_noop() {
    let (preimage, postimage) = equal_length_pair();
    let before = tempdir().expect("before dir");
    fs::write(before.path().join("big.bin"), &preimage).expect("write preimage");
    let after = tempdir().expect("after dir");
    fs::write(after.path().join("big.bin"), &postimage).expect("write postimage");

    let diff = crate::diff_folders(before.path(), after.path()).expect("diff dirs");
    assert!(diff.contains("GIT binary patch"), "git should emit a binary patch");

    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("big.bin");
    fs::write(&target, &preimage).expect("write preimage");
    let patch = write_patch(dir.path(), &diff);

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert_eq!(fs::read(&target).expect("read patched"), postimage);

    // The replay must resolve to already-patched, not re-arrange the
    // already rearranged bytes again.
    apply_patch_to_dir(dir.path(), &patch).expect("reapply");
    assert_eq!(fs::read(&target).expect("read patched"), postimage);
}

/// A `git diff --binary` delete record that still carries the pre-image,
/// built by mirroring [`BINARY_CREATE_PATCH`]: the forward block becomes
/// `literal 0` and the reverse block carries the pre-image. `pnpm patch`
/// and `pnpm patch-commit` diff with `--irreversible-delete`, so a record
/// like this only reaches the apply side from patches authored elsewhere.
fn binary_delete_patch_with_data() -> String {
    BINARY_CREATE_PATCH
        .replace(
            "diff --git a/added.bin b/added.bin\nnew file mode 100644",
            "diff --git a/gone.bin b/gone.bin\ndeleted file mode 100644",
        )
        .replace(
            "0000000000000000000000000000000000000000..0ae8040411865fbc997e77a0a3ccce72e6e91cd8",
            "0ae8040411865fbc997e77a0a3ccce72e6e91cd8..0000000000000000000000000000000000000000",
        )
        .replace(
            "literal 7\nOcmc~xEoVr|%mV-lAOeH{\n\nliteral 0\nHcmV?d00001",
            "literal 0\nHcmV?d00001\n\nliteral 7\nOcmc~xEoVr|%mV-lAOeH{",
        )
}

/// The content the forward block of [`BINARY_CREATE_PATCH`] carries.
fn created_content() -> Vec<u8> {
    use diffy::patch_set::{
        ParseOptions,
        PatchSet,
    };
    let file_patch = PatchSet::parse(BINARY_CREATE_PATCH, ParseOptions::gitdiff())
        .next()
        .expect("one record")
        .expect("parse");
    match file_patch.patch() {
        diffy::patch_set::PatchKind::Binary(binary_patch) => binary_patch
            .apply(&[])
            .expect("decode content"),
        diffy::patch_set::PatchKind::Text(_) => unreachable!("binary fixture"),
    }
}

#[test]
fn binary_delete_with_data_rejects_mismatched_preimage() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("gone.bin");
    fs::write(&target, b"different content entirely").expect("write wrong content");
    let patch = write_patch(dir.path(), &binary_delete_patch_with_data());

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not delete");
    assert!(matches!(error, PatchApplyError::PatchFailed { .. }), "error: {error}");
    assert!(target.exists(), "mismatched file must be kept");
}

#[test]
fn binary_delete_with_data_removes_matching_file() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("gone.bin");
    fs::write(&target, created_content()).expect("write preimage");
    let patch = write_patch(dir.path(), &binary_delete_patch_with_data());

    apply_patch_to_dir(dir.path(), &patch).expect("apply");
    assert!(!target.exists(), "matching file must be removed");
}

#[test]
fn binary_manifest_record_is_previewed_for_build_decision() {
    // The same binary record as BINARY_MODIFY_PATCH, aimed at the manifest:
    // the preview must resolve it, the build decision reads what it adds.
    let patch_body = BINARY_MODIFY_PATCH.replace(
        "diff --git a/bin.dat b/bin.dat",
        "diff --git a/package.json b/package.json",
    );
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("package.json"), BINARY_PREIMAGE).expect("write manifest");
    let patch = write_patch(dir.path(), &patch_body);

    let preview = preview_patch(dir.path(), &patch).expect("preview");
    assert_eq!(
        preview.written_paths,
        vec!["package.json".to_string()],
        "the binary manifest record must be previewed",
    );
    assert_eq!(
        preview.manifest.as_deref(),
        Some(String::from_utf8_lossy(BINARY_POSTIMAGE).as_ref()),
    );
}

#[test]
fn binary_delta_modify_rejects_same_length_base_with_changed_byte() {
    let mut altered = BIG_BIN_PREIMAGE.to_vec();
    altered[100] ^= 0xff;
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("big.bin");
    fs::write(&target, &altered).expect("write altered preimage");
    let patch = write_patch(dir.path(), BINARY_DELTA_MODIFY_PATCH);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(matches!(error, PatchApplyError::PatchFailed { .. }), "error: {error}");
    assert_eq!(fs::read(&target).expect("read target"), altered);
}

#[test]
fn binary_patch_without_full_index_line_is_rejected() {
    let patch_body = BINARY_MODIFY_PATCH.replace(
        "index 9a20e1af1471b1378e27f2c97c3047811aea78ce..2ba8392e9cff316d90d283b8635b340073fd704c",
        "index 9a20e1a..2ba8392",
    );
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("bin.dat");
    fs::write(&target, BINARY_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), &patch_body);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(error.to_string().contains("full index line"), "error: {error}");
    assert_eq!(fs::read(&target).expect("read target"), BINARY_PREIMAGE);
}

/// A create record whose forward block holds only the two-byte zlib
/// header, a stream that ends before its end marker.
const TRUNCATED_STREAM_PATCH: &str = "\
diff --git a/cut.bin b/cut.bin
new file mode 100644
index 0000000000000000000000000000000000000000..0ae8040411865fbc997e77a0a3ccce72e6e91cd8
GIT binary patch
literal 1
Bc$@$L

literal 0
HcmV?d00001

";

#[test]
fn binary_patch_with_truncated_stream_is_rejected() {
    let dir = tempdir().expect("temp dir");
    let patch = write_patch(dir.path(), TRUNCATED_STREAM_PATCH);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(error.to_string().contains("corrupt binary data"), "error: {error}");
    assert!(!dir.path().join("cut.bin").exists(), "nothing should be created");
}

#[test]
fn binary_rename_without_binary_data_is_rejected_before_moving() {
    let patch_body = "\
diff --git a/old.bin b/new.bin
similarity index 92%
rename from old.bin
rename to new.bin
index d88d75086be4e95d2edadda5a4361e3e64e8532f..4b802595e1f2ab0feb61fb2d757bba69b382856a 100644
Binary files a/old.bin and b/new.bin differ
";
    let dir = tempdir().expect("temp dir");
    let from = dir.path().join("old.bin");
    fs::write(&from, RENAME_PREIMAGE).expect("write preimage");
    let patch = write_patch(dir.path(), patch_body);

    let error = apply_patch_to_dir(dir.path(), &patch).expect_err("should not apply");
    assert!(error.to_string().contains("no binary data"), "error: {error}");
    assert!(from.exists(), "the source must stay in place");
    assert!(!dir.path().join("new.bin").exists(), "nothing should be moved");
}
