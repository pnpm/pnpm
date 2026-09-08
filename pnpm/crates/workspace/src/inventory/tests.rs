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
        &[],
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
        find_workspace_inventory(workspace.path(), &["Cargo.toml"], &["target"], &[]).unwrap();

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

    let inventory = find_workspace_inventory_with(
        workspace.path(),
        &["Cargo.toml"],
        &[],
        &[],
        |directory| {
            if directory == unreadable {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                Ok(())
            }
        },
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(inventory.manifests("Cargo.toml").unwrap(), [project.join("Cargo.toml")]);
}

#[test]
fn reports_the_nested_directory_that_failed() {
    let workspace = tempfile::tempdir().unwrap();
    let broken = workspace.path().join("broken");
    fs::create_dir_all(&broken).unwrap();

    let error = find_workspace_inventory_with(
        workspace.path(),
        &["Cargo.toml"],
        &[],
        &[],
        |directory| {
            if directory == broken {
                Err(std::io::Error::other("injected read failure"))
            } else {
                Ok(())
            }
        },
        |_| Ok(()),
    )
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

    let inventory = find_workspace_inventory(workspace.path(), &["Cargo.toml"], &[], &[]).unwrap();

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

    let inventory = find_workspace_inventory_with(
        workspace.path(),
        &["Cargo.toml"],
        &[],
        &[],
        |_| Ok(()),
        |path| {
            if path == candidate {
                fs::remove_dir(&candidate)?;
                symlink(outside.path(), &candidate)?;
            }
            Ok(())
        },
    )
    .unwrap();

    assert!(inventory.manifests("Cargo.toml").unwrap().is_empty());
}

#[test]
fn prunes_managed_paths_before_opening_without_excluding_matching_project_names() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = workspace.path().join("managed/cache");
    let project = workspace.path().join("projects/cache");
    for directory in [&cache, &project] {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(directory.join("pyproject.toml"), "[project]\n").unwrap();
    }
    for excluded in [cache.clone(), std::path::PathBuf::from("managed/cache")] {
        let inventory = find_workspace_inventory_with(
            workspace.path(),
            &["Cargo.toml", "pyproject.toml"],
            &[],
            &[excluded],
            |_| Ok(()),
            |path| {
                assert_ne!(path, cache, "managed directory must not be opened");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(inventory.manifests("Cargo.toml").unwrap(), [project.join("Cargo.toml")]);
        assert_eq!(
            inventory.manifests("pyproject.toml").unwrap(),
            [project.join("pyproject.toml")],
        );
    }
}

#[test]
fn reads_children_without_accumulating_unvisited_sibling_handles() {
    let workspace = tempfile::tempdir().unwrap();
    for index in 0..128 {
        let project = workspace.path().join(format!("member-{index}"));
        fs::create_dir(&project).unwrap();
        fs::write(project.join("Cargo.toml"), "[workspace]\n").unwrap();
    }
    let unread = std::cell::Cell::new(0usize);
    let peak = std::cell::Cell::new(0usize);
    let inventory = find_workspace_inventory_with(
        workspace.path(),
        &["Cargo.toml"],
        &[],
        &[],
        |directory| {
            if directory != workspace.path() {
                unread.set(unread.get() - 1);
            }
            Ok(())
        },
        |_| {
            unread.set(unread.get() + 1);
            peak.set(peak.get().max(unread.get()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(inventory.manifests("Cargo.toml").unwrap().len(), 128);
    assert_eq!(peak.get(), 1);
    assert_eq!(unread.get(), 0);
}

#[cfg(unix)]
#[test]
fn discovers_deep_trees_with_a_small_handle_limit() {
    const ROOT_ENV: &str = "PNPM_TEST_DEEP_INVENTORY_ROOT";
    if let Some(root) = std::env::var_os(ROOT_ENV) {
        let inventory =
            find_workspace_inventory(std::path::Path::new(&root), &["Cargo.toml"], &[], &[])
                .unwrap();
        assert_eq!(inventory.manifests("Cargo.toml").unwrap().len(), 129);
        return;
    }
    let workspace = tempfile::tempdir().unwrap();
    let mut directory = workspace.path().to_path_buf();
    for _ in 0..128 {
        let sibling = directory.join("s");
        fs::create_dir(&sibling).unwrap();
        fs::write(sibling.join("Cargo.toml"), "[workspace]\n").unwrap();
        directory.push("d");
        fs::create_dir(&directory).unwrap();
    }
    fs::write(directory.join("Cargo.toml"), "[workspace]\n").unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", r#"ulimit -n 64; exec "$@""#, "sh"])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inventory::tests::discovers_deep_trees_with_a_small_handle_limit",
            "--nocapture",
        ])
        .env(ROOT_ENV, workspace.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[cfg(unix)]
#[test]
fn does_not_follow_an_ancestor_swapped_before_a_queued_child_is_opened() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let parent = workspace.path().join("parent");
    let child = parent.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(outside.path().join("child")).unwrap();
    fs::write(outside.path().join("child/Cargo.toml"), "[workspace]\n").unwrap();
    let inventory = find_workspace_inventory_with(
        workspace.path(),
        &["Cargo.toml"],
        &[],
        &[],
        |_| Ok(()),
        |path| {
            if path == child {
                fs::rename(&parent, workspace.path().join("moved"))?;
                symlink(outside.path(), &parent)?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(inventory.manifests("Cargo.toml").unwrap().len(), 0);
}
