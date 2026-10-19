use super::{fs, json, packlist, tempdir, touch, write};
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn file_selection_matches_typescript_packlist() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let compilation = Command::new("pnpm")
        .args(["--filter", "@pnpm/fs.packlist...", "exec", "tsgo", "--build"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        compilation.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&compilation.stdout),
        String::from_utf8_lossy(&compilation.stderr),
    );

    let mut fixtures = Vec::new();
    let mut inputs = Vec::new();
    let mut actual = Vec::new();
    for (manifest, ignore_kind) in manifests()
        .into_iter()
        .flat_map(|manifest| (0..3).map(move |kind| (manifest.clone(), kind)))
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for file in [
            "index.js",
            "index.test.js",
            "a",
            "b",
            "foo",
            "foofoo",
            "README.md",
            "LICENSE",
            "dist/index.js",
            "dist/other.js",
            "dist/lib/app.js",
            "dist/lib/chunk.js",
            "lib/index.js",
            "lib/index.test.js",
            "lib/a",
            "lib/b",
            "lib/.env",
            "npm-debug.log",
            ".DS_Store",
            "file.orig",
            "yarn.lock",
            "pnpm-lock.yaml",
            "package-lock.json",
            "lib/yarn.lock",
            "lib/file.orig",
            "lib/npm-debug.log",
        ] {
            touch(root, file);
        }
        write(root, "package.json", &manifest.to_string());
        fs::create_dir(root.join("sub")).unwrap();
        pnpm_fs::symlink_dir(&root.join("lib"), &root.join("sub/link")).unwrap();
        write(root, ".gitignore", "dist/\n");
        write(root, ".npmignore", "dist/\n!npm-debug.log\n!file.orig\n!.DS_Store\n!yarn.lock\n");
        if ignore_kind != 0 {
            fs::remove_file(root.join(".npmignore")).unwrap();
        }
        if ignore_kind == 2 {
            fs::remove_file(root.join(".gitignore")).unwrap();
        }
        write(root, "dist/.npmignore", "index.js\n");
        write(root, "lib/.gitignore", "*.test.js\n");
        write(
            root,
            "node_modules/bundled/package.json",
            r#"{"name":"bundled","files":["lib/**"]}"#,
        );
        touch(root, "node_modules/bundled/lib/index.js");
        touch(root, "node_modules/bundled/lib/file.orig");
        inputs.push(json!({ "root": root, "manifest": manifest }));
        actual.push(packlist(root, &manifest).unwrap());
        fixtures.push(dir);
    }
    let mut oracle = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/parity.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    oracle.stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&inputs).unwrap())
        .unwrap();
    let output = oracle.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let expected: Vec<Vec<String>> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected.len(), inputs.len());
    for ((input, actual), expected) in inputs.iter().zip(actual).zip(expected) {
        assert_eq!(actual, expected, "input: {input}");
    }
}

fn manifests() -> Vec<Value> {
    let mut manifests = vec![json!({}), json!({ "bundleDependencies": ["bundled"] })];
    for files in [
        json!([]),
        json!(["**"]),
        json!(["lib"]),
        json!(["lib/*"]),
        json!(["@(a|b)"]),
        json!(["+(foo)"]),
        json!(["lib/!(*.test.js)"]),
        json!(["!(*.test.js)"]),
        json!(["**", "!dist", "dist/index.js"]),
        json!(["**", "!sub", "sub/link"]),
        json!(["file.orig", "npm-debug.log", ".DS_Store", "yarn.lock", "lib/yarn.lock"]),
    ] {
        manifests.push(json!({ "files": files }));
        manifests.push(json!({ "files": files, "bundleDependencies": ["bundled"] }));
        manifests.push(json!({ "files": files, "main": "dist/index.js", "browser": "dist/lib/app.js", "bin": { "x": "lib/index.js" } }));
    }
    manifests
}
