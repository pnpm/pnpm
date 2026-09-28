use super::{assert_eq, fs, pacquet, read_lockfile, setup};
use assert_cmd::assert::OutputAssertExt;

/// `vue-loader` has an entry in pnpm's built-in compatibility database that
/// adds peer dependencies. The entry extends the published package, not a
/// project that happens to share its name and version.
#[test]
fn update_leaves_project_matching_compatibility_db_entry_unchanged() {
    let (root, workspace, anchor) = setup();
    let manifest = r#"{ "name": "vue-loader", "version": "0.0.0" }"#;
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        manifest,
    );
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let importer = lockfile.importers.get(".").expect("root importer");
    assert_eq!(importer.dependencies, None);
    assert_eq!(importer.dev_dependencies, None);
    assert_eq!(importer.optional_dependencies, None);

    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}
