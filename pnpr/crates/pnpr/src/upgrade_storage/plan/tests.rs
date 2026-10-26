use super::{Step, plan_with_registries};
use pnpr_package_name::Ecosystem;
use std::collections::BTreeSet;

fn plan(
    paths: &[String],
    orgs: &BTreeSet<(String, Ecosystem)>,
) -> Result<Vec<(String, Step)>, String> {
    plan_with_registries(paths, orgs, &BTreeSet::new())
}

fn steps(paths: &[&str], orgs: &[(&str, Ecosystem)]) -> Vec<(String, Step)> {
    let paths: Vec<String> = paths
        .iter()
        .map(ToString::to_string)
        .collect();
    let orgs: BTreeSet<(String, Ecosystem)> = orgs
        .iter()
        .map(|(org, ecosystem)| ((*org).to_string(), *ecosystem))
        .collect();
    plan(&paths, &orgs).unwrap()
}

fn moved(to: &str, document: bool) -> Step {
    Step::Move { to: to.to_string(), document }
}

#[test]
fn packages_without_an_org_move_into_the_default_namespace() {
    assert_eq!(
        steps(
            &[
                "left-pad/package.json",
                "left-pad/left-pad-1.0.0.tgz",
                "@scope/App/package.json",
                "@scope/App/App-1.0.0.tgz",
            ],
            &[],
        ),
        [
            ("left-pad/package.json", moved("default/npm/packages/@/left-pad/document.json", true)),
            (
                "left-pad/left-pad-1.0.0.tgz",
                moved("default/npm/packages/@/left-pad/blobs/1.0.0.tgz", false)
            ),
            (
                "@scope/App/package.json",
                moved("default/npm/packages/@scope/!41pp/document.json", true)
            ),
            (
                "@scope/App/App-1.0.0.tgz",
                moved("default/npm/packages/@scope/!41pp/blobs/1.0.0.tgz", false),
            ),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn nested_image_repositories_split_into_marked_directories() {
    assert_eq!(
        steps(
            &[
                "acme/app/package.json",
                "acme/app/sha256-aa",
                "acme/app/tool/package.json",
                "acme/app/tool/sha256-bb",
            ],
            &[("", Ecosystem::Oci)],
        ),
        [
            ("acme/app/package.json", moved("default/oci/packages/@acme/app/document.json", true)),
            ("acme/app/sha256-aa", moved("default/oci/packages/@acme/app/blobs/sha256-aa", false)),
            (
                "acme/app/tool/package.json",
                moved("default/oci/packages/@acme/@app/tool/document.json", true)
            ),
            (
                "acme/app/tool/sha256-bb",
                moved("default/oci/packages/@acme/@app/tool/blobs/sha256-bb", false)
            ),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn an_org_directory_becomes_its_namespace() {
    assert_eq!(
        steps(
            &[
                "npm~internal/tool/package.json",
                "npm~internal/tool/tool-1.0.0.tgz",
                "npm~internal/.revisions/sha512/abc/index.json",
                "npm~internal/.tombstones/v0/@scope/gone.json",
                "npm~internal/.package-index/tool/.present",
                "cargo~internal/serde/package.json",
                "cargo~internal/serde/serde-1.0.0.crate",
            ],
            &[("internal", Ecosystem::Npm), ("internal", Ecosystem::Cargo)],
        ),
        [
            (
                "npm~internal/tool/package.json",
                moved("orgs/internal/npm/packages/@/tool/document.json", true)
            ),
            (
                "npm~internal/tool/tool-1.0.0.tgz",
                moved("orgs/internal/npm/packages/@/tool/blobs/1.0.0.tgz", false),
            ),
            ("npm~internal/.revisions/sha512/abc/index.json", Step::Remove,),
            (
                "npm~internal/.tombstones/v0/@scope/gone.json",
                moved("orgs/internal/npm/.tombstones/@scope/gone.json", false),
            ),
            ("npm~internal/.package-index/tool/.present", Step::Remove),
            (
                "cargo~internal/serde/package.json",
                moved("orgs/internal/cargo/packages/@/serde/document.json", true),
            ),
            (
                "cargo~internal/serde/serde-1.0.0.crate",
                moved("orgs/internal/cargo/packages/@/serde/blobs/1.0.0.crate", false),
            ),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn shared_records_and_root_files_stay() {
    assert_eq!(
        steps(
            &[
                "htpasswd",
                ".staged/abc.json",
                ".team-rosters/npm/main.json",
                ".pnpr-cache/~public/x/package.json",
            ],
            &[],
        ),
        [],
    );
}

#[test]
fn legacy_root_records_move_to_current_namespaces() {
    assert_eq!(
        steps(
            &[
                ".team-rosters/v0/main.json",
                ".team-rosters/v0/crates.json",
                ".team-rosters/v0/pypi/main.json",
                ".rule-overrides/v0/main.json",
                ".rule-overrides/v0/crates.json",
                ".scim-users/v0/directory.json",
                ".pipeline-runs/v1/demo/100.json",
                ".pnpr-artifacts/v0/sha256/ab/cd/ef",
                "default/npm/.tombstones/v0/!40scope!2fgone.json",
            ],
            &[("crates", Ecosystem::Cargo)],
        ),
        [
            (".team-rosters/v0/main.json", moved(".team-rosters/npm/main.json", false)),
            (".team-rosters/v0/crates.json", moved(".team-rosters/cargo/crates.json", false)),
            (".team-rosters/v0/pypi/main.json", moved(".team-rosters/pypi/main.json", false)),
            (".rule-overrides/v0/main.json", moved(".rule-overrides/npm/main.json", false)),
            (".rule-overrides/v0/crates.json", moved(".rule-overrides/cargo/crates.json", false)),
            (".scim-users/v0/directory.json", moved(".scim-users/directory.json", false)),
            (".pipeline-runs/v1/demo/100.json", moved(".pipeline-runs/demo/100.json", false)),
            (".pnpr-artifacts/v0/sha256/ab/cd/ef", moved(".artifacts/sha256/ab/cd/ef", false)),
            (
                "default/npm/.tombstones/v0/!40scope!2fgone.json",
                moved("default/npm/.tombstones/@scope/gone.json", false),
            ),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn a_pending_publish_journal_is_refused() {
    for journal in [".pnpr-journal/0001/manifest.json", ".journal/0001/manifest.json"] {
        let error = plan(&[journal.to_string()], &BTreeSet::new()).unwrap_err();
        assert!(error.contains("pending publish journal"), "{error}");
    }
}

#[test]
fn conflicting_or_ambiguous_legacy_record_moves_are_refused() {
    let duplicate = vec![
        ".team-rosters/v0/main.json".to_string(),
        ".team-rosters/v0/npm/main.json".to_string(),
    ];
    let error = plan(&duplicate, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("multiple records map to"), "{error}");

    let existing_tombstone = vec![
        "default/npm/.tombstones/v0/!40scope!2fgone.json".to_string(),
        "default/npm/.tombstones/@scope/gone.json".to_string(),
    ];
    let error = plan(&existing_tombstone, &BTreeSet::new()).unwrap_err();
    assert!(error.contains("multiple records map to"), "{error}");

    let ambiguous_orgs = BTreeSet::from([
        ("shared".to_string(), Ecosystem::Npm),
        ("shared".to_string(), Ecosystem::Pypi),
    ]);
    let error = plan(&[".team-rosters/v0/shared.json".to_string()], &ambiguous_orgs).unwrap_err();
    assert!(error.contains("matches registries in more than one ecosystem"), "{error}");
}

#[test]
fn the_default_namespace_records_move_with_it() {
    assert_eq!(
        steps(
            &[
                ".revisions/sha512/abc/index.json",
                ".tombstones/v0/left-pad.json",
                ".pnpr-uploads/x"
            ],
            &[]
        ),
        [
            (".revisions/sha512/abc/index.json", Step::Remove,),
            (
                ".tombstones/v0/left-pad.json",
                moved("default/npm/.tombstones/@/left-pad.json", false)
            ),
            (".pnpr-uploads/x", Step::Remove),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn tar_integrities_keep_their_namespace() {
    assert_eq!(
        steps(&[".tar-integrity/@scope/pkg/package.json"], &[]),
        [(
            ".tar-integrity/@scope/pkg/package.json".to_string(),
            moved(".tar-integrity/packages/@scope/pkg/document.json", true),
        )],
    );
}

#[test]
fn files_outside_any_package_are_orphans() {
    assert_eq!(
        steps(
            &["stray/file.tgz", "left-pad/package.json", "left-pad/.left-pad-1.0.0.tgz.tmp"],
            &[]
        ),
        [
            ("stray/file.tgz", Step::Orphan),
            ("left-pad/package.json", moved("default/npm/packages/@/left-pad/document.json", true)),
            ("left-pad/.left-pad-1.0.0.tgz.tmp", Step::Orphan),
        ]
        .map(|(from, step)| (from.to_string(), step)),
    );
}

#[test]
fn an_interrupted_upgrade_resumes_where_it_stopped() {
    assert_eq!(
        steps(
            &[
                "default/npm/packages/@/left-pad/blobs/1.0.0.tgz",
                "left-pad/package.json",
                "orgs/acme/npm/packages/@/tool/document.json",
                "orgs/acme/oci/packages/@acme/@app/tool/document.json",
                ".tar-integrity/packages/@scope/pkg/document.json",
            ],
            &[("acme", Ecosystem::Npm), ("acme", Ecosystem::Oci)],
        ),
        [(
            "left-pad/package.json".to_string(),
            moved("default/npm/packages/@/left-pad/document.json", true)
        )],
    );
}

#[test]
fn an_org_that_is_also_a_package_is_refused() {
    let paths = vec!["acme/package.json".to_string(), "acme/tool/package.json".to_string()];
    let error = plan(&paths, &BTreeSet::from([("acme".to_string(), Ecosystem::Npm)])).unwrap_err();
    assert!(error.contains(r#""acme" is both an org and a package"#), "{error}");
}

#[test]
fn an_org_named_default_is_refused() {
    let error = plan(&[], &BTreeSet::from([("default".to_string(), Ecosystem::Npm)])).unwrap_err();
    assert!(error.contains(r#"an org named "default""#), "{error}");
}

#[test]
fn a_legacy_repository_named_like_a_new_layout_directory_is_refused() {
    for document in ["default/packages/foo/package.json", "orgs/acme/packages/tool/package.json"] {
        let error = plan(&[document.to_string()], &BTreeSet::new()).unwrap_err();
        assert!(error.contains("is named like a directory of the new layout"), "{error}");
    }
}

#[test]
fn a_literal_org_colliding_with_a_grouped_legacy_org_is_refused() {
    let paths = vec!["npm~internal/tool/package.json".to_string()];
    let orgs = BTreeSet::from([
        ("internal".to_string(), Ecosystem::Npm),
        ("npm~internal".to_string(), Ecosystem::Cargo),
    ]);
    let error = plan(&paths, &orgs).unwrap_err();
    assert!(
        error.contains(r#""npm~internal" is both a configured org and the legacy namespace"#),
        "{error}",
    );
}

#[test]
fn multiple_default_ecosystems_with_root_data_are_refused() {
    let orgs = BTreeSet::from([(String::new(), Ecosystem::Npm), (String::new(), Ecosystem::Cargo)]);
    for path in ["left-pad/package.json", ".tombstones/v0/left-pad.json"] {
        let error = plan(&[path.to_string()], &orgs).unwrap_err();
        assert!(error.contains("more than one ecosystem is configured without an org"), "{error}");
    }
}

#[test]
fn a_shared_org_with_a_legacy_directory_is_refused() {
    let paths = vec!["acme/tool/package.json".to_string()];
    let orgs = BTreeSet::from([
        ("acme".to_string(), Ecosystem::Npm),
        ("acme".to_string(), Ecosystem::Oci),
    ]);
    let error = plan(&paths, &orgs).unwrap_err();
    assert!(error.contains(r#""acme" is configured for more than one ecosystem"#), "{error}");
}

#[test]
fn dot_segments_in_legacy_package_paths_are_orphans() {
    assert_eq!(
        steps(&["acme/../package.json", "acme/.hidden/package.json"], &[("", Ecosystem::Oci)]),
        [
            ("acme/../package.json".to_string(), Step::Orphan),
            ("acme/.hidden/package.json".to_string(), Step::Orphan),
        ],
    );
}
