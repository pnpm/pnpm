use std::{collections::HashSet, fs, path::Path};

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use crate::{
    apply::{ReleaseNotesOptions, apply_release_plan},
    error::VersioningError,
    intents::{IntentBumpType, read_change_intents, write_change_intent},
    ledger::read_ledger,
    parked_publication::{ParkedPublication, read_parked_publications},
    pending::read_pending_changelog,
    plan::{AssembleReleasePlanOptions, WorkspaceProject, assemble_release_plan, to_project_dir},
    settings::VersioningSettings,
};

fn make_workspace(dir: &Path, names: &[&str]) -> Vec<WorkspaceProject> {
    names
        .iter()
        .map(|name| {
            let root_dir = dir.join(name);
            fs::create_dir_all(&root_dir).expect("create package dir");
            let manifest = serde_json::json!({ "name": name, "version": "1.0.0" });
            fs::write(root_dir.join("package.json"), manifest.to_string())
                .expect("write package.json");
            WorkspaceProject {
                root_dir,
                private: false,
                name: Some((*name).to_string()),
                version: Some("1.0.0".to_string()),
                prod_dependencies: Vec::new(),
            }
        })
        .collect()
}

fn intent(dir: &Path, releases: &[(&str, IntentBumpType)], summary: &str) {
    let releases: IndexMap<String, IntentBumpType> = releases
        .iter()
        .map(|(name, bump)| ((*name).to_string(), *bump))
        .collect();
    write_change_intent(dir, &releases, summary).expect("intent writes");
}

/// Plans and applies the pending intents of a workspace of `a`, `b`, and
/// `c`, where `a` and `b` are bound for a registry that stores notes.
fn apply_with_notes(dir: &Path, versioning: &VersioningSettings) -> Vec<WorkspaceProject> {
    apply_with_summary(dir, versioning, 2, None)
}

/// [`apply_with_notes`] with the first `bound` projects bound for the
/// registry and `summary` given to `pnpm version -r --release-notes`.
fn apply_with_summary(
    dir: &Path,
    versioning: &VersioningSettings,
    bound: usize,
    summary: Option<&str>,
) -> Vec<WorkspaceProject> {
    let projects = make_workspace(dir, &["a", "b", "c"]);
    let registries = vec![REGISTRY; bound];
    try_apply(dir, &projects, versioning, &registries, summary).expect("plan applies");
    projects
}

const REGISTRY: &str = "https://pnpr.example/";

/// Plans and applies the pending intents of `projects`, the first of which
/// are bound for the notes registries in `registries`, in order.
fn try_apply(
    dir: &Path,
    projects: &[WorkspaceProject],
    versioning: &VersioningSettings,
    registries: &[&str],
    summary: Option<&str>,
) -> Result<(), VersioningError> {
    let intents = read_change_intents(dir).expect("intents read");
    let ledger = read_ledger(dir).expect("ledger reads");
    let plan = assemble_release_plan(
        projects,
        dir,
        &intents,
        &ledger,
        Some(versioning),
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles");
    let notes = ReleaseNotesOptions {
        registries: projects
            .iter()
            .zip(registries)
            .map(|(project, registry)| {
                (to_project_dir(dir, &project.root_dir), (*registry).to_owned())
            })
            .collect(),
        summary: summary.map(str::to_owned),
    };
    apply_release_plan(&plan, dir, projects, &intents, Some(versioning), &HashSet::new(), &notes)
        .map(drop)
}

fn parked(dir: &Path, name: &str, version: &str) -> String {
    read_pending_changelog(dir, name, version).expect("pending read").expect("section is parked")
}

#[test]
fn splits_intents_between_version_notes_and_the_publication_note() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    intent(dir, &[("a", IntentBumpType::Minor), ("b", IntentBumpType::Patch)], "Both a and b.");
    intent(dir, &[("a", IntentBumpType::Patch), ("c", IntentBumpType::Patch)], "A and outside c.");
    intent(
        dir,
        &[("a", IntentBumpType::Patch), ("b", IntentBumpType::Patch), ("c", IntentBumpType::Patch)],
        "Everyone.",
    );
    let versioning =
        VersioningSettings { fixed: vec![vec!["a".into(), "b".into()]], ..Default::default() };
    apply_with_notes(dir, &versioning);

    let note_a = parked(dir, "a", "1.1.0");
    assert!(note_a.starts_with("### Minor Changes\n\n- Only a."), "{note_a}");
    assert!(note_a.contains("- A and outside c.") && note_a.contains("- Everyone."), "{note_a}");
    assert!(!note_a.contains("Both a and b."), "{note_a}");
    let note_b = parked(dir, "b", "1.1.0");
    assert_eq!(note_b, "### Patch Changes\n\n- Everyone.\n");
    let section_c = parked(dir, "c", "1.0.1");
    assert!(section_c.starts_with("## 1.0.1\n"), "c keeps its full section: {section_c}");

    let publications = read_parked_publications(dir).expect("publications read");
    assert_eq!(
        publications,
        vec![ParkedPublication {
            name: Some("v1.1.0".to_string()),
            members: vec!["a@1.1.0".to_string(), "b@1.1.0".to_string()],
            notes: "### Minor Changes\n\n- Both a and b.\n\n  Packages: `a@1.1.0`, `b@1.1.0`\n\n### Patch Changes\n\n- Everyone.\n\n  Packages: `a@1.1.0`, `b@1.1.0`\n".to_string(),
        }],
    );
}

#[test]
fn a_publication_without_shared_intents_or_name_is_not_parked() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    apply_with_notes(dir, &VersioningSettings::default());

    assert_eq!(parked(dir, "a", "1.1.0"), "### Minor Changes\n\n- Only a.\n");
    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
}

#[test]
fn a_confirmed_publication_is_collected() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor), ("b", IntentBumpType::Minor)], "Both.");
    let versioning = VersioningSettings::default();
    let projects = apply_with_notes(dir, &versioning);
    assert_eq!(read_parked_publications(dir).expect("publications read").len(), 1);

    let intents = read_change_intents(dir).expect("intents read");
    let confirmed: HashSet<String> = ["a@1.1.0".to_string(), "b@1.1.0".to_string()].into();
    let empty_plan = crate::plan::ReleasePlan::default();
    let no_notes = ReleaseNotesOptions::default();
    apply_release_plan(&empty_plan, dir, &projects, &intents, None, &confirmed, &no_notes)
        .expect("collection applies");

    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
    assert_eq!(read_change_intents(dir).expect("intents read").len(), 0);
}

#[test]
fn a_summary_goes_above_the_composed_publication_note() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    intent(dir, &[("a", IntentBumpType::Minor), ("b", IntentBumpType::Minor)], "Both.");
    apply_with_summary(dir, &VersioningSettings::default(), 2, Some("## Highlights\n\n"));

    assert_eq!(parked(dir, "a", "1.1.0"), "### Minor Changes\n\n- Only a.\n");
    let publications = read_parked_publications(dir).expect("publications read");
    assert_eq!(
        publications[0].notes,
        "## Highlights\n\n### Minor Changes\n\n- Both.\n\n  Packages: `a@1.1.0`, `b@1.1.0`\n",
    );
}

#[test]
fn a_summary_alone_makes_the_publication_note() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    intent(dir, &[("b", IntentBumpType::Patch)], "Only b.");
    apply_with_summary(dir, &VersioningSettings::default(), 2, Some("Highlights."));

    assert_eq!(parked(dir, "a", "1.1.0"), "### Minor Changes\n\n- Only a.\n");
    let publications = read_parked_publications(dir).expect("publications read");
    assert_eq!(
        publications,
        vec![ParkedPublication {
            name: None,
            members: vec!["a@1.1.0".to_string(), "b@1.0.1".to_string()],
            notes: "Highlights.\n".to_string(),
        }],
    );
}

#[test]
fn a_summary_goes_above_the_version_note_of_a_single_package_release() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    apply_with_summary(dir, &VersioningSettings::default(), 1, Some("Highlights."));

    assert_eq!(parked(dir, "a", "1.1.0"), "Highlights.\n\n### Minor Changes\n\n- Only a.\n");
    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
}

#[test]
fn a_publication_confirmed_over_several_runs_is_collected() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor), ("b", IntentBumpType::Minor)], "Both.");
    let projects = apply_with_notes(dir, &VersioningSettings::default());
    let empty_plan = crate::plan::ReleasePlan::default();
    let no_notes = ReleaseNotesOptions::default();
    let collect = |confirmed: &[&str]| {
        let intents = read_change_intents(dir).expect("intents read");
        let confirmed: HashSet<String> = confirmed
            .iter()
            .map(ToString::to_string)
            .collect();
        apply_release_plan(&empty_plan, dir, &projects, &intents, None, &confirmed, &no_notes)
            .expect("collection applies");
    };

    collect(&["a@1.1.0"]);
    assert_eq!(read_parked_publications(dir).expect("publications read").len(), 1);
    collect(&["b@1.1.0"]);

    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
}

#[test]
fn a_publication_across_several_registries_is_refused_before_versioning() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor), ("b", IntentBumpType::Minor)], "Both.");
    let projects = make_workspace(dir, &["a", "b", "c"]);

    let err = try_apply(
        dir,
        &projects,
        &VersioningSettings::default(),
        &[REGISTRY, "https://other-pnpr.example/"],
        None,
    )
    .expect_err("a publication cannot reach two registries");

    assert!(matches!(err, VersioningError::PublicationSeveralRegistries { .. }), "{err}");
    let manifest = fs::read_to_string(dir.join("a/package.json")).expect("read manifest");
    assert!(manifest.contains(r#""version":"1.0.0""#), "{manifest}");
    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
}

#[test]
fn releases_without_a_publication_may_go_to_several_registries() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let dir = workspace.path();
    intent(dir, &[("a", IntentBumpType::Minor)], "Only a.");
    intent(dir, &[("b", IntentBumpType::Patch)], "Only b.");
    let projects = make_workspace(dir, &["a", "b", "c"]);

    try_apply(
        dir,
        &projects,
        &VersioningSettings::default(),
        &[REGISTRY, "https://other-pnpr.example/"],
        None,
    )
    .expect("plan applies");

    assert_eq!(parked(dir, "a", "1.1.0"), "### Minor Changes\n\n- Only a.\n");
    assert_eq!(parked(dir, "b", "1.0.1"), "### Patch Changes\n\n- Only b.\n");
    assert_eq!(read_parked_publications(dir).expect("publications read"), Vec::new());
}
