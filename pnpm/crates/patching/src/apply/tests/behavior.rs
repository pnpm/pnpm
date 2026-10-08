use super::{
    FILE_TXT_PATCH_HEADER, IS_POSITIVE_PATCH, PatchApplyError, applied_hunk, apply_patch_to_dir,
    assert_eq, fs, tempdir, text_block_fnl, write_patch,
};

#[cfg(unix)]
use super::{IS_POSITIVE_INDEX_JS, IS_POSITIVE_INDEX_JS_PATCHED};

#[test]
fn applies_patch_with_crlf_line_endings() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("file.txt");
    fs::write(&target, "one\ntwo\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let hunk = text_block_fnl! {
        "@@ -1,2 +1,3 @@"
        " one"
        "+added"
        " two"
    };
    let crlf_patch = format!("{FILE_TXT_PATCH_HEADER}{hunk}").replace('\n', "\r\n");
    let patch = write_patch(patch_dir.path(), &crlf_patch);

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let after = fs::read_to_string(target).unwrap();
    assert_eq!(after, "one\nadded\r\ntwo\n");
}

/// `diffy` matches the `new file mode` value exactly, so its `\r` must be
/// gone before parsing. The created file keeps the patch's CRLF endings,
/// as it does on pnpm 11.
#[test]
fn applies_a_crlf_patch_that_creates_a_file() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/sass/ext/_true.scss b/sass/ext/_true.scss"
        "new file mode 100644"
        "index 0000000000000000000000000000000000000000..d3f18165a7c5601ad806777478fa35e2e601cfc0"
        "--- /dev/null"
        "+++ b/sass/ext/_true.scss"
        "@@ -0,0 +1,3 @@"
        "+@function error($error) {"
        "+  @error $error;"
        "+}"
    };
    let patch = write_patch(patch_dir.path(), &patch.replace('\n', "\r\n"));

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let after = fs::read_to_string(patched.path().join("sass/ext/_true.scss")).unwrap();
    assert_eq!(after, "@function error($error) {\r\n  @error $error;\r\n}\r\n");
}

#[test]
fn applies_a_crlf_patch_that_deletes_a_file() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("gone.txt");
    fs::write(&target, "one\ntwo\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/gone.txt b/gone.txt"
        "deleted file mode 100644"
        "index 1111111111111111111111111111111111111111..0000000000000000000000000000000000000000"
        "--- a/gone.txt"
        "+++ /dev/null"
        "@@ -1,2 +0,0 @@"
        "-one"
        "-two"
    };
    let patch = write_patch(patch_dir.path(), &patch.replace('\n', "\r\n"));

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    assert!(!target.exists());
}

/// A patch with no `---`/`+++` headers takes its paths from the
/// `diff --git` line, which is another value `diffy` matches exactly.
#[test]
fn applies_a_crlf_patch_that_only_changes_the_mode() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("run.sh");
    fs::write(&target, "#!/bin/sh\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/run.sh b/run.sh"
        "old mode 100644"
        "new mode 100755"
    };
    let patch = write_patch(patch_dir.path(), &patch.replace('\n', "\r\n"));

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "mode must be updated to executable");
    }
}

#[test]
fn applies_an_lf_patch_that_only_changes_the_mode() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("run.sh");
    fs::write(&target, "#!/bin/sh\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/run.sh b/run.sh"
        "old mode 100644"
        "new mode 100755"
    };
    let patch = write_patch(patch_dir.path(), patch);

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "mode must be updated to executable");
    }
}

#[cfg(unix)]
#[test]
fn applies_a_patch_that_removes_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let patched = tempdir().unwrap();
    let target = patched.path().join("script.sh");
    fs::write(&target, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();

    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/script.sh b/script.sh"
        "old mode 100755"
        "new mode 100644"
    };
    let patch = write_patch(patch_dir.path(), patch);

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let mode = fs::metadata(&target)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o644, "mode must be updated to non-executable");
}

#[test]
fn applies_a_patch_that_creates_an_executable_file() {
    let patched = tempdir().unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/bin/cli.sh b/bin/cli.sh"
        "new file mode 100755"
        "--- /dev/null"
        "+++ b/bin/cli.sh"
        "@@ -0,0 +1,2 @@"
        "+#!/bin/sh"
        "+echo hi"
    };
    let patch = write_patch(patch_dir.path(), patch);

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let target = patched.path().join("bin/cli.sh");
    assert_eq!(fs::read_to_string(&target).unwrap(), "#!/bin/sh\necho hi\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "created file must have executable mode");
    }
}

#[test]
fn mode_change_patch_is_idempotent() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("run.sh");
    fs::write(&target, "#!/bin/sh\n").unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = text_block_fnl! {
        "diff --git a/run.sh b/run.sh"
        "old mode 100644"
        "new mode 100755"
    };
    let patch = write_patch(patch_dir.path(), patch);

    apply_patch_to_dir(patched.path(), &patch).expect("first apply must succeed");
    apply_patch_to_dir(patched.path(), &patch).expect("re-apply must succeed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "mode must remain executable after re-apply");
    }
}

#[test]
fn applies_an_insertion_with_context_on_both_sides() {
    let original = text_block_fnl! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -1,2 +1,3 @@"
        " one"
        "+added"
        " two"
    };
    let expected = text_block_fnl! {
        "one"
        "added"
        "two"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

#[test]
fn applies_a_zero_context_insertion_after_the_first_line() {
    let original = text_block_fnl! {
        "one"
        "two"
    };
    let hunk = text_block_fnl! {
        "@@ -1,0 +2 @@"
        "+added"
    };
    let expected = text_block_fnl! {
        "one"
        "added"
        "two"
    };
    let after = applied_hunk(original, hunk);
    eprintln!("AFTER:\n{after}\n");
    assert_eq!(after, expected);
}

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

/// The patch context here is the U+FFFD chars themselves, so we
/// can construct a real patch that applies cleanly against the
/// lossy-decoded target.
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

/// `git diff --irreversible-delete`, which `pnpm patch-commit` runs,
/// writes the header of a deleted file without its preimage.
#[test]
fn applies_delete_that_carries_no_preimage() {
    let patched = tempdir().unwrap();
    let target = patched.path().join("to-delete.txt");
    fs::write(&target, "going away\n").unwrap();
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
    assert!(!target.exists(), "deleted target must be gone");
}

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

    let mode = fs::metadata(&target)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755, "executable bit must be preserved across the rewrite");
    assert_eq!(fs::read_to_string(&target).unwrap(), IS_POSITIVE_INDEX_JS_PATCHED);
}

/// `Modify` must NOT destroy the target when the rewrite can't finish.
/// Simulate the write failure by making the patched directory read-
/// only after staging the target: the temp file open fails with
/// `PermissionDenied`, and the original target must still be on disk
/// with its original content. Mirrors the crash-safety guarantee of
/// the atomic-replace pattern in
/// [`pnpm_lockfile::save_lockfile::write_atomic`](../../lockfile/src/save_lockfile.rs).
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
    let dir_mode = fs::metadata(patched.path())
        .unwrap()
        .permissions()
        .mode();
    fs::set_permissions(patched.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let err = apply_patch_to_dir(patched.path(), &patch);

    // Restore write perms so tempdir cleanup works.
    fs::set_permissions(patched.path(), fs::Permissions::from_mode(dir_mode)).unwrap();

    err.expect_err("apply must surface the write failure");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        IS_POSITIVE_INDEX_JS,
        "target must NOT be destroyed when the rewrite can't finish",
    );
}
