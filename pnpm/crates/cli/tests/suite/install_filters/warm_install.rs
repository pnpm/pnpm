use super::{
    HELLO, ManifestDeps, PARENT, WorkspaceFixture, assert_eq, fs, importing_started_count,
    reports_up_to_date,
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
