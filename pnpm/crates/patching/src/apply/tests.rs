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

mod files;

mod behavior;

mod manifests;
