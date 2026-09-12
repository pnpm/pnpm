use super::{
    BINDING_GYP_DELETE_PATCH, BINDING_GYP_PATCH, IS_POSITIVE_INDEX_JS,
    IS_POSITIVE_INDEX_JS_PATCHED, IS_POSITIVE_PATCH, MANIFEST, MANIFEST_PATCH,
    MANIFEST_SECOND_PATCH, PatchApplyError, applied_hunk, apply_patch_to_dir, assert_eq, fs,
    preview_patch, tempdir, text_block, text_block_fnl, write_patch,
};

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

#[test]
fn applies_a_zero_context_insertion_at_the_start_of_the_file() {
    let original = text_block_fnl! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -0,0 +1 @@"
        "+added"
    };
    let expected = text_block_fnl! {
        "added"
        "one"
        "two"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_to_an_empty_file() {
    let hunk = text_block_fnl! {
        "@@ -0,0 +1 @@"
        "+added"
    };
    assert_eq!(applied_hunk("", hunk), "added\n");
}

#[test]
fn applies_a_multi_line_zero_context_insertion_in_the_middle_of_the_file() {
    let original = text_block_fnl! {
        "one"
        "two"
        "three"
        "four"
    };
    let hunk = text_block_fnl! {
        "@@ -2,0 +3,2 @@"
        "+first"
        "+second"
    };
    let expected = text_block_fnl! {
        "one"
        "two"
        "first"
        "second"
        "three"
        "four"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_at_the_end_of_the_file() {
    let original = text_block_fnl! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -2,0 +3 @@"
        "+added"
    };
    let expected = text_block_fnl! {
        "one"
        "two"
        "added"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_to_a_crlf_file() {
    let original = concat!("one\r\n", "two\r\n");
    let hunk = text_block_fnl! {
        "@@ -1,0 +2 @@"
        "+added"
    };
    let expected = concat!("one\r\n", "added\n", "two\r\n");
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_to_a_file_without_a_final_newline() {
    let original = text_block! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -1,0 +2 @@"
        "+added"
    };
    let expected = text_block! {
        "one"
        "added"
        "two"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_that_ends_the_file_without_a_newline() {
    let original = text_block_fnl! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -2,0 +3 @@"
        "+added"
        r"\ No newline at end of file"
    };
    let expected = text_block! {
        "one"
        "two"
        "added"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_without_a_newline_to_an_empty_file() {
    let hunk = text_block_fnl! {
        "@@ -0,0 +1 @@"
        "+added"
        r"\ No newline at end of file"
    };
    assert_eq!(applied_hunk("", hunk), "added");
}

/// A file rewritten twice is one file, and a delete costs a lookup rather
/// than a scan of everything written before it.
#[test]
fn previews_each_written_path_once() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), &format!("{MANIFEST_PATCH}{MANIFEST_SECOND_PATCH}"));

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    assert_eq!(preview.written_paths, ["package.json"]);
}

/// A file a patch adds can be a build trigger of its own, so the caller
/// needs the created paths as well as the manifest.
#[test]
fn previews_the_paths_a_patch_creates() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), BINDING_GYP_PATCH);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    assert_eq!(preview.manifest, None);
    assert_eq!(preview.written_paths, ["binding.gyp"]);
}

/// A build trigger a patch writes and then removes is not one the
/// package ends up with, so it must not hold the install for approval.
#[test]
fn previews_no_path_for_a_file_the_patch_creates_and_then_deletes() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch =
        write_patch(patch_dir.path(), &format!("{BINDING_GYP_PATCH}{BINDING_GYP_DELETE_PATCH}"));

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    assert!(preview.written_paths.is_empty(), "written_paths: {:?}", preview.written_paths);
    assert!(!patched.path().join("binding.gyp").exists(), "no binding.gyp remains");
}

/// The build phase owns the real apply. Applying twice to one slot is not
/// something a read-only question should risk.
#[test]
fn previewing_leaves_the_directory_untouched() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH);

    preview_patch(patched.path(), &patch).expect("preview must succeed");

    let on_disk = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(on_disk, MANIFEST);
}

#[test]
fn missing_patch_file_errors_patch_not_found() {
    let patched = tempdir().unwrap();
    let missing = patched.path().join("does-not-exist.patch");
    let err = apply_patch_to_dir(patched.path(), &missing).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::PatchNotFound { .. }), "got: {err:?}");
}

#[test]
fn missing_target_file_errors_patch_failed() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    let err = apply_patch_to_dir(patched.path(), &patch).expect_err("must fail");
    assert!(matches!(err, PatchApplyError::PatchFailed { .. }), "got: {err:?}");
}

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

/// `..` in a patch path is rejected — a malicious or
/// misconfigured patch must not be able to read/write/delete
/// outside `patched_dir`.
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
    assert_eq!(fs::read_to_string(&target).unwrap(), "i was here first\n");
}

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

#[test]
fn delete_that_carries_no_preimage_on_an_already_deleted_file_is_noop() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(
        patch_dir.path(),
        text_block_fnl! {
            "diff --git a/to-delete.txt b/to-delete.txt"
            "deleted file mode 100644"
            "index 8993..0000"
        },
    );

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    assert!(!patched.path().join("to-delete.txt").exists());
}

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
/// post-patch content must succeed (no-op), the same
/// [retry-with-reverse-in-dry-run](https://github.com/ds300/patch-package/blob/be4dfd77d9/src/applyPatches.ts)
/// idempotency `patch-package` provides. Triggers in practice when two
/// snapshots of the same
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

    let after = fs::read_to_string(patched.path().join("file.txt")).unwrap();
    assert_eq!(after, already_patched);

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
/// sibling snapshot that shares the same store inode. The rewrite gives
/// the target a fresh inode so the other hardlinks (the store, sibling
/// snapshots) keep the original content.
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

    assert_eq!(fs::read_to_string(&slot_file).unwrap(), IS_POSITIVE_INDEX_JS_PATCHED);
    assert_eq!(fs::read_to_string(&store_file).unwrap(), IS_POSITIVE_INDEX_JS);
    assert_ne!(
        fs::metadata(&slot_file).unwrap().ino(),
        fs::metadata(&store_file).unwrap().ino(),
        "slot must no longer share the store's inode after patching",
    );
}

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
