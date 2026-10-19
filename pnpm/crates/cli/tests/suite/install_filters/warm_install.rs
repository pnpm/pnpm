use super::{
    DEP, HELLO, ManifestDeps, NO_DEPS, PARENT, WorkspaceFixture, assert_eq, fs, has_snapshot,
    importing_started_count, reports_up_to_date, set_dependency, set_version,
};

#[test]
fn full_recursive_install_keeps_the_unfiltered_up_to_date_path() {
    let fixture = WorkspaceFixture::new();
    fixture.project("app", "app", ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() });
    fixture.project(
        "lib",
        "lib",
        ManifestDeps {
            prod: &[(PARENT, "100.0.0")],
            peer: &[(HELLO, "1.0.0")],
            ..Default::default()
        },
    );
    fixture.run(["install"]);
    let lockfile_path = fixture.workspace.join("pnpm-lock.yaml");
    let before = fs::read(&lockfile_path).expect("read lockfile");
    fs::write(&lockfile_path, &before).expect("rewrite lockfile without changing its contents");

    let records = fixture.run(["--recursive", "install"]);

    assert_eq!(fs::read(lockfile_path).expect("read lockfile"), before);
    assert_eq!(importing_started_count(&records), 0, "a full selection must not relink");
    assert!(reports_up_to_date(&records));
}

/// A `--filter`ed install with nothing to do short-circuits like the
/// unfiltered one: the fast path validates the whole workspace, and the
/// full install that preceded it materialized every project.
#[test]
fn filtered_install_takes_the_up_to_date_path_after_a_full_install() {
    let fixture = WorkspaceFixture::new();
    fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    let lockfile_path = fixture.workspace.join("pnpm-lock.yaml");
    let before = fs::read(&lockfile_path).expect("read lockfile");

    let records = fixture.run(["--filter", "selected", "install"]);

    assert!(reports_up_to_date(&records));
    assert_eq!(importing_started_count(&records), 0, "a no-op selection must not relink");
    assert_eq!(fs::read(&lockfile_path).expect("read lockfile"), before);
    assert!(!fixture.state().filtered_install, "the short-circuit narrowed nothing");
}

#[test]
fn filtered_hoisted_prod_install_preserves_unselected_dev_dependencies() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");
    fixture.project(
        "selected",
        "selected",
        ManifestDeps {
            prod: &[(HELLO, "1.0.0")],
            dev: &[(NO_DEPS, "1.0.0")],
            ..Default::default()
        },
    );
    fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { dev: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    assert!(
        fixture.workspace
            .join("node_modules")
            .join(HELLO)
            .is_dir(),
    );
    assert!(
        fixture.workspace
            .join("node_modules")
            .join(NO_DEPS)
            .is_dir(),
    );
    assert!(
        fixture.workspace
            .join("node_modules")
            .join(PARENT)
            .is_dir(),
    );

    fixture.run(["--filter", "selected", "install", "--prod"]);

    assert!(
        fixture.workspace
            .join("node_modules")
            .join(HELLO)
            .is_dir(),
        "selected prod dependency must remain hoisted",
    );
    assert!(
        !fixture.workspace
            .join("node_modules")
            .join(NO_DEPS)
            .exists(),
        "selected devDependency must be pruned under --prod",
    );
    assert!(
        fixture.workspace
            .join("node_modules")
            .join(PARENT)
            .is_dir(),
        "unselected prior devDependency must remain hoisted",
    );
    assert!(
        fixture.workspace
            .join("node_modules")
            .join(DEP)
            .is_dir(),
        "unselected prior transitive devDependency must remain hoisted",
    );
    let current = fixture.current();
    assert!(
        current.importers["packages/selected"].dev_dependencies.is_none(),
        "current lockfile must strip devDependencies from selected importer",
    );
    assert!(
        current.importers["packages/unselected"].dev_dependencies.is_some(),
        "current lockfile must retain devDependencies on unselected importer",
    );
    assert!(has_snapshot(&current, PARENT, "100.0.0"));
    assert!(!has_snapshot(&current, NO_DEPS, "1.0.0"));
}

#[test]
fn filtered_repeat_install_re_resolves_when_transitive_injected_dependency_changes() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("injectWorkspacePackages: true\ndedupeInjectedDeps: false\n");
    let pkg_a = fixture.project(
        "pkg-a",
        "pkg-a",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "pkg-b",
        "pkg-b",
        ManifestDeps { prod: &[("pkg-a", "workspace:*")], ..Default::default() },
    );
    fixture.project(
        "pkg-c",
        "pkg-c",
        ManifestDeps { prod: &[("pkg-b", "workspace:*")], ..Default::default() },
    );
    fixture.run(["install"]);
    assert!(!has_snapshot(&fixture.wanted(), NO_DEPS, "1.0.0"));

    set_dependency(&pkg_a, "dependencies", NO_DEPS, "1.0.0");
    let records = fixture.run(["--filter", "pkg-c", "install"]);

    assert!(
        !reports_up_to_date(&records),
        "transitive injected dependency change must invalidate repeat-install fast path",
    );
    assert!(
        has_snapshot(&fixture.wanted(), NO_DEPS, "1.0.0"),
        "re-resolved lockfile must include new transitive dependency of pkg-a",
    );
}

#[test]
fn filtered_repeat_install_fails_when_linked_dependency_version_breaks_range() {
    let fixture = WorkspaceFixture::new();
    let pkg_a = fixture.project(
        "pkg-a",
        "pkg-a",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "pkg-b",
        "pkg-b",
        ManifestDeps { prod: &[("pkg-a", "workspace:^1.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);

    set_version(&pkg_a, "2.0.0");
    let output = fixture.command_at(&fixture.workspace, ["--filter", "pkg-b", "install"]);

    assert!(
        !output.status.success(),
        "breaking workspace:^1.0.0 on linked pkg-a must invalidate fast path and fail resolution",
    );
}
