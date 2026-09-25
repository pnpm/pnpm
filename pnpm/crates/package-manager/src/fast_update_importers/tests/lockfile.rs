use super::{
    WITH_ONLY_A_PEER_VARIANT, WITH_SHARED_OPTIONAL_CHILD, lockfile, manifest_from, parsed_lockfile,
    try_fast_update_importers, with_a_direct_foo_at,
};
use serde_json::json;

#[test]
fn rejects_a_dependency_the_lockfile_holds_no_version_of() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0", "extra": "^1.0.0" } }));

    assert!(
        try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none(),
        "only the resolver can fetch a package the lockfile never saw",
    );
}
#[test]
fn rejects_adding_a_dependency_to_a_lockfile_that_records_publish_dates() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    subject.time = Some(
        [
            ("bar@2.0.0".to_string(), "2020-01-01T00:00:00.000Z".to_string()),
            ("opt@5.0.0".to_string(), "2020-01-01T00:00:00.000Z".to_string()),
        ]
        .into_iter()
        .collect(),
    );
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "only a resolution can record the publish date of a new direct dependency",
    );
}
#[test]
fn rejects_adding_a_dependency_the_lockfile_holds_only_as_a_peer_variant() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.0.0" } }),
    );

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_ONLY_A_PEER_VARIANT),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "the bare foo@1.1.0 names no snapshot, so which peer variant the edge takes is the resolver's call",
    );
}
#[test]
fn rejects_a_range_change_on_a_bare_edge_the_lockfile_holds_only_as_a_peer_variant() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.1.0" } }),
    );

    assert!(
        try_fast_update_importers(&with_a_direct_foo_at("1.1.0"), &[(".".to_string(), &manifest)],)
            .is_none(),
        "the bare record names no snapshot, so leaving it in place would keep the link dangling",
    );
}
