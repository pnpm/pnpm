use std::fs;

use tempfile::tempdir;

use super::read_modules_dir;

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

#[test]
fn missing_dir_has_no_packages() {
    let dir = tempdir().expect("create temp dir");
    let names = read_modules_dir(&dir.path().join("node_modules")).expect("read modules dir");
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn lists_unscoped_and_scoped_packages() {
    let dir = tempdir().expect("create temp dir");
    let modules_dir = dir.path().join("node_modules");
    for pkg in ["foo", "@scope/bar", "@scope/baz"] {
        fs::create_dir_all(modules_dir.join(pkg)).expect("create package dir");
    }

    let names = sorted(read_modules_dir(&modules_dir).expect("read modules dir"));
    assert_eq!(names, ["@scope/bar", "@scope/baz", "foo"]);
}

#[test]
fn skips_dot_entries_and_files() {
    let dir = tempdir().expect("create temp dir");
    let modules_dir = dir.path().join("node_modules");
    for entry in ["foo", ".bin", ".pnpm", ".ignored", ".cache"] {
        fs::create_dir_all(modules_dir.join(entry)).expect("create dir entry");
    }
    fs::write(modules_dir.join(".modules.yaml"), "").expect("write file entry");
    fs::write(modules_dir.join("not-a-package.txt"), "").expect("write file entry");

    let names = read_modules_dir(&modules_dir).expect("read modules dir");
    assert_eq!(names, ["foo"]);
}
