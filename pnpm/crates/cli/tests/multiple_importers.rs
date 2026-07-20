//! Ports of the upstream multi-importer install suites — the
//! workspace-subset (`--filter`) and shared-lockfile scenarios from
//! `installing/deps-installer/test/install/multipleImporters.ts`, the
//! workspace cases of `installing/deps-restorer/test/index.ts`, and the
//! CLI-level `pnpm/test/monorepo/index.ts` items. See
//! `plans/TEST_PORTING.md` § "Support Workspaces".
//!
//! Upstream drives subset installs through `mutateModules` /
//! `mutateModulesInSingleProject` with a shared lockfile dir; the CLI
//! equivalent is `pnpm --filter <project> install` in a
//! `pnpm-workspace.yaml` workspace.

#![cfg(unix)] // pnpm CLI: 'program not found' on Windows runners.

pub mod _utils;
pub use _utils::*;

use pacquet_testing_utils::fs::is_path_executable;
use serde_json::json;
use std::fs;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
const FOOBAR: &str = "@pnpm.e2e/foobar";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const NO_DEPS: &str = "@foo/no-deps";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
const QAR: &str = "@pnpm.e2e/qar";

/// TS: `dependencies of other importers are not pruned when (headless)
/// installing for a subset of importers` (`multipleImporters.ts:438`).
/// After bumping a selected project's dependency lockfile-only, the
/// frozen subset install must replace only the selected project's old
/// slot; the unselected project's materialization stays.
#[test]
fn deps_of_other_importers_are_not_pruned_when_headless_installing_a_subset() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("modulesCacheMaxAge: 0\n");
    let project_1 = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(FOO, "1.0.0")], ..Default::default() },
    );
    let project_2 = fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    assert!(fixture.slot(FOO, "1.0.0").exists());

    fixture.run(["--filter", "project-1", "add", &format!("{FOO}@2.0.0"), "--lockfile-only"]);
    fixture.run(["--filter", "project-1", "install", "--frozen-lockfile"]);

    assert!(has_link(&project_1, FOO));
    assert!(has_link(&project_2, NO_DEPS));
    assert!(fixture.slot(FOO, "2.0.0").exists());
    assert!(!fixture.slot(FOO, "1.0.0").exists(), "the replaced slot must be pruned");
    assert!(
        fixture.slot(NO_DEPS, "1.0.0").exists(),
        "the unselected project's slot must survive the subset install",
    );
}

/// TS: `install only the dependencies of the specified importer. The
/// current lockfile has importers that do not exist anymore`
/// (`multipleImporters.ts:208`). A project leaves the workspace while
/// its materialization stays on disk; a later subset install must keep
/// the stale importer (and its packages) in the current lockfile.
#[test]
fn stale_current_lockfile_importers_are_retained_on_subset_install() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("hoistPattern:\n  - '*'\n");
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(FOO, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    let project_3 = fixture.project(
        "project-3",
        "project-3",
        ManifestDeps { prod: &[(FOOBAR, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    let before = fixture.current();
    assert!(before.importers.contains_key("packages/project-3"));

    fs::remove_file(project_3.join("package.json")).expect("drop project-3 from the workspace");
    fixture.run(["install", "--lockfile-only", "--no-prefer-frozen-lockfile"]);
    assert!(
        !fixture.wanted().importers.contains_key("packages/project-3"),
        "the wanted lockfile must drop the removed importer",
    );

    fixture.run(["--filter", "project-1", "add", PARENT]);

    let current = fixture.current();
    assert!(
        current.importers.contains_key("packages/project-3"),
        "the still-materialized stale importer must stay in the current lockfile",
    );
    assert!(has_snapshot(&current, FOOBAR, "100.0.0"));
}

/// TS: `current lockfile contains only installed dependencies when
/// adding a new importer to workspace with shared lockfile`
/// (`multipleImporters.ts:730`).
#[test]
fn current_lockfile_contains_only_installed_dependencies() {
    let fixture = WorkspaceFixture::new();
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(FOO, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "project-1", "install", "--lockfile-only"]);
    fixture.run(["--filter", "project-2", "install"]);

    let current = fixture.current();
    let package_keys: Vec<String> =
        current.packages.iter().flatten().map(|(key, _)| key.to_string()).collect();
    assert_eq!(package_keys, [format!("{NO_DEPS}@1.0.0")]);
}

/// TS: `headless install is used when package linked to another package
/// in the workspace` (`multipleImporters.ts:540`). The subset install
/// after a lockfile-only resolve must go headless (it announces
/// "Lockfile is up to date, resolution step is skipped").
///
/// Upstream additionally asserts the unselected link target's own
/// dependencies are *not* installed; pacquet expands the subset closure
/// through importer-level links, so that tail lives in
/// [`known_failures::subset_install_does_not_install_unselected_link_targets_dependencies`].
#[test]
fn headless_install_is_used_when_package_is_linked_to_another_workspace_package() {
    let fixture = WorkspaceFixture::new();
    let project_1 = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps {
            prod: &[(FOO, "1.0.0"), ("project-2", "link:../project-2")],
            ..Default::default()
        },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    let records = fixture.run(["--filter", "project-1", "install"]);

    assert!(has_up_to_date_log(&records), "the subset install must go headless: {records:#?}");
    assert!(has_link(&project_1, FOO));
    assert!(has_link(&project_1, "project-2"));
}

/// TS: `headless install is used with an up-to-date lockfile when
/// package references another package via workspace: protocol`
/// (`multipleImporters.ts:598`).
#[test]
fn headless_install_is_used_with_workspace_protocol_references() {
    let fixture = WorkspaceFixture::new();
    let project_1 = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps {
            prod: &[(FOO, "1.0.0"), ("project-2", "workspace:1.0.0")],
            ..Default::default()
        },
    );
    let project_2 = fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    let records = fixture.run(["install"]);

    assert!(has_up_to_date_log(&records), "the install must go headless: {records:#?}");
    assert!(has_link(&project_1, FOO));
    assert!(has_link(&project_1, "project-2"));
    assert!(has_link(&project_2, NO_DEPS));
}

/// TS: `headless install is used when packages are not linked from the
/// workspace (unless workspace ranges are used)`
/// (`multipleImporters.ts:656`). With `linkWorkspacePackages: false`, a
/// `workspace:*` range still links while a plain semver range resolves
/// from the registry — and the resulting mixed lockfile still passes
/// the freshness gate, so the reinstall goes headless.
#[test]
fn headless_install_is_used_when_packages_are_not_linked_from_the_workspace() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("linkWorkspacePackages: false\n");
    let foo_project = fixture.project(
        "foo",
        FOO,
        ManifestDeps { prod: &[(QAR, "workspace:*")], ..Default::default() },
    );
    let bar_project = fixture.project(
        "bar",
        "@pnpm.e2e/bar",
        ManifestDeps { prod: &[(QAR, "100.0.0")], ..Default::default() },
    );
    let qar_project = fixture.project("qar", QAR, ManifestDeps::default());
    set_version(&qar_project, "100.0.0");
    fixture.run(["install", "--lockfile-only"]);

    let records = fixture.run(["install"]);

    assert!(has_up_to_date_log(&records), "the install must go headless: {records:#?}");
    let foo_qar = fs::canonicalize(foo_project.join("node_modules").join(QAR))
        .expect("foo's workspace-range dependency is linked");
    assert_eq!(foo_qar, fs::canonicalize(&qar_project).expect("canonicalize the qar project"));
    let bar_qar = fs::canonicalize(bar_project.join("node_modules").join(QAR))
        .expect("bar's registry dependency is materialized");
    assert!(
        bar_qar.starts_with(fs::canonicalize(fixture.slot(QAR, "100.0.0")).expect("qar slot")),
        "a plain semver range must resolve from the registry when linking is off, got {bar_qar:?}",
    );
}

/// TS: `partial installation in a monorepo does not remove dependencies
/// of other workspace projects when lockfile is frozen`
/// (`multipleImporters.ts:865`). The wanted lockfile is edited so the
/// unselected project's transitive pin points elsewhere; the frozen
/// subset install must still not remove the previously-materialized
/// slots of the unselected project.
#[test]
fn partial_frozen_install_does_not_remove_dependencies_of_other_workspace_projects() {
    let fixture = WorkspaceFixture::new();
    // Project-1 pins 101.0.0 directly — outside project-2's transitive
    // `^100.0.0` range, so the resolver cannot converge the transitive
    // onto it and both versions get locked and materialized.
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(FOO, "1.0.0"), (DEP, "101.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    assert!(fixture.slot(DEP, "100.1.0").exists());

    // Repin the unselected project's transitive from 100.1.0 (what the
    // resolver picked for `^100.0.0`) to the 101.0.0 entry that
    // project-1's direct dependency already locked — the upstream test
    // rewrites the lockfile to the same effect, leaving the 100.1.0
    // materialization orphaned.
    repin_snapshot_dependency(
        &fixture.workspace.join("pnpm-lock.yaml"),
        &format!("{PARENT}@100.0.0"),
        DEP,
        "101.0.0",
    );

    fixture.run(["--filter", "project-1", "install", "--frozen-lockfile"]);

    assert!(fixture.slot(FOO, "1.0.0").exists());
    assert!(
        fixture.slot(PARENT, "100.0.0").exists(),
        "the unselected project's direct dependency must not be removed",
    );
    assert!(
        fixture.slot(DEP, "100.1.0").exists(),
        "the unselected project's previously-materialized transitive must not be removed",
    );
}

/// TS: `resolve a subdependency from the workspace`
/// (`multipleImporters.ts:1427`). With `linkWorkspacePackages: deep`, a
/// transitive resolves to the workspace project as a `link:`, and the
/// headless reinstall does not fail on links in subdeps.
#[test]
fn resolve_a_subdependency_from_the_workspace() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("linkWorkspacePackages: deep\n");
    fixture.project(
        "project",
        "project",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let dep_project = fixture.project("dep", DEP, ManifestDeps::default());
    set_version(&dep_project, "100.1.0");
    fixture.run(["install"]);

    let wanted = fixture.wanted();
    let parent_snapshots = snapshot_entries(&wanted, PARENT);
    assert_eq!(parent_snapshots.len(), 1);
    let subdependency = parent_snapshots[0]
        .1
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&DEP.parse().expect("parse package name")))
        .expect("parent snapshot records the subdependency")
        .to_string();
    assert_eq!(subdependency, "link:packages/dep");

    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove node_modules");
    fixture.run(["install", "--frozen-lockfile"]);
}

/// TS: `resolve a subdependency from the workspace, when it uses the
/// workspace protocol` (`multipleImporters.ts:1563`). The `workspace:*`
/// pin arrives through an override while `linkWorkspacePackages` is
/// off.
#[test]
fn resolve_a_subdependency_from_the_workspace_via_workspace_protocol_override() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "linkWorkspacePackages: false\noverrides:\n  '{DEP}': 'workspace:*'\n",
    ));
    fixture.project(
        "project",
        "project",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let dep_project = fixture.project("dep", DEP, ManifestDeps::default());
    set_version(&dep_project, "100.1.0");
    fixture.run(["install"]);

    let wanted = fixture.wanted();
    let parent_snapshots = snapshot_entries(&wanted, PARENT);
    assert_eq!(parent_snapshots.len(), 1);
    let subdependency = parent_snapshots[0]
        .1
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&DEP.parse().expect("parse package name")))
        .expect("parent snapshot records the subdependency")
        .to_string();
    assert_eq!(subdependency, "link:packages/dep");

    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove node_modules");
    fixture.run(["install", "--frozen-lockfile"]);
}

/// TS: `installing in a workspace` (`deps-restorer/test/index.ts:789`).
/// A subset headless install keeps the other project's packages in the
/// current lockfile.
#[test]
fn subset_headless_install_keeps_other_projects_packages_in_current_lockfile() {
    let fixture = WorkspaceFixture::new();
    let foo_project = fixture.project(
        "foo",
        "foo",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let bar_project = fixture.project(
        "bar",
        "bar",
        ManifestDeps { prod: &[("foo", "workspace:*"), (NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    assert!(has_link(&bar_project, "foo"));

    fixture.run(["--filter", "foo", "install", "--frozen-lockfile"]);

    assert!(has_link(&foo_project, HELLO));
    assert!(has_link(&bar_project, "foo"));
    let current = fixture.current();
    assert!(has_snapshot(&current, HELLO, "1.0.0"));
    assert!(
        has_snapshot(&current, NO_DEPS, "1.0.0"),
        "the other project's package must stay in the current lockfile",
    );
}

/// TS: `installing a package deeply installs all required dependencies`
/// (`deps-restorer/test/index.ts:897`). The selected project depends on
/// an external registry package whose own dependency resolves to a
/// workspace project (`link:packages/f` in its snapshot); the frozen
/// subset install must materialize that linked project's own
/// dependencies too.
#[test]
fn subset_headless_install_deeply_materializes_workspace_linked_dependencies() {
    let fixture = WorkspaceFixture::new();
    let f_project = fixture.project(
        "f",
        "@pnpm.e2e/internal-f",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let g_project = fixture.project(
        "g",
        "@pnpm.e2e/internal-g",
        ManifestDeps {
            prod: &[("@pnpm.e2e/external-depend-on-internal-dep", "1.0.0")],
            ..Default::default()
        },
    );
    fixture.run(["install", "--lockfile-only"]);
    // Pacquet's resolver records the external manifest's
    // `link:packages/f` relative to the dependent importer
    // (`link:packages/g/packages/f`); the upstream fixture lockfile
    // records it relative to the lockfile dir. Rewrite the snapshot to
    // the upstream (lockfile-relative) shape — the subject under test
    // is the headless materialization of snapshot-level links, and
    // upstream drives it from a hand-authored lockfile too.
    repin_snapshot_dependency(
        &fixture.workspace.join("pnpm-lock.yaml"),
        "@pnpm.e2e/external-depend-on-internal-dep@1.0.0",
        "@pnpm.e2e/internal-f",
        "link:packages/f",
    );

    fixture.run(["--filter", "@pnpm.e2e/internal-g", "install", "--frozen-lockfile"]);

    assert!(has_link(&g_project, "@pnpm.e2e/external-depend-on-internal-dep"));
    assert!(
        has_link(&f_project, HELLO),
        "the snapshot-linked workspace project's own dependencies must be materialized",
    );
    assert!(fixture.slot(HELLO, "1.0.0").exists());
}

/// TS: `linking the package's bin to another workspace package in a
/// monorepo` (`pnpm/test/monorepo/index.ts:1317`), including the frozen
/// reinstall after wiping every `node_modules`.
#[test]
fn links_workspace_package_bin_into_dependent_project() {
    let fixture = WorkspaceFixture::new();
    let hello_project = fixture.workspace.join("packages/hello");
    fs::create_dir_all(&hello_project).expect("create the hello project");
    fs::write(
        hello_project.join("package.json"),
        json!({ "name": "hello", "version": "1.0.0", "bin": "index.js" }).to_string(),
    )
    .expect("write hello's package.json");
    fs::write(hello_project.join("index.js"), "#!/usr/bin/env node\n").expect("write hello's bin");
    let main_project = fixture.project(
        "main",
        "main",
        ManifestDeps { prod: &[("hello", "workspace:*")], ..Default::default() },
    );

    fixture.run(["install"]);
    let bin_path = main_project.join("node_modules/.bin/hello");
    assert!(is_path_executable(&bin_path), "expected an executable bin at {bin_path:?}");

    fs::remove_dir_all(main_project.join("node_modules")).expect("remove main's node_modules");
    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove root node_modules");
    fixture.run(["install", "--frozen-lockfile"]);

    assert!(is_path_executable(&bin_path), "the frozen reinstall must re-link the bin");
}

/// TS: `custom virtual store directory in a workspace with shared
/// lockfile` (`pnpm/test/monorepo/index.ts:1514`). `.modules.yaml`
/// records the resolved custom `virtualStoreDir`, on the fresh install
/// and again after a frozen reinstall from a wiped state.
#[test]
fn custom_virtual_store_directory_in_a_workspace_with_shared_lockfile() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("virtualStoreDir: virtual-store\n");
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(FOO, "1.0.0")], ..Default::default() },
    );

    let expected = fixture.workspace.join("virtual-store");
    let assert_recorded_virtual_store = |phase: &str| {
        let modules = fixture.modules();
        assert_eq!(
            dunce::canonicalize(&modules.virtual_store_dir)
                .unwrap_or_else(|_| modules.virtual_store_dir.clone().into()),
            dunce::canonicalize(&expected).expect("canonicalize the custom virtual store"),
            "[{phase}] .modules.yaml must record the custom virtualStoreDir",
        );
    };

    fixture.run(["install"]);
    assert_recorded_virtual_store("fresh");

    fs::remove_dir_all(&expected).expect("remove the virtual store");
    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove node_modules");
    fixture.run(["install", "--frozen-lockfile"]);
    assert_recorded_virtual_store("frozen");
}

/// TS: `symlink local package from the location described in its
/// publishConfig.directory when linkDirectory is true`
/// (`multipleImporters.ts:1766`).
#[test]
fn symlink_local_package_from_publish_config_directory() {
    let fixture = WorkspaceFixture::new();
    let project_1 = fixture.project("project-1", "project-1", ManifestDeps::default());
    let project_2 = fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[("project-1", "workspace:*")], ..Default::default() },
    );
    let mut project_1_manifest = read_manifest(&project_1);
    project_1_manifest["publishConfig"] = json!({
        "directory": "dist",
        "linkDirectory": true,
    });
    write_manifest_value(&project_1, &project_1_manifest);
    let publish_dir = project_1.join("dist");
    fs::create_dir_all(&publish_dir).expect("create publish directory");
    fs::write(
        publish_dir.join("package.json"),
        json!({ "name": "project-1-dist", "version": "1.0.0" }).to_string(),
    )
    .expect("write publish-directory manifest");

    let assert_publish_dir_is_linked = || {
        let linked = read_manifest(&project_2.join("node_modules/project-1"));
        assert_eq!(linked["name"], "project-1-dist");
    };

    fixture.run(["install"]);
    assert_publish_dir_is_linked();
    assert_eq!(
        fixture.wanted().importers["packages/project-1"].publish_directory.as_deref(),
        Some("dist"),
    );

    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove root node_modules");
    fs::remove_dir_all(project_2.join("node_modules")).expect("remove project-2 node_modules");
    fixture.run(["install", "--frozen-lockfile"]);
    assert_publish_dir_is_linked();
}

/// TS: `recursive install with shared-workspace-lockfile builds
/// workspace projects in correct order` (`pnpm/test/monorepo/index.ts:734`).
#[test]
fn recursive_install_builds_workspace_projects_in_correct_order() {
    let fixture = WorkspaceFixture::new();
    let dependency = fixture.project("project-999", "project-999", ManifestDeps::default());
    let scriptless_intermediate = fixture.project(
        "project-500",
        "project-500",
        ManifestDeps { dev: &[("project-999", "workspace:*")], ..Default::default() },
    );
    let dependent = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { dev: &[("project-500", "workspace:*")], ..Default::default() },
    );
    for (project, name) in [(&dependency, "project-999"), (&dependent, "project-1")] {
        let mut manifest = read_manifest(project);
        manifest["scripts"] = json!({
            "install": append_order_script(&format!("{name}-install")),
            "postinstall": append_order_script(&format!("{name}-postinstall")),
            "prepare": append_order_script(&format!("{name}-prepare")),
        });
        write_manifest_value(project, &manifest);
    }
    let order_path = fixture.workspace.join("order.txt");
    let mut expected = [
        "project-999-install",
        "project-999-postinstall",
        "project-999-prepare",
        "project-1-install",
        "project-1-postinstall",
        "project-1-prepare",
    ]
    .join("\n");
    expected.push('\n');

    fixture.run(["install"]);
    assert_eq!(fs::read_to_string(&order_path).expect("read fresh lifecycle order"), expected);

    fs::remove_file(&order_path).expect("reset lifecycle order");
    for modules_dir in [
        fixture.workspace.join("node_modules"),
        dependency.join("node_modules"),
        scriptless_intermediate.join("node_modules"),
        dependent.join("node_modules"),
    ] {
        if modules_dir.exists() {
            fs::remove_dir_all(modules_dir).expect("remove node_modules");
        }
    }
    fixture.run(["install", "--frozen-lockfile"]);
    assert_eq!(fs::read_to_string(order_path).expect("read frozen lifecycle order"), expected);
}

/// TS: `link the bin file of a workspace project that is created by a
/// lifecycle script` (`multipleImporters.ts:1900`).
#[test]
fn link_bin_of_workspace_project_created_by_lifecycle_script() {
    let fixture = WorkspaceFixture::new();
    let consumer = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[("project-2", "link:../project-2")], ..Default::default() },
    );
    let provider = fixture.project("project-2", "project-2", ManifestDeps::default());
    let mut consumer_manifest = read_manifest(&consumer);
    consumer_manifest["scripts"] = json!({ "prepare": "bin" });
    write_manifest_value(&consumer, &consumer_manifest);
    let mut provider_manifest = read_manifest(&provider);
    provider_manifest["bin"] = json!({ "bin": "bin.js" });
    provider_manifest["scripts"] = json!({
        "prepare": r#"node -e "require('fs').renameSync('__bin.js', 'bin.js')""#,
    });
    write_manifest_value(&provider, &provider_manifest);
    fs::write(
        provider.join("__bin.js"),
        "#!/usr/bin/env node\nrequire('fs').writeFileSync('created-by-prepare', '')\n",
    )
    .expect("write deferred bin");

    fixture.run(["install"]);
    assert!(consumer.join("created-by-prepare").exists());

    fs::remove_file(consumer.join("created-by-prepare")).expect("remove lifecycle marker");
    fs::rename(provider.join("bin.js"), provider.join("__bin.js")).expect("reset deferred bin");
    for modules_dir in [
        fixture.workspace.join("node_modules"),
        consumer.join("node_modules"),
        provider.join("node_modules"),
    ] {
        if modules_dir.exists() {
            fs::remove_dir_all(modules_dir).expect("remove node_modules");
        }
    }
    fixture.run(["install", "--frozen-lockfile"]);
    assert!(consumer.join("created-by-prepare").exists());
}

fn append_order_script(label: &str) -> String {
    format!(r#"node -e "require('fs').appendFileSync('../../order.txt', '{label}\\n')""#)
}

mod known_failures {
    //! Multi-importer cases blocked on features pacquet hasn't built
    //! yet. Each entry stubs the not-yet-built subject under test
    //! through [`pacquet_testing_utils::allow_known_failure`] so the
    //! test exits early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn per_project_workspace_lockfiles() -> KnownResult<()> {
        Err(KnownFailure::new(
            "`sharedWorkspaceLockfile: false` is not supported by \
             pacquet's install family \
             (`ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED`).",
        ))
    }

    fn importer_level_link_closure_divergence() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet's subset-install materialization closure follows \
             importer-level `link:` / workspace-linked dependencies of \
             the selected projects and installs the link targets' own \
             dependencies (`materialization_closure` in \
             `crates/package-manager/src/current_lockfile.rs`). The \
             TypeScript CLI keeps those targets shallow — only \
             `--filter <project>...` widens the selection — so the two \
             stacks need a shared decision before this can be pinned.",
        ))
    }

    /// TS: `dependencies of workspace projects are built during
    /// headless installation` (`pnpm/test/monorepo/index.ts:1281`) —
    /// the upstream fixture turns off `sharedWorkspaceLockfile`.
    #[test]
    fn workspace_project_dependencies_built_during_headless_install_with_dedicated_lockfiles() {
        allow_known_failure!(per_project_workspace_lockfiles());
    }

    /// TS: `custom virtual store directory in a workspace with not
    /// shared lockfile` (`pnpm/test/monorepo/index.ts:1467`).
    #[test]
    fn custom_virtual_store_directory_with_dedicated_lockfiles() {
        allow_known_failure!(per_project_workspace_lockfiles());
    }

    /// The tail of TS `headless install is used when package linked to
    /// another package in the workspace` (`multipleImporters.ts:540`):
    /// the unselected `link:` target's own dependencies must not be
    /// installed by the subset install.
    #[test]
    fn subset_install_does_not_install_unselected_link_targets_dependencies() {
        allow_known_failure!(importer_level_link_closure_divergence());
    }
}
