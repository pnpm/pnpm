use super::{AddMockedRegistry, CommandExtra, CommandTempCwd, dead_registry_url, fs, pacquet_at};
use assert_cmd::assert::OutputAssertExt;

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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

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

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let entry = &wanted.catalogs.expect("catalog snapshots")["default"]["@pnpm.e2e/has-optional-peer-with-peer"];
    assert_eq!(entry.specifier, ">=1.0.0 <2");
    assert_eq!(entry.version, "1.0.0");

    drop((root, mock_instance));
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
    pacquet_at(&fixture.workspace)
        .with_arg("install")
        .assert()
        .success();

    let before = pnpm_lockfile::Lockfile::load_wanted_from_dir(&fixture.workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let old_key = "@pnpm.e2e/pkg-with-1-dep@100.0.0".parse().expect("old key");
    let child_name = "@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("child name");
    let old_child = before.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&old_key))
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&child_name))
        .cloned()
        .expect("old locked child");

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial override");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace(
            "'@pnpm.e2e/pkg-with-1-dep': 100.0.0",
            "'@pnpm.e2e/pkg-with-1-dep': 100.1.0",
        ),
    )
    .expect("update exact override");
    pacquet_at(&fixture.workspace)
        .with_arg("install")
        .assert()
        .success();

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
            lockfile.snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&new_key))
                .and_then(|snapshot| snapshot.dependencies.as_ref())
                .and_then(|dependencies| dependencies.get(&child_name)),
            Some(&old_child),
        );
        assert!(
            lockfile.snapshots
                .as_ref()
                .is_some_and(|snapshots| !snapshots.contains_key(&old_key)),
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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

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

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let current = pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(&workspace.join(
        "node_modules/.pnpm",
    ))
    .expect("load current lockfile")
    .expect("current lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    let removed_name = "is-positive".parse().expect("removed package name");
    for lockfile in [&wanted, &current] {
        dbg!(&lockfile.snapshots, &lockfile.packages);
        assert!(
            lockfile.snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&parent_key))
                .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
                .is_none_or(|dependencies| !dependencies.contains_key(&removed_name)),
        );
        assert!(
            lockfile.snapshots
                .as_ref()
                .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
        );
        assert!(
            lockfile.packages
                .as_ref()
                .is_none_or(|packages| !packages.contains_key(&removed_key)),
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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

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

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let overrides = wanted.overrides.as_ref().expect("recorded overrides");
    assert_eq!(overrides["@pnpm.e2e/foo"], "1.0.0");
    assert_eq!(overrides["is-positive"], "-");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    assert!(
        wanted.snapshots
            .as_ref()
            .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
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
    pacquet_at(&fixture.workspace)
        .with_arg("install")
        .assert()
        .success();

    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read initial catalog");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}overrides:\n  '@pnpm.e2e/pkg-with-1-dep': 100.1.0\n"),
    )
    .expect("add the override");
    pacquet_at(&fixture.workspace)
        .with_arg("install")
        .assert()
        .success();

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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

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

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let entry = &wanted.catalogs.as_ref().expect("catalog snapshots")["default"]["@pnpm.e2e/pkg-with-good-optional"];
    assert_eq!(entry.specifier, ">=1.0.0 <2");
    assert_eq!(entry.version, "1.0.0");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    assert!(
        wanted.snapshots
            .as_ref()
            .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
    );
    assert!(
        wanted.packages
            .as_ref()
            .is_none_or(|packages| !packages.contains_key(&removed_key)),
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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let foobar_key = "@pnpm.e2e/foobar@100.0.0".parse().expect("foobar key");
    let foo_name = "@pnpm.e2e/foo".parse().expect("foo name");
    let foobar_foo_child = |lockfile: &pnpm_lockfile::Lockfile| {
        lockfile.snapshots
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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
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
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert_eq!(foobar_foo_child(&wanted).as_deref(), Some("@pnpm.e2e/foo@100.0.0"));
    let extended_child = wanted.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&foobar_key))
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&"is-positive".parse().expect("name")))
        .and_then(|dep_ref| dep_ref.resolve(&"is-positive".parse().expect("name")))
        .map(|key| key.to_string());
    assert_eq!(extended_child.as_deref(), Some("is-positive@1.0.0"));
    let foo_100_1_0 = "@pnpm.e2e/foo@100.1.0".parse().expect("foo 100.1.0 key");
    assert!(
        wanted.snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(&foo_100_1_0)),
    );

    drop((root, mock_instance));
}
