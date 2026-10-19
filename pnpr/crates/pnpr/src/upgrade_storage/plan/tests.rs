use super::{Step, plan};
use std::collections::BTreeSet;

fn steps(paths: &[&str], orgs: &[&str]) -> Vec<(String, Step)> {
    let paths: Vec<String> = paths
        .iter()
        .map(ToString::to_string)
        .collect();
    let orgs: BTreeSet<String> = orgs
        .iter()
        .map(ToString::to_string)
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
                "acme/app/package.json",
                "acme/app/sha256-aa",
                "acme/app/tool/package.json",
                "acme/app/tool/sha256-bb",
            ],
            &[],
        ),
        [
            ("left-pad/package.json", moved("default/packages/left-pad/document.json", true)),
            (
                "left-pad/left-pad-1.0.0.tgz",
                moved("default/packages/left-pad/blobs/left-pad-1.0.0.tgz", false)
            ),
            (
                "@scope/App/package.json",
                moved("default/packages/!40scope!2f!41pp/document.json", true)
            ),
            (
                "@scope/App/App-1.0.0.tgz",
                moved("default/packages/!40scope!2f!41pp/blobs/!41pp-1.0.0.tgz", false),
            ),
            ("acme/app/package.json", moved("default/packages/acme!2fapp/document.json", true)),
            ("acme/app/sha256-aa", moved("default/packages/acme!2fapp/blobs/sha256-aa", false)),
            (
                "acme/app/tool/package.json",
                moved("default/packages/acme!2fapp!2ftool/document.json", true)
            ),
            (
                "acme/app/tool/sha256-bb",
                moved("default/packages/acme!2fapp!2ftool/blobs/sha256-bb", false)
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
            ],
            &["npm~internal"],
        ),
        [
            (
                "npm~internal/tool/package.json",
                moved("orgs/npm!7einternal/packages/tool/document.json", true)
            ),
            (
                "npm~internal/tool/tool-1.0.0.tgz",
                moved("orgs/npm!7einternal/packages/tool/blobs/tool-1.0.0.tgz", false),
            ),
            (
                "npm~internal/.revisions/sha512/abc/index.json",
                moved("orgs/npm!7einternal/.revisions/sha512/abc/index.json", false),
            ),
            (
                "npm~internal/.tombstones/v0/@scope/gone.json",
                moved("orgs/npm!7einternal/.tombstones/v0/!40scope!2fgone.json", false),
            ),
            ("npm~internal/.package-index/tool/.present", Step::Remove),
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
                ".team-rosters/v0/main.json",
                ".pnpr-journal/0001/manifest.json",
                ".pnpr-cache/~public/x/package.json",
            ],
            &[],
        ),
        [],
    );
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
            (
                ".revisions/sha512/abc/index.json",
                moved("default/.revisions/sha512/abc/index.json", false)
            ),
            (".tombstones/v0/left-pad.json", moved("default/.tombstones/v0/left-pad.json", false)),
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
            moved(".tar-integrity/packages/!40scope!2fpkg/document.json", true),
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
            ("left-pad/package.json", moved("default/packages/left-pad/document.json", true)),
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
                "default/packages/left-pad/blobs/left-pad-1.0.0.tgz",
                "left-pad/package.json",
                "orgs/acme/packages/tool/document.json",
                ".tar-integrity/packages/!40scope!2fpkg/document.json",
            ],
            &["acme"],
        ),
        [(
            "left-pad/package.json".to_string(),
            moved("default/packages/left-pad/document.json", true)
        )],
    );
}

#[test]
fn an_org_that_is_also_a_package_is_refused() {
    let paths = vec!["acme/package.json".to_string(), "acme/tool/package.json".to_string()];
    let error = plan(&paths, &BTreeSet::from(["acme".to_string()])).unwrap_err();
    assert!(error.contains(r#""acme" is both an org and a package"#), "{error}");
}

#[test]
fn an_org_named_default_is_refused() {
    let error = plan(&[], &BTreeSet::from(["default".to_string()])).unwrap_err();
    assert!(error.contains(r#"an org named "default""#), "{error}");
}

#[test]
fn a_legacy_repository_named_like_a_new_layout_directory_is_refused() {
    for document in ["default/packages/foo/package.json", "orgs/acme/packages/tool/package.json"] {
        let error = plan(&[document.to_string()], &BTreeSet::new()).unwrap_err();
        assert!(error.contains("is named like a directory of the new layout"), "{error}");
    }
}
