//! Injected workspace dependencies: a project copied (not symlinked) into a
//! consumer's `node_modules` through a `file:` snapshot.

use super::{
    super::Decision, backdate_validated_files, content_check_decision, isolated_included,
    record_installed, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

const LOCKFILE: &str = "lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
  injectWorkspacePackages: true

importers:

  .:
    dependencies:
      pkg-a:
        specifier: workspace:*
        version: file:pkg-a

  pkg-a: {}

packages:

  pkg-a@file:pkg-a:
    resolution: {directory: pkg-a, type: directory}

snapshots:

  pkg-a@file:pkg-a: {}
";

/// The decision for a repeat install of a workspace whose root injects the
/// `pkg-a` project, with manifests handed over in memory: `pkg-a` has no
/// `package.json` on disk, and `sibling_manifest` is what it declares now.
/// `None` hands over only the root's manifest, as a filtered install or a
/// per-project install does.
fn injected_sibling_decision(
    shared_workspace_lockfile: bool,
    sibling_manifest: Option<&serde_json::Value>,
) -> Decision {
    injected_sibling_decision_with_lockfile(shared_workspace_lockfile, sibling_manifest, LOCKFILE)
}

fn injected_sibling_decision_with_lockfile(
    shared_workspace_lockfile: bool,
    sibling_manifest: Option<&serde_json::Value>,
    lockfile: &str,
) -> Decision {
    injected_sibling_decision_with_projects(
        (shared_workspace_lockfile, sibling_manifest, lockfile),
        &[],
    )
}

/// An extra workspace project: its directory, the manifest the previous
/// install recorded, and the manifest it declares now.
type ExtraProject<'a> = (&'a str, serde_json::Value, serde_json::Value);

/// [`injected_sibling_decision_with_lockfile`] with `extra_projects` in the
/// workspace too.
fn injected_sibling_decision_with_projects(
    (shared_workspace_lockfile, sibling_manifest, lockfile): (
        bool,
        Option<&serde_json::Value>,
        &str,
    ),
    extra_projects: &[ExtraProject<'_>],
) -> Decision {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let sibling_dir = workspace_root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::write(workspace_root.join(Lockfile::FILE_NAME), lockfile).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    config.inject_workspace_packages = true;
    if lockfile.contains("'13.0'") {
        config.lockfile_format = pnpm_lockfile::LockfileFormat::V13;
    }
    config.shared_workspace_lockfile = shared_workspace_lockfile;
    fs::create_dir_all(&config.modules_dir).unwrap();
    // A fresh install links the injected `workspace:*` dependency.
    fs::create_dir_all(config.modules_dir.join("pkg-a")).unwrap();
    fs::create_dir_all(sibling_dir.join("node_modules")).unwrap();
    let config = config.leak();

    let settings = crate::install_state::settings::current_settings(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::from([
        (
            workspace_root.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("root".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: true,
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
    let extra_dirs: Vec<_> = extra_projects
        .iter()
        .map(|(dir, ..)| workspace_root.join(dir))
        .collect();
    for ((_, installed, _), dir) in extra_projects.iter().zip(&extra_dirs) {
        fs::create_dir_all(dir).unwrap();
        projects.insert(
            dir.to_string_lossy().into_owned(),
            ProjectEntry {
                name: installed["name"].as_str().map(Into::into),
                version: installed["version"].as_str().map(Into::into),
                has_modules_dir: false,
            },
        );
    }
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);

    let root_manifest = PackageManifest::from_value(
        workspace_root.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "pkg-a": "workspace:*" },
        }),
    );
    let installed_sibling =
        PackageManifest::from_value(sibling_dir.join("package.json"), unchanged_sibling());
    let manifests_of = |pick: for<'p> fn(&'p ExtraProject<'_>) -> &'p serde_json::Value| -> Vec<_> {
        extra_projects
            .iter()
            .zip(&extra_dirs)
            .map(|(project, dir)| {
                PackageManifest::from_value(dir.join("package.json"), pick(project).clone())
            })
            .collect()
    };
    let installed_extra = manifests_of(|(_, installed, _)| installed);
    let current_extra = manifests_of(|(_, _, current)| current);
    let mut installed = vec![
        (workspace_root.to_path_buf(), &root_manifest),
        (sibling_dir.clone(), &installed_sibling),
    ];
    installed.extend(
        extra_dirs
            .iter()
            .cloned()
            .zip(&installed_extra),
    );
    record_installed(workspace_root, config, &installed);
    let sibling_manifest = sibling_manifest.map(|manifest| {
        PackageManifest::from_value(sibling_dir.join("package.json"), manifest.clone())
    });
    let mut project_manifests = vec![(workspace_root.to_path_buf(), &root_manifest)];
    if let Some(sibling_manifest) = &sibling_manifest {
        project_manifests.push((sibling_dir, sibling_manifest));
    }
    project_manifests.extend(extra_dirs.into_iter().zip(&current_extra));
    content_check_decision(&dir, config, true, &project_manifests)
}

fn unchanged_sibling() -> serde_json::Value {
    serde_json::json!({ "name": "pkg-a", "version": "1.0.0" })
}

/// With a shared lockfile the injected project is an importer of the same
/// lockfile, so the per-project manifest comparison already covers a change
/// to its dependencies and the fast path may stand.
#[test]
fn returns_up_to_date_for_an_injected_workspace_dependency_with_a_shared_lockfile() {
    assert_eq!(injected_sibling_decision(true, Some(&unchanged_sibling())), Decision::UpToDate);
}

/// The injected project gained a dependency its `file:` snapshot does not
/// record, so the lockfile has to be re-resolved.
#[test]
fn returns_skipped_when_an_injected_workspace_dependency_gains_a_dependency() {
    let sibling = serde_json::json!({
        "name": "pkg-a",
        "version": "1.0.0",
        "dependencies": { "is-positive": "1.0.0" },
    });
    let decision = injected_sibling_decision(true, Some(&sibling));
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("no longer satisfied")),
        "expected Skipped(manifest no longer satisfied), got {decision:?}",
    );
}

/// With per-project lockfiles the consumer's lockfile records the injected
/// project's dependencies in its own `file:` snapshot, so the full install has
/// to re-check it.
#[test]
fn returns_skipped_for_an_injected_workspace_dependency_without_a_shared_lockfile() {
    assert_skipped_for_local_file_dependency(&injected_sibling_decision(
        false,
        Some(&unchanged_sibling()),
    ));
}

/// The root's manifest says `workspace:*` and nothing about injection, so
/// only its lockfile can tell that `pkg-a` is an injected copy.
#[test]
fn returns_skipped_for_an_injected_workspace_dependency_whose_project_is_not_listed() {
    assert_skipped_for_local_file_dependency(&injected_sibling_decision(false, None));
}

/// An optional peer leaves the injected project's own `importers` entry
/// empty, so only checking the consumer that injects `pkg-a` catches the
/// missing `peerDependencies` on `packages["pkg-a@file:pkg-a"]`.
#[test]
fn returns_skipped_when_an_injected_workspace_dependency_gains_an_optional_peer() {
    let sibling = serde_json::json!({
        "name": "pkg-a",
        "version": "1.0.0",
        "peerDependencies": { "is-positive": "^1.0.0" },
        "peerDependenciesMeta": { "is-positive": { "optional": true } },
    });
    let decision = injected_sibling_decision(true, Some(&sibling));
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("no longer satisfied")),
        "expected Skipped(manifest no longer satisfied), got {decision:?}",
    );
}

fn assert_skipped_for_local_file_dependency(decision: &Decision) {
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "expected Skipped(local file dependency), got {decision:?}",
    );
}

#[test]
fn recorded_injected_inputs_allow_a_dedicated_lockfile_repeat_install() {
    let lockfile = LOCKFILE.replace("'9.0'", "'13.0'")
        .replace("resolution: {directory: pkg-a, type: directory}",
            "resolution: {directory: pkg-a, type: directory}\n    specifiers: {manifestSource: workspace}");
    assert_eq!(
        injected_sibling_decision_with_lockfile(false, Some(&unchanged_sibling()), &lockfile),
        Decision::UpToDate,
    );
    let changed = serde_json::json!({ "name": "pkg-a", "version": "1.0.0", "dependencies": { "is-positive": "1.0.0" } });
    let decision = injected_sibling_decision_with_lockfile(false, Some(&changed), &lockfile);
    assert!(matches!(decision, Decision::Skipped { .. }), "{decision:?}");
}

/// No importer's directory walk reaches a workspace project that only a
/// registry package injects. Its own importer entry still has to agree with
/// its manifest.
#[test]
fn recorded_injected_inputs_do_not_vouch_for_a_changed_nested_project() {
    let lockfile = LOCKFILE.replace("'9.0'", "'13.0'")
        .replace(
            "  pkg-a: {}\n",
            "  pkg-a:\n    dependencies:\n      bar:\n        specifier: 1.0.0\n        version: 1.0.0\n\n  pkg-b: {}\n",
        )
        .replace(
            "  pkg-a@file:pkg-a:\n    resolution: {directory: pkg-a, type: directory}\n\nsnapshots:\n\n  pkg-a@file:pkg-a: {}\n",
            "  bar@1.0.0:\n    resolution: {integrity: sha512-AAAA}\n\n  pkg-a@file:pkg-a:\n    resolution: {directory: pkg-a, type: directory}\n    specifiers: {manifestSource: workspace, dependencies: {bar: 1.0.0}}\n\n  pkg-b@file:pkg-b:\n    resolution: {directory: pkg-b, type: directory}\n    specifiers: {manifestSource: workspace}\n\nsnapshots:\n\n  bar@1.0.0:\n    dependencies:\n      pkg-b: file:pkg-b\n\n  pkg-a@file:pkg-a:\n    dependencies:\n      bar: 1.0.0\n\n  pkg-b@file:pkg-b: {}\n",
        );
    assert!(lockfile.contains("pkg-b: file:pkg-b"), "{lockfile}");
    let sibling = serde_json::json!({
        "name": "pkg-a",
        "version": "1.0.0",
        "dependencies": { "bar": "1.0.0" },
    });
    let pkg_b = serde_json::json!({ "name": "pkg-b", "version": "1.0.0" });
    let changed = serde_json::json!({ "name": "pkg-b", "version": "1.0.0", "dependencies": { "is-positive": "1.0.0" } });
    let decision = |current: &serde_json::Value| {
        injected_sibling_decision_with_projects(
            (false, Some(&sibling), &lockfile),
            &[("pkg-b", pkg_b.clone(), current.clone())],
        )
    };
    assert_eq!(decision(&pkg_b), Decision::UpToDate);
    assert_skipped_for_local_file_dependency(&decision(&changed));
}
