//! The generated workspace behind the linked-workspace scenario.
//!
//! Every project links the whole next level through `workspace:*`, and every
//! linked project declares a peer its consumers provide. The install's peer
//! report therefore has to walk a `link:` graph whose subgraphs hundreds of
//! importers share, which is the shape that made
//! [pnpm/pnpm#14906](https://github.com/pnpm/pnpm/issues/14906) quadratic.
//!
//! The fixture names no registry package, so the timed offline resolve
//! measures the workspace walk and nothing else.

use super::{
    LINKED_WORKSPACE_DEPTH,
    LINKED_WORKSPACE_VERSION,
    LINKED_WORKSPACE_WIDTH,
};
use serde_json::Value;
use std::{
    fs,
    path::Path,
};

/// The workspace project every other project both depends on and declares as
/// its peer. Present in each consumer's own dependencies, so the peer always
/// resolves and `autoInstallPeers` never reaches for the registry.
const SHARED: &str = "@pnpmtest/linked-benchmark-shared";

pub(super) fn root_manifest() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": "linked-workspace-benchmark-root",
        "version": LINKED_WORKSPACE_VERSION,
        "private": true,
        "dependencies": level_dependencies(0),
    }))
    .expect("serialize linked-workspace root manifest")
}

/// Write every non-root project of the fixture under `<dst_dir>/packages`.
pub(super) fn create_projects(dst_dir: &Path) {
    let packages_dir = dst_dir.join("packages");
    if packages_dir.exists() {
        fs::remove_dir_all(&packages_dir).expect("clear the linked-workspace projects");
    }
    write_project(&packages_dir.join("shared"), SHARED, &serde_json::Map::new(), false);
    for level in 0..LINKED_WORKSPACE_DEPTH {
        let dependencies = level_dependencies(level + 1);
        for index in 0..LINKED_WORKSPACE_WIDTH {
            write_project(
                &packages_dir.join(format!("level-{level}-{index:02}")),
                &package_name(level, index),
                &dependencies,
                true,
            );
        }
    }
}

/// `workspace:*` on the shared project plus every project of `level`, which
/// is empty past the last level.
fn level_dependencies(level: usize) -> serde_json::Map<String, Value> {
    let names = (level < LINKED_WORKSPACE_DEPTH)
        .then(|| (0..LINKED_WORKSPACE_WIDTH).map(move |index| package_name(level, index)))
        .into_iter()
        .flatten();
    std::iter::once(SHARED.to_string())
        .chain(names)
        .map(|name| (name, Value::String("workspace:*".to_string())))
        .collect()
}

fn write_project(
    dir: &Path,
    name: &str,
    dependencies: &serde_json::Map<String, Value>,
    declares_peer: bool,
) {
    let mut manifest = serde_json::json!({
        "name": name,
        "version": LINKED_WORKSPACE_VERSION,
        "private": true,
        "dependencies": dependencies,
    });
    if declares_peer {
        let peer_dependencies = serde_json::Map::from_iter([(
            SHARED.to_string(),
            Value::String(LINKED_WORKSPACE_VERSION.to_string()),
        )]);
        manifest
            .as_object_mut()
            .expect("package manifest is an object")
            .insert("peerDependencies".to_string(), Value::Object(peer_dependencies));
    }
    fs::create_dir_all(dir).expect("create a linked-workspace project directory");
    fs::write(
        dir.join("package.json"),
        serde_json::to_vec(&manifest).expect("serialize a linked-workspace manifest"),
    )
    .expect("write a linked-workspace manifest");
}

fn package_name(level: usize, index: usize) -> String {
    format!("@pnpmtest/linked-benchmark-level-{level}-{index:02}")
}
