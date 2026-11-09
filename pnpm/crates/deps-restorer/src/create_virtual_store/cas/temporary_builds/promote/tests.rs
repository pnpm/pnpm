use super::place_package;
use std::fs;
use tempfile::tempdir;

#[test]
fn a_package_another_install_placed_is_kept() {
    let root = tempdir().expect("temp dir");
    let (staged, target) = (root.path().join("staged"), root.path().join("target"));
    for (dir, content) in [(&staged, "ours"), (&target, "theirs")] {
        fs::create_dir(dir).expect("create dir");
        fs::write(dir.join("package.json"), content).expect("write manifest");
    }

    place_package(&staged, &target, &root.path().join("aside")).expect("place");

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

    place_package(&staged, &target, &root.path().join("aside")).expect("place");

    assert_eq!(fs::read_to_string(target.join("package.json")).expect("read"), "ours");
    assert!(!target.join("partial.txt").exists());
}
