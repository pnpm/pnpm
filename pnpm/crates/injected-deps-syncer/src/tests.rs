use crate::{SyncInjectedDeps, sync_injected_deps, sync_injected_deps_of_modules_dir};
use pretty_assertions::assert_eq;
use std::{collections::HashSet, ffi::OsStr, fs, path::Path};
use tempfile::TempDir;

fn write_file(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
    fs::write(path, content).expect("write file");
}

/// A publish directory that `prepare` has not (re)built yet reads back empty
/// from `DirectoryFetcher`, which would otherwise diff as "the target holds
/// everything the source doesn't" and delete the already-injected copy.
#[test]
fn sync_leaves_an_injected_copy_alone_when_its_publish_directory_is_missing() {
    let dir = TempDir::new().expect("temp dir");
    let workspace = dir.path();

    write_file(
        &workspace.join("project-1/package.json"),
        &serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "publishConfig": { "directory": "dist" },
        })
        .to_string(),
    );
    // project-1/dist is intentionally never created here, standing in for a
    // publish directory that only a `prepare` script builds, which did not
    // run in this sync.

    let target = workspace.join("project-2/node_modules/project-1");
    write_file(&target.join("index.js"), "already built");

    write_file(
        &workspace.join("node_modules/.modules.yaml"),
        &serde_json::json!({
            "virtualStoreDir": "node_modules/.pnpm",
            "injectedDeps": {
                "project-1/dist": ["project-2/node_modules/project-1"],
            },
        })
        .to_string(),
    );

    let manifest = serde_json::json!({
        "name": "project-1",
        "publishConfig": { "directory": "dist" },
    });
    sync_injected_deps(&SyncInjectedDeps {
        pkg_name: Some("project-1"),
        pkg_root_dir: Path::new("project-1"),
        workspace_dir: Some(workspace),
        modules_dir_name: OsStr::new("node_modules"),
        workspace_modules_dir: &workspace.join("node_modules"),
        extend_node_path: false,
        manifest_before_scripts: Some(&manifest),
        ignored_directories: Vec::new(),
    })
    .expect("sync should not fail when the publish directory is missing");

    assert_eq!(
        fs::read_to_string(target.join("index.js")).expect("read the injected copy"),
        "already built",
        "a missing publish directory must not wipe the already-injected copy",
    );
    assert!(!workspace.join("project-1/dist").exists(), "the publish directory stays missing");
}

/// Same scenario as
/// [`sync_leaves_an_injected_copy_alone_when_its_publish_directory_is_missing`],
/// through the dedicated-per-project-lockfile entry point instead.
#[test]
fn sync_of_modules_dir_leaves_an_injected_copy_alone_when_its_publish_directory_is_missing() {
    let dir = TempDir::new().expect("temp dir");
    let workspace = dir.path();
    let project_1 = workspace.join("project-1");
    let project_2 = workspace.join("project-2");

    write_file(
        &project_1.join("package.json"),
        &serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "publishConfig": { "directory": "dist" },
        })
        .to_string(),
    );
    // project-1/dist is intentionally never created here, standing in for a
    // publish directory that only a `prepare` script builds, which did not
    // run in this sync.

    let target = project_2.join("node_modules/project-1");
    write_file(&target.join("index.js"), "already built");

    write_file(
        &project_2.join("node_modules/.modules.yaml"),
        &serde_json::json!({
            "virtualStoreDir": "node_modules/.pnpm",
            "injectedDeps": {
                "../project-1/dist": ["node_modules/project-1"],
            },
        })
        .to_string(),
    );

    // The set the caller filters `injectedDeps` sources against, keyed the
    // same way `collect_injected_deps` resolves a `file:` snapshot's source:
    // publish-directory-inclusive, and lexically normalized.
    let source_dirs: HashSet<_> = [pnpm_fs::lexical_normalize(&project_1.join("dist"))].into();
    sync_injected_deps_of_modules_dir(&project_2, &project_2.join("node_modules"), &source_dirs)
        .expect("sync should not fail when the publish directory is missing");

    assert_eq!(
        fs::read_to_string(target.join("index.js")).expect("read the injected copy"),
        "already built",
        "a missing publish directory must not wipe the already-injected copy",
    );
    assert!(!project_1.join("dist").exists(), "the publish directory stays missing");
}
