use super::{WhenPlaced, merge_missing_entries, place_package};
use std::{fs, path::Path};
use tempfile::tempdir;

#[test]
fn a_package_another_install_placed_is_kept() {
    let root = tempdir().expect("temp dir");
    let (staged, target) = (root.path().join("staged"), root.path().join("target"));
    for (dir, content) in [(&staged, "ours"), (&target, "theirs")] {
        fs::create_dir(dir).expect("create dir");
        fs::write(dir.join("package.json"), content).expect("write manifest");
    }

    place_package((&staged, &target), &root.path().join("aside"), WhenPlaced::Keep).expect("place");

    assert_eq!(fs::read_to_string(target.join("package.json")).expect("read"), "theirs");
}

#[test]
fn an_unfinished_import_is_replaced() {
    let root = tempdir().expect("temp dir");
    let (staged, target) = (root.path().join("staged"), root.path().join("target"));
    fs::create_dir(&staged).expect("create staged");
    fs::write(staged.join("package.json"), "ours").expect("write manifest");
    fs::create_dir(&target).expect("create target");
    fs::write(target.join("partial.txt"), "").expect("write a partial import");

    place_package((&staged, &target), &root.path().join("aside"), WhenPlaced::Keep).expect("place");

    assert_eq!(fs::read_to_string(target.join("package.json")).expect("read"), "ours");
    assert!(!target.join("partial.txt").exists());
}

#[test]
fn a_forced_build_replaces_the_placed_package() {
    let root = tempdir().expect("temp dir");
    let (staged, target) = (root.path().join("staged"), root.path().join("target"));
    for (dir, content) in [(&staged, "rebuilt"), (&target, "placed")] {
        fs::create_dir(dir).expect("create dir");
        fs::write(dir.join("package.json"), content).expect("write manifest");
    }

    place_package((&staged, &target), &root.path().join("aside"), WhenPlaced::Replace)
        .expect("place");

    assert_eq!(fs::read_to_string(target.join("package.json")).expect("read"), "rebuilt");
}

#[test]
fn the_links_a_partly_linked_slot_lacks_are_merged_into_it() {
    let root = tempdir().expect("temp dir");
    let (staged, target) = (root.path().join("staged"), root.path().join("target"));
    for dir in ["dep", "existing", "@scope/pkg", "@scope/scoped-dep"] {
        fs::create_dir_all(staged.join(dir)).expect("create staged entry");
    }
    fs::write(staged.join("existing/marker"), "staged").expect("write staged entry");
    fs::create_dir_all(target.join("existing")).expect("create target entry");

    merge_missing_entries(&staged, &target, Path::new("@scope/pkg")).expect("merge");

    assert!(target.join("dep").is_dir());
    assert!(target.join("@scope/scoped-dep").is_dir());
    assert!(!target.join("existing/marker").exists(), "an entry the slot has stays");
    assert!(!target.join("@scope/pkg").exists(), "the package is placed separately");
}
