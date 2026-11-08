use super::rebase_packument;
use serde_json::json;

#[test]
fn a_concurrent_version_and_tag_survive_the_rebase() {
    let base = json!({
        "name": "pkg",
        "dist-tags": { "latest": "1.0.0", "beta": "1.0.0" },
        "versions": { "1.0.0": { "version": "1.0.0" } },
        "time": { "1.0.0": "t1", "modified": "t1" },
    });
    let ours = json!({
        "name": "pkg",
        "dist-tags": { "latest": "1.1.0", "beta": "1.0.0" },
        "versions": { "1.0.0": { "version": "1.0.0" }, "1.1.0": { "version": "1.1.0" } },
        "time": { "1.0.0": "t1", "1.1.0": "t2", "modified": "t2" },
    });
    let current = json!({
        "name": "pkg",
        "dist-tags": { "latest": "1.0.0", "beta": "2.0.0-beta" },
        "versions": { "1.0.0": { "version": "1.0.0" }, "2.0.0-beta": { "version": "2.0.0-beta" } },
        "time": { "1.0.0": "t1", "2.0.0-beta": "t3", "modified": "t3" },
    });

    let rebased = rebase_packument(Some(&base), &ours, &current).unwrap();

    assert_eq!(rebased["dist-tags"], json!({ "latest": "1.1.0", "beta": "2.0.0-beta" }));
    assert_eq!(
        rebased["versions"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["1.0.0", "2.0.0-beta", "1.1.0"],
    );
    assert_eq!(rebased["time"]["2.0.0-beta"], "t3");
    assert_eq!(rebased["time"]["1.1.0"], "t2");
    assert_eq!(rebased["time"]["modified"], "t2");
}

#[test]
fn a_first_publish_rebases_onto_a_package_created_meanwhile() {
    let ours = json!({
        "name": "pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "version": "1.0.0" } },
    });
    let current = json!({
        "name": "pkg",
        "dist-tags": { "latest": "2.0.0" },
        "versions": { "2.0.0": { "version": "2.0.0" } },
    });

    let rebased = rebase_packument(None, &ours, &current).unwrap();

    assert_eq!(rebased["dist-tags"]["latest"], "1.0.0");
    assert_eq!(
        rebased["versions"]
            .as_object()
            .unwrap()
            .len(),
        2,
    );
}

#[test]
fn a_field_other_than_versions_tags_and_times_is_taken_whole() {
    let base = json!({ "repository": { "type": "git", "url": "old" } });
    let ours = json!({ "repository": { "type": "git", "url": "new" } });
    let current = json!({ "repository": { "type": "svn", "url": "old" } });

    let rebased = rebase_packument(Some(&base), &ours, &current).unwrap();

    assert_eq!(rebased["repository"], json!({ "type": "git", "url": "new" }));
}

#[test]
fn a_first_publish_does_not_rebase_onto_a_package_another_user_claimed() {
    let ours = json!({
        "maintainers": [{ "name": "alice" }],
        "versions": { "1.0.0": { "version": "1.0.0" } },
    });
    let current = json!({
        "maintainers": [{ "name": "bob" }],
        "versions": { "2.0.0": { "version": "2.0.0" } },
    });

    assert_eq!(rebase_packument(None, &ours, &current), None);
}

#[test]
fn a_publish_does_not_rebase_once_the_owners_changed() {
    let base = json!({
        "maintainers": [{ "name": "alice" }, { "name": "bob" }],
        "versions": {},
    });
    let ours = json!({
        "maintainers": [{ "name": "alice" }, { "name": "bob" }],
        "versions": { "1.0.0": { "version": "1.0.0" } },
    });
    let current = json!({ "maintainers": [{ "name": "bob" }], "versions": {} });

    assert_eq!(rebase_packument(Some(&base), &ours, &current), None);
}

#[test]
fn a_client_revision_precondition_forbids_rebasing() {
    use super::{StagedPublish, rebase_bases};
    use crate::server::publishing::{StagedBase, Verification};
    use pnpr_package_name::CanonicalPackageName;
    use pnpr_registry::Ecosystem;

    let mut staged = [StagedPublish {
        name: CanonicalPackageName::parse("widget", Ecosystem::Npm).unwrap(),
        ecosystem: Ecosystem::Npm,
        document: Vec::new(),
        base: StagedBase {
            revision_checked: true,
            packument: Some(json!({ "versions": {} })),
            ..StagedBase::default()
        },
        slots: Vec::new(),
        org: None,
        publisher: None,
        verification: Verification::default(),
    }];
    assert!(rebase_bases(&mut staged).is_none());
    staged[0].base.revision_checked = false;
    assert!(rebase_bases(&mut staged).is_some());
}
