use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, bump_mtime, dead_registry_url, fs, pacquet_at,
};
use assert_cmd::assert::OutputAssertExt;

/// A dependency cycle in the lockfile (`circular-deps-1-of-2` and
/// `circular-deps-2-of-2` depend on each other) is reused like any other
/// unchanged subtree when a new dependency needs a resolution.
#[test]
fn adding_a_local_dependency_reuses_a_cyclic_subtree_without_the_registry() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{existing}trustLockfile: true\n"))
        .expect("trust the lockfile");
    fs::create_dir_all(workspace.join("local")).expect("create the local package");
    fs::write(workspace.join("local/package.json"), r#"{"name":"local","version":"1.0.0"}"#)
        .expect("write the local package manifest");
    let manifest_path = workspace.join("package.json");
    let manifest = |dependencies: serde_json::Value| {
        fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
            .expect("write package.json");
    };
    manifest(serde_json::json!({ "@pnpm.e2e/circular-deps-1-of-2": "1.0.2" }));
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={}\n{npmrc}\n", dead_registry_url()))
        .expect("rewrite .npmrc with a dead registry");
    manifest(serde_json::json!({
        "@pnpm.e2e/circular-deps-1-of-2": "1.0.2",
        "local": "file:./local",
    }));
    bump_mtime(&manifest_path);

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    drop((root, mock_instance));
}

/// A tree that reuses a cyclic subtree serializes the same as a fresh
/// resolve of the same manifest.
#[test]
fn a_reused_cyclic_subtree_matches_a_fresh_resolve() {
    let cycle = serde_json::json!({ "@pnpm.e2e/circular-deps-1-of-2": "1.0.2" });
    let both = serde_json::json!({
        "dependencies": { "@pnpm.e2e/circular-deps-1-of-2": "1.0.2", "@pnpm.e2e/foo": "100.0.0" }
    })
    .to_string();

    let reused = CommandTempCwd::init().add_mocked_registry();
    let reused_manifest = reused.workspace.join("package.json");
    fs::write(&reused_manifest, serde_json::json!({ "dependencies": cycle }).to_string())
        .expect("write the reuse scenario's initial manifest");
    pacquet_at(&reused.workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::write(&reused_manifest, &both).expect("add the second dep to the reuse scenario");
    bump_mtime(&reused_manifest);
    pacquet_at(&reused.workspace)
        .with_arg("install")
        .assert()
        .success();
    let reused_lockfile =
        fs::read_to_string(reused.workspace.join("pnpm-lock.yaml")).expect("read reused lockfile");

    let fresh = CommandTempCwd::init().add_mocked_registry();
    fs::write(fresh.workspace.join("package.json"), &both).expect("write the fresh manifest");
    pacquet_at(&fresh.workspace)
        .with_arg("install")
        .assert()
        .success();
    let fresh_lockfile =
        fs::read_to_string(fresh.workspace.join("pnpm-lock.yaml")).expect("read fresh lockfile");

    pretty_assertions::assert_eq!(reused_lockfile, fresh_lockfile);

    drop((reused, fresh));
}
