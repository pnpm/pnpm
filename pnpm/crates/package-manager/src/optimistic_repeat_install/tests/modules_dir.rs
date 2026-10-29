use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        settings::current_settings,
    },
    backdate_validated_files, isolated_included, setup_fresh_install_with_config, write_state,
};
use indexmap::IndexMap;
use pnpm_config::{Config, ProjectConfig};
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};

fn sibling_decision(sibling_modules_dir: &str, configure: impl FnOnce(&mut Config)) -> Decision {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        "",
        |config| {
            config.modules_dir = config.modules_dir.with_file_name("vendor");
            config.install_state_dir = config.modules_dir.join(".pnpm");
            configure(config);
        },
    );
    let sibling_dir = dir.path().join("pkg-a");
    fs::create_dir_all(sibling_dir.join(sibling_modules_dir)).unwrap();
    let sibling_manifest_path = sibling_dir.join("package.json");
    fs::write(
        &sibling_manifest_path,
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
    )
    .unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_manifest_path).unwrap();

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry {
            name: Some("pkg-a".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ]),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    })
}

#[test]
fn a_sibling_with_its_custom_modules_dir_is_installed() {
    assert_eq!(sibling_decision("vendor", |_| {}), Decision::UpToDate);
}

#[test]
fn a_sibling_with_only_node_modules_is_not_installed_under_a_custom_modules_dir() {
    assert!(matches!(
        sibling_decision("node_modules", |_| {}),
        Decision::Skipped { reason } if reason.contains("no node_modules directory"),
    ));
}

#[test]
fn a_sibling_with_the_modules_dir_its_package_configs_entry_names_is_installed() {
    let decision = sibling_decision("deps", |config| {
        config.shared_workspace_lockfile = false;
        config.package_configs = Some(IndexMap::from([(
            "pkg-a".to_string(),
            ProjectConfig { modules_dir: Some("deps".to_string()), ..ProjectConfig::default() },
        )]));
    });
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn a_root_with_a_multi_component_modules_dir_is_installed() {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"1.0.0"}"#,
        |config| {
            config.modules_dir = config.modules_dir.with_file_name("custom").join("vendor");
            config.install_state_dir = config.modules_dir.join(".pnpm");
        },
    );
    fs::create_dir_all(&config.modules_dir).unwrap();
    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&[(
            dir.path().to_path_buf(),
            &root_manifest,
        )]),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// The root declares `foo` and an optional `bar` that was never linked;
/// `link_foo` shapes the `foo` entry before the check runs.
fn root_direct_dependency_decision(link_foo: impl FnOnce(&std::path::Path)) -> Decision {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"1.0.0"},"optionalDependencies":{"bar":"1.0.0"}"#,
        |_| {},
    );
    link_foo(&config.modules_dir.join("foo"));

    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&[(
            dir.path().to_path_buf(),
            &root_manifest,
        )]),
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    })
}

#[test]
fn a_direct_dependency_linked_to_an_existing_target_is_installed() {
    let decision = root_direct_dependency_decision(|link| {
        let target = link.with_file_name(".pnpm").join("foo@1.0.0");
        fs::create_dir_all(&target).unwrap();
        pnpm_fs::symlink_dir(&target, link).unwrap();
    });
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn a_direct_dependency_linked_to_a_missing_target_is_not_installed() {
    let decision = root_direct_dependency_decision(|link| {
        pnpm_fs::symlink_dir(&link.with_file_name(".pnpm").join("foo@1.0.0"), link).unwrap();
    });
    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("missing target"),
    ));
}

#[test]
fn a_hoisted_sibling_dependency_linked_to_a_missing_target_is_not_installed() {
    let linker = pnpm_config::NodeLinker::Hoisted;
    let (dir, config, root_manifest) =
        setup_fresh_install_with_config(linker, "root", "1.0.0", "", |_| {});
    let sibling_dir = dir.path().join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    let sibling_manifest_path = sibling_dir.join("package.json");
    fs::write(
        &sibling_manifest_path,
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
    )
    .unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_manifest_path).unwrap();
    pnpm_fs::symlink_dir(&dir.path().join("missing-foo"), &config.modules_dir.join("foo")).unwrap();

    let settings = current_settings(config, linker, isolated_included(), None);
    let projects = BTreeMap::from([
        (
            dir.path()
                .to_string_lossy()
                .into_owned(),
            ProjectEntry {
                name: Some("root".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: false,
            },
        ),
        (
            sibling_dir.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("pkg-a".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: false,
            },
        ),
    ]);
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ]),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: linker,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    });
    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("missing target"),
    ));
}

#[test]
fn expanded_selected_modules_dir_check_follows_aliases_paths_and_matching_versions() {
    let linker = pnpm_config::NodeLinker::Isolated;
    let (dir, config, root_manifest) =
        setup_fresh_install_with_config(linker, "root", "1.0.0", "", |config| {
            config.link_workspace_packages = pnpm_config::LinkWorkspacePackages::DirectOnly;
        });
    let write_pkg = |rel: &str, json: &str, create_modules_dir: bool| {
        let pkg_dir = dir.path().join(rel);
        fs::create_dir_all(&pkg_dir).unwrap();
        if create_modules_dir {
            fs::create_dir_all(pkg_dir.join("node_modules")).unwrap();
        }
        let manifest_path = pkg_dir.join("package.json");
        fs::write(&manifest_path, json).unwrap();
        (pkg_dir, PackageManifest::from_path(manifest_path).unwrap())
    };
    let (app_dir, app_manifest) = write_pkg(
        "app",
        r#"{"name":"app","version":"1.0.0","dependencies":{"alias-b":"npm:pkg-b@^1.0.0","rel-c":"workspace:..\\pkg-c","pkg-d":"catalog:","file-f":"file:../pkg-f"},"peerDependencies":{"pkg-e":"workspace:*"}}"#,
        true,
    );
    let (pkg_b_dir, pkg_b_manifest) = write_pkg(
        "pkg-b",
        r#"{"name":"pkg-b","version":"1.2.0","dependencies":{"foo":"1.0.0"}}"#,
        true,
    );
    let (pkg_c_dir, pkg_c_manifest) = write_pkg(
        "pkg-c",
        r#"{"name":"pkg-c","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        true,
    );
    let (pkg_d_v1_dir, pkg_d_v1_manifest) = write_pkg(
        "pkg-d-v1",
        r#"{"name":"pkg-d","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        false,
    );
    let (pkg_d_v2_dir, pkg_d_v2_manifest) = write_pkg(
        "pkg-d-v2",
        r#"{"name":"pkg-d","version":"2.1.0-beta.0","dependencies":{"foo":"1.0.0"}}"#,
        true,
    );
    let (pkg_e_dir, pkg_e_manifest) = write_pkg(
        "pkg-e",
        r#"{"name":"pkg-e","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        true,
    );
    let (pkg_f_dir, pkg_f_manifest) = write_pkg(
        "pkg-f",
        r#"{"name":"pkg-f","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        true,
    );

    let project_manifests = [
        (dir.path().to_path_buf(), &root_manifest),
        (app_dir.clone(), &app_manifest),
        (pkg_b_dir.clone(), &pkg_b_manifest),
        (pkg_c_dir.clone(), &pkg_c_manifest),
        (pkg_d_v1_dir, &pkg_d_v1_manifest),
        (pkg_d_v2_dir.clone(), &pkg_d_v2_manifest),
        (pkg_e_dir.clone(), &pkg_e_manifest),
        (pkg_f_dir.clone(), &pkg_f_manifest),
    ];
    let catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("pkg-d".to_string(), "^2.0.0".to_string())]),
    )]);
    let check = OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&project_manifests),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
        layout: crate::RepeatInstallLayout {
            node_linker: linker,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    };
    let state = pnpm_workspace_state::WorkspaceState::default();
    let selected = [app_dir.as_path()];

    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        ),
        None,
    );

    fs::remove_dir_all(pkg_b_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("pkg-b"),
    );
    fs::create_dir_all(pkg_b_dir.join("node_modules")).unwrap();

    fs::remove_dir_all(pkg_c_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("pkg-c"),
    );
    fs::create_dir_all(pkg_c_dir.join("node_modules")).unwrap();

    fs::remove_dir_all(pkg_d_v2_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("pkg-d"),
    );
    fs::create_dir_all(pkg_d_v2_dir.join("node_modules")).unwrap();

    fs::remove_dir_all(pkg_e_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("pkg-e"),
    );
    let mut no_auto_peers_state = state.clone();
    no_auto_peers_state.settings.auto_install_peers = Some(false);
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &no_auto_peers_state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        ),
        None,
    );
    fs::create_dir_all(pkg_e_dir.join("node_modules")).unwrap();

    fs::remove_dir_all(pkg_f_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("pkg-f"),
    );
}

#[test]
fn expanded_selected_modules_dir_check_under_dedicated_lockfiles_with_injection_follows_only_link_deps()
 {
    let linker = pnpm_config::NodeLinker::Isolated;
    let (dir, config, root_manifest) =
        setup_fresh_install_with_config(linker, "root", "1.0.0", "", |config| {
            config.shared_workspace_lockfile = false;
            config.inject_workspace_packages = true;
        });
    let write_pkg = |rel: &str, json: &str, create_modules_dir: bool| {
        let pkg_dir = dir.path().join(rel);
        fs::create_dir_all(&pkg_dir).unwrap();
        if create_modules_dir {
            fs::create_dir_all(pkg_dir.join("node_modules")).unwrap();
        }
        let manifest_path = pkg_dir.join("package.json");
        fs::write(&manifest_path, json).unwrap();
        (pkg_dir, PackageManifest::from_path(manifest_path).unwrap())
    };
    let (app_dir, app_manifest) = write_pkg(
        "app",
        r#"{"name":"app","version":"1.0.0","dependencies":{"injected":"workspace:*","linked":"link:../linked"}}"#,
        true,
    );
    let (injected_dir, injected_manifest) = write_pkg(
        "injected",
        r#"{"name":"injected","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        false,
    );
    let (linked_dir, linked_manifest) = write_pkg(
        "linked",
        r#"{"name":"linked","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
        false,
    );

    let project_manifests = [
        (dir.path().to_path_buf(), &root_manifest),
        (app_dir.clone(), &app_manifest),
        (injected_dir, &injected_manifest),
        (linked_dir.clone(), &linked_manifest),
    ];
    let catalogs = BTreeMap::default();
    let check = OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&project_manifests),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
        layout: crate::RepeatInstallLayout {
            node_linker: linker,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    };
    let state = pnpm_workspace_state::WorkspaceState::default();
    let selected = [app_dir.as_path()];

    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        )
        .as_deref(),
        Some("linked"),
    );

    fs::create_dir_all(linked_dir.join("node_modules")).unwrap();
    assert_eq!(
        super::super::modules_dirs::first_selected_project_missing_modules_dir(
            &check,
            &state,
            &selected,
            super::super::modules_dirs::SelectedReach::WithWorkspaceDeps,
        ),
        None,
    );
}

/// The repeat-install decision for a single-project workspace whose manifest
/// declares `dependencies_json`, after `prepare_modules` has had its way with
/// the modules directory the install created.
fn root_decision(
    dependencies_json: &str,
    prepare_modules: impl FnOnce(&std::path::Path),
) -> Decision {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        dependencies_json,
        |_| {},
    );
    prepare_modules(&config.modules_dir);
    fs::create_dir_all(dir.path().join("lib")).unwrap();
    let lockfile: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      lib:\n        specifier: workspace:*\n        version: link:lib\n",
    )
    .unwrap();
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: crate::DeclaredManifests::new(&[(
            dir.path().to_path_buf(),
            &root_manifest,
        )]),
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifests: crate::RepeatInstallManifests::ON_DISK,
    })
}

/// A `workspace:` dependency links into a directory that is on disk, so the
/// linker creates its entry on every install. Deleting the entry moves
/// nothing the recorded state sees, so the fast path has to notice the
/// absence itself instead of reporting "Already up to date" over a missing
/// link (<https://github.com/pnpm/pnpm/issues/17485>).
#[test]
fn a_deleted_workspace_dependency_link_is_reinstalled() {
    let decision = root_decision(r#""dependencies":{"lib":"workspace:*"}"#, |modules_dir| {
        fs::remove_dir(modules_dir.join("lib")).unwrap();
    });
    assert!(
        matches!(
            &decision,
            Decision::Skipped { reason } if reason.contains("direct dependency link is missing")
        ),
        "expected the fast path to stand down, got {decision:?}",
    );
}

#[test]
fn a_linked_workspace_dependency_keeps_the_fast_path() {
    let decision = root_decision(r#""dependencies":{"lib":"workspace:*"}"#, |modules_dir| {
        fs::create_dir_all(modules_dir.join("lib")).unwrap();
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// The absence check stays off for a registry dependency: an excluded or
/// skipped optional one has no entry, and a full install would not create one
/// either.
#[test]
fn a_missing_registry_dependency_keeps_the_fast_path() {
    let decision = root_decision(r#""dependencies":{"foo":"1.0.0"}"#, |_| {});
    assert_eq!(decision, Decision::UpToDate);
}

/// The wanted lockfile of the workspace [`workspace_link_dangling`] checks:
/// both projects link the `lib` project.
const LINKED_LIB_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:lib

  pkg-a:
    dependencies:
      foo:
        specifier: 1.0.0
        version: 1.0.0
      lib:
        specifier: workspace:*
        version: link:../lib
";

/// Whether the direct-dependency link gate fires for a workspace of the root
/// and `pkg-a`, with [`LINKED_LIB_LOCKFILE`] as the wanted lockfile, after
/// `prepare` lays out the modules directories under the workspace root.
/// `held` names the projects the gate holds, by directory relative to the
/// root, or every project when `None`.
fn workspace_link_dangling(
    node_linker: pnpm_config::NodeLinker,
    configure: impl FnOnce(&mut Config),
    held: Option<&[&str]>,
    prepare: impl FnOnce(&std::path::Path),
) -> bool {
    workspace_link_dangling_with_lockfile(
        Some(LINKED_LIB_LOCKFILE),
        node_linker,
        configure,
        held,
        prepare,
    )
}

/// [`workspace_link_dangling`] with `lockfile` as the wanted lockfile, or
/// none when `None`.
fn workspace_link_dangling_with_lockfile(
    lockfile: Option<&str>,
    node_linker: pnpm_config::NodeLinker,
    configure: impl FnOnce(&mut Config),
    held: Option<&[&str]>,
    prepare: impl FnOnce(&std::path::Path),
) -> bool {
    let lockfile: Option<pnpm_lockfile::Lockfile> =
        lockfile.map(|lockfile| serde_saphyr::from_str(lockfile).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sibling_dir = root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"lib":"workspace:*"}}"#,
    )
    .unwrap();
    fs::write(
        sibling_dir.join("package.json"),
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"lib":"workspace:*","foo":"1.0.0"}}"#,
    )
    .unwrap();
    let root_manifest = PackageManifest::from_path(root.join("package.json")).unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_dir.join("package.json")).unwrap();
    let mut config = Config::new();
    config.modules_dir = root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    configure(&mut config);
    let config = config.leak();
    prepare(root);
    let held = held.map(|dirs| {
        dirs.iter()
            .map(|dir| pnpm_fs::lexical_normalize(&root.join(dir)))
            .collect::<std::collections::HashSet<_>>()
    });
    super::super::modules_dirs::direct_dependency_link_dangling(
        &OptimisticRepeatInstallCheck {
            workspace_root: root,
            config,
            project_manifests: crate::DeclaredManifests::new(&[
                (root.to_path_buf(), &root_manifest),
                (sibling_dir, &sibling_manifest),
            ]),
            is_workspace_install: true,
            lockfile: MaybeLazyLockfile::Loaded(lockfile.as_ref()),
            catalogs: &BTreeMap::default(),
            layout: crate::RepeatInstallLayout {
                node_linker,
                included: isolated_included(),
                supported_architectures: None,
            },
            manifests: crate::RepeatInstallManifests::ON_DISK,
        },
        held.as_ref(),
    )
}

/// Create `entry` as a link to `target`, both relative to `root`.
fn link_dir(root: &std::path::Path, target: &str, entry: &str) {
    let entry = root.join(entry);
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    fs::create_dir_all(root.join(target)).unwrap();
    pnpm_fs::symlink_dir(&root.join(target), &entry).unwrap();
}

/// Create `entry` as a link to the workspace's `lib` project.
fn link_lib(root: &std::path::Path, entry: &str) {
    link_dir(root, "lib", entry);
}

#[test]
fn a_workspace_link_missing_from_a_sibling_is_dangling() {
    let dangling = workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |_| {},
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("pkg-a/node_modules")).unwrap();
        },
    );
    assert!(dangling);
}

/// The hoisted linker places a sibling's `workspace:` dependency in either
/// modules directory, so an entry in one of them is enough.
#[test]
fn a_hoisted_workspace_link_in_either_modules_dir_is_not_dangling() {
    for sibling_entry in ["pkg-a/node_modules/lib", "node_modules/lib"] {
        let dangling = workspace_link_dangling(
            pnpm_config::NodeLinker::Hoisted,
            |_| {},
            None,
            |root| {
                link_lib(root, "node_modules/lib");
                if sibling_entry != "node_modules/lib" {
                    link_lib(root, sibling_entry);
                }
            },
        );
        assert!(!dangling, "{sibling_entry}");
    }
}

/// Under `dedupeDirectDeps` a sibling's dependency that resolves to the root's
/// target is linked only into the root's modules directory, while the sibling
/// keeps its own for the dependencies that differ.
#[test]
fn a_deduped_workspace_link_is_not_dangling() {
    let dangling = workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |config| config.dedupe_direct_deps = true,
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("pkg-a/node_modules/foo")).unwrap();
        },
    );
    assert!(!dangling);
}

/// `symlink: false` links no direct dependency at all.
#[test]
fn a_missing_workspace_link_is_not_dangling_without_symlinks() {
    let dangling = workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |config| config.symlink = false,
        None,
        |root| fs::create_dir_all(root.join("node_modules")).unwrap(),
    );
    assert!(!dangling);
}

/// A filtered install does not link the projects it did not select, so their
/// missing entries leave the gate alone. A broken link still counts anywhere.
#[test]
fn only_held_projects_need_their_workspace_links() {
    let unselected_missing = |root: &std::path::Path| {
        link_lib(root, "node_modules/lib");
        fs::create_dir_all(root.join("pkg-a/node_modules")).unwrap();
    };
    assert!(!workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |_| {},
        Some(&[""]),
        unselected_missing,
    ));
    assert!(workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |_| {},
        Some(&["pkg-a"]),
        unselected_missing,
    ));
    let unselected_broken = |root: &std::path::Path| {
        link_lib(root, "node_modules/lib");
        link_dir(root, "gone", "pkg-a/node_modules/lib");
        fs::remove_dir(root.join("gone")).unwrap();
    };
    assert!(workspace_link_dangling(
        pnpm_config::NodeLinker::Isolated,
        |_| {},
        Some(&[""]),
        unselected_broken,
    ));
}

/// Under `dedupeDirectDeps` a root entry stands in for a sibling's only when
/// it links the same target.
#[test]
fn a_root_link_to_another_target_does_not_stand_in_for_a_deduped_one() {
    let lockfile = LINKED_LIB_LOCKFILE.replace("version: link:../lib", "version: link:../other");
    let dangling = workspace_link_dangling_with_lockfile(
        Some(&lockfile),
        pnpm_config::NodeLinker::Isolated,
        |config| config.dedupe_direct_deps = true,
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("other")).unwrap();
            fs::create_dir_all(root.join("pkg-a/node_modules/foo")).unwrap();
        },
    );
    assert!(dangling);
}

/// The lockfile, not the manifest, says what the last install linked. An
/// override that removes the dependency leaves it out of the importer, and an
/// injected workspace dependency resolves to a `file:` snapshot that a
/// platform check may skip.
#[test]
fn a_local_dependency_the_lockfile_does_not_link_may_be_missing() {
    let removed = LINKED_LIB_LOCKFILE.replace(
        "      lib:\n        specifier: workspace:*\n        version: link:../lib\n",
        "",
    );
    let injected = LINKED_LIB_LOCKFILE.replace("version: link:../lib", "version: file:lib");
    for lockfile in [removed, injected] {
        let dangling = workspace_link_dangling_with_lockfile(
            Some(&lockfile),
            pnpm_config::NodeLinker::Isolated,
            |_| {},
            None,
            |root| {
                link_lib(root, "node_modules/lib");
                fs::create_dir_all(root.join("pkg-a/node_modules/foo")).unwrap();
            },
        );
        assert!(!dangling, "{lockfile}");
    }
}

/// A root link to the target the manifest names proves a deduped sibling
/// installed without reading any lockfile.
#[test]
fn a_deduped_workspace_link_needs_no_lockfile() {
    let dangling = workspace_link_dangling_with_lockfile(
        None,
        pnpm_config::NodeLinker::Isolated,
        |config| config.dedupe_direct_deps = true,
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("pkg-a/node_modules/foo")).unwrap();
        },
    );
    assert!(!dangling);
}

/// The current lockfile stands in for a deleted wanted lockfile.
#[test]
fn a_deleted_workspace_link_is_found_through_the_current_lockfile() {
    let dangling = workspace_link_dangling_with_lockfile(
        None,
        pnpm_config::NodeLinker::Isolated,
        |_| {},
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("pkg-a/node_modules")).unwrap();
            fs::create_dir_all(root.join("node_modules/.pnpm")).unwrap();
            fs::write(
                root.join("node_modules/.pnpm").join(pnpm_lockfile::Lockfile::CURRENT_FILE_NAME),
                LINKED_LIB_LOCKFILE,
            )
            .unwrap();
        },
    );
    assert!(dangling);
}

/// Under `sharedWorkspaceLockfile: false` a sibling records its links in its
/// own lockfile.
#[test]
fn a_deleted_workspace_link_is_found_through_a_dedicated_lockfile() {
    let dangling = workspace_link_dangling_with_lockfile(
        None,
        pnpm_config::NodeLinker::Isolated,
        |config| config.shared_workspace_lockfile = false,
        None,
        |root| {
            link_lib(root, "node_modules/lib");
            fs::create_dir_all(root.join("pkg-a/node_modules")).unwrap();
            fs::write(
                root.join("pkg-a").join(pnpm_lockfile::Lockfile::FILE_NAME),
                "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      lib:\n        specifier: workspace:*\n        version: link:../lib\n",
            )
            .unwrap();
        },
    );
    assert!(dangling);
}
