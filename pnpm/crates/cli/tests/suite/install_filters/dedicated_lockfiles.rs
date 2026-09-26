use super::{
    HELLO, ManifestDeps, PARENT, WorkspaceFixture, assert_full_wanted, has_link, has_snapshot,
    read_lockfile,
};

#[test]
fn filtered_install_with_dedicated_lockfiles_installs_only_selected_project() {
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
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["--filter", "selected", "install"]);

    let selected_lockfile = read_lockfile(&selected.join("pnpm-lock.yaml"));
    assert_full_wanted(&selected_lockfile, &["."]);
    assert!(has_snapshot(&selected_lockfile, HELLO, "1.0.0"));
    assert!(has_link(&selected, HELLO));
    assert!(!unselected.join("pnpm-lock.yaml").exists(), "unselected must not be installed");
    assert!(!unselected.join("node_modules").exists(), "unselected node_modules must be absent");
    assert!(
        !fixture.workspace.join("pnpm-lock.yaml").exists(),
        "dedicated lockfiles must not write a shared workspace lockfile",
    );
}

#[test]
fn recursive_install_with_dedicated_lockfiles_installs_every_project() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    let first = fixture.project(
        "first",
        "first",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let second = fixture.project(
        "second",
        "second",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["--recursive", "install"]);

    for (project, dependency) in [(&first, HELLO), (&second, PARENT)] {
        let lockfile = read_lockfile(&project.join("pnpm-lock.yaml"));
        assert_full_wanted(&lockfile, &["."]);
        assert!(has_link(project, dependency));
    }
    assert!(!fixture.workspace.join("pnpm-lock.yaml").exists());
}
