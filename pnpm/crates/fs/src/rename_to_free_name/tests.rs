use super::rename_to_free_name;
use std::fs;

#[test]
fn renames_to_the_requested_name_when_free() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("src");
    fs::create_dir(&src).expect("create src");
    let dst = tmp.path().join("dst");

    let landed = rename_to_free_name(&src, &dst).expect("rename");

    assert_eq!(landed.as_deref(), Some(dst.as_path()));
    assert!(!src.exists());
}

#[test]
fn never_overwrites_an_occupied_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dst = tmp.path().join("dst");
    for occupied in [dst.clone(), tmp.path().join("dst_1")] {
        fs::create_dir(&occupied).expect("create occupied dir");
        fs::write(occupied.join("kept"), "earlier").expect("write kept file");
    }
    let src = tmp.path().join("src");
    fs::create_dir(&src).expect("create src");
    fs::write(src.join("moved"), "later").expect("write moved file");

    let landed = rename_to_free_name(&src, &dst).expect("rename");

    let expected = tmp.path().join("dst_2");
    assert_eq!(landed.as_deref(), Some(expected.as_path()));
    assert_eq!(fs::read_to_string(expected.join("moved")).expect("read moved"), "later");
    assert_eq!(fs::read_to_string(dst.join("kept")).expect("read kept"), "earlier");
}
