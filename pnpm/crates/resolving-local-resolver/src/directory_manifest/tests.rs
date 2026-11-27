use super::{
    LocalDirectory,
    LocalDirectoryUse,
    PublishingProjectSearch,
    WorkspaceProjectManifests,
    local_directory_manifest,
};
use pnpm_resolving_resolver_base::{
    WorkspacePackage,
    WorkspacePackages,
};
use pretty_assertions::assert_eq;
use serde_json::{
    Value,
    json,
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::Arc,
};
use tempfile::TempDir;

fn no_projects<'a>(_: &Path) -> Option<&'a Value> {
    None
}

fn describe<'a>(
    dir: &'a Path,
    usage: LocalDirectoryUse,
    workspace_root: &'a Path,
    project_manifest: &'a dyn Fn(&Path) -> Option<&'a Value>,
) -> Option<Value> {
    local_directory_manifest(&LocalDirectory {
        dir,
        usage,
        workspace_root,
        project_manifest,
        publishing_project_search: PublishingProjectSearch::Anywhere,
    })
    .expect("read the manifest")
}

#[test]
fn a_directory_is_described_by_any_supported_manifest_file() {
    let manifests = [
        ("package.json", r#"{"name":"lib","dependencies":{"dep":"1.0.0"}}"#),
        ("package.json5", "{name: 'lib', dependencies: {dep: '1.0.0'}}"),
        ("package.yaml", "name: lib\ndependencies:\n  dep: 1.0.0\n"),
    ];
    for (basename, contents) in manifests {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join(basename), contents).expect("write the manifest");
        for usage in [LocalDirectoryUse::Copied, LocalDirectoryUse::Linked] {
            assert_eq!(
                describe(tmp.path(), usage, tmp.path(), &no_projects),
                Some(json!({ "name": "lib", "dependencies": { "dep": "1.0.0" } })),
                "{basename} read for a {usage:?} directory",
            );
        }
    }
}

#[test]
fn a_project_held_in_memory_replaces_its_manifest_file() {
    let tmp = TempDir::new().expect("tempdir");
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).expect("create the project");
    fs::write(lib.join("package.json"), r#"{"name":"lib","version":"1.0.0"}"#)
        .expect("write the manifest file");
    let in_memory = json!({ "name": "lib", "version": "1.0.0", "dependencies": { "dep": "1" } });
    let packages: WorkspacePackages = BTreeMap::from([(
        "lib".to_string(),
        BTreeMap::from([(
            "1.0.0".to_string(),
            WorkspacePackage { root_dir: lib.join("../lib"), manifest: in_memory.clone() },
        )]),
    )]);
    let projects = WorkspaceProjectManifests::new(Arc::new(packages));
    for usage in [LocalDirectoryUse::Copied, LocalDirectoryUse::Linked] {
        let described = describe(&lib, usage, tmp.path(), &|dir| projects.manifest_at(dir));
        assert_eq!(described.as_ref(), Some(&in_memory), "a {usage:?} project");
    }
}

#[test]
fn a_publishing_project_held_in_memory_describes_its_copied_publish_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(lib.join("dist")).expect("create the publish directory");
    fs::write(lib.join("package.json"), r#"{"name":"lib"}"#).expect("write the manifest file");
    fs::write(lib.join("dist/package.json"), r#"{"name":"built"}"#)
        .expect("write the built manifest");
    let in_memory = json!({ "name": "lib", "publishConfig": { "directory": "dist" } });
    let project_manifest = |dir: &Path| (dir == lib).then_some(&in_memory);
    let dist = lib.join("dist");
    assert_eq!(
        describe(&dist, LocalDirectoryUse::Copied, tmp.path(), &project_manifest),
        Some(in_memory.clone()),
    );
    assert_eq!(
        describe(&dist, LocalDirectoryUse::Linked, tmp.path(), &project_manifest),
        Some(json!({ "name": "built" })),
    );
}

#[test]
fn an_unreadable_ancestor_fails_only_a_directory_without_a_manifest() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(tmp.path().join("package.json"), "{").expect("write a malformed manifest");
    let dir = tmp.path().join("vendor/dep");
    fs::create_dir_all(&dir).expect("create the directory");
    let read = || {
        local_directory_manifest(&LocalDirectory {
            dir: &dir,
            usage: LocalDirectoryUse::Copied,
            workspace_root: tmp.path(),
            project_manifest: &no_projects,
            publishing_project_search: PublishingProjectSearch::Anywhere,
        })
    };
    read().expect_err("the publishing project search reads the malformed ancestor");
    fs::write(dir.join("package.json"), r#"{"name":"dep"}"#).expect("write the manifest");
    assert_eq!(read().expect("read the manifest"), Some(json!({ "name": "dep" })));
}

#[test]
fn nothing_describes_a_directory_without_a_manifest_or_a_publishing_project() {
    let tmp = TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("ws");
    let dir = workspace.join("dep");
    fs::create_dir_all(&dir).expect("create the directory");
    let described = local_directory_manifest(&LocalDirectory {
        dir: &dir,
        usage: LocalDirectoryUse::Copied,
        workspace_root: &workspace,
        project_manifest: &no_projects,
        publishing_project_search: PublishingProjectSearch::InsideWorkspace,
    });
    assert_eq!(described.expect("read the manifest"), None);
}
