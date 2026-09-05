use super::{find_workspace_inventory, find_workspace_inventory_with};
use pretty_assertions::assert_eq;
use std::fs;

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[test]
fn discovers_multiple_manifest_kinds_in_one_inventory() {
    let workspace = tempfile::tempdir().unwrap();
    let node = workspace.path().join("packages/node");
    let rust = workspace.path().join("packages/rust");
    fs::create_dir_all(&node).unwrap();
    fs::create_dir_all(&rust).unwrap();
    fs::write(node.join("package.json"), "{}").unwrap();
    fs::write(rust.join("Cargo.toml"), "[workspace]\n").unwrap();

    let inventory = find_workspace_inventory(
        workspace.path(),
        &["package.json", "Cargo.toml", "pyproject.toml"],
        &[".git", ".pnpm", "node_modules", "target"],
    )
    .unwrap();

    assert_eq!(inventory.manifests("package.json"), [node.join("package.json")]);
    assert_eq!(inventory.manifests("Cargo.toml"), [rust.join("Cargo.toml")]);
    assert!(inventory.manifests("pyproject.toml").is_empty());
    assert!(inventory.manifests("unknown").is_empty());
}

#[test]
fn prunes_generated_directories() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("rust/project");
    let generated = workspace.path().join("target/generated");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&generated).unwrap();
    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::write(generated.join("Cargo.toml"), "[workspace]\n").unwrap();

    let inventory =
        find_workspace_inventory(workspace.path(), &["Cargo.toml"], &["target"]).unwrap();

    assert_eq!(inventory.manifests("Cargo.toml"), [project.join("Cargo.toml")]);
}

#[test]
fn skips_unreadable_unrelated_directories() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("rust/project");
    let unreadable = workspace.path().join("unrelated");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&unreadable).unwrap();
    fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();

    let inventory =
        find_workspace_inventory_with(workspace.path(), &["Cargo.toml"], &[], |directory| {
            if directory == unreadable {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                fs::read_dir(directory)
            }
        })
        .unwrap();

    assert_eq!(inventory.manifests("Cargo.toml"), [project.join("Cargo.toml")]);
}

#[cfg(unix)]
#[test]
fn does_not_follow_directory_symlinks() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    symlink(outside.path(), workspace.path().join("linked")).unwrap();

    let inventory = find_workspace_inventory(workspace.path(), &["Cargo.toml"], &[]).unwrap();

    assert!(inventory.manifests("Cargo.toml").is_empty());
}
