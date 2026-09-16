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
        read(&root.path().join("requirements.txt"), root.path()).unwrap(),
        ["alpha>=1 ; python_full_version >= '3.10'"],
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
        let error = format!("{:?}", read(&path, root.path()).unwrap_err());
        assert!(error.contains("requirements.txt:2"), "{error}");
    }
    fs::write(&path, r"alpha\").unwrap();
    let error = format!("{:?}", read(&path, root.path()).unwrap_err());
    assert!(error.contains("unfinished"), "{error}");
}

#[test]
fn includes_cannot_escape_the_workspace_or_use_absolute_paths() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let outside = root.path().join("outside.txt");
    fs::write(&outside, "secret-package-name\n").unwrap();
    let path = workspace.join("requirements.txt");
    for include in ["../outside.txt".to_string(), outside.display().to_string()] {
        fs::write(&path, format!("-r {include}\n")).unwrap();
        let error = format!("{:?}", read(&path, &workspace).unwrap_err());
        assert!(error.contains("requirements.txt:1"), "{error}");
    }
}

#[cfg(unix)]
#[test]
fn includes_cannot_follow_symlinks_outside_the_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret-package-name\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    let path = root.path().join("requirements.txt");
    fs::write(&path, "-r linked/secret.txt\n").unwrap();
    let error = format!("{:?}", read(&path, root.path()).unwrap_err());
    assert!(error.contains("escapes"), "{error}");
}

#[test]
fn requirements_must_be_regular_and_bounded_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("requirements.txt");
    fs::create_dir(&path).unwrap();
    let error = format!("{:?}", read(&path, root.path()).unwrap_err());
    assert!(error.contains("regular file"), "{error}");
    fs::remove_dir(&path).unwrap();
    fs::File::create(&path)
        .unwrap()
        .set_len(super::MAX_FILE_BYTES + 1)
        .unwrap();
    let error = format!("{:?}", read(&path, root.path()).unwrap_err());
    assert!(error.contains("exceeds"), "{error}");
}

#[test]
fn branching_includes_are_read_once_and_deep_includes_fail() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..32 {
        fs::write(
            root.path().join(format!("{index}.txt")),
            format!("-r {}.txt\n-r {}.txt\n", index + 1, index + 1),
        )
        .unwrap();
    }
    fs::write(root.path().join("32.txt"), "alpha\n").unwrap();
    assert_eq!(read(&root.path().join("0.txt"), root.path()).unwrap(), ["alpha"]);
    for index in 32..65 {
        fs::write(root.path().join(format!("{index}.txt")), format!("-r {}.txt\n", index + 1))
            .unwrap();
    }
    fs::write(root.path().join("65.txt"), "alpha\n").unwrap();
    let error = format!("{:?}", read(&root.path().join("0.txt"), root.path()).unwrap_err());
    assert!(error.contains("include limit exceeded"), "{error}");
}
