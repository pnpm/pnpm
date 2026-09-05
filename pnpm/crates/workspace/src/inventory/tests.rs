use super::{InventoryTraversalEvent, find_workspace_inventory, find_workspace_inventory_with};
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

    assert_eq!(inventory.manifests("package.json").unwrap(), [node.join("package.json")]);
    assert_eq!(inventory.manifests("Cargo.toml").unwrap(), [rust.join("Cargo.toml")]);
    assert!(inventory.manifests("pyproject.toml").unwrap().is_empty());
    assert!(inventory.manifests("unknown").is_none());
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

    assert_eq!(inventory.manifests("Cargo.toml").unwrap(), [project.join("Cargo.toml")]);
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
        find_workspace_inventory_with(workspace.path(), &["Cargo.toml"], &[], |event| {
            if matches!(event, InventoryTraversalEvent::BeforeRead(directory) if directory == unreadable)
            {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                Ok(())
            }
        })
        .unwrap();

    assert_eq!(inventory.manifests("Cargo.toml").unwrap(), [project.join("Cargo.toml")]);
}

#[test]
fn reports_the_nested_directory_that_failed() {
    let workspace = tempfile::tempdir().unwrap();
    let broken = workspace.path().join("broken");
    fs::create_dir_all(&broken).unwrap();

    let error = find_workspace_inventory_with(workspace.path(), &["Cargo.toml"], &[], |event| {
        if matches!(event, InventoryTraversalEvent::BeforeRead(directory) if directory == broken) {
            Err(std::io::Error::other("injected read failure"))
        } else {
            Ok(())
        }
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains(&broken.display().to_string()), "{error}");
    assert!(error.contains("injected read failure"), "{error}");
}

#[cfg(unix)]
#[test]
fn does_not_follow_directory_symlinks() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    symlink(outside.path(), workspace.path().join("linked")).unwrap();

    let inventory = find_workspace_inventory(workspace.path(), &["Cargo.toml"], &[]).unwrap();

    assert!(inventory.manifests("Cargo.toml").unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn does_not_follow_a_directory_swapped_for_a_symlink_before_descent() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let candidate = workspace.path().join("candidate");
    fs::create_dir(&candidate).unwrap();
    fs::write(outside.path().join("Cargo.toml"), "[workspace]\n").unwrap();

    let inventory =
        find_workspace_inventory_with(workspace.path(), &["Cargo.toml"], &[], |event| {
            if matches!(
                event,
                InventoryTraversalEvent::BeforeOpenDirectory(path) if path == candidate
            ) {
                fs::remove_dir(&candidate)?;
                symlink(outside.path(), &candidate)?;
            }
            Ok(())
        })
        .unwrap();

    assert!(inventory.manifests("Cargo.toml").unwrap().is_empty());
}
