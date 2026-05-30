//! Multi-importer fresh-resolve coverage for `pacquet install` in a
//! `pnpm-workspace.yaml` monorepo.
//!
//! Issue [#11901](https://github.com/pnpm/pnpm/issues/11901): before
//! the fix, only the workspace root manifest got walked, so sibling
//! projects' deps never landed in the lockfile or on disk. This test
//! installs a two-project workspace from scratch (no lockfile, no
//! `--frozen-lockfile`) and asserts every importer has its own
//! lockfile entry, every direct dep is symlinked under each
//! importer's `node_modules`, and shared transitive deps land once
//! in the virtual store.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::fs;

/// A workspace with two sibling projects, each pulling in a
/// different mocked package, runs through the fresh-resolve path and
/// writes per-importer lockfile entries plus per-importer
/// `node_modules` symlinks.
#[test]
fn fresh_resolve_walks_every_workspace_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Workspace root manifest: empty so any deps installed are
    // attributable to the sibling importers below.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    // `packages/*` pattern picks up both siblings. Append to the
    // pnpm-workspace.yaml the helper already wrote (which holds
    // `storeDir` / `cacheDir`).
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Two siblings with distinct direct deps. Using two different
    // packages (rather than one shared dep) makes the per-importer
    // entry assertions less ambiguous.
    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "@scope/a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "@scope/b",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/b/package.json");

    // Run the install. No --frozen-lockfile and no pre-existing
    // lockfile → fresh-resolve path.
    pacquet.with_arg("install").assert().success();

    // Each sibling has its own direct-dep symlink. Before the fix,
    // these were absent because pacquet only walked the root.
    let a_dep = workspace.join("packages/a/node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(
        is_symlink_or_junction(&a_dep).expect("query packages/a symlink"),
        "packages/a/node_modules direct-dep symlink missing — sibling importer's deps weren't walked",
    );
    let b_dep = workspace.join("packages/b/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&b_dep).expect("query packages/b symlink"),
        "packages/b/node_modules direct-dep symlink missing — sibling importer's deps weren't walked",
    );

    // Shared virtual store: both packages land under
    // `<workspace>/node_modules/.pnpm/<name>@<version>` exactly once.
    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin-parent@1.0.0").exists(),
        "hello-world-js-bin-parent virtual-store entry missing",
    );
    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin@1.0.0").exists(),
        "hello-world-js-bin virtual-store entry missing",
    );

    // Lockfile records every importer. Before the fix, only `.`
    // appeared.
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("packages/a:"),
        "pnpm-lock.yaml missing importers entry for packages/a:\n{lockfile}",
    );
    assert!(
        lockfile.contains("packages/b:"),
        "pnpm-lock.yaml missing importers entry for packages/b:\n{lockfile}",
    );
    // hello-world-js-bin-parent is a direct dep of packages/a, so it
    // should appear in that importer's section — not just in
    // `packages:` where any transitive could also surface the name.
    // Slice the lockfile to packages/a's importer block and check
    // there.
    let a_importer_section = lockfile
        .split("  packages/a:\n")
        .nth(1)
        .and_then(|tail| tail.split("\n  packages/").next())
        .expect("pnpm-lock.yaml missing packages/a importer section");
    assert!(
        a_importer_section.contains("hello-world-js-bin-parent"),
        "pnpm-lock.yaml packages/a importer missing hello-world-js-bin-parent:\n{lockfile}",
    );

    drop((root, mock_instance));
}
