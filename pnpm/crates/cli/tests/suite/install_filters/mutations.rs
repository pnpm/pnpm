use super::{
    DEP, HELLO, HELLO_PARENT, ManifestDeps, PARENT, WorkspaceFixture, assert_eq,
    assert_full_wanted, assert_root_and_selected_are_materialized, assert_stage_once,
    compatible_update_scenario, dependency_spec, fs, has_snapshot, importer,
    importer_has_group_dependency, importer_specifier, importer_version, read_lockfile,
    replace_dependencies, set_dependency, snapshot_entries, transitive_update_scenario,
    workspace_with_installable_root,
};

#[test]
fn filtered_add_mutates_only_selected_importers() {
    let fixture = WorkspaceFixture::new();
    let selected_a = fixture.project("selected-a", "selected-a", ManifestDeps::default());
    let selected_b = fixture.project("selected-b", "selected-b", ManifestDeps::default());
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "add",
        HELLO,
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&selected_b, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_update_mutates_only_selected_importers() {
    let fixture = WorkspaceFixture::new();
    let selected_a = fixture.project(
        "selected-a",
        "selected-a",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let selected_b = fixture.project(
        "selected-b",
        "selected-b",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    for project in [&selected_a, &selected_b, &unselected] {
        set_dependency(project, "dependencies", DEP, "^100.0.0");
    }
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "update",
        DEP,
        "--latest",
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(dependency_spec(&selected_b, "dependencies", DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_eq!(importer_version(&after, "packages/unselected", DEP), "100.0.0");
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_remove_mutates_only_selected_importers() {
    let fixture = WorkspaceFixture::new();
    let selected_a = fixture.project(
        "selected-a",
        "selected-a",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let selected_b = fixture.project(
        "selected-b",
        "selected-b",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "remove",
        HELLO,
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&selected_b, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&selected_b, "dependencies", PARENT).as_deref(), Some("100.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_add_materializes_the_workspace_root_without_mutating_it() {
    let (fixture, selected, unselected, root_manifest) =
        workspace_with_installable_root(ManifestDeps::default());

    fixture.run(["--filter", "selected", "add", PARENT]);

    assert_root_and_selected_are_materialized(&fixture, &selected, &unselected, PARENT);
    assert_eq!(dependency_spec(&selected, "dependencies", PARENT).as_deref(), Some("^100.1.0"));
    assert_eq!(
        fs::read(fixture.workspace.join("package.json")).expect("read manifest"),
        root_manifest,
    );
}

#[test]
fn filtered_update_materializes_the_workspace_root_without_mutating_it() {
    let (fixture, selected, unselected, root_manifest) =
        workspace_with_installable_root(ManifestDeps {
            prod: &[(DEP, "100.0.0")],
            ..Default::default()
        });

    fixture.run(["--filter", "selected", "update", DEP, "--latest"]);

    assert_root_and_selected_are_materialized(&fixture, &selected, &unselected, DEP);
    assert_eq!(dependency_spec(&selected, "dependencies", DEP).as_deref(), Some("101.0.0"));
    assert_eq!(
        fs::read(fixture.workspace.join("package.json")).expect("read manifest"),
        root_manifest,
    );
}

#[test]
fn filtered_remove_materializes_the_workspace_root_without_mutating_it() {
    let (fixture, selected, unselected, root_manifest) =
        workspace_with_installable_root(ManifestDeps {
            prod: &[(HELLO, "1.0.0"), (PARENT, "100.0.0")],
            ..Default::default()
        });

    fixture.run(["--filter", "selected", "remove", HELLO]);

    assert_root_and_selected_are_materialized(&fixture, &selected, &unselected, PARENT);
    assert_eq!(dependency_spec(&selected, "dependencies", HELLO), None);
    assert_eq!(
        fs::read(fixture.workspace.join("package.json")).expect("read manifest"),
        root_manifest,
    );
}

#[test]
fn filtered_update_from_selected_child_uses_discovered_manifest_as_source_of_truth() {
    let fixture = WorkspaceFixture::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "0.0.0")], ..Default::default() },
    );
    let sibling = fixture.project(
        "sibling",
        "sibling",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let sibling_manifest = fs::read(sibling.join("package.json")).expect("read manifest");
    fixture.run_at(&selected, ["--filter", ".", "update", HELLO, "--latest", "--lockfile-only"]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected, "dependencies", HELLO).as_deref(), Some("1.0.0"));
    assert_eq!(importer_specifier(&after, "packages/selected", HELLO), "1.0.0");
    assert_eq!(importer_version(&after, "packages/selected", HELLO), "1.0.0");
    assert_eq!(fs::read(sibling.join("package.json")).expect("read manifest"), sibling_manifest);
    assert_eq!(importer(&after, "packages/sibling"), importer(&before, "packages/sibling"));
    assert!(!after.importers.contains_key("."), "missing root must not become an importer");
}

#[test]
fn filtered_update_preserves_prior_importer_when_unselected_manifest_changed_externally() {
    let fixture = WorkspaceFixture::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "0.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let prior_importer = importer(&before, "packages/unselected").clone();
    let prior_parent = snapshot_entries(&before, PARENT);
    let prior_child = snapshot_entries(&before, DEP);
    replace_dependencies(&unselected, &[(HELLO_PARENT, "1.0.0")]);
    let external_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    fixture.run(["--filter", "selected", "update", HELLO, "--latest", "--lockfile-only"]);
    let after = fixture.wanted();

    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        external_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), &prior_importer);
    assert_eq!(snapshot_entries(&after, PARENT), prior_parent);
    assert_eq!(snapshot_entries(&after, DEP), prior_child);
    assert!(snapshot_entries(&after, HELLO_PARENT).is_empty());
    assert_eq!(dependency_spec(&selected, "dependencies", HELLO).as_deref(), Some("1.0.0"));
    assert_eq!(importer_version(&after, "packages/selected", HELLO), "1.0.0");
}

#[test]
fn filtered_compatible_update_does_not_cross_importer_cache_boundaries() {
    compatible_update_scenario("a-selected", "z-unselected");
    compatible_update_scenario("z-selected", "a-unselected");
}

#[test]
fn filtered_compatible_update_keeps_workspace_manifest_preferences() {
    let fixture = WorkspaceFixture::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    set_dependency(&selected, "dependencies", DEP, "^100.0.0");
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");

    fixture.run(["--filter", "selected", "update", DEP, "--lockfile-only"]);
    let lockfile = fixture.wanted();

    assert_eq!(importer_version(&lockfile, "packages/selected", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "packages/unselected", DEP), "100.0.0");
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
}

#[test]
fn filtered_transitive_update_keeps_one_canonical_shared_snapshot() {
    let (selected_first, selected_first_child) =
        transitive_update_scenario("a-selected", "z-unselected");
    let (unselected_first, unselected_first_child) =
        transitive_update_scenario("z-selected", "a-unselected");
    assert_eq!(selected_first_child, unselected_first_child);
    assert_eq!(selected_first, unselected_first);
}

#[test]
fn filtered_add_with_dedicated_lockfiles_mutates_only_selected_project() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    let selected = fixture.project("selected", "selected", ManifestDeps::default());
    let unselected = fixture.project("unselected", "unselected", ManifestDeps::default());

    fixture.run(["--filter", "selected", "add", HELLO, "--lockfile-only"]);

    assert_eq!(dependency_spec(&selected, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&unselected, "dependencies", HELLO), None);
    let selected_lockfile = read_lockfile(&selected.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&selected_lockfile, ".", HELLO), "1.0.0");
    assert!(!unselected.join("pnpm-lock.yaml").exists(), "unselected must not be mutated");
    assert!(!fixture.workspace.join("pnpm-lock.yaml").exists());
}

#[test]
fn recursive_add_with_dedicated_lockfiles_excludes_workspace_root() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    fixture.write_root_manifest("workspace-root", ManifestDeps::default());
    let member_a = fixture.project("member-a", "member-a", ManifestDeps::default());
    let member_b = fixture.project("member-b", "member-b", ManifestDeps::default());

    fixture.run(["-r", "add", HELLO, "--lockfile-only"]);

    assert_eq!(dependency_spec(&fixture.workspace, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&member_a, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&member_b, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    for member in [&member_a, &member_b] {
        assert_eq!(
            importer_version(&read_lockfile(&member.join("pnpm-lock.yaml")), ".", HELLO),
            "1.0.0",
        );
    }
    assert!(
        !fixture.workspace.join("pnpm-lock.yaml").exists(),
        "the auto-excluded root must not get its own lockfile",
    );
}

#[test]
fn filtered_update_with_dedicated_lockfiles_mutates_only_selected_project() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    for project in [&selected, &unselected] {
        set_dependency(project, "dependencies", DEP, "^100.0.0");
    }
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let unselected_lockfile_before =
        fs::read(unselected.join("pnpm-lock.yaml")).expect("read lockfile");

    fixture.run(["--filter", "selected", "update", DEP, "--latest", "--lockfile-only"]);

    assert_eq!(dependency_spec(&selected, "dependencies", DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(
        importer_version(&read_lockfile(&selected.join("pnpm-lock.yaml")), ".", DEP),
        "101.0.0",
    );
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(
        fs::read(unselected.join("pnpm-lock.yaml")).expect("read lockfile"),
        unselected_lockfile_before,
    );
    assert!(!fixture.workspace.join("pnpm-lock.yaml").exists());
}

#[test]
fn filtered_remove_with_dedicated_lockfiles_mutates_only_selected_project() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let unselected_lockfile_before =
        fs::read(unselected.join("pnpm-lock.yaml")).expect("read lockfile");

    fixture.run(["--filter", "selected", "remove", HELLO, "--lockfile-only"]);

    assert_eq!(dependency_spec(&selected, "dependencies", HELLO), None);
    let selected_lockfile = read_lockfile(&selected.join("pnpm-lock.yaml"));
    assert!(!has_snapshot(&selected_lockfile, HELLO, "1.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(
        fs::read(unselected.join("pnpm-lock.yaml")).expect("read lockfile"),
        unselected_lockfile_before,
    );
    assert!(!fixture.workspace.join("pnpm-lock.yaml").exists());
}

#[test]
fn recursive_add_auto_excludes_workspace_root() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("workspace-root", ManifestDeps::default());
    let member_a = fixture.project("member-a", "member-a", ManifestDeps::default());
    let member_b = fixture.project("member-b", "member-b", ManifestDeps::default());
    fixture.run(["-r", "add", HELLO, "--lockfile-only"]);
    let wanted = fixture.wanted();

    assert_eq!(dependency_spec(&fixture.workspace, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&member_a, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&member_b, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert!(!importer_has_group_dependency(&wanted, ".", "dependencies", HELLO));
    assert!(importer_has_group_dependency(&wanted, "packages/member-a", "dependencies", HELLO,));
    assert!(importer_has_group_dependency(&wanted, "packages/member-b", "dependencies", HELLO,));
}
