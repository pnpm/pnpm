use super::dedicated_injected_source_dirs;
use std::{collections::HashSet, fs};

#[test]
fn includes_the_publish_directory_of_an_undiscovered_workspace_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().to_path_buf();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "root", "version": "1.0.0", "publishConfig": { "directory": "dist" } }"#,
    )
    .expect("write root package.json");

    let source_dirs =
        dedicated_injected_source_dirs(&[], std::slice::from_ref(&root)).expect("source dirs");

    let expected: HashSet<_> =
        [pnpm_fs::lexical_normalize(&root), pnpm_fs::lexical_normalize(&root.join("dist"))]
            .into_iter()
            .collect();
    assert_eq!(source_dirs, expected);
}
