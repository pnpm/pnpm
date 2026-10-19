use super::{fs, json, packlist, tempdir, touch};

#[test]
fn root_files_entries_include_package_contents() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for file in ["package.json", "README.md", "data.txt", "lib/runtime.js"] {
        touch(root, file);
    }

    for entry in ["", "./", "/"] {
        let manifest = json!({ "name": "x", "version": "0.0.0", "files": [entry] });
        let out = packlist(root, &manifest).unwrap();
        dbg!(&out);
        assert_eq!(out, ["README.md", "data.txt", "lib/runtime.js", "package.json"], "{entry:?}");
    }
}

#[test]
fn root_files_entries_respect_exclusions_and_nested_ignore_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for file in [
        "package.json",
        "data.txt",
        "lib/runtime.js",
        "lib/runtime.test.js",
        "test/fixture.js",
        "package-lock.json",
        "node_modules/dependency/index.js",
    ] {
        touch(root, file);
    }
    fs::write(root.join(".npmignore"), "data.txt\n").unwrap();
    fs::write(root.join("lib/.npmignore"), "*.test.js\n").unwrap();

    for entry in ["", "./", "/"] {
        let manifest = json!({
            "name": "x",
            "version": "0.0.0",
            "files": [entry, "!test"],
        });
        let out = packlist(root, &manifest).unwrap();
        dbg!(&out);
        assert_eq!(out, [".npmignore", "data.txt", "lib/runtime.js", "package.json"], "{entry:?}");
    }
}

#[test]
fn dot_files_entry_does_not_include_package_contents() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "lib/runtime.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": ["."] });
    let out = packlist(root, &manifest).unwrap();
    dbg!(&out);
    assert_eq!(out, ["package.json"]);
}

#[test]
fn non_string_files_entries_do_not_include_package_contents() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "README.md");
    touch(root, "index.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": [null, false, 42] });
    let out = packlist(root, &manifest).unwrap();
    dbg!(&out);
    assert_eq!(out, ["README.md", "package.json"]);
}
