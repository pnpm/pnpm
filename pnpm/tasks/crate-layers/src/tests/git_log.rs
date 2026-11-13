use crate::{
    cochange::{self, CoChange},
    tree::git,
};
use pretty_assertions::assert_eq;
use std::{collections::BTreeMap, fs, path::Path};

fn run_git(dir: &Path, args: &[&str]) {
    let config = [
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
    ];
    let args = config
        .iter()
        .chain(args)
        .copied()
        .collect::<Vec<_>>();
    git(dir, &args).unwrap();
}

fn commit(dir: &Path, message: &str) {
    run_git(dir, &["add", "--all"]);
    run_git(dir, &["commit", "--quiet", "--message", message]);
}

fn write(dir: &Path, path: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "fn main() {}\n").unwrap();
}

#[test]
fn reads_moves_and_unusual_paths_and_stops_before_a_shallow_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let origin = temp.path().join("origin");
    fs::create_dir(&origin).unwrap();
    run_git(&origin, &["init", "--quiet"]);
    write(&origin, "crates/base/a.rs");
    write(&origin, "crates/top/a.rs");
    commit(&origin, "outside the clone");
    write(&origin, "crates/base/b.rs");
    commit(&origin, "the shallow boundary");
    fs::create_dir_all(origin.join("crates/top")).unwrap();
    run_git(&origin, &["mv", "crates/base/b.rs", "crates/top/b.rs"]);
    commit(&origin, "a move");
    write(&origin, "crates/base/ünïcode.rs");
    fs::write(origin.join("crates/top/a.rs"), "fn changed() {}\n").unwrap();
    commit(&origin, "an unusual path");

    let clone = temp.path().join("clone");
    // `--no-local` makes a clone from a path honor `--depth`.
    run_git(temp.path(), &["clone", "--quiet", "--no-local", "--depth=3", "origin", "clone"]);

    let log = cochange::read_log(&clone, 100).unwrap();
    dbg!(&log);
    let dirs = ["base", "top"]
        .map(|name| (name.to_owned(), format!("crates/{name}")))
        .into_iter()
        .collect();
    let co_change = CoChange::count(&log, cochange::crate_of(&dirs), 10);
    dbg!(&co_change);
    assert_eq!(co_change.commits, 2);
    let expected = BTreeMap::from([("base".to_owned(), 2), ("top".to_owned(), 2)]);
    assert_eq!(co_change.changes, expected);
    assert_eq!(co_change.together("base", "top"), 2);
}

#[cfg(unix)]
#[test]
#[cfg_attr(target_os = "macos", ignore = "APFS rejects a file name that is not UTF-8")]
fn reads_a_path_that_is_not_utf8() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    run_git(repo, &["init", "--quiet"]);
    fs::create_dir_all(repo.join("crates/base")).unwrap();
    fs::write(
        repo.join("crates/base")
            .join(OsStr::from_bytes(b"\xff.rs")),
        "",
    )
    .unwrap();
    fs::write(repo.join(OsStr::from_bytes(b"\xfe")), "").unwrap();
    write(repo, "crates/top/a.rs");
    commit(repo, "paths that are not UTF-8");

    let log = cochange::read_log(repo, 100).unwrap();
    dbg!(&log);
    let dirs = ["base", "top"]
        .map(|name| (name.to_owned(), format!("crates/{name}")))
        .into_iter()
        .collect();
    let co_change = CoChange::count(&log, cochange::crate_of(&dirs), 10);
    assert_eq!(co_change.together("base", "top"), 1);
}
