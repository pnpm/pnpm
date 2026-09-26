use super::{
    IS_POSITIVE_INDEX_JS, IS_POSITIVE_PATCH, MANIFEST, MANIFEST_CREATE_PATCH,
    MANIFEST_DELETE_PATCH, MANIFEST_PATCH, MANIFEST_PATCH_MIXED_CASE,
    MANIFEST_PATCH_WITH_DOT_SEGMENT, MANIFEST_SECOND_PATCH, apply_patch_to_dir, assert_eq, fs,
    preview_patch, tempdir, write_patch,
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
