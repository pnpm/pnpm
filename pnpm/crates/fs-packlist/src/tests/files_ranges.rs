use super::{fs, json, packlist, tempdir, touch};

#[test]
fn numeric_brace_ranges_select_package_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for file in ["package.json", "part1.js", "part2.js", "part3.js", "part4.js"] {
        touch(root, file);
    }
    let manifest = json!({ "name": "x", "version": "0.0.0", "files": ["part{1..3}.js"] });
    let out = packlist(root, &manifest).unwrap();
    dbg!(&out);
    assert_eq!(out, ["package.json", "part1.js", "part2.js", "part3.js"]);
}

#[test]
fn numeric_brace_ranges_exclude_package_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for file in ["package.json", "part1.js", "part2.js", "part3.js", "part4.js"] {
        touch(root, file);
    }
    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["*.js", "!part{1..3}.js"],
    });
    let out = packlist(root, &manifest).unwrap();
    dbg!(&out);
    assert_eq!(out, ["package.json", "part4.js"]);
}

#[test]
fn npmignore_expands_padded_descending_ranges() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for file in ["package.json", "part01.js", "part02.js", "part03.js", "part04.js"] {
        touch(root, file);
    }
    fs::write(root.join(".npmignore"), "part{03..01..2}.js\n").unwrap();
    let manifest = json!({ "name": "x", "version": "0.0.0" });
    let out = packlist(root, &manifest).unwrap();
    dbg!(&out);
    assert_eq!(out, ["package.json", "part02.js", "part04.js"]);
}
