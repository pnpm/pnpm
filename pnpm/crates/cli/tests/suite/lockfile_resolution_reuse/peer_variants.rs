use super::{
    AddMockedRegistry,
    CommandExtra,
    CommandTempCwd,
    fs,
    pacquet_at,
};
use assert_cmd::assert::OutputAssertExt;

/// `@pnpm.e2e/abc-parent-with-missing-peers` depends on `@pnpm.e2e/abc`,
/// whose peers `peer-a`, `peer-b` and `peer-c` the root provides, so the
/// lockfile holds `abc` only as a peer-suffixed snapshot. Promoting it to
/// a direct dependency has to go through the resolver: the bare
/// `abc@1.0.0` the `packages:` block names has no snapshot to link.
#[test]
fn promoting_a_peer_suffixed_transitive_dependency_resolves_its_importer_edge() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let mut dependencies = serde_json::json!({
        "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        "@pnpm.e2e/peer-a": "1.0.0",
        "@pnpm.e2e/peer-b": "1.0.0",
        "@pnpm.e2e/peer-c": "1.0.0",
    });
    fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
        .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let abc: pnpm_lockfile::PkgName = "@pnpm.e2e/abc".parse().expect("package name");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    let locked_abc: Vec<_> = snapshots
        .keys()
        .filter(|key| key.name == abc)
        .collect();
    assert!(!locked_abc.is_empty(), "the fixture reaches abc through its parent");
    assert!(
        locked_abc
            .iter()
            .all(|key| !key.suffix.peer().is_empty()),
        "the fixture holds abc only as a peer variant: {locked_abc:?}",
    );

    dependencies["@pnpm.e2e/abc"] = "1.0.0".into();
    fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
        .expect("promote abc to a direct dependency");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let edge = &wanted.importers["."].dependencies.as_ref().expect("dependencies")[&abc];
    let linked: pnpm_lockfile::PackageKey =
        format!("@pnpm.e2e/abc@{}", edge.version).parse().expect("snapshot key");
    assert!(
        !linked.suffix.peer().is_empty(),
        "the importer edge carries the peers abc resolved: {}",
        edge.version,
    );
    assert!(
        wanted.snapshots
            .as_ref()
            .expect("snapshots")
            .contains_key(&linked),
        "the importer edge names a snapshot the lockfile holds: {}",
        edge.version,
    );
    assert!(
        workspace
            .join("node_modules")
            .join("@pnpm.e2e")
            .join("abc")
            .join("package.json")
            .exists(),
        "the promoted dependency links to a package the virtual store holds",
    );

    drop((root, mock_instance));
}

/// The new-importer shape of the same defect: a workspace member added
/// after the lockfile was written declares `@pnpm.e2e/abc`, and its whole
/// importer entry is written from the versions the lockfile already holds
/// rather than one edge at a time.
#[test]
fn a_new_workspace_member_links_a_dependency_locked_only_as_a_peer_variant() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\npackages:\n  - packages/*\n"),
    )
    .expect("declare the workspace members");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");
    let dependencies = serde_json::json!({
        "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        "@pnpm.e2e/peer-a": "1.0.0",
        "@pnpm.e2e/peer-b": "1.0.0",
        "@pnpm.e2e/peer-c": "1.0.0",
    });
    let existing = workspace.join("packages").join("existing");
    fs::create_dir_all(&existing).expect("create the member directory");
    fs::write(
        existing.join("package.json"),
        serde_json::json!({ "name": "existing", "version": "1.0.0", "dependencies": dependencies })
            .to_string(),
    )
    .expect("write the member package.json");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let mut added_dependencies = dependencies;
    added_dependencies["@pnpm.e2e/abc"] = "1.0.0".into();
    let added = workspace.join("packages").join("web-ui");
    fs::create_dir_all(&added).expect("create the new member directory");
    fs::write(
        added.join("package.json"),
        serde_json::json!({
            "name": "web-ui",
            "version": "1.0.0",
            "dependencies": added_dependencies,
        })
        .to_string(),
    )
    .expect("write the new member package.json");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let abc: pnpm_lockfile::PkgName = "@pnpm.e2e/abc".parse().expect("package name");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let edge =
        &wanted.importers["packages/web-ui"].dependencies.as_ref().expect("dependencies")[&abc];
    let linked: pnpm_lockfile::PackageKey =
        format!("@pnpm.e2e/abc@{}", edge.version).parse().expect("snapshot key");
    assert!(
        wanted.snapshots
            .as_ref()
            .expect("snapshots")
            .contains_key(&linked),
        "the new member's edge names a snapshot the lockfile holds: {}",
        edge.version,
    );
    assert!(
        added
            .join("node_modules")
            .join("@pnpm.e2e")
            .join("abc")
            .join("package.json")
            .exists(),
        "the new member links to a package the virtual store holds",
    );

    drop((root, mock_instance));
}
