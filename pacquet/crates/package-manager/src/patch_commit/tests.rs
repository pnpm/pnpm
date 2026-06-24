use super::{
    DiffTempFile, PatchCommitError, PatchCommitFs, PkgFilesForDiff, RealPatchCommitFs,
    diff_folders, normalize_diff_output, prepare_pkg_files_for_diff,
    prepare_pkg_files_for_diff_with_fs, remove_existing_temp_dir_with_fs, temporary_filtered_dir,
};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{cell::Cell, fs, io, path::Path};
use tempfile::tempdir;

#[test]
fn patch_commit_diff_dirs_strips_absolute_temp_paths() {
    let before = tempdir().expect("before dir");
    let after = tempdir().expect("after dir");
    fs::write(before.path().join("index.js"), "module.exports = false\n").unwrap();
    fs::write(after.path().join("index.js"), "module.exports = true\n").unwrap();

    let diff = diff_folders(before.path(), after.path()).expect("diff dirs");

    assert!(diff.contains("diff --git a/index.js b/index.js"), "diff: {diff}");
    assert!(!diff.contains(&before.path().display().to_string()), "diff: {diff}");
    assert!(!diff.contains(&after.path().display().to_string()), "diff: {diff}");
}

#[test]
fn patch_commit_diff_dirs_filters_ds_store_diffs() {
    let before = tempdir().expect("before dir");
    let after = tempdir().expect("after dir");
    fs::create_dir_all(before.path().join("sub")).unwrap();
    fs::create_dir_all(after.path().join("sub")).unwrap();
    fs::write(before.path().join(".DS_Store"), "before").unwrap();
    fs::write(after.path().join(".DS_Store"), "after").unwrap();
    fs::write(before.path().join("sub/.DS_Store"), "before").unwrap();
    fs::write(after.path().join("sub/.DS_Store"), "after").unwrap();

    let diff = diff_folders(before.path(), after.path()).expect("diff dirs");

    assert!(!diff.contains(".DS_Store"), "diff: {diff}");
}

#[test]
fn patch_commit_diff_dirs_keeps_files_whose_names_contain_ds_store() {
    let before = tempdir().expect("before dir");
    let after = tempdir().expect("after dir");
    fs::write(before.path().join("foo.DS_Store.js"), "before\n").unwrap();
    fs::write(after.path().join("foo.DS_Store.js"), "after\n").unwrap();

    let diff = diff_folders(before.path(), after.path()).expect("diff dirs");

    assert!(diff.contains("foo.DS_Store.js"), "diff: {diff}");
}

#[test]
fn patch_commit_diff_dirs_accepts_paths_that_start_with_dash() {
    let tmp = tempdir().expect("temp dir");
    let before = tmp.path().join("-before");
    let after = tmp.path().join("-after");
    fs::create_dir(&before).unwrap();
    fs::create_dir(&after).unwrap();
    fs::write(before.join("index.js"), "before\n").unwrap();
    fs::write(after.join("index.js"), "after\n").unwrap();

    let diff = diff_folders(&before, &after).expect("diff dirs");

    assert!(diff.contains("diff --git a/index.js b/index.js"), "diff: {diff}");
}

#[cfg(unix)]
#[test]
fn patch_commit_diff_temp_files_are_owner_only() {
    let temp_file = DiffTempFile::new("stdout").expect("diff temp file");

    let mode = fs::metadata(&temp_file.path).expect("diff temp metadata").permissions().mode();

    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_honors_files_field() {
    let edit_dir = tempdir().expect("edit dir");
    fs::write(
        edit_dir.path().join("package.json"),
        r#"{"name":"pkg","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(edit_dir.path().join("index.js"), "included\n").unwrap();
    fs::write(edit_dir.path().join("ignore.txt"), "excluded\n").unwrap();

    let filtered = prepare_pkg_files_for_diff(edit_dir.path()).expect("prepare files");
    let PkgFilesForDiff::Temporary(path) = filtered else {
        panic!("files field should require a temporary filtered dir");
    };

    assert!(path.join("index.js").is_file());
    assert!(path.join("package.json").is_file());
    assert!(!path.join("ignore.txt").exists());

    fs::remove_dir_all(path).unwrap();
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_creates_nested_parent_dirs() {
    let edit_dir = tempdir().expect("edit dir");
    fs::write(
        edit_dir.path().join("package.json"),
        r#"{"name":"pkg","version":"1.0.0","files":["lib/index.js"]}"#,
    )
    .unwrap();
    fs::create_dir(edit_dir.path().join("lib")).unwrap();
    fs::write(edit_dir.path().join("lib/index.js"), "included\n").unwrap();
    fs::write(edit_dir.path().join("ignore.txt"), "excluded\n").unwrap();

    let filtered = prepare_pkg_files_for_diff(edit_dir.path()).expect("prepare files");
    let PkgFilesForDiff::Temporary(path) = filtered else {
        panic!("files field should require a temporary filtered dir");
    };

    assert_eq!(fs::read_to_string(path.join("lib/index.js")).unwrap(), "included\n");
    assert!(!path.join("ignore.txt").exists());

    fs::remove_dir_all(path).unwrap();
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_reports_nested_parent_create_errors() {
    let edit_dir = tempdir().expect("edit dir");
    fs::write(
        edit_dir.path().join("package.json"),
        r#"{"name":"pkg","version":"1.0.0","files":["lib/index.js"]}"#,
    )
    .unwrap();
    fs::create_dir(edit_dir.path().join("lib")).unwrap();
    fs::write(edit_dir.path().join("lib/index.js"), "included\n").unwrap();
    fs::write(edit_dir.path().join("ignore.txt"), "excluded\n").unwrap();

    let err = prepare_pkg_files_for_diff_with_fs(
        edit_dir.path(),
        &CreateDirErrorFs { fail_on: "lib", created_dirs: Cell::new(0) },
    )
    .expect_err("nested parent creation should fail");

    assert!(matches!(err, PatchCommitError::CreateTempDir { .. }));
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_reports_hard_link_errors() {
    let edit_dir = tempdir().expect("edit dir");
    fs::write(
        edit_dir.path().join("package.json"),
        r#"{"name":"pkg","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(edit_dir.path().join("index.js"), "included\n").unwrap();
    fs::write(edit_dir.path().join("ignore.txt"), "excluded\n").unwrap();

    let err = prepare_pkg_files_for_diff_with_fs(edit_dir.path(), &HardLinkErrorFs)
        .expect_err("hard link creation should fail");

    assert!(matches!(err, PatchCommitError::LinkFile { .. }));
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_rejects_packlist_paths_that_escape_source() {
    let root = tempdir().expect("root dir");
    let edit_dir = root.path().join("edit");
    fs::create_dir(&edit_dir).expect("create edit dir");
    fs::write(
        edit_dir.join("package.json"),
        r#"{"name":"pkg","version":"1.0.0","main":"../secret.js","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(edit_dir.join("index.js"), "included\n").unwrap();
    fs::write(root.path().join("secret.js"), "outside\n").unwrap();

    let err = prepare_pkg_files_for_diff(&edit_dir).expect_err("escaping packlist path");

    assert!(matches!(err, PatchCommitError::InvalidPackageFilePath { .. }));
}

#[test]
fn patch_commit_prepare_pkg_files_for_diff_uses_filtered_view_when_packlist_matches_all_files() {
    let edit_dir = tempdir().expect("edit dir");
    fs::write(edit_dir.path().join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#).unwrap();
    fs::write(edit_dir.path().join("index.js"), "included\n").unwrap();

    let filtered = prepare_pkg_files_for_diff(edit_dir.path()).expect("prepare files");
    let PkgFilesForDiff::Temporary(path) = filtered else {
        panic!("package files should be prepared in a temporary filtered dir");
    };

    assert_eq!(path, temporary_filtered_dir(edit_dir.path()));
    assert_eq!(fs::read_to_string(path.join("index.js")).unwrap(), "included\n");
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn patch_commit_diff_dirs_returns_empty_for_equal_dirs() {
    let before = tempdir().expect("before dir");
    let after = tempdir().expect("after dir");
    fs::write(before.path().join("index.js"), "module.exports = true\n").unwrap();
    fs::write(after.path().join("index.js"), "module.exports = true\n").unwrap();

    let diff = diff_folders(before.path(), after.path()).expect("diff dirs");

    assert_eq!(diff, "");
}

#[test]
fn patch_commit_diff_normalization_leaves_hunk_content_untouched() {
    let diff = "\
diff --git a/tmp/before/index.js b/tmp/after/index.js
index 123..456 100644
--- a/tmp/before/index.js
+++ b/tmp/after/index.js
@@ -1 +1 @@
-console.log(\"/tmp/before/ must stay in content\")
+console.log(\"/tmp/after/ must stay in content\")
--- /tmp/before/ also stays when content resembles a file header
+++ /tmp/after/ also stays when content resembles a file header
";

    let normalized = normalize_diff_output(diff, "/tmp/before", "/tmp/after");

    assert!(normalized.contains("diff --git a/index.js b/index.js"), "{normalized}");
    assert!(normalized.contains("--- a/index.js"), "{normalized}");
    assert!(normalized.contains("+++ b/index.js"), "{normalized}");
    assert!(normalized.contains(r#"-console.log("/tmp/before/ must stay in content")"#));
    assert!(normalized.contains(r#"+console.log("/tmp/after/ must stay in content")"#));
    assert!(normalized.contains("--- /tmp/before/ also stays"));
    assert!(normalized.contains("+++ /tmp/after/ also stays"));
}

#[test]
fn patch_commit_diff_dirs_reports_git_errors() {
    let tmp = tempdir().expect("temp dir");

    let err = diff_folders(&tmp.path().join("missing-a"), &tmp.path().join("missing-b"))
        .expect_err("missing dirs should fail");

    assert!(matches!(err, PatchCommitError::DiffFailed { .. }));
}

#[test]
fn patch_commit_remove_existing_temp_dir_removes_existing_dir() {
    let tmp = tempdir().expect("temp dir");
    let temp_dir = tmp.path().join("filtered");
    fs::create_dir(&temp_dir).expect("create temp dir");

    remove_existing_temp_dir_with_fs(&temp_dir, &RealPatchCommitFs).expect("remove temp dir");

    assert!(!temp_dir.exists(), "temp dir should be removed");
}

#[test]
fn patch_commit_remove_existing_temp_dir_reports_remove_errors() {
    let tmp = tempdir().expect("temp dir");
    let temp_dir = tmp.path().join("filtered");
    fs::create_dir(&temp_dir).expect("create temp dir");

    let err = remove_existing_temp_dir_with_fs(&temp_dir, &RemoveDirErrorFs)
        .expect_err("remove error should be reported");

    assert!(matches!(err, PatchCommitError::RemoveTempDir { .. }));
}

#[cfg(unix)]
#[test]
fn patch_commit_remove_existing_temp_dir_rejects_symlinked_temp_dir() {
    let tmp = tempdir().expect("temp dir");
    let outside = tmp.path().join("outside");
    let temp_dir = tmp.path().join("filtered");
    fs::create_dir(&outside).expect("create outside dir");
    fs::write(outside.join("sentinel"), "keep").expect("write outside sentinel");
    std::os::unix::fs::symlink(&outside, &temp_dir).expect("symlink temp dir");

    let err = remove_existing_temp_dir_with_fs(&temp_dir, &RealPatchCommitFs)
        .expect_err("symlinked temp dir should be rejected");

    assert!(matches!(err, PatchCommitError::UnsafeTempDir { .. }));
    assert_eq!(fs::read_to_string(outside.join("sentinel")).unwrap(), "keep");
}

struct CreateDirErrorFs {
    fail_on: &'static str,
    created_dirs: Cell<usize>,
}

impl PatchCommitFs for CreateDirErrorFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if path.ends_with(self.fail_on) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked create_dir_all"));
        }
        self.created_dirs.set(self.created_dirs.get() + 1);
        fs::create_dir_all(path)
    }

    fn hard_link(&self, source: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(source, target)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

struct HardLinkErrorFs;

impl PatchCommitFs for HardLinkErrorFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn hard_link(&self, _source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked hard_link"))
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

struct RemoveDirErrorFs;

impl PatchCommitFs for RemoveDirErrorFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn hard_link(&self, source: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(source, target)
    }

    fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked remove_dir_all"))
    }
}
