use crate::apply::{PatchApplyError, apply_patch_to_dir, preview_patch};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;
use text_block_macros::{text_block, text_block_fnl};

/// An `is-positive` patch: a single-hunk Modify on `index.js`.
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

/// The `is-positive@1.0.0` `index.js` body the patch applies against.
/// Six lines before the modified region, three lines of context.
/// Indentation uses tabs because the file the patch was authored
/// against uses tabs.
const IS_POSITIVE_INDEX_JS: &str = "\
'use strict';
module.exports = function (n) {
\tif (typeof n !== 'number') {
\t\tthrow new TypeError('Expected a number');
\t}

\treturn n >= 0;
};
";

/// `is-positive`'s `index.js` after the patch lands: trailing blank
/// line plus a `// a change` comment.
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

/// The single-file header [`applied_hunk`] prepends to every hunk it is given.
const FILE_TXT_PATCH_HEADER: &str = text_block_fnl! {
    "diff --git a/file.txt b/file.txt"
    "--- a/file.txt"
    "+++ b/file.txt"
};

/// Apply a patch whose sole file operation is `hunk` against a `file.txt`
/// holding `original`, and return what the file holds afterwards.
/// [`FILE_TXT_PATCH_HEADER`] is supplied here so a caller only spells out the
/// coordinates under test.
fn applied_hunk(original: &str, hunk: &str) -> String {
    let patched = tempdir().unwrap();
    let target = patched.path().join("file.txt");
    fs::write(&target, original).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), &format!("{FILE_TXT_PATCH_HEADER}{hunk}"));

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    fs::read_to_string(&target).unwrap()
}

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

/// A manifest with no install scripts, and a patch that adds one.
const MANIFEST: &str = text_block_fnl! {
    "{"
    r#"  "name": "is-positive","#
    r#"  "scripts": {"#
    r#"    "test": "node test.js""#
    "  }"
    "}"
};

const MANIFEST_PATCH: &str = text_block_fnl! {
    "diff --git a/package.json b/package.json"
    "--- a/package.json"
    "+++ b/package.json"
    "@@ -2,5 +2,6 @@"
    r#"   "name": "is-positive","#
    r#"   "scripts": {"#
    r#"-    "test": "node test.js""#
    r#"+    "test": "node test.js","#
    r#"+    "postinstall": "node postinstall.js""#
    "   }"
    " }"
};

const BINDING_GYP_PATCH: &str = text_block_fnl! {
    "diff --git a/binding.gyp b/binding.gyp"
    "new file mode 100644"
    "--- /dev/null"
    "+++ b/binding.gyp"
    "@@ -0,0 +1 @@"
    "+{}"
};

/// [`MANIFEST_PATCH`] with a `.` segment in its headers, which
/// `apply_patch_to_dir` resolves away.
const MANIFEST_PATCH_WITH_DOT_SEGMENT: &str = text_block_fnl! {
    "diff --git a/./package.json b/./package.json"
    "--- a/./package.json"
    "+++ b/./package.json"
    "@@ -2,5 +2,6 @@"
    r#"   "name": "is-positive","#
    r#"   "scripts": {"#
    r#"-    "test": "node test.js""#
    r#"+    "test": "node test.js","#
    r#"+    "postinstall": "node postinstall.js""#
    "   }"
    " }"
};

/// Removes the manifest [`MANIFEST`] holds, so a later record can write a
/// fresh one.
const MANIFEST_DELETE_PATCH: &str = text_block_fnl! {
    "diff --git a/package.json b/package.json"
    "deleted file mode 100644"
    "--- a/package.json"
    "+++ /dev/null"
    "@@ -1,6 +0,0 @@"
    "-{"
    r#"-  "name": "is-positive","#
    r#"-  "scripts": {"#
    r#"-    "test": "node test.js""#
    "-  }"
    "-}"
};

/// Writes a manifest that declares an install script.
const MANIFEST_CREATE_PATCH: &str = text_block_fnl! {
    "diff --git a/package.json b/package.json"
    "new file mode 100644"
    "--- /dev/null"
    "+++ b/package.json"
    "@@ -0,0 +1,6 @@"
    "+{"
    r#"+  "name": "is-positive","#
    r#"+  "scripts": {"#
    r#"+    "postinstall": "node postinstall.js""#
    "+  }"
    "+}"
};

/// [`MANIFEST_PATCH`] with a differently cased header, which reaches the
/// same file wherever names are not case-sensitive.
const MANIFEST_PATCH_MIXED_CASE: &str = text_block_fnl! {
    "diff --git a/Package.json b/Package.json"
    "--- a/Package.json"
    "+++ b/Package.json"
    "@@ -2,5 +2,6 @@"
    r#"   "name": "is-positive","#
    r#"   "scripts": {"#
    r#"-    "test": "node test.js""#
    r#"+    "test": "node test.js","#
    r#"+    "postinstall": "node postinstall.js""#
    "   }"
    " }"
};

/// A second `package.json` record, applying on top of [`MANIFEST_PATCH`]'s
/// output.
const MANIFEST_SECOND_PATCH: &str = text_block_fnl! {
    "diff --git a/package.json b/package.json"
    "--- a/package.json"
    "+++ b/package.json"
    "@@ -3,5 +3,6 @@"
    r#"   "scripts": {"#
    r#"     "test": "node test.js","#
    r#"-    "postinstall": "node postinstall.js""#
    r#"+    "postinstall": "node postinstall.js","#
    r#"+    "preinstall": "node preinstall.js""#
    "   }"
    " }"
};

/// Removes the `binding.gyp` [`BINDING_GYP_PATCH`] creates.
const BINDING_GYP_DELETE_PATCH: &str = text_block_fnl! {
    "diff --git a/binding.gyp b/binding.gyp"
    "deleted file mode 100644"
    "--- a/binding.gyp"
    "+++ /dev/null"
    "@@ -1 +0,0 @@"
    "-{}"
};

/// The preview must read what the real apply writes, or the build gate
/// and the build disagree about the same package.
#[test]
fn previews_the_manifest_a_patch_would_write() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    let applied = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.as_deref(), Some(applied.as_str()));
    assert_eq!(preview.written_paths, ["package.json"]);
}

/// A patch may carry several records for one file, and the real apply
/// feeds each the previous one's output.
#[test]
fn previews_repeated_manifest_records_in_order() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), &format!("{MANIFEST_PATCH}{MANIFEST_SECOND_PATCH}"));

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    let applied = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.as_deref(), Some(applied.as_str()));
    assert!(applied.contains("preinstall"), "applied: {applied}");
}

/// The preview has to name a file the way the apply resolves it, or a
/// patch spelled with a `.` segment would edit the manifest unseen.
#[test]
fn previews_a_manifest_named_through_a_dot_segment() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH_WITH_DOT_SEGMENT);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    let applied = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.as_deref(), Some(applied.as_str()));
    assert_eq!(preview.written_paths, ["package.json"]);
}

/// A patch may replace the manifest outright rather than editing it, and
/// the script the replacement declares is build work like any other.
#[test]
fn previews_a_manifest_the_patch_deletes_and_writes_again() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch =
        write_patch(patch_dir.path(), &format!("{MANIFEST_DELETE_PATCH}{MANIFEST_CREATE_PATCH}"));

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");
    let applied = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.as_deref(), Some(applied.as_str()));
    assert!(applied.contains("postinstall"), "applied: {applied}");
    assert_eq!(preview.written_paths, ["package.json"]);
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

/// Where two spellings name one file, the applier writes the real manifest,
/// so the preview has to read it as the manifest too.
#[cfg(unix)]
#[test]
fn previews_a_manifest_reached_through_another_spelling_of_its_name() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let other_spelling = patched.path().join("Package.json");
    // A case-insensitive volume already resolves both spellings to the one
    // manifest, which is the condition under test. Where names are
    // case-sensitive, a symlink is what makes two names one file.
    if !other_spelling.exists() {
        std::os::unix::fs::symlink("package.json", &other_spelling).unwrap();
    }
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH_MIXED_CASE);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    let manifest = preview.manifest.expect("the patch reaches the manifest");
    assert!(manifest.contains("postinstall"), "manifest: {manifest:?}");
}

/// Where the two spellings are two files, the applier edits the other one,
/// and calling it the manifest would gate a build the package never gains.
#[test]
fn previews_no_manifest_for_a_differently_cased_file_of_its_own() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    fs::write(patched.path().join("Package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH_MIXED_CASE);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    // On a case-insensitive volume the two writes above are one file, and
    // the patch does reach the manifest.
    let separate_files = fs::read_to_string(patched.path().join("Package.json")).unwrap()
        == fs::read_to_string(patched.path().join("package.json")).unwrap()
        && fs::canonicalize(patched.path().join("Package.json")).unwrap()
            != fs::canonicalize(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.is_none(), separate_files);
}

/// The global virtual store keeps a slot across installs, so the preview
/// can meet a directory an earlier install already patched.
#[test]
fn previews_an_already_patched_manifest() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), MANIFEST).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH);
    apply_patch_to_dir(patched.path(), &patch).expect("apply must succeed");

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    let applied = fs::read_to_string(patched.path().join("package.json")).unwrap();
    assert_eq!(preview.manifest.as_deref(), Some(applied.as_str()));
}

/// A manifest a publisher wrote with a byte order mark still has to reach
/// the caller intact, since the caller parses what it gets back.
#[test]
fn previews_a_manifest_that_starts_with_a_byte_order_mark() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("package.json"), format!("\u{feff}{MANIFEST}")).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), MANIFEST_PATCH);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    let manifest = preview.manifest.expect("the patch modifies the manifest");
    assert!(manifest.starts_with('\u{feff}'), "manifest: {manifest:?}");
    assert!(manifest.contains("postinstall"), "manifest: {manifest:?}");
}

#[test]
fn previews_nothing_when_the_patch_leaves_the_manifest_alone() {
    let patched = tempdir().unwrap();
    fs::write(patched.path().join("index.js"), IS_POSITIVE_INDEX_JS).unwrap();
    let patch_dir = tempdir().unwrap();
    let patch = write_patch(patch_dir.path(), IS_POSITIVE_PATCH);

    let preview = preview_patch(patched.path(), &patch).expect("preview must succeed");

    assert_eq!(preview.manifest, None);
    assert_eq!(preview.written_paths, ["index.js"]);
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
    let dir_mode = fs::metadata(patched.path()).unwrap().permissions().mode();
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
