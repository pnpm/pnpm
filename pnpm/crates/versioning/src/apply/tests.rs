use std::{fs, path::Path};

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use super::{ApplyReleasePlanOptions, apply_release_plan};
use crate::{
    changelog::prepend_changelog_section,
    intents::{IntentBumpType, read_change_intents, write_change_intent},
    ledger::read_ledger,
    plan::{
        AssembleReleasePlanOptions, DependencyField, ManifestDependency, WorkspaceProject,
        assemble_release_plan,
    },
    settings::VersioningSettings,
};

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
            let root_dir = dir.path().join(name.replace(['@', '/'], "_"));
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
    manifest["version"].as_str().expect("version is a string").to_string()
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
        &intents,
        &ledger,
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");

    let applied = apply_release_plan(
        &plan,
        workspace.dir.path(),
        &intents,
        None,
        ApplyReleasePlanOptions::default(),
    )
    .expect("plan applies");
    let mut applied_names: Vec<String> =
        applied.iter().map(|release| format!("{}@{}", release.name, release.new_version)).collect();
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
    let keys: Vec<&str> = ledger.keys().map(String::as_str).collect();
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
        ..VersioningSettings::default()
    };

    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let prerelease_plan = assemble_release_plan(
        &workspace.projects,
        &intents,
        &ledger,
        Some(&versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert_eq!(prerelease_plan.releases[0].new_version, "2.1.0-alpha.0");
    apply_release_plan(
        &prerelease_plan,
        workspace.dir.path(),
        &intents,
        Some(&versioning),
        ApplyReleasePlanOptions::default(),
    )
    .expect("plan applies");

    // The prose is still needed for the stable changelog at graduation.
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    assert_eq!(intents.len(), 1);

    // Return to the main lane: the accumulated stable version releases and
    // the intent is garbage-collected.
    let graduated_projects = [WorkspaceProject {
        root_dir: workspace.projects[0].root_dir.clone(),
        name: Some("cli".to_string()),
        version: Some("2.1.0-alpha.0".to_string()),
        prod_dependencies: Vec::new(),
    }];
    let ledger = read_ledger(workspace.dir.path()).expect("ledger reads");
    let graduation_plan = assemble_release_plan(
        &graduated_projects,
        &intents,
        &ledger,
        None,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    assert_eq!(graduation_plan.releases[0].new_version, "2.1.0");
    apply_release_plan(
        &graduation_plan,
        workspace.dir.path(),
        &intents,
        None,
        ApplyReleasePlanOptions::default(),
    )
    .expect("plan applies");

    let changelog =
        fs::read_to_string(workspace.projects[0].root_dir.join("CHANGELOG.md")).expect("read");
    assert!(changelog.contains("## 2.1.0-alpha.0"));
    assert!(changelog.contains("## 2.1.0"));
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 0);
}

#[test]
fn snapshot_releases_rewrite_manifests_without_consuming_intents_or_writing_changelogs() {
    let workspace = make_workspace(&[("lib", "1.0.0", &[])]);
    let releases = IndexMap::from([("lib".to_string(), IntentBumpType::Patch)]);
    write_change_intent(workspace.dir.path(), &releases, "A fix.").expect("intent writes");
    let intents = read_change_intents(workspace.dir.path()).expect("intents read");
    let opts = AssembleReleasePlanOptions {
        snapshot_suffix: Some("preview-20260712000000".to_string()),
        ..AssembleReleasePlanOptions::default()
    };
    let plan = assemble_release_plan(
        &workspace.projects,
        &intents,
        &read_ledger(workspace.dir.path()).expect("ledger reads"),
        None,
        &opts,
    )
    .expect("plan assembles");
    apply_release_plan(
        &plan,
        workspace.dir.path(),
        &intents,
        None,
        ApplyReleasePlanOptions { snapshot: true },
    )
    .expect("plan applies");

    assert_eq!(manifest_version(&workspace.projects[0].root_dir), "0.0.0-preview-20260712000000");
    assert_eq!(read_change_intents(workspace.dir.path()).expect("intents read").len(), 1);
    assert!(read_ledger(workspace.dir.path()).expect("ledger reads").is_empty());
    assert!(!workspace.projects[0].root_dir.join("CHANGELOG.md").exists());
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
