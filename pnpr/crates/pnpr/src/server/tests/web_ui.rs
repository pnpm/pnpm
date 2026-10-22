use std::path::Path;

use tempfile::TempDir;

use super::super::web_ui::ui_dir_beside;

fn write_package(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("package.json"), format!(r#"{{"name":"{name}"}}"#)).unwrap();
}

#[test]
fn finds_the_ui_package_beside_an_installed_pnpr() {
    let root = TempDir::new().unwrap();
    let scope = root.path().join("node_modules/@pnpm");
    write_package(&scope.join("pnpr"), "@pnpm/pnpr");
    write_package(&scope.join("pnpr-ui"), "@pnpm/pnpr-ui");

    assert_eq!(ui_dir_beside(&scope.join("pnpr/bin/pnpr")), Some(scope.join("pnpr-ui/dist")));
}

#[test]
fn ignores_a_ui_folder_beside_a_pnpr_binary_outside_an_npm_package() {
    let root = TempDir::new().unwrap();
    write_package(&root.path().join("pnpr-ui"), "@pnpm/pnpr-ui");
    std::fs::create_dir_all(root.path().join("a/bin")).unwrap();
    assert_eq!(ui_dir_beside(&root.path().join("a/bin/pnpr")), None);

    let scope = root.path().join("node_modules/@pnpm");
    write_package(&scope.join("pnpr"), "@pnpm/pnpr");
    write_package(&scope.join("pnpr-ui"), "not-the-ui");
    assert_eq!(ui_dir_beside(&scope.join("pnpr/bin/pnpr")), None);
    assert_eq!(ui_dir_beside(&scope.join("pnpr/pnpr")), None);
}
