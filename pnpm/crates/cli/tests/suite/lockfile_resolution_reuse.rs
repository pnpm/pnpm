//! A second non-frozen install reuses the prior lockfile's resolution
//! and transitive subtree for an unchanged dependency, instead of
//! re-resolving it from the registry.
//!
//! See `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`. pacquet avoids
//! re-resolving an unchanged tree by reading the prior lockfile's
//! recorded resolution + child refs, so a re-install with the registry
//! gone still succeeds for the unchanged subtree.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::bump_mtime,
};
use std::{fs, net::TcpListener, path::Path, process::Command};

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

#[test]
fn compatible_package_range_update_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\nfetchRetries: 0\nfetchTimeout: 1000\n"),
    )
    .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-optional-peer-with-peer": ">=1.0.0 <2"
            }
        })
        .to_string(),
    )
    .expect("update dependency range");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let name = "@pnpm.e2e/has-optional-peer-with-peer".parse().expect("package name");
    assert_eq!(
        wanted.importers["."].dependencies.as_ref().expect("dependencies")[&name].specifier,
        ">=1.0.0 <2",
    );

    drop((root, mock_instance));
}

#[test]
fn compatible_catalog_range_update_reuses_the_locked_peer_snapshot() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-optional-peer-with-peer": "catalog:"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}trustLockfile: true\nfetchRetries: 0\nfetchTimeout: 1000\ncatalog:\n  '@pnpm.e2e/has-optional-peer-with-peer': ^1.0.0\n",
        ),
    )
    .expect("write initial catalog");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial catalog");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace(
            "'@pnpm.e2e/has-optional-peer-with-peer': ^1.0.0",
            "'@pnpm.e2e/has-optional-peer-with-peer': '>=1.0.0 <2'",
        ),
    )
    .expect("update catalog range");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let entry = &wanted.catalogs.expect("catalog snapshots")["default"]["@pnpm.e2e/has-optional-peer-with-peer"];
    assert_eq!(entry.specifier, ">=1.0.0 <2");
    assert_eq!(entry.version, "1.0.0");

    drop((root, mock_instance));
}

/// A `registry=` URL on a localhost port with nothing listening, so any
/// resolution attempt against it fails fast with a connection refusal.
fn dead_registry_url() -> String {
    // Bind to an ephemeral port, read it, then drop the listener so the
    // port is (almost certainly) free again — anything that connects to
    // it gets refused.
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port to learn a free one");
    let addr = listener.local_addr().expect("read the ephemeral port");
    drop(listener);
    format!("http://127.0.0.1:{}/", addr.port())
}

#[test]
fn exact_override_update_reuses_the_locked_children() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let manifest_path = fixture.workspace.join("package.json");
    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/parent-of-pkg-with-1-dep": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}overrides:\n  '@pnpm.e2e/pkg-with-1-dep': 100.0.0\n"),
    )
    .expect("write initial override");
    pacquet_at(&fixture.workspace).with_arg("install").assert().success();

    let before = pnpm_lockfile::Lockfile::load_wanted_from_dir(&fixture.workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let old_key = "@pnpm.e2e/pkg-with-1-dep@100.0.0".parse().expect("old key");
    let child_name = "@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("child name");
    let old_child = before
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&old_key))
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&child_name))
        .cloned()
        .expect("old locked child");

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial override");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml
            .replace("'@pnpm.e2e/pkg-with-1-dep': 100.0.0", "'@pnpm.e2e/pkg-with-1-dep': 100.1.0"),
    )
    .expect("update exact override");
    pacquet_at(&fixture.workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&fixture.workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let current = pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(
        &fixture.workspace.join("node_modules/.pnpm"),
    )
    .expect("load current lockfile")
    .expect("current lockfile");
    let new_key = "@pnpm.e2e/pkg-with-1-dep@100.1.0".parse().expect("new key");
    for lockfile in [&wanted, &current] {
        assert_eq!(
            lockfile
                .snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&new_key))
                .and_then(|snapshot| snapshot.dependencies.as_ref())
                .and_then(|dependencies| dependencies.get(&child_name)),
            Some(&old_child),
        );
        assert!(
            lockfile.snapshots.as_ref().is_some_and(|snapshots| !snapshots.contains_key(&old_key)),
        );
    }

    drop(fixture);
}

#[test]
fn dependency_removal_override_prunes_the_locked_subtree_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}overrides:\n  is-positive: '-'\n"))
        .expect("add dependency removal override");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let current = pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(
        &workspace.join("node_modules/.pnpm"),
    )
    .expect("load current lockfile")
    .expect("current lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    let removed_name = "is-positive".parse().expect("removed package name");
    for lockfile in [&wanted, &current] {
        dbg!(&lockfile.snapshots, &lockfile.packages);
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&parent_key))
                .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
                .is_none_or(|dependencies| !dependencies.contains_key(&removed_name)),
        );
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
        );
        assert!(
            lockfile.packages.as_ref().is_none_or(|packages| !packages.contains_key(&removed_key)),
        );
    }
    dbg!(&workspace);
    assert!(
        !workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive",
            )
            .exists(),
    );

    drop((root, mock_instance));
}

/// A changed override is absorbed even though an unchanged override's
/// configured value is a `catalog:` reference. Override values are
/// compared catalog-resolved, so with the catalogs settled the
/// `catalog:` override shows no drift and only the added removal is
/// applied — which the dead registry proves needs no resolution.
#[test]
fn removal_override_composes_with_a_settled_catalog_override() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
                "@pnpm.e2e/bar": "catalog:"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}trustLockfile: true\ncatalog:\n  '@pnpm.e2e/bar': 100.0.0\n  '@pnpm.e2e/foo': 1.0.0\noverrides:\n  '@pnpm.e2e/foo': 'catalog:'\n",
        ),
    )
    .expect("write the settled catalog override");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}  is-positive: '-'\n"))
        .expect("add dependency removal override");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let overrides = wanted.overrides.as_ref().expect("recorded overrides");
    assert_eq!(overrides["@pnpm.e2e/foo"], "1.0.0");
    assert_eq!(overrides["is-positive"], "-");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    assert!(
        wanted.snapshots.as_ref().is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
    );

    drop((root, mock_instance));
}

/// An override on a cataloged package replaces the `catalog:` specifier
/// outright, so the entry is dropped rather than moved. The seed only
/// feeds the resolver — the catalogs section is rebuilt from what the
/// resolution recorded — so the rewrite needs no guard for this.
#[test]
fn an_override_on_a_cataloged_package_drops_the_catalog_entry() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let manifest_path = fixture.workspace.join("package.json");
    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "catalog:"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}catalog:\n  '@pnpm.e2e/pkg-with-1-dep': 100.0.0\n"),
    )
    .expect("write initial catalog");
    pacquet_at(&fixture.workspace).with_arg("install").assert().success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial catalog");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}overrides:\n  '@pnpm.e2e/pkg-with-1-dep': 100.1.0\n"),
    )
    .expect("add the override");
    pacquet_at(&fixture.workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&fixture.workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(wanted.catalogs.is_none());
    let name = "@pnpm.e2e/pkg-with-1-dep".parse().expect("package name");
    let dependency = &wanted.importers["."].dependencies.as_ref().expect("dependencies")[&name];
    assert_eq!(dependency.specifier, "100.1.0");
    assert_eq!(dependency.version.to_string(), "100.1.0");

    drop(fixture);
}

/// Both resolver-consulting rewrites in one edit: the catalog rewrite
/// settles the widened range and the override rewrite replays the removal
/// onto its result, so neither has to be the only change.
#[test]
fn a_catalog_edit_and_a_removal_override_are_absorbed_in_one_pass() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "catalog:"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}trustLockfile: true\nfetchRetries: 0\nfetchTimeout: 1000\ncatalog:\n  '@pnpm.e2e/pkg-with-good-optional': ^1.0.0\n",
        ),
    )
    .expect("write initial catalog");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial catalog");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{}overrides:\n  is-positive: '-'\n",
            workspace_yaml.replace(
                "'@pnpm.e2e/pkg-with-good-optional': ^1.0.0",
                "'@pnpm.e2e/pkg-with-good-optional': '>=1.0.0 <2'",
            ),
        ),
    )
    .expect("widen the catalog range and add the removal override");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let entry = &wanted.catalogs.as_ref().expect("catalog snapshots")["default"]["@pnpm.e2e/pkg-with-good-optional"];
    assert_eq!(entry.specifier, ">=1.0.0 <2");
    assert_eq!(entry.version, "1.0.0");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    assert!(
        wanted.snapshots.as_ref().is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
    );
    assert!(wanted.packages.as_ref().is_none_or(|packages| !packages.contains_key(&removed_key)));

    drop((root, mock_instance));
}

#[test]
fn adding_and_removing_an_ignored_optional_dependency_uses_the_safe_path() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}ignoredOptionalDependencies:\n  - is-positive\n"),
    )
    .expect("add ignored optional dependency");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let current = pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(
        &workspace.join("node_modules/.pnpm"),
    )
    .expect("load current lockfile")
    .expect("current lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    let removed_name = "is-positive".parse().expect("removed package name");
    for lockfile in [&wanted, &current] {
        assert_eq!(
            lockfile.ignored_optional_dependencies.as_deref(),
            Some(["is-positive".to_string()].as_slice()),
        );
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&parent_key))
                .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
                .is_none_or(|dependencies| !dependencies.contains_key(&removed_name)),
        );
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
        );
        assert!(
            lockfile.packages.as_ref().is_none_or(|packages| !packages.contains_key(&removed_key)),
        );
    }
    assert!(
        !workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive",
            )
            .exists(),
    );

    fs::write(&npmrc_path, live_npmrc).expect("restore live registry");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace("ignoredOptionalDependencies:\n  - is-positive\n", ""),
    )
    .expect("remove ignored optional dependency");
    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        !String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let restored_key = "is-positive@1.0.0".parse().expect("restored package key");
    let restored_name = "is-positive".parse().expect("restored package name");
    assert!(
        wanted
            .snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&parent_key))
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
            .is_some_and(|dependencies| dependencies.contains_key(&restored_name)),
    );
    assert!(
        wanted.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&restored_key)),
    );
    assert!(
        workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive",
            )
            .exists(),
    );

    drop((root, mock_instance));
}

#[test]
fn reuses_unchanged_subtree_without_re_resolving_from_the_registry() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    // Trust the lockfile so the post-resolution verifier doesn't fetch
    // each entry's metadata from the registry — that verification is a
    // separate concern from resolution reuse, and (now that it always runs
    // and fails closed) it would hit the dead registry regardless of
    // whether resolution was reused, masking what this test proves.
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{existing}trustLockfile: true\n"))
        .expect("append trustLockfile to pnpm-workspace.yaml");

    // `@pnpm.e2e/pkg-with-1-dep@100.0.0` depends on
    // `@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`, so the lockfile records
    // a two-node subtree (the direct dep plus its transitive child).
    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } })
            .to_string(),
    )
    .expect("write package.json");

    // Fresh install against the live registry: warms the store and writes
    // the lockfile.
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0")
            && lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@"),
        "the fresh install must record the direct dep and its transitive child:\n{lockfile}",
    );

    // Repoint the registry at a dead port. Any re-resolution now fails.
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    // Widen the range to `^100.0.0`. The locked `100.0.0` still satisfies
    // it (so the dep is reusable), but the manifest change forces the
    // non-frozen fresh-lockfile resolution path rather than the
    // up-to-date short-circuit.
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "^100.0.0" } })
            .to_string(),
    )
    .expect("rewrite package.json with a widened range");

    // Succeeds only because the unchanged subtree is reused from the
    // lockfile — re-resolving either package would hit the dead registry.
    pacquet_at(&workspace).with_arg("install").assert().success();

    drop((root, mock_instance));
}

/// A lockfile produced via the reuse path is byte-for-byte identical to
/// one produced by resolving the same manifest entirely from scratch.
///
/// The discriminating test above proves reuse *fires*; this proves it's
/// *correct* — that reusing an unchanged subtree yields the same tree a
/// fresh resolve would, so reuse can never silently drift the resolution.
///
/// Compared **byte-for-byte**: the writer sorts every lockfile map by its
/// rendered key, so build-insertion order no longer leaks into the file. A
/// reuse build and a fresh build of the same manifest therefore emit
/// identical bytes — this is the byte-stability guarantee from
/// [#12117](https://github.com/pnpm/pnpm/issues/12117).
#[test]
fn a_reused_tree_is_structurally_identical_to_a_fresh_resolve() {
    let both = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }
    })
    .to_string();

    let reused = CommandTempCwd::init().add_mocked_registry();
    let reused_manifest = reused.workspace.join("package.json");
    fs::write(
        &reused_manifest,
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } })
            .to_string(),
    )
    .expect("write the reuse scenario's initial manifest");
    pacquet_at(&reused.workspace).with_arg("install").assert().success();
    fs::write(&reused_manifest, &both).expect("add the second dep to the reuse scenario");
    bump_mtime(&reused_manifest);
    pacquet_at(&reused.workspace).with_arg("install").assert().success();
    let reused_lockfile =
        fs::read_to_string(reused.workspace.join("pnpm-lock.yaml")).expect("read reused lockfile");

    let fresh = CommandTempCwd::init().add_mocked_registry();
    fs::write(fresh.workspace.join("package.json"), &both).expect("write the fresh manifest");
    pacquet_at(&fresh.workspace).with_arg("install").assert().success();
    let fresh_lockfile =
        fs::read_to_string(fresh.workspace.join("pnpm-lock.yaml")).expect("read fresh lockfile");

    pretty_assertions::assert_eq!(
        reused_lockfile,
        fresh_lockfile,
        "a tree built via subtree reuse must serialize byte-for-byte identically to a fresh resolve",
    );

    drop((reused, fresh));
}

/// An edge denied subtree reuse re-resolves from the registry instead of
/// reading another edge's reused resolution out of the wanted-dep cache.
///
/// The synthesized `ResolveResult` a reused node is built from carries a
/// manifest without `dependencies` (a reused node's children come from
/// the snapshot graph), so it must never satisfy a fresh-resolve cache
/// lookup: a fresh edge walks children from the manifest, and the
/// dependency-less manifest would make it record the package as a leaf.
/// When that leaf occurrence sits at a shallower depth than the healthy
/// reused occurrence it wins children ownership, so the package's
/// snapshot collapses to `{}`, its peer suffix is dropped, and its
/// dependents re-point at the bare instance
/// (`'@yarnpkg/shell@4.0.0': {}` in the original report).
///
/// Scenario, driven by the `@pnpm.e2e/reuse-chain-*` fixtures
/// (`grand → parent → target`, where `target` deps `@pnpm.e2e/abc` +
/// `@pnpm.e2e/dep-of-pkg-with-1-dep`):
///
/// * `pkg-a` deps `grand`, so its unchanged walk reuses `target` at
///   depth 2 and caches the synthesized resolution under the exact-pin
///   wanted key.
/// * `pkg-b` deps `parent` plus `dep-of-pkg-with-1-dep` directly. The
///   test bumps that direct dep; `target`'s snapshot also depends on
///   it, so pkg-b's transitive edge to `target` (depth 1) is denied
///   reuse by the changed-direct-dep gate and resolves fresh — with
///   the same wanted key pkg-a already cached.
///
/// Importers resolve in order, so pkg-a's cache entry exists when
/// pkg-b's denied edge looks up; the depth-1 occurrence out-ranks the
/// depth-2 one for children ownership, making the corruption (before
/// the fix) deterministic rather than a race.
///
/// `target`'s dep `@pnpm.e2e/abc` wants peers nothing in the subtree
/// provides, so auto-install-peers suffixes `target` — the corrupted
/// instance is then visible as a distinct bare-key `{}` snapshot rather
/// than colliding with the healthy suffixed one.
#[test]
fn an_edge_denied_reuse_keeps_the_subtree_instead_of_reading_the_synthesized_reuse_result() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'pkg-a'\n  - 'pkg-b'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir(workspace.join("pkg-a")).expect("mkdir pkg-a");
    fs::write(
        workspace.join("pkg-a/package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/reuse-chain-grand": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write pkg-a/package.json");

    let pkg_b_manifest = |dep_of_pkg_with_1_dep: &str| {
        serde_json::json!({
            "name": "pkg-b",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/reuse-chain-parent": "1.0.0",
                "@pnpm.e2e/dep-of-pkg-with-1-dep": dep_of_pkg_with_1_dep,
            },
        })
        .to_string()
    };
    fs::create_dir(workspace.join("pkg-b")).expect("mkdir pkg-b");
    let pkg_b_manifest_path = workspace.join("pkg-b/package.json");
    fs::write(&pkg_b_manifest_path, pkg_b_manifest("100.0.0")).expect("write pkg-b/package.json");

    pacquet_at(workspace).with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0(@pnpm.e2e/peer-a@"),
        "the fresh install must suffix the target with the auto-installed peers:\n{lockfile}",
    );

    // Bump `dep-of-pkg-with-1-dep` in pkg-b only: `target`'s snapshot
    // depends on it, so pkg-b's edge to `target` is denied reuse while
    // pkg-a's (already-walked) edge reused it.
    fs::write(&pkg_b_manifest_path, pkg_b_manifest("100.1.0"))
        .expect("bump dep-of-pkg-with-1-dep in pkg-b");
    bump_mtime(&pkg_b_manifest_path);
    pacquet_at(workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml after bump");
    assert!(
        !lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0': {}"),
        "the denied edge must not record the target as an empty leaf:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/reuse-chain-target@1.0.0(@pnpm.e2e/peer-a@"),
        "the target must keep its peer-suffixed snapshot:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/abc': 1.0.0("),
        "the target's snapshot must keep its dependency on @pnpm.e2e/abc:\n{lockfile}",
    );

    drop(fixture);
}

/// Re-installing an unchanged manifest must leave `pnpm-lock.yaml`
/// byte-identical: the lockfile maps are sorted at emit time, so the
/// `importers` / `packages` / `snapshots` / dependency maps don't
/// serialize in `HashMap` iteration order and a no-op re-install can't
/// reorder the file into a spurious git diff
/// ([#12117](https://github.com/pnpm/pnpm/issues/12117)). The manifest
/// carries several dependencies so at least one map holds multiple keys,
/// giving order a chance to differ.
#[test]
fn reinstalling_an_unchanged_manifest_keeps_the_lockfile_byte_identical() {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }
    })
    .to_string();

    let project = CommandTempCwd::init().add_mocked_registry();
    fs::write(project.workspace.join("package.json"), &manifest).expect("write manifest");

    pacquet_at(&project.workspace).with_arg("install").assert().success();
    let lockfile_path = project.workspace.join("pnpm-lock.yaml");
    let first = fs::read_to_string(&lockfile_path).expect("read lockfile after first install");

    pacquet_at(&project.workspace).with_arg("install").assert().success();
    let second = fs::read_to_string(&lockfile_path).expect("read lockfile after second install");

    pretty_assertions::assert_eq!(
        first,
        second,
        "a no-op re-install must not reorder the lockfile",
    );

    drop(project);
}

#[test]
fn peer_setting_change_on_a_peerless_lockfile_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\nfetchRetries: 0\nfetchTimeout: 1000\n"),
    )
    .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read workspace yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}dedupePeers: true\n"))
        .expect("turn dedupePeers on");
    let dead_registry = dead_registry_url();
    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert_eq!(
        wanted.settings.expect("recorded settings").dedupe_peers,
        Some(true),
        "the changed setting must be recorded without resolving",
    );

    drop((root, mock_instance));
}

#[test]
fn peer_setting_change_falls_back_when_the_lockfile_records_peers() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\nfetchRetries: 0\nfetchTimeout: 1000\n"),
    )
    .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read workspace yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}dedupePeers: true\n"))
        .expect("turn dedupePeers on");

    // Unlike the peerless sibling above, this install has to resolve, so it
    // keeps the mocked registry reachable: pointing it at a dead one would
    // only assert that the resolve happened to be satisfiable offline.
    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        !String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "a peer setting change over a peer-suffixed graph must go through resolution",
    );

    drop((root, mock_instance));
}

#[test]
fn an_unused_patch_is_recorded_without_resolution_and_a_used_one_is_not() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches").join("is-positive@1.0.0.patch"), IS_POSITIVE_PATCH)
        .expect("write the patch fixture");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\nallowUnusedPatches: true\n"),
    )
    .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(!installed_is_positive(&workspace).contains("// patched"));

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    // A key naming a package no importer depends on cannot rekey anything.
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}patchedDependencies:\n  absent-package@1.0.0: patches/is-positive@1.0.0.patch\n",
        ),
    )
    .expect("patch a package the lockfile does not record");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "a patch matching no locked package must not trigger resolution",
    );
    assert_eq!(
        patched_dependency_keys(&workspace),
        vec!["absent-package@1.0.0".to_string()],
        "the new patchedDependencies entry is still recorded",
    );
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("The following patches were not used: absent-package@1.0.0"),
        "and the rewrite still says what the resolution it replaced would have said",
    );

    // Patching a locked package renames its snapshot, which the rewrite
    // does without asking the registry anything — the tarball it patches
    // is already in the store.
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace(
            "  absent-package@1.0.0: patches/is-positive@1.0.0.patch\n",
            "  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
        ),
    )
    .expect("patch a locked package");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "patching a locked package only renames its snapshot, so no resolution is needed",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(
        wanted
            .snapshots
            .as_ref()
            .expect("snapshots")
            .keys()
            .any(|key| key.to_string().starts_with("is-positive@1.0.0(patch_hash=")),
        "the patched package's snapshot key carries the patch hash",
    );
    assert!(
        installed_is_positive(&workspace).contains("// patched"),
        "the rewrite still materializes the patched package",
    );

    // Dropping the patch renames the snapshot back and restores the
    // unpatched package, again without resolving.
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace(
            "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
            "",
        ),
    )
    .expect("drop the patch");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "dropping a patch renames the snapshot back without resolving",
    );
    assert!(
        !installed_is_positive(&workspace).contains("// patched"),
        "the unpatched package is materialized again",
    );

    drop((root, mock_instance));
}

fn installed_is_positive(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("node_modules").join("is-positive").join("index.js"))
        .expect("read the installed is-positive")
}

fn patched_dependency_keys(workspace: &Path) -> Vec<String> {
    pnpm_lockfile::Lockfile::load_wanted_from_dir(workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile")
        .patched_dependencies
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn patching_a_package_with_an_install_script_rebuilds_it() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/install-script-example": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}trustLockfile: true\nstrictDepBuilds: false\nallowBuilds:\n  '@pnpm.e2e/install-script-example': true\n",
        ),
    )
    .expect("allow the install script");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(generated_by_install(&workspace), "module.exports = function () {}\n");

    fs::write(
        workspace.join("patches").join("install-script-example.patch"),
        concat!(
            "diff --git a/create.js b/create.js\n",
            "--- a/create.js\n",
            "+++ b/create.js\n",
            "@@ -1,4 +1,4 @@\n",
            " 'use strict'\n",
            " const fs = require('fs')\n",
            " \n",
            "-fs.writeFileSync(process.argv[2] + '.js', 'module.exports = function () {}\\n', 'utf8')\n",
            "+fs.writeFileSync(process.argv[2] + '.js', 'patched\\n', 'utf8')\n",
        ),
    )
    .expect("write the patch");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}patchedDependencies:\n  '@pnpm.e2e/install-script-example@1.0.0': patches/install-script-example.patch\n",
        ),
    )
    .expect("patch the package");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "the rekey needs no resolution",
    );
    assert_eq!(
        generated_by_install(&workspace),
        "patched\n",
        "the install script reran against the patched sources",
    );

    drop((root, mock_instance));
}

fn generated_by_install(workspace: &Path) -> String {
    fs::read_to_string(
        workspace
            .join("node_modules")
            .join("@pnpm.e2e")
            .join("install-script-example")
            .join("generated-by-install.js"),
    )
    .expect("read the file the install script generates")
}

#[test]
fn dropping_a_dependency_from_the_manifest_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("drop is-positive from package.json");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "dropping an importer edge needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("is-positive@")),
        "the dropped package is pruned from the lockfile",
    );
    assert!(
        !workspace.join("node_modules").join("is-positive").exists(),
        "and unlinked from node_modules",
    );

    drop((root, mock_instance));
}

#[test]
fn remove_command_drops_the_dependency_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            },
            "scripts": {
                "postinstall": r#"node -e "require('fs').writeFileSync('postinstall-ran','')""#
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();
    fs::remove_file(workspace.join("postinstall-ran"))
        .expect("the full install ran the project postinstall");

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_args(["remove", "is-positive"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "removing a dependency needs no resolution",
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert!(manifest["dependencies"].get("is-positive").is_none(), "the manifest entry is gone");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("is-positive@")),
        "the removed package is pruned from the lockfile",
    );
    assert!(
        !workspace.join("node_modules").join("is-positive").exists(),
        "and unlinked from node_modules",
    );
    assert!(
        manifest["dependencies"].get("@pnpm.e2e/pkg-with-1-dep").is_some(),
        "the surviving dependency keeps its manifest entry",
    );
    assert!(
        wanted.importers["."].dependencies.as_ref().is_some_and(|dependencies| {
            dependencies.contains_key(&"@pnpm.e2e/pkg-with-1-dep".parse().expect("alias"))
        }),
        "and its importer entry",
    );
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep").exists(),
        "and its node_modules link",
    );
    assert!(
        !workspace.join("postinstall-ran").exists(),
        "a remove runs no project lifecycle script",
    );

    drop((root, mock_instance));
}

#[test]
fn moving_a_dependency_between_groups_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0"
            },
            "optionalDependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("move the dependency to optionalDependencies");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "moving a dependency between groups needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let importer = &wanted.importers["."];
    let moved_alias = "@pnpm.e2e/pkg-with-1-dep".parse().expect("alias");
    assert!(
        importer
            .optional_dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(&moved_alias)),
        "the importer records the moved dependency under optionalDependencies",
    );
    assert!(
        !importer
            .dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(&moved_alias)),
        "and no longer under dependencies",
    );
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    for prefix in ["@pnpm.e2e/pkg-with-1-dep@", "@pnpm.e2e/dep-of-pkg-with-1-dep@"] {
        let (key, snapshot) = snapshots
            .iter()
            .find(|(key, _)| key.to_string().starts_with(prefix))
            .expect("moved subtree snapshot");
        assert!(snapshot.optional, "{key} is only reachable through an optional edge now");
    }
    let is_positive_snapshot = snapshots
        .iter()
        .find_map(|(key, snapshot)| key.to_string().starts_with("is-positive@").then_some(snapshot))
        .expect("is-positive snapshot");
    assert!(!is_positive_snapshot.optional, "the untouched prod dependency keeps its flags");
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep").exists(),
        "the moved dependency stays linked",
    );

    drop((root, mock_instance));
}

#[test]
fn combined_manifest_and_ignore_list_drift_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            },
            "optionalDependencies": {
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            },
            "optionalDependencies": {
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("drop one prod dependency");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}ignoredOptionalDependencies:\n  - is-positive\n"),
    )
    .expect("widen the ignore list");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "the removal and the widened ignore list are absorbed in one pass",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let packages: Vec<String> =
        wanted.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    assert!(
        packages.iter().all(|key| key.starts_with("@pnpm.e2e/foo@")),
        "both the removed dependency and the newly ignored optional are gone: {packages:?}",
    );
    assert_eq!(
        wanted.ignored_optional_dependencies.as_deref(),
        Some(&["is-positive".to_string()][..]),
    );
    assert!(
        !workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep").exists(),
        "the removed dependency is unlinked",
    );

    drop((root, mock_instance));
}

#[test]
fn a_remove_with_an_unchanged_pnpmfile_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
                pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_args(["remove", "is-positive"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "an unchanged pnpmfile does not force resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    let pinned = snapshots
        .iter()
        .find(|(key, _)| key.to_string().starts_with("@pnpm.e2e/pkg-with-1-dep@"))
        .expect("hooked package snapshot")
        .1;
    assert!(
        pinned.dependencies.as_ref().is_some_and(|dependencies| dependencies
            .get(&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias"))
            .is_some_and(|reference| reference.to_string() == "100.0.0")),
        "the hook's pin survives the fast update",
    );

    drop((root, mock_instance));
}

#[test]
fn a_remove_keeps_the_specifiers_a_project_rewriting_pnpmfile_recorded() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.dependencies && pkg.dependencies['is-positive']) {
                pkg.dependencies['is-positive'] = '1.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "^1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();
    let recorded_specifier = |lockfile: &pnpm_lockfile::Lockfile| {
        lockfile.importers["."].dependencies.as_ref().expect("dependencies")
            [&"is-positive".parse().expect("alias")]
            .specifier
            .clone()
    };
    let initial = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let initial_specifier = recorded_specifier(&initial);

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert =
        pacquet_at(&workspace).with_args(["remove", "@pnpm.e2e/pkg-with-1-dep"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "the removal needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert_eq!(
        recorded_specifier(&wanted),
        initial_specifier,
        "the fast update must not rewrite the specifier the hooked resolution recorded",
    );

    drop((root, mock_instance));
}

#[test]
fn a_frozen_install_tolerates_the_importer_of_a_removed_workspace_project() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_two_member_workspace(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("packages/b")).expect("remove the member");

    // pnpm's importer-set gate lives in the auto-frozen branch, which an
    // explicit `--frozen-lockfile` short-circuits past.
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}

#[test]
fn removing_a_workspace_project_prunes_its_importer_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    write_two_member_workspace(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("packages/b")).expect("remove the member");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "dropping a workspace project needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let mut importers: Vec<_> = wanted.importers.keys().map(String::as_str).collect();
    importers.sort_unstable();
    assert_eq!(
        importers,
        vec![".", "packages/a"],
        "the departed project's importer is gone, the root and its sibling stay",
    );
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("@pnpm.e2e/bar@")),
        "and so is what only it depended on",
    );

    drop((root, mock_instance));
}

/// A config drift the fast rewrites cannot absorb (a changed
/// `packageExtensions`) forces every subtree to re-resolve, but each
/// edge whose recorded version still satisfies its range keeps it: the
/// prior lockfile pins per edge even when it cannot seed subtree
/// reuse. Without the pin, `@pnpm.e2e/foobar`'s open `^100.0.0` edge
/// would re-pick the highest locked `@pnpm.e2e/foo` (100.1.0, locked
/// by the other workspace member) and churn the lockfile.
#[test]
fn config_drift_full_resolve_keeps_still_satisfied_pins() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}packages:\n  - packages/*\n"))
        .expect("declare the workspace members");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");
    let write_member = |name: &str, dependencies: serde_json::Value| {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create the member directory");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": dependencies,
            })
            .to_string(),
        )
        .expect("write the member package.json");
    };
    // The direct exact dep dedupes foobar's `^100.0.0` edge onto
    // 100.0.0 while it is the only locked version.
    write_member(
        "a",
        serde_json::json!({ "@pnpm.e2e/foobar": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }),
    );
    write_member("b", serde_json::json!({}));
    pacquet_at(&workspace).with_arg("install").assert().success();
    let foobar_key = "@pnpm.e2e/foobar@100.0.0".parse().expect("foobar key");
    let foo_name = "@pnpm.e2e/foo".parse().expect("foo name");
    let foobar_foo_child = |lockfile: &pnpm_lockfile::Lockfile| {
        lockfile
            .snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&foobar_key))
            .and_then(|snapshot| snapshot.dependencies.as_ref())
            .and_then(|dependencies| dependencies.get(&foo_name))
            .and_then(|dep_ref| dep_ref.resolve(&foo_name))
            .map(|key| key.to_string())
    };
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert_eq!(foobar_foo_child(&wanted).as_deref(), Some("@pnpm.e2e/foo@100.0.0"));

    // Lock a second, higher foo through the other member; foobar's
    // subtree is untouched and keeps its recorded 100.0.0 child.
    write_member("a", serde_json::json!({ "@pnpm.e2e/foobar": "100.0.0" }));
    write_member("b", serde_json::json!({ "@pnpm.e2e/foo": "100.1.0" }));
    pacquet_at(&workspace).with_arg("install").assert().success();
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert_eq!(foobar_foo_child(&wanted).as_deref(), Some("@pnpm.e2e/foo@100.0.0"));

    // Non-absorbable config drift: a package extension that visibly
    // changes foobar's dependency set, so the assertion below also
    // proves the recorded subtree was re-resolved (the extension
    // applied) rather than reused wholesale under the drift.
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}packageExtensions:\n  '@pnpm.e2e/foobar':\n    dependencies:\n      is-positive: 1.0.0\n",
        ),
    )
    .expect("add a package extension");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert_eq!(foobar_foo_child(&wanted).as_deref(), Some("@pnpm.e2e/foo@100.0.0"));
    let extended_child = wanted
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&foobar_key))
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&"is-positive".parse().expect("name")))
        .and_then(|dep_ref| dep_ref.resolve(&"is-positive".parse().expect("name")))
        .map(|key| key.to_string());
    assert_eq!(extended_child.as_deref(), Some("is-positive@1.0.0"));
    let foo_100_1_0 = "@pnpm.e2e/foo@100.1.0".parse().expect("foo 100.1.0 key");
    assert!(
        wanted.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&foo_100_1_0)),
    );

    drop((root, mock_instance));
}

/// A workspace whose two members each depend on a package of their own.
fn write_two_member_workspace(workspace: &Path) {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}packages:\n  - packages/*\n"))
        .expect("declare the workspace members");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");
    for (name, dependency) in [("a", "@pnpm.e2e/foo"), ("b", "@pnpm.e2e/bar")] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create the member directory");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { dependency: "100.0.0" },
            })
            .to_string(),
        )
        .expect("write the member package.json");
    }
}

#[test]
fn add_command_reuses_a_locked_version_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    // The transitive `^100.0.0` of `@pnpm.e2e/pkg-with-1-dep` locks
    // `100.1.0`, so promoting it to a direct dependency at that version
    // changes nothing but the importer edge.
    let assert = pacquet_at(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"])
        .assert()
        .success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "adding an already-locked version needs no resolution",
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(manifest["dependencies"]["@pnpm.e2e/dep-of-pkg-with-1-dep"], "100.1.0");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let added = &wanted.importers["."].dependencies.as_ref().expect("dependencies")
        [&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias")];
    assert_eq!(added.specifier, "100.1.0");
    assert_eq!(added.version.to_string(), "100.1.0");
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("dep-of-pkg-with-1-dep").exists(),
        "the added dependency is linked into node_modules",
    );

    drop((root, mock_instance));
}

#[test]
fn add_command_resolves_a_version_the_lockfile_does_not_hold() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let assert = pacquet_at(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0"])
        .assert()
        .success();
    assert!(
        !String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "a version nothing locks has to be fetched",
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert_eq!(
        wanted.importers["."].dependencies.as_ref().expect("dependencies")
            [&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias")]
            .version
            .to_string(),
        "101.0.0",
    );

    drop((root, mock_instance));
}
