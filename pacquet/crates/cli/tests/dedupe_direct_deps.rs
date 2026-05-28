//! `dedupeDirectDeps` workspace coverage for `pacquet install`.
//!
//! Ports pnpm's
//! [`installing/deps-installer/test/install/dedupeDirectDeps.ts`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts):
//! when the workspace root provides the same `(alias → resolution)`
//! a non-root project depends on, the dep must not appear under
//! that project's `node_modules/`, and a project whose direct deps
//! are entirely deduped must not have a `node_modules/` created at
//! all. Setting `dedupeDirectDeps: false` must restore the
//! per-project symlinks.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, path::Path, process::Command};

/// With `dedupeDirectDeps` left at its default (`true`), a sibling
/// project whose only direct dep is also a direct dep of the
/// workspace root must not get a `node_modules/` of its own.
#[test]
fn dedupes_direct_deps_against_workspace_root_by_default() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    pacquet.with_arg("install").assert().success();

    // Root still has the dep linked.
    let root_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&root_dep).expect("query root symlink"),
        "root node_modules direct-dep symlink missing",
    );

    // The deduped sibling has no node_modules at all — pnpm's
    // `linkDirectDepsAndDedupe` ends with `rimraf(project.modulesDir)`
    // when every dep was deduped. Pacquet achieves the same effect
    // by never creating the directory in the first place.
    assert!(
        !workspace.join("packages/dup/node_modules").exists(),
        "packages/dup/node_modules should not exist when every direct dep is deduped against root",
    );

    drop((root, mock_instance));
}

/// `dedupeDirectDeps: false` opts out — every sibling gets its
/// own per-project symlink even when the workspace root already
/// resolves the same alias to the same target.
#[test]
fn dedupe_direct_deps_disabled_keeps_per_project_symlinks() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeDirectDeps: false\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    pacquet.with_arg("install").assert().success();

    let root_dep = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&root_dep).expect("query root symlink"),
        "root node_modules direct-dep symlink missing",
    );
    let sibling_dep = workspace.join("packages/dup/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&sibling_dep).expect("query sibling symlink"),
        "sibling direct-dep symlink should be kept when dedupeDirectDeps: false",
    );

    drop((root, mock_instance));
}

/// A frozen-lockfile install (the headless path) dedupes too.
/// Mirrors pnpm's second `mutateModules(... frozenLockfile: true)`
/// call in
/// [`dedupeDirectDeps.ts:107`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts#L107)
/// which asserts the same on-disk shape after running through the
/// `install_frozen_lockfile` codepath.
#[test]
fn dedupes_direct_deps_with_frozen_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    // First install seeds the lockfile and node_modules.
    pacquet.with_arg("install").assert().success();
    assert!(
        !workspace.join("packages/dup/node_modules").exists(),
        "first install should already have skipped packages/dup/node_modules creation",
    );

    // Tear down node_modules so the frozen-lockfile install is a
    // pure replay (pnpm's test does the same via `rimrafSync`).
    fs_remove_dir_all(&workspace.join("node_modules"));
    fs_remove_dir_all(&workspace.join("packages/dup/node_modules"));

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin"))
            .expect("query root symlink after frozen install"),
        "frozen-lockfile install should re-link the root's direct dep",
    );
    assert!(
        !workspace.join("packages/dup/node_modules").exists(),
        "frozen-lockfile install should keep packages/dup/node_modules absent",
    );

    drop((root, mock_instance));
}

fn fs_remove_dir_all(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {path:?}: {error}"),
    }
}

/// Build a fresh `pacquet` `Command` rooted at `workspace`. Used to
/// drive a second invocation in the same workspace because
/// [`assert_cmd::Command::assert`] consumes the wrapped command.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// Partial dedupe: a sibling with one shared dep and one unique
/// dep keeps the unique dep symlinked under its `node_modules/`
/// while the shared dep is omitted.
#[test]
fn dedupes_only_overlapping_direct_deps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/mixed")).expect("mkdir packages/mixed");
    fs::write(
        workspace.join("packages/mixed/package.json"),
        serde_json::json!({
            "name": "@scope/mixed",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0",
                "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write packages/mixed/package.json");

    pacquet.with_arg("install").assert().success();

    let mixed_modules = workspace.join("packages/mixed/node_modules");
    let shared = mixed_modules.join("@pnpm.e2e/hello-world-js-bin");
    assert!(
        !shared.exists(),
        "shared direct-dep should be deduped against root, but found {shared:?}",
    );
    let unique = mixed_modules.join("@pnpm.e2e/hello-world-js-bin-parent");
    assert!(
        is_symlink_or_junction(&unique).expect("query unique symlink"),
        "unique direct-dep symlink missing under packages/mixed/node_modules",
    );

    drop((root, mock_instance));
}

/// Mirrors pnpm's [`'dedupe direct dependencies after public hoisting'`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/dedupeDirectDeps.ts#L113):
/// a transitive of the root that gets publicly hoisted into root's
/// `node_modules/` should dedupe a non-root importer's *direct* dep
/// with the same alias.
///
/// Asserts the dedupe behavior on the frozen-lockfile path: pacquet
/// pre-computes the hoist plan before the symlink phase so the dedupe
/// map folds in publicly-hoisted aliases. The seed install (no public
/// hoist pattern) writes the lockfile; the frozen install then runs
/// with the pattern set, hoists `dep-of-pkg-with-1-dep` to root, and
/// the dedupe pass skips re-creating the project-2 symlink for it.
#[test]
fn dedupes_direct_dep_against_publicly_hoisted_root_dep() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    let base_yaml = workspace_yaml.clone();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, &workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/dup")).expect("mkdir packages/dup");
    fs::write(
        workspace.join("packages/dup/package.json"),
        serde_json::json!({
            "name": "@scope/dup",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/dup/package.json");

    // Seed install (no publicHoistPattern → no hoist, no dedupe
    // surprises here). The point is to produce the lockfile pacquet's
    // frozen-install path will replay.
    pacquet.with_arg("install").assert().success();

    // Flip on publicHoistPattern and clear node_modules so the
    // frozen-install path is a pure replay.
    let mut workspace_yaml = base_yaml;
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\npublicHoistPattern:\n  - '@pnpm.e2e/dep-of-pkg-with-1-dep'\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    fs_remove_dir_all(&workspace.join("node_modules"));
    fs_remove_dir_all(&workspace.join("packages/dup/node_modules"));

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    // Root has both direct + hoisted entries.
    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep"))
            .expect("query root direct dep"),
        "root should still have its direct dep symlinked",
    );
    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep"))
            .expect("query root public-hoisted dep"),
        "publicHoistPattern should land the transitive at root/node_modules",
    );
    // Sibling has no node_modules because its only direct dep was
    // deduped against root's public-hoisted entry.
    assert!(
        !workspace.join("packages/dup/node_modules").exists(),
        "packages/dup/node_modules should not exist: dep-of-pkg-with-1-dep deduped against publicly-hoisted root entry",
    );

    drop((root, mock_instance));
}

/// Mirrors pnpm's [`'shamefully-hoist + dedupe-direct-deps=true'`](https://github.com/pnpm/pnpm/blob/39101f5e37/pnpm/test/install/hoist.ts#L77):
/// with `publicHoistPattern: ['*']` (the explicit form of
/// `shamefullyHoist: true` — pacquet doesn't bridge the legacy flag to
/// the pattern, see [`hoist::shamefully_hoist_legacy_publicly_hoists_everything`]),
/// every transitive lands at the workspace root's `node_modules/`. A
/// non-root importer's direct dep that also lands at root via hoist
/// gets deduped from the importer.
///
/// Runs through pacquet's fresh-install path (single `pacquet install`,
/// no `--frozen-lockfile`), exercising the hoist pass that fresh
/// install now runs end-to-end.
#[test]
fn dedupe_under_shamefully_hoist() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\n\
         shamefullyHoist: true\n\
         hoistPattern: []\n\
         publicHoistPattern:\n  - '*'\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/project")).expect("mkdir packages/project");
    fs::write(
        workspace.join("packages/project/package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foobar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/project/package.json");

    pacquet.with_arg("install").assert().success();

    // Root has every direct dep + every shamefully-hoisted transitive.
    for alias in [
        "@pnpm.e2e/pkg-with-1-dep",
        "@pnpm.e2e/dep-of-pkg-with-1-dep",
        "@pnpm.e2e/foobar",
        "@pnpm.e2e/foo",
    ] {
        let entry = workspace.join("node_modules").join(alias);
        assert!(
            is_symlink_or_junction(&entry).expect("query root entry"),
            "expected root/node_modules/{alias} to be a symlink",
        );
    }

    // Project has neither its direct dep `foobar` (deduped against the
    // shamefully-hoisted root entry) nor its transitive `foo` (which
    // never reaches the project's `node_modules/` to begin with —
    // transitives only materialize via hoist).
    let project_modules = workspace.join("packages/project/node_modules");
    assert!(
        !project_modules.join("@pnpm.e2e/foobar").exists(),
        "project's foobar should be deduped against the shamefully-hoisted root entry",
    );
    assert!(
        !project_modules.join("@pnpm.e2e/foo").exists(),
        "transitive `foo` should only appear at root via hoist, not under project",
    );

    drop((root, mock_instance));
}
