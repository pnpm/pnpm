use crate::apply::{PatchApplyError, apply_patch_to_dir};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;
use text_block_macros::text_block_fnl;

/// Mirrors the upstream `is-positive` fixture at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-restorer/test/fixtures/simple-with-patch/patches/is-positive.patch>:
/// a single-hunk Modify on `index.js`.
const IS_POSITIVE_PATCH: &str = "\
diff --git a/index.js b/index.js
index 8e020cac3320e72cb40e66b4c4573cc51c55e1e4..8be55d95c50a2a28e021e586ce5b928d9fea140e 100644
--- a/index.js
+++ b/index.js
@@ -7,3 +7,5 @@ module.exports = function (n) {

 \treturn n >= 0;
 };
+
+// a change
";

/// The upstream `is-positive@1.0.0` `index.js` body the patch
/// applies against. Six lines before the modified region, three
/// lines of context. Indentation uses tabs because the file
/// upstream's patch was authored against uses tabs.
const IS_POSITIVE_INDEX_JS: &str = "\
'use strict';
module.exports = function (n) {
\tif (typeof n !== 'number') {
\t\tthrow new TypeError('Expected a number');
\t}

\treturn n >= 0;
};
";

/// `is-positive`'s `index.js` after the upstream patch lands:
/// trailing blank line plus a `// a change` comment.
const IS_POSITIVE_INDEX_JS_PATCHED: &str = "\
'use strict';
module.exports = function (n) {
\tif (typeof n !== 'number') {
\t\tthrow new TypeError('Expected a number');
\t}

\treturn n >= 0;
};

// a change
";

fn write_patch(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("patch.patch");
    fs::write(&path, body).expect("write patch");
    path
}

/// Happy path: applying the upstream is-positive patch over
/// is-positive's actual `index.js` produces the expected output.
/// Mirrors upstream's
/// [`'install with patchedDependencies'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/test/install/patch.ts)
/// happy-path coverage at the unit level.
#[test]
fn applies_modify_against_existing_file() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("index.js"), IS_POSITIVE_INDEX_JS).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let after = fs::read_to_string(patched.path().join("index.js")).unwrap();
    assert_eq!(after, IS_POSITIVE_INDEX_JS_PATCHED);
}

/// `ERR_PNPM_PATCH_NOT_FOUND` for a missing patch file. Mirrors
/// upstream's
/// [`if (err.code === 'ENOENT') throw new PnpmError('PATCH_NOT_FOUND', ...)`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/apply-patch/src/index.ts).
#[test]
fn missing_patch_file_errors_patch_not_found() {
    let patched = tempdir().unwrap();
    let missing = patched.path().join("does-not-exist.patch");
    let err = apply_patch_to_dir(patched.path(), &missing).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::PatchNotFound { .. }), "got: {err:?}");
}

/// `ERR_PNPM_INVALID_PATCH` when the patch body can't be parsed —
/// e.g. truncated hunk header. Mirrors upstream's `catch (err) ...
/// throw new PnpmError('INVALID_PATCH', ...)` branch.
#[test]
fn malformed_patch_errors_invalid_patch() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("file.txt"), "hello\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ THIS IS NOT A HUNK HEADER\n",
    );

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::InvalidPatch { .. }), "got: {err:?}");
}

/// `ERR_PNPM_PATCH_FAILED` when a hunk can't be applied — e.g. the
/// context doesn't match the on-disk file. Mirrors upstream's
/// `if (!success) throw new PnpmError('PATCH_FAILED', ...)`.
#[test]
fn unmatching_hunk_errors_patch_failed() {
    let patched = tempdir().unwrap();
    // Write a file whose contents diverge from the patch's context
    // lines so diffy's apply can't locate the hunk.
    fs::write(patched.path().join("index.js"), "totally different contents\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::PatchFailed { .. }), "got: {err:?}");
}

/// `ERR_PNPM_PATCH_FAILED` when the target file is missing. The
/// patch refers to `index.js` but the patched dir is empty.
#[test]
fn missing_target_file_errors_patch_failed() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::PatchFailed { .. }), "got: {err:?}");
}

/// File creation: a patch that adds a brand-new file (`--- /dev/null`).
#[test]
fn applies_create_for_new_file() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/created.txt b/created.txt
new file mode 100644
--- /dev/null
+++ b/created.txt
@@ -0,0 +1,2 @@
+first line
+second line
",
    );

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    let after = fs::read_to_string(patched.path().join("created.txt")).unwrap();
    assert_eq!(after, "first line\nsecond line\n");
}

/// Target-file reads use lossy UTF-8 decoding, matching Node's
/// `fs.readFile(..., 'utf8')` and the patch-file reader. A target
/// file with stray non-UTF-8 bytes must NOT cause `Modify` to
/// fail with `InvalidData` — those bytes round-trip through
/// `String::from_utf8_lossy` as U+FFFD before
/// [`diffy::apply`] sees them.
///
/// The patch context here is the U+FFFD chars themselves, so we
/// can construct a real patch that applies cleanly against the
/// lossy-decoded target. Flagged by Copilot during review of
/// pacquet#427.
#[test]
fn modify_target_with_invalid_utf8_bytes_does_not_error() {
    let patched = tempdir().unwrap();
    // Three invalid UTF-8 bytes that lossy-decode to three U+FFFD
    // chars (`\xEF\xBF\xBD` each).
    fs::write(patched.path().join("blob.txt"), [0xffu8, 0xfeu8, 0xfdu8, b'\n']).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/blob.txt b/blob.txt
--- a/blob.txt
+++ b/blob.txt
@@ -1 +1,2 @@
 \u{fffd}\u{fffd}\u{fffd}
+added line
",
    );

    apply_patch_to_dir(patched.path(), &patch).expect("lossy decoding must allow apply");
    let after = fs::read_to_string(patched.path().join("blob.txt")).unwrap();
    assert!(after.contains("added line"), "got: {after:?}");
}

/// `..` in a patch path is rejected — a malicious or
/// misconfigured patch must not be able to read/write/delete
/// outside `patched_dir`. `CodeRabbit` flagged this as Critical
/// during review of pacquet#427.
#[test]
fn parent_dir_segment_in_modify_errors() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/../escape.txt b/../escape.txt
--- a/../escape.txt
+++ b/../escape.txt
@@ -1 +1 @@
-existing
+modified
",
    );
    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must reject ..");
    let PatchApplyError::PatchFailed { message, .. } = err else {
        panic!("expected PatchFailed, got: {err:?}");
    };
    assert!(message.contains("escapes target dir"), "got: {message}");
}

#[test]
fn parent_dir_segment_in_create_errors() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/../planted.txt b/../planted.txt
new file mode 100644
--- /dev/null
+++ b/../planted.txt
@@ -0,0 +1 @@
+pwned
",
    );
    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must reject ..");
    let PatchApplyError::PatchFailed { message, .. } = err else {
        panic!("expected PatchFailed, got: {err:?}");
    };
    assert!(message.contains("escapes target dir"), "got: {message}");
}

#[test]
fn parent_dir_segment_in_delete_errors() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/../victim.txt b/../victim.txt
deleted file mode 100644
--- a/../victim.txt
+++ /dev/null
@@ -1 +0,0 @@
-going away
",
    );
    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must reject ..");
    let PatchApplyError::PatchFailed { message, .. } = err else {
        panic!("expected PatchFailed, got: {err:?}");
    };
    assert!(message.contains("escapes target dir"), "got: {message}");
}

/// `Create` refuses to overwrite an existing file. Matches `patch`
/// and `git apply` semantics for `--- /dev/null` hunks. Flagged by
/// Copilot during review of pacquet#427.
#[test]
fn create_on_existing_file_errors() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("already-here.txt");
    fs::write(&target, "i was here first\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/already-here.txt b/already-here.txt
new file mode 100644
--- /dev/null
+++ b/already-here.txt
@@ -0,0 +1 @@
+overwriting
",
    );
    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must refuse overwrite");
    let PatchApplyError::PatchFailed { message, .. } = err else {
        panic!("expected PatchFailed, got: {err:?}");
    };
    assert!(message.contains("already exists"), "got: {message}");
    // The original file must be untouched.
    assert_eq!(fs::read_to_string(&target).unwrap(), "i was here first\n");
}

/// `Delete` validates hunks before unlinking. A stale patch (one
/// whose `-` lines don't match the actual file content) must NOT
/// silently remove the file. Flagged by Copilot during review of
/// pacquet#427.
#[test]
fn delete_with_mismatching_hunks_errors_without_unlinking() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("to-delete.txt");
    // The patch expects the file to contain "going away\n", but
    // the actual file diverges. A correct implementation must
    // refuse to delete.
    fs::write(&target, "actually different content\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/to-delete.txt b/to-delete.txt
deleted file mode 100644
--- a/to-delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-going away
",
    );
    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must refuse mismatch");
    assert!(matches!(err, PatchApplyError::PatchFailed { .. }), "got: {err:?}");
    assert!(target.exists(), "file must NOT be unlinked when the patch doesn't match");
}

/// File deletion: a patch that removes an existing file
/// (`+++ /dev/null`). The target is unlinked.
#[test]
fn applies_delete_for_removed_file() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("to-delete.txt");
    fs::write(&target, "going away\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/to-delete.txt b/to-delete.txt
deleted file mode 100644
--- a/to-delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-going away
",
    );

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    assert!(!target.exists(), "deleted target must be gone");
}

/// Reading the patch-file path itself can fail with errors other
/// than `NotFound` — here, the patch path is a directory rather
/// than a file. The classifier must surface this as `ReadPatchFile`
/// (not `PatchNotFound`).
#[cfg(unix)]
#[test]
fn read_patch_file_surfaces_non_not_found_error() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    // Pass the directory itself as the patch path. `fs::read` on a
    // directory returns `IsADirectory`, never `NotFound`.
    let err = apply_patch_to_dir(patched.path(), patch_dir.path())
        .expect_err("reading a directory as a patch file should fail");
    assert!(
        matches!(err, PatchApplyError::ReadPatchFile { .. }),
        "expected ReadPatchFile error, got {err:?}",
    );
}

/// A delete patch that does not consume the entire file leaves
/// non-empty content behind after `diffy::apply`. The implementation
/// must refuse to remove the file in that case rather than silently
/// dropping the unpatched tail. Tests the `if !after.is_empty()`
/// guard in the Delete arm.
#[test]
fn delete_patch_leaving_non_empty_result_errors_without_unlinking() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("partial.txt");
    // Two lines on disk. The patch below claims to delete the file
    // but only removes the first line — `diffy::apply` applies the
    // single hunk and returns the unpatched tail (`stay\n`), which
    // is non-empty, so the implementation must error.
    fs::write(&target, "going away\nstay\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        text_block_fnl! {
            "diff --git a/partial.txt b/partial.txt"
            "deleted file mode 100644"
            "--- a/partial.txt"
            "+++ /dev/null"
            "@@ -1 +0,0 @@"
            "-going away"
        },
    );

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must refuse partial delete");
    match err {
        PatchApplyError::PatchFailed { message, .. } => {
            assert!(message.contains("non-empty"), "got: {message:?}");
        }
        other => panic!("expected PatchFailed, got {other:?}"),
    }
    assert!(target.exists(), "target must NOT be unlinked when content remains");
}

/// Rename and copy file operations are not yet supported. A patch
/// that contains one of these headers must error cleanly via
/// `PatchFailed`, not silently no-op.
#[test]
fn rename_operation_errors_as_unsupported() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("from.txt"), "hello\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        text_block_fnl! {
            "diff --git a/from.txt b/to.txt"
            "similarity index 100%"
            "rename from from.txt"
            "rename to to.txt"
        },
    );

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("rename must error");
    match err {
        PatchApplyError::PatchFailed { message, .. } => {
            assert!(
                message.contains("rename/copy operations in patches are not yet supported"),
                "got: {message:?}",
            );
        }
        other => panic!("expected PatchFailed, got {other:?}"),
    }
}

/// `Create` resolves the parent dir via `create_dir_all`. When the
/// parent path is a regular file rather than a directory, that call
/// fails and must surface as `PatchFailed` with a `create parent of`
/// prefix.
#[cfg(unix)]
#[test]
fn create_with_unwritable_parent_path_errors() {
    let patched = tempdir().unwrap();
    // Plant a regular file where the patch wants to create the
    // nested target's parent directory.
    fs::write(patched.path().join("blocker"), b"not a dir").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        text_block_fnl! {
            "diff --git a/blocker/nested.txt b/blocker/nested.txt"
            "new file mode 100644"
            "--- /dev/null"
            "+++ b/blocker/nested.txt"
            "@@ -0,0 +1 @@"
            "+hi"
        },
    );

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("create_dir_all must fail");
    match err {
        PatchApplyError::PatchFailed { message, .. } => {
            assert!(message.contains("create parent of"), "got: {message:?}");
        }
        other => panic!("expected PatchFailed, got {other:?}"),
    }
}

/// Re-applying a Modify patch over a file that already contains the
/// post-patch content must succeed (no-op), matching upstream
/// `@pnpm/patch-package`'s
/// [retry-with-reverse-in-dry-run](https://github.com/ds300/patch-package/blob/master/src/applyPatches.ts)
/// idempotency. Triggers in practice when two snapshots of the same
/// patched package share a hardlinked store file: the first apply
/// mutates the store inode through the hardlink, so the second
/// snapshot's apply sees already-patched content and would otherwise
/// fail with "error applying hunk `#1`". Reported against pacquet's
/// configDependencies preview engine for `msw@2.12.14`.
///
/// The patch substitutes one line (`old` → `new`) so forward apply
/// against the already-patched file fails to find the `-old` context
/// line — the case that exercises the reverse-apply idempotency
/// branch. A purely additive patch wouldn't reach the reverse
/// branch: diffy's fuzz matching shifts the insertion point and
/// double-applies the addition, producing duplicate content.
#[test]
fn modify_on_already_patched_file_is_noop() {
    let original = "alpha\nbravo\nold\ndelta\necho\n";
    let already_patched = "alpha\nbravo\nnew\ndelta\necho\n";
    let patch_text = "\
diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,5 +1,5 @@
 alpha
 bravo
-old
+new
 delta
 echo
";

    let patched = tempdir().unwrap();
    fs::write(patched.path().join("file.txt"), already_patched).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), patch_text);

    apply_patch_to_dir(patched.path(), &patch).expect("re-apply must succeed");

    // Content stays at the post-patch state — we didn't double-patch.
    let after = fs::read_to_string(patched.path().join("file.txt")).unwrap();
    assert_eq!(after, already_patched);

    // Sanity: a fresh file at the original state still applies normally.
    let fresh = tempdir().unwrap();
    fs::write(fresh.path().join("file.txt"), original).unwrap();
    apply_patch_to_dir(fresh.path(), &patch).expect("fresh apply must succeed");
    assert_eq!(fs::read_to_string(fresh.path().join("file.txt")).unwrap(), already_patched);
}

/// `Modify` must NOT mutate any other hardlink pointing at the same
/// inode. Pacquet's import layer hardlinks files from the content-
/// addressable store into `node_modules/.pnpm/<slot>/node_modules/<pkg>`,
/// so a plain truncating `fs::write` on the patched target would
/// silently corrupt the store copy and leak patched content into every
/// sibling snapshot that shares the same store inode. The fix is to
/// unlink the target before writing — the rewritten file gets a fresh
/// inode and the other hardlinks (the store, sibling snapshots) keep
/// the original content. This was the root cause of the
/// `error applying hunk #1` failure reported against pacquet's
/// configDependencies preview engine for `msw@2.12.14`: worker A
/// patched its slot's hardlink, mutating the store, and worker B
/// then read already-patched content from its own hardlinked slot.
#[cfg(unix)]
#[test]
fn modify_does_not_mutate_hardlinked_store_file() {
    use std::os::unix::fs::MetadataExt;

    // Simulate the store: one canonical file plus a hardlink into a
    // package slot. They share an inode the way pacquet's link_file
    // arranges it on filesystems where reflink isn't available.
    let store = tempdir().unwrap();
    let store_file = store.path().join("index.js");
    fs::write(&store_file, IS_POSITIVE_INDEX_JS).unwrap();

    let patched = tempdir().unwrap();
    let slot_file = patched.path().join("index.js");
    fs::hard_link(&store_file, &slot_file).unwrap();
    assert_eq!(
        fs::metadata(&slot_file).unwrap().ino(),
        fs::metadata(&store_file).unwrap().ino(),
        "test setup: slot must share inode with store",
    );

    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);
    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    // Slot has patched content.
    assert_eq!(fs::read_to_string(&slot_file).unwrap(), IS_POSITIVE_INDEX_JS_PATCHED);
    // Store copy is untouched.
    assert_eq!(fs::read_to_string(&store_file).unwrap(), IS_POSITIVE_INDEX_JS);
    // And the slot points at a new inode now — the unlink broke the
    // shared-inode link cleanly.
    assert_ne!(
        fs::metadata(&slot_file).unwrap().ino(),
        fs::metadata(&store_file).unwrap().ino(),
        "slot must no longer share the store's inode after patching",
    );
}

/// `Modify` must preserve the target file's mode. Without an explicit
/// `set_permissions` after the unlink-then-write, the rewrite would
/// take its mode from the process umask and silently drop the
/// executable bit on patched shebang scripts under `bin/`.
#[cfg(unix)]
#[test]
fn modify_preserves_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let patched = tempdir().unwrap();
    let target = patched.path().join("index.js");
    fs::write(&target, IS_POSITIVE_INDEX_JS).unwrap();
    // Mark the script executable, the way an npm-published bin entry
    // would be.
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();

    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);
    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "executable bit must be preserved across the rewrite");
    assert_eq!(fs::read_to_string(&target).unwrap(), IS_POSITIVE_INDEX_JS_PATCHED);
}

/// `Modify` must NOT destroy the target when the rewrite can't finish.
/// Simulate the write failure by making the patched directory read-
/// only after staging the target: the temp file open fails with
/// `PermissionDenied`, and the original target must still be on disk
/// with its original content. Mirrors the crash-safety guarantee of
/// the atomic-replace pattern in
/// [`pacquet_lockfile::save_lockfile::write_atomic`](../../lockfile/src/save_lockfile.rs).
/// `CodeRabbit` flagged the prior `unlink → write` ordering as a
/// data-loss risk during review of pnpm/pnpm#11782.
#[cfg(unix)]
#[test]
fn modify_does_not_destroy_target_on_write_failure() {
    use std::os::unix::fs::PermissionsExt;

    let patched = tempdir().unwrap();
    let target = patched.path().join("index.js");
    fs::write(&target, IS_POSITIVE_INDEX_JS).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    // Make the directory read-only so the sibling temp file open in
    // `write_atomic_with_mode` fails with `PermissionDenied`.
    let dir_mode = fs::metadata(patched.path()).unwrap().permissions().mode();
    fs::set_permissions(patched.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let err = apply_patch_to_dir(patched.path(), &patch);

    // Restore write perms so tempdir cleanup works.
    fs::set_permissions(patched.path(), fs::Permissions::from_mode(dir_mode)).unwrap();

    err.expect_err("apply must surface the write failure");
    // The crucial invariant: the original target survives untouched.
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        IS_POSITIVE_INDEX_JS,
        "target must NOT be destroyed when the rewrite can't finish",
    );
}

/// Re-applying a Create patch over a file that already exists with the
/// expected post-patch content is treated as no-op. A genuine
/// pre-existing file with different content still errors (covered by
/// [`create_on_existing_file_errors`]).
#[test]
fn create_on_already_created_file_with_matching_content_is_noop() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("created.txt");
    fs::write(&target, "first line\nsecond line\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/created.txt b/created.txt
new file mode 100644
--- /dev/null
+++ b/created.txt
@@ -0,0 +1,2 @@
+first line
+second line
",
    );

    apply_patch_to_dir(patched.path(), &patch).expect("re-apply must succeed");
    assert_eq!(fs::read_to_string(&target).unwrap(), "first line\nsecond line\n");
}

/// Re-applying a Delete patch when the target is already gone is a
/// no-op. Mirrors upstream's `@pnpm/patch-package` reverse-dry-run
/// idempotency: re-running `pnpm install` after a previously
/// successful delete must not error.
#[test]
fn delete_on_already_deleted_file_is_noop() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        "\
diff --git a/to-delete.txt b/to-delete.txt
deleted file mode 100644
--- a/to-delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-going away
",
    );

    apply_patch_to_dir(patched.path(), &patch).expect("re-apply must succeed");
    assert!(!patched.path().join("to-delete.txt").exists());
}
