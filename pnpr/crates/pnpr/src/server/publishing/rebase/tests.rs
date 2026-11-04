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

    let rebased = rebase_packument(Some(&base), &ours, &current);

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

    let rebased = rebase_packument(None, &ours, &current);

    assert_eq!(rebased["dist-tags"]["latest"], "1.0.0");
    assert_eq!(
        rebased["versions"]
            .as_object()
            .unwrap()
            .len(),
        2,
    );
}
