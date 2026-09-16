use super::read;
use std::fs;

#[test]
fn includes_are_relative_and_can_be_repeated_without_being_cycles() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(
        root.path().join("requirements.txt"),
        "-rnested/base.txt\n--requirement=nested/base.txt\n",
    )
    .unwrap();
    fs::write(root.path().join("nested/base.txt"), "--requirement ../common.txt\n").unwrap();
    fs::write(root.path().join("common.txt"), "alpha>=1; python_version >= '3.10' # comment\n")
        .unwrap();
    assert_eq!(
        read(&root.path().join("requirements.txt")).unwrap(),
        ["alpha>=1; python_version >= '3.10'", "alpha>=1; python_version >= '3.10'"],
    );
}

#[test]
fn invalid_requirements_report_the_file_and_line() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("requirements.txt");
    for contents in
        ["# comment\nnot a valid requirement\n", "# comment\n--index-url=https://example.com\n"]
    {
        fs::write(&path, contents).unwrap();
        let error = format!("{:?}", read(&path).unwrap_err());
        assert!(error.contains("requirements.txt:2"), "{error}");
    }
    fs::write(&path, r"alpha\").unwrap();
    let error = format!("{:?}", read(&path).unwrap_err());
    assert!(error.contains("unfinished"), "{error}");
}
