use std::{collections::HashSet, fs, path::Path};

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use super::apply_release_plan;
use crate::{
    changelog::prepend_changelog_section,
    intents::{IntentBumpType, read_change_intents, write_change_intent},
    ledger::read_ledger,
    pending::read_pending_changelog,
    plan::{
        AssembleReleasePlanOptions, DependencyField, ManifestDependency, WorkspaceProject,
        assemble_release_plan,
    },
    settings::{ChangelogSettings, ChangelogStorage, VersioningSettings},
};

/// The existing changelog assertions predate the `registry`-storage default,
/// so they opt back into committed CHANGELOG.md files.
fn repository() -> VersioningSettings {
    VersioningSettings {
        changelog: Some(ChangelogSettings {
            format: None,
            storage: Some(ChangelogStorage::Repository),
        }),
        ..VersioningSettings::default()
    }
}

struct Workspace {
    dir: tempfile::TempDir,
    projects: Vec<WorkspaceProject>,
}

type FixturePkg<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

fn make_workspace(pkgs: &[FixturePkg<'_>]) -> Workspace {
    let dir = tempfile::tempdir().expect("create temp workspace");
    let projects = pkgs
        .iter()
        .map(|(name, version, deps)| {
            let root_dir = dir
                .path()
                .join(name.replace(['@', '/'], "_"));
            fs::create_dir_all(&root_dir).expect("create package dir");
            let dependencies: serde_json::Map<String, serde_json::Value> = deps
                .iter()
                .map(|(alias, spec)| ((*alias).to_string(), serde_json::json!(spec)))
                .collect();
            let manifest = serde_json::json!({
                "name": name,
                "version": version,
                "dependencies": dependencies,
            });
            fs::write(
                root_dir.join("package.json"),
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
                ),
            )
            .expect("write package.json");
            WorkspaceProject {
                manifest_path: pnpm_package_manifest::project_manifest_path(&root_dir, None),
                root_dir,
                name: Some((*name).to_string()),
                version: Some((*version).to_string()),
                prod_dependencies: deps
                    .iter()
                    .map(|(alias, spec)| ManifestDependency {
                        field: DependencyField::Dependencies,
                        alias: (*alias).to_string(),
                        spec: (*spec).to_string(),
                    })
                    .collect(),
            }
        })
        .collect();
    Workspace { dir, projects }
}

fn manifest_version(root_dir: &Path) -> String {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root_dir.join("package.json")).expect("read"))
            .expect("parse");
    manifest["version"]
        .as_str()
        .expect("version is a string")
        .to_string()
}

#[test]
fn apply_bumps_manifests_writes_changelogs_records_the_ledger_and_deletes_consumed_intents() {
    let workspace =
        make_workspace(&[("lib", "1.0.0", &[]), ("cli", "2.0.0", &[("lib", "workspace:*")])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "Added a feature.")
        .expect("intent writes");
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &intents,
        &ledger,
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");

    let applied = apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &intents,
        Some(&repository()),
        &HashSet::new(),
    )
    .expect("plan applies");
    let mut applied_names: Vec<String> = applied
        .iter()
        .map(|release| format!("{}@{}", release.name, release.new_version))
        .collect();
    applied_names.sort();
    assert_eq!(applied_names, ["cli@2.0.1", "lib@1.1.0"]);

    let lib_dir = &workspace.projects[0].root_dir;
    let cli_dir = &workspace.projects[1].root_dir;
    assert_eq!(manifest_version(lib_dir), "1.1.0");
    assert_eq!(manifest_version(cli_dir), "2.0.1");

    let lib_changelog = fs::read_to_string(lib_dir.join("CHANGELOG.md")).expect("read changelog");
    assert!(lib_changelog.contains("# lib"));
    assert!(lib_changelog.contains("## 1.1.0"));
    assert!(lib_changelog.contains("### Minor Changes"));
    assert!(lib_changelog.contains("- Added a feature."));

    let cli_changelog = fs::read_to_string(cli_dir.join("CHANGELOG.md")).expect("read changelog");
    assert!(cli_changelog.contains("## 2.0.1"));
    assert!(cli_changelog.contains("- Updated dependencies:"));
    assert!(cli_changelog.contains("  - lib@1.1.0"));

    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let keys: Vec<&str> = ledger
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["lib@1.1.0"]);

    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
}

#[test]
fn intent_files_consumed_only_by_lane_prereleases_survive_until_graduation() {
    let workspace = make_workspace(&[("cli", "2.0.0", &[])]);
    let releases = IndexMap::from([("cli".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "Added a feature.")
        .expect("intent writes");
    let versioning = VersioningSettings {
        lanes: IndexMap::from([("cli".to_string(), "alpha".to_string())]),
        ..repository()
    };

    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let prerelease_plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &intents,
        &ledger,
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert_eq!(prerelease_plan.releases[0].version.next, "2.1.0-alpha.0");
    apply_release_plan(
        &prerelease_plan,
        workspace.dir.path(),
        &workspace.projects,
        &intents,
        Some(&versioning),
        &HashSet::new(),
    )
    .expect("plan applies");

    // The prose is still needed for the stable changelog at graduation.
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    assert_eq!(intents.len(), 1);

    // Return to the main lane: the accumulated stable version releases and
    // the intent is garbage-collected.
    let graduated_projects = [WorkspaceProject {
        root_dir: workspace.projects[0].root_dir.clone(),
        manifest_path: workspace.projects[0].root_dir.clone().join("package.json"),
        name: Some("cli".to_string()),
        version: Some("2.1.0-alpha.0".to_string()),
        prod_dependencies: Vec::new(),
    }];
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let graduation_plan = assemble_release_plan(
        &graduated_projects,
        workspace.dir.path(),
        &intents,
        &ledger,
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert_eq!(graduation_plan.releases[0].version.next, "2.1.0");
    apply_release_plan(
        &graduation_plan,
        workspace.dir.path(),
        &graduated_projects,
        &intents,
        Some(&repository()),
        &HashSet::new(),
    )
    .expect("plan applies");

    let changelog =
        fs::read_to_string(workspace.projects[0].root_dir.join("CHANGELOG.md")).expect("read");
    assert!(changelog.contains("## 2.1.0-alpha.0"));
    assert!(changelog.contains("## 2.1.0"));
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
}

#[test]
fn a_none_only_intent_is_garbage_collected_by_a_run_with_an_empty_plan() {
    let workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::None)]);
    write_change_intent(workspace.dir.path(), &releases, "refactor, no release needed")
        .expect("intent writes");
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert!(plan.releases.is_empty());
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
}

#[test]
fn registry_storage_parks_the_section_and_defers_intent_gc() {
    let workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "Added a feature.")
        .expect("intent writes");
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");

    // No versioning => registry storage. Nothing confirmed published, so the
    // intent (the only copy of the prose) survives.
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");

    assert!(!workspace.projects[0].root_dir.join("CHANGELOG.md").exists());
    let section = read_pending_changelog(workspace.dir.path(), "lib", "1.1.0")
        .expect("pending read")
        .expect("section is parked");
    assert!(section.contains("## 1.1.0"), "unexpected: {section}");
    assert!(section.contains("- Added a feature."), "unexpected: {section}");
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 1);
}

#[test]
fn registry_storage_collects_an_intent_and_its_section_once_confirmed() {
    let workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "Added a feature.")
        .expect("intent writes");
    let first_intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &first_intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &first_intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");

    // A later run: nothing new to release, but the previous release is now
    // confirmed published, so its intent and parked section are collected.
    let released = [WorkspaceProject {
        root_dir: workspace.projects[0].root_dir.clone(),
        manifest_path: workspace.projects[0].root_dir.clone().join("package.json"),
        name: Some("lib".to_string()),
        version: Some("1.1.0".to_string()),
        prod_dependencies: Vec::new(),
    }];
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let empty_plan = assemble_release_plan(
        &released,
        workspace.dir.path(),
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert!(empty_plan.releases.is_empty());
    let confirmed = HashSet::from(["lib@1.1.0".to_string()]);
    apply_release_plan(&empty_plan, workspace.dir.path(), &released, &intents, None, &confirmed)
        .expect("plan applies");

    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
    assert!(
        read_pending_changelog(workspace.dir.path(), "lib", "1.1.0")
            .expect("pending read")
            .is_none(),
    );
}

#[test]
fn registry_storage_collects_a_dependency_only_release_section_when_confirmed() {
    let workspace =
        make_workspace(&[("lib", "1.0.0", &[]), ("cli", "2.0.0", &[("lib", "workspace:*")])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "A feature.").expect("intent writes");
    let first_intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &first_intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &first_intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");

    // cli was bumped only because lib changed: it has a parked section but,
    // carrying no consumed intents, no ledger entry.
    assert!(
        read_pending_changelog(workspace.dir.path(), "cli", "2.0.1")
            .expect("pending read")
            .is_some(),
    );
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    assert_eq!(
        ledger
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["lib@1.1.0"],
    );

    // A later run confirms both published versions (the CLI derives this set
    // from the parked files; here it is passed directly).
    let released = [
        WorkspaceProject {
            root_dir: workspace.projects[0].root_dir.clone(),
            manifest_path: workspace.projects[0].root_dir.clone().join("package.json"),
            name: Some("lib".to_string()),
            version: Some("1.1.0".to_string()),
            prod_dependencies: Vec::new(),
        },
        WorkspaceProject {
            root_dir: workspace.projects[1].root_dir.clone(),
            manifest_path: workspace.projects[1].root_dir.clone().join("package.json"),
            name: Some("cli".to_string()),
            version: Some("2.0.1".to_string()),
            prod_dependencies: vec![ManifestDependency {
                field: DependencyField::Dependencies,
                alias: "lib".to_string(),
                spec: "workspace:*".to_string(),
            }],
        },
    ];
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let empty_plan = assemble_release_plan(
        &released,
        workspace.dir.path(),
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    let confirmed = HashSet::from(["lib@1.1.0".to_string(), "cli@2.0.1".to_string()]);
    apply_release_plan(&empty_plan, workspace.dir.path(), &released, &intents, None, &confirmed)
        .expect("plan applies");

    // The dependency-only section is collected even though it has no ledger entry.
    assert!(
        read_pending_changelog(workspace.dir.path(), "cli", "2.0.1")
            .expect("pending read")
            .is_none(),
    );
    assert!(
        read_pending_changelog(workspace.dir.path(), "lib", "1.1.0")
            .expect("pending read")
            .is_none(),
    );
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
}

#[test]
fn registry_storage_keeps_an_intent_whose_release_is_not_confirmed() {
    let workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(workspace.dir.path(), &releases, "Added a feature.")
        .expect("intent writes");
    let first_intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &first_intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &first_intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");

    let released = [WorkspaceProject {
        root_dir: workspace.projects[0].root_dir.clone(),
        manifest_path: workspace.projects[0].root_dir.clone().join("package.json"),
        name: Some("lib".to_string()),
        version: Some("1.1.0".to_string()),
        prod_dependencies: Vec::new(),
    }];
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let empty_plan = assemble_release_plan(
        &released,
        workspace.dir.path(),
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    // Nothing confirmed published: the intent and its section stay put.
    apply_release_plan(
        &empty_plan,
        workspace.dir.path(),
        &released,
        &intents,
        None,
        &HashSet::new(),
    )
    .expect("plan applies");

    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 1);
    assert!(
        read_pending_changelog(workspace.dir.path(), "lib", "1.1.0")
            .expect("pending read")
            .is_some(),
    );
}

#[test]
fn prepend_keeps_the_title_above_the_new_section_even_without_a_trailing_newline() {
    let dir = tempfile::tempdir().expect("create temp dir");
    fs::write(dir.path().join("CHANGELOG.md"), "# lib").expect("write changelog");
    prepend_changelog_section(dir.path(), "lib", "## 1.0.1\n\n### Patch Changes\n\n- A fix.\n")
        .expect("changelog updates");
    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).expect("read changelog");
    assert!(changelog.starts_with("# lib\n\n## 1.0.1"), "unexpected changelog: {changelog}");
}

#[test]
fn apply_updates_the_selected_json5_manifest_without_creating_json() {
    let mut workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let root_dir = workspace.projects[0].root_dir.clone();
    fs::remove_file(root_dir.join("package.json")).expect("remove the fixture JSON manifest");
    let manifest_path = root_dir.join("package.json5");
    fs::write(
        &manifest_path,
        "// release metadata\n{ name: 'lib', version: '1.0.0', /* version note */ }\n",
    )
    .expect("write the JSON5 manifest");
    let yaml = "name: alternate\nversion: 9.0.0\n";
    fs::write(root_dir.join("package.yaml"), yaml).expect("write the alternate YAML manifest");
    // The fixture swaps the manifest files after the projects were built;
    // discovery resolves the path once the files exist, so mirror that here.
    workspace.projects[0].manifest_path =
        pnpm_package_manifest::project_manifest_path(&root_dir, None);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Patch)]);
    write_change_intent(workspace.dir.path(), &releases, "Fixed a bug.").expect("intent writes");
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let plan = assemble_release_plan(
        &workspace.projects,
        workspace.dir.path(),
        &intents,
        &ledger,
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");

    let applied = apply_release_plan(
        &plan,
        workspace.dir.path(),
        &workspace.projects,
        &intents,
        Some(&repository()),
        &HashSet::new(),
    )
    .expect("plan applies");

    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].new_version, "1.0.1");
    let manifest = pnpm_package_manifest::PackageManifest::from_path(manifest_path.clone())
        .expect("read the updated manifest");
    assert_eq!(manifest.value()["version"], "1.0.1");
    let written = fs::read_to_string(manifest_path).expect("read the JSON5 source");
    eprintln!("WRITTEN:\n{written}");
    assert!(written.contains("// release metadata"));
    assert!(written.contains("/* version note */"));
    assert!(!root_dir.join("package.json").exists());
    assert_eq!(fs::read_to_string(root_dir.join("package.yaml")).expect("read YAML"), yaml);
}

#[test]
fn apply_bumps_package_yaml_manifest() {
    let dir = tempfile::tempdir().expect("create temp workspace");
    let root_dir = dir.path().join("lib");
    fs::create_dir_all(&root_dir).expect("create package dir");
    fs::write(root_dir.join("package.yaml"), "name: lib\nversion: 1.0.0\n")
        .expect("write package.yaml");
    let projects = vec![WorkspaceProject {
        root_dir: root_dir.clone(),
        manifest_path: pnpm_package_manifest::project_manifest_path(&root_dir, None),
        name: Some("lib".to_string()),
        version: Some("1.0.0".to_string()),
        prod_dependencies: Vec::new(),
    }];
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Minor)]);
    write_change_intent(dir.path(), &releases, "Added a feature.").expect("intent writes");
    let intents = read_change_intents(dir.path()).expect("intents read");
    let ledger = read_ledger(dir.path()).expect("ledger reads");
    let plan = assemble_release_plan(
        &projects,
        dir.path(),
        &intents,
        &ledger,
        Some(&repository()),
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    apply_release_plan(
        &plan,
        dir.path(),
        &projects,
        &intents,
        Some(&repository()),
        &HashSet::new(),
    )
    .expect("plan applies");

    let yaml_content =
        fs::read_to_string(root_dir.join("package.yaml")).expect("read package.yaml");
    assert!(yaml_content.contains("version: 1.1.0"), "unexpected yaml: {yaml_content}");
}
