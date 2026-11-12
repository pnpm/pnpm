//! Tests for the [`crate::OpenDir`] module.

use super::OpenDir;
use crate::symlink_dir;
use std::{fs, io, path::Path};
use tempfile::tempdir;

fn create_tree(root: &Path) {
    fs::create_dir_all(root.join("real/metadata/registry")).unwrap();
    fs::write(root.join("real/metadata/registry/pkg.json"), "{}").unwrap();
    fs::write(root.join("real/file"), "").unwrap();
    symlink_dir(&root.join("real"), &root.join("link")).unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
        })
        .collect();
    names.sort();
    names
}

/// Replace `root/real` with a link to `root/elsewhere`, moving the real
/// directory to `root/moved`.
#[cfg(unix)]
fn substitute_real(root: &Path) {
    fs::rename(root.join("real"), root.join("moved")).unwrap();
    symlink_dir(&root.join("elsewhere"), &root.join("real")).unwrap();
}

#[test]
fn open_follows_a_link_in_its_place() {
    let root = tempdir().unwrap();
    create_tree(root.path());

    let dir = OpenDir::open(&root.path().join("link")).expect("open through the link");

    assert_eq!(dir.path(), root.path().join("link"));
    assert_eq!(
        dir.canonical_path().unwrap(),
        Some(dunce::canonicalize(root.path().join("real")).unwrap()),
    );
}

#[test]
fn open_rejects_a_file_and_nothing() {
    let root = tempdir().unwrap();
    create_tree(root.path());

    let file = OpenDir::open(&root.path().join("real/file")).unwrap_err();
    let missing = OpenDir::open(&root.path().join("missing")).unwrap_err();

    assert_eq!(file.kind(), io::ErrorKind::NotADirectory);
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}

#[test]
fn open_child_dir_opens_only_a_real_directory() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(root.path()).unwrap();

    let real = dir.open_child_dir("real".as_ref()).unwrap();
    let link = dir.open_child_dir("link".as_ref()).unwrap();
    let file = OpenDir::open(&root.path().join("real"))
        .unwrap()
        .open_child_dir("file".as_ref())
        .unwrap();
    let missing = dir.open_child_dir("missing".as_ref()).unwrap();

    assert_eq!(real.map(|dir| dir.path().to_path_buf()), Some(root.path().join("real")));
    assert!(link.is_none());
    assert!(file.is_none());
    assert!(missing.is_none());
}

#[test]
fn entry_names_lists_the_children_without_the_dot_entries() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let mut names = dir.entry_names().unwrap();
    names.sort();

    assert_eq!(names, ["file", "metadata"]);
}

#[test]
fn entry_names_lists_the_same_names_when_called_again() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let mut first = dir.entry_names().unwrap();
    let mut second = dir.entry_names().unwrap();
    first.sort();
    second.sort();

    assert_eq!(first, ["file", "metadata"]);
    assert_eq!(second, first);
}

#[test]
fn remove_child_dir_removes_a_directory_with_its_contents() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::write(root.path().join("real/metadata/registry/other.json"), "{}").unwrap();
    symlink_dir(&root.path().join("real/file"), &root.path().join("real/metadata/to-file"))
        .unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert!(removed);
    assert_eq!(names(&root.path().join("real")), ["file"]);
}

#[test]
fn remove_child_dir_keeps_a_file_a_link_and_reports_nothing() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    symlink_dir(&root.path().join("real/metadata"), &root.path().join("real/metadata-link"))
        .unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let file = dir.remove_child_dir("file".as_ref()).unwrap();
    let link = dir.remove_child_dir("metadata-link".as_ref()).unwrap();
    let missing = dir.remove_child_dir("missing".as_ref()).unwrap();

    assert!(!file);
    assert!(!link);
    assert!(!missing);
    assert!(root.path().join("real/file").exists());
    assert!(
        root.path()
            .join("real/metadata-link")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
    );
    assert!(
        root.path()
            .join("real/metadata/registry/pkg.json")
            .exists(),
    );
}

/// The race the handle closes: the directory is replaced by a link to
/// another directory after the open, and the removal must still act on
/// the opened directory, not on the link's target.
#[cfg(unix)]
#[test]
fn remove_child_dir_ignores_a_link_substituted_after_the_open() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("elsewhere/metadata")).unwrap();
    fs::write(root.path().join("elsewhere/metadata/victim"), "").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();
    substitute_real(root.path());

    let canonical = dir.canonical_path().unwrap();
    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert_eq!(canonical, None);
    assert!(removed);
    assert!(
        root.path()
            .join("elsewhere/metadata/victim")
            .exists(),
    );
    assert_eq!(names(&root.path().join("moved")), ["file"]);
}

fn list_from_several_threads(dir: &OpenDir) {
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| assert_every_listing_is_whole(dir));
        }
    });
}

fn assert_every_listing_is_whole(dir: &OpenDir) {
    for _ in 0..200 {
        let mut names = dir.entry_names().unwrap();
        names.sort();
        assert_eq!(names, ["file", "metadata"]);
    }
}

#[test]
fn entry_names_is_complete_when_listings_overlap() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    list_from_several_threads(&dir);
}

#[cfg(unix)]
#[test]
fn entry_names_ignores_a_link_substituted_after_the_open() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("elsewhere/metadata")).unwrap();
    fs::write(root.path().join("elsewhere/decoy"), "").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();
    substitute_real(root.path());

    let mut names = dir.entry_names().unwrap();
    names.sort();

    assert_eq!(names, ["file", "metadata"]);
}

/// Same race one level up: the parent is replaced after the open, and a
/// child opened through it must be the opened parent's child.
#[cfg(unix)]
#[test]
fn open_child_dir_ignores_a_link_substituted_after_the_open() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("real/v12/metadata")).unwrap();
    fs::create_dir_all(root.path().join("elsewhere/v12/metadata")).unwrap();
    fs::write(root.path().join("elsewhere/v12/metadata/victim"), "").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();
    substitute_real(root.path());

    let child = dir
        .open_child_dir("v12".as_ref())
        .unwrap()
        .expect("the opened directory still has v12");
    let canonical = child.canonical_path().unwrap();
    let removed = child.remove_child_dir("metadata".as_ref()).unwrap();

    assert_eq!(canonical, None);
    assert!(removed);
    assert!(
        root.path()
            .join("elsewhere/v12/metadata/victim")
            .exists(),
    );
    assert_eq!(names(&root.path().join("moved/v12")), Vec::<String>::new());
}

#[test]
fn child_file_stat_and_remove_child_file_act_only_on_regular_files() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::write(root.path().join("real/payload"), b"hello").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let stat = dir
        .child_file_stat("payload".as_ref())
        .unwrap()
        .expect("regular file stat");
    assert_eq!(stat.len, 5);
    assert!(
        dir.child_file_stat("metadata".as_ref())
            .unwrap()
            .is_none(),
    );
    assert!(
        dir.child_file_stat("missing".as_ref())
            .unwrap()
            .is_none(),
    );

    assert!(!dir.remove_child_file("metadata".as_ref()).unwrap());
    assert!(dir.remove_child_file("payload".as_ref()).unwrap());
    assert!(!dir.remove_child_file("payload".as_ref()).unwrap());
    assert!(
        !root
            .path()
            .join("real/payload")
            .exists(),
    );
}

#[cfg(unix)]
#[test]
fn open_descendant_and_remove_child_file_reject_substituted_symlink() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("elsewhere/sub")).unwrap();
    fs::write(root.path().join("elsewhere/sub/victim"), b"keep").unwrap();
    fs::create_dir_all(root.path().join("real/sub")).unwrap();
    fs::write(root.path().join("real/sub/target"), b"gone").unwrap();

    let base = OpenDir::open(root.path()).unwrap();
    substitute_real(root.path());

    assert!(
        base.open_descendant_dir(Path::new("real/sub"))
            .unwrap()
            .is_none(),
    );
    assert!(
        root.path()
            .join("elsewhere/sub/victim")
            .exists(),
    );

    let moved_sub = base
        .open_descendant_dir(Path::new("moved/sub"))
        .unwrap()
        .expect("real descendant opens");
    assert!(moved_sub.remove_child_file("target".as_ref()).unwrap());
    assert!(
        base.open_descendant_dir(Path::new("moved"))
            .unwrap()
            .unwrap()
            .remove_empty_child_dir("sub".as_ref())
            .unwrap(),
    );
}

#[test]
fn has_symlink_child_and_try_reopen_distinguish_links_and_real_entries() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(root.path()).unwrap();

    assert!(dir.has_symlink_child("link".as_ref()).unwrap());
    assert!(!dir.has_symlink_child("real".as_ref()).unwrap());
    assert!(!dir.has_symlink_child("missing".as_ref()).unwrap());

    let reopened = dir.try_reopen().unwrap();
    assert_eq!(reopened.canonical_path().unwrap(), dir.canonical_path().unwrap());
}

#[test]
fn read_child_file_reads_regular_files_and_ignores_links_and_dirs() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::write(root.path().join("real/payload"), b"hello-open-dir").unwrap();
    symlink_dir(&root.path().join("real/metadata"), &root.path().join("real/dir-link")).unwrap();
    #[cfg(unix)]
    {
        fs::write(root.path().join("outside"), b"secret").unwrap();
        std::os::unix::fs::symlink(root.path().join("outside"), root.path().join("real/file-link"))
            .unwrap();
        let _sock = std::os::unix::net::UnixListener::bind(root.path().join("real/sock")).unwrap();
    }
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    assert_eq!(
        dir.read_child_file("payload".as_ref())
            .unwrap()
            .as_deref(),
        Some(&b"hello-open-dir"[..]),
    );
    assert!(
        dir.read_child_file("dir-link".as_ref())
            .unwrap()
            .is_none(),
    );
    #[cfg(unix)]
    {
        assert!(
            dir.read_child_file("file-link".as_ref())
                .unwrap()
                .is_none(),
        );
        assert!(
            dir.read_child_file("sock".as_ref())
                .unwrap()
                .is_none(),
        );
    }
    assert!(
        dir.read_child_file("metadata".as_ref())
            .unwrap()
            .is_none(),
    );
    assert!(
        dir.read_child_file("missing".as_ref())
            .unwrap()
            .is_none(),
    );
}

#[test]
fn child_methods_reject_non_single_component_names() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::write(root.path().join("outside.txt"), b"keep").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    for bad in ["", ".", "..", "../outside.txt", "metadata/registry", "metadata/", "/outside.txt"] {
        assert_eq!(
            dir.open_child_dir(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.remove_child_dir(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.child_file_stat(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.read_child_file(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.has_symlink_child(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.remove_child_file(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.remove_empty_child_dir(bad.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.create_child_file_new(bad.as_ref(), b"x")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            dir.write_child_file(bad.as_ref(), b"x")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
    }
    assert_eq!(fs::read(root.path().join("outside.txt")).unwrap(), b"keep");
}

#[test]
fn create_and_write_child_file_stay_anchored_and_refuse_symlinks() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    dir.create_child_file_new(".marker".as_ref(), b"v1").unwrap();
    assert_eq!(
        dir.create_child_file_new(".marker".as_ref(), b"v2")
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists,
    );
    assert_eq!(
        dir.read_child_file(".marker".as_ref())
            .unwrap()
            .as_deref(),
        Some(&b"v1"[..]),
    );

    dir.write_child_file(".marker".as_ref(), b"v2").unwrap();
    assert_eq!(
        dir.read_child_file(".marker".as_ref())
            .unwrap()
            .as_deref(),
        Some(&b"v2"[..]),
    );

    #[cfg(unix)]
    {
        fs::write(root.path().join("victim"), b"untouched").unwrap();
        std::os::unix::fs::symlink(root.path().join("victim"), root.path().join("real/sym"))
            .unwrap();
        assert_eq!(
            dir.create_child_file_new("sym".as_ref(), b"bad")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists,
        );
        assert_eq!(
            dir.write_child_file("sym".as_ref(), b"bad")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(fs::read(root.path().join("victim")).unwrap(), b"untouched");

        fs::create_dir_all(root.path().join("elsewhere")).unwrap();
        fs::write(root.path().join("elsewhere/.marker"), b"other").unwrap();
        substitute_real(root.path());
        dir.write_child_file(".marker".as_ref(), b"anchored").unwrap();
        assert_eq!(fs::read(root.path().join("moved/.marker")).unwrap(), b"anchored");
        assert_eq!(fs::read(root.path().join("elsewhere/.marker")).unwrap(), b"other");
    }
}

#[test]
fn read_child_file_bounded_enforces_size_cap_and_pinned_helper_cleans_up_temps() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("small"), b"abcd").unwrap();
    fs::write(root.path().join("large"), vec![b'x'; 512]).unwrap();
    let dir = OpenDir::open(root.path()).unwrap();

    assert_eq!(dir.read_child_file_bounded("missing".as_ref(), 4).unwrap(), None);
    assert_eq!(
        dir.read_child_file_bounded("small".as_ref(), 4)
            .unwrap()
            .as_deref(),
        Some(&b"abcd"[..]),
    );
    assert_eq!(
        dir.read_child_file_bounded("small".as_ref(), 3)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData,
    );
    assert_eq!(
        dir.read_child_file_bounded("large".as_ref(), 64)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData,
    );

    let pinned = root.path().join("pinned");
    fs::create_dir_all(&pinned).unwrap();
    super::write_child_file_in_pinned_dir(
        &pinned,
        ".marker".as_ref(),
        b"first",
        super::ChildWrite::CreateNew,
        || Ok(false),
    )
    .unwrap();
    assert_eq!(
        super::write_child_file_in_pinned_dir(
            &pinned,
            ".marker".as_ref(),
            b"second",
            super::ChildWrite::Replace,
            || Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::PermissionDenied,
    );
    let entries: Vec<_> = fs::read_dir(&pinned)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, vec![std::ffi::OsString::from(".marker")]);
    assert_eq!(fs::read(pinned.join(".marker")).unwrap(), b"first");
}

#[cfg(windows)]
#[test]
fn windows_retargeted_ancestor_junction_stays_bound_to_opened_root() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("real_a/cache/sub")).unwrap();
    fs::write(root.path().join("real_a/cache/keep.txt"), b"from-a").unwrap();
    fs::create_dir_all(root.path().join("real_b/cache/sub")).unwrap();
    fs::write(root.path().join("real_b/cache/keep.txt"), b"from-b").unwrap();

    let junction_path = root.path().join("links");
    junction::create(root.path().join("real_a"), &junction_path).unwrap();

    let dir = OpenDir::open(&junction_path.join("cache")).unwrap();

    fs::remove_dir(&junction_path).unwrap();
    junction::create(root.path().join("real_b"), &junction_path).unwrap();

    assert_eq!(dir.canonical_path().unwrap(), None);
    assert_eq!(
        dir.read_child_file("keep.txt".as_ref())
            .unwrap()
            .as_deref(),
        Some(&b"from-a"[..]),
    );
    assert!(dir.remove_child_dir("sub".as_ref()).unwrap());
    assert!(dir.remove_child_file("keep.txt".as_ref()).unwrap());
    assert!(
        root.path()
            .join("real_b/cache/keep.txt")
            .exists(),
    );
    assert!(
        root.path()
            .join("real_b/cache/sub")
            .exists(),
    );
}
