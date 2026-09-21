use super::{
    Arc, DependencyGroup, HashMap, Mutex, RecordingResolver, WorkspaceImporter, fake_manifest,
    fake_result, graph_versions_of, importer_opts, resolve_workspace, reuse_graph_lockfile,
    workspace_opts,
};
use std::path::PathBuf;

#[tokio::test]
async fn auto_dedupe_reopens_candidate_consumers_but_reuses_unrelated_subtrees() {
    let (_temp, manifest) = fake_manifest(serde_json::json!({"app": "1.0.0", "stable": "1.0.0"}));
    let importers = [WorkspaceImporter { id: ".".into(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("app".into(), "1.0.0".into()),
                fake_result(
                    "app",
                    "1.0.0",
                    None,
                    serde_json::json!({"name": "app", "version": "1.0.0", "dependencies": {"foo": "^1.0.0"}}),
                ),
            ),
            (
                ("foo".into(), "^1.0.0".into()),
                fake_result(
                    "foo",
                    "1.5.0",
                    None,
                    serde_json::json!({"name": "foo", "version": "1.5.0"}),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut options = workspace_opts(false, false);
    options.reuse.lockfile = Some(Arc::new(reuse_graph_lockfile(
        ".",
        &[("app", "1.0.0", "1.0.0"), ("stable", "1.0.0", "1.0.0")],
        &[
            ("app@1.0.0", &[("foo", "1.0.0")]),
            ("foo@1.0.0", &[]),
            ("foo@1.5.0", &[]),
            ("stable@1.0.0", &[("untouched", "1.0.0")]),
            ("untouched@1.0.0", &[]),
        ],
        &[],
    )));
    options.reuse.dedupe.insert("foo".into(), None);
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], options, |_| {
            importer_opts(PathBuf::from("/repo"), None)
        })
        .await
        .unwrap();
    assert_eq!(graph_versions_of(&result, "foo"), ["1.5.0"]);
    assert_eq!(graph_versions_of(&result, "untouched"), ["1.0.0"]);
    let seen = resolver.seen.lock().unwrap();
    let mut names: Vec<_> = seen
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["app", "foo"], "only the candidate and its consumer need metadata");
}
