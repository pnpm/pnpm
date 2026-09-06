use super::{PipelineRunStore, PublishPipelineRun};
use serde_json::json;
use tempfile::TempDir;

fn run(workspace: &str, run_id: &str) -> PublishPipelineRun {
    PublishPipelineRun {
        workspace: workspace.to_string(),
        run_id: run_id.to_string(),
        summary: json!({ "pipeline": "default", "runId": run_id }),
        events: vec![json!({ "event": "taskStarted", "task": "packages/a#build" })],
    }
}

#[tokio::test]
async fn publish_then_get_roundtrips_the_record() {
    let root = TempDir::new().expect("create storage root");
    let store = PipelineRunStore::new(root.path()).expect("open store");
    store.publish(&run("demo-1234", "100-default")).await.expect("publish");

    let stored = store.get("demo-1234", "100-default").expect("get").expect("run exists");
    assert_eq!(stored.summary["pipeline"], "default");
    assert_eq!(stored.events.len(), 1);
    assert!(store.get("demo-1234", "999-missing").expect("get").is_none());
}

#[tokio::test]
async fn list_returns_newest_first_and_honors_the_workspace_filter() {
    let root = TempDir::new().expect("create storage root");
    let store = PipelineRunStore::new(root.path()).expect("open store");
    store.publish(&run("ws-a", "100-default")).await.expect("publish");
    store.publish(&run("ws-a", "200-default")).await.expect("publish");
    store.publish(&run("ws-b", "150-default")).await.expect("publish");

    let all = store.list(None, 10).expect("list");
    let ids: Vec<&str> = all.iter().map(|entry| entry.run_id.as_str()).collect();
    assert_eq!(ids, ["200-default", "150-default", "100-default"]);

    let only_a = store.list(Some("ws-a"), 10).expect("list");
    assert_eq!(only_a.len(), 2);
    assert!(only_a.iter().all(|entry| entry.workspace == "ws-a"));

    let limited = store.list(None, 1).expect("list");
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].run_id, "200-default");
}

#[tokio::test]
async fn a_run_id_is_append_only() {
    let root = TempDir::new().expect("create storage root");
    let store = PipelineRunStore::new(root.path()).expect("open store");
    store.publish(&run("demo", "100-default")).await.expect("publish");

    let error = store.publish(&run("demo", "100-default")).await.expect_err("re-publish refused");
    let rendered = error.to_string();
    assert!(rendered.contains("append-only"), "unexpected error: {rendered}");
}

#[tokio::test]
async fn path_shaped_identifiers_are_refused() {
    let root = TempDir::new().expect("create storage root");
    let store = PipelineRunStore::new(root.path()).expect("open store");
    for (workspace, run_id) in [
        ("../escape", "100-default"),
        ("demo/nested", "100-default"),
        ("demo", "../escape"),
        ("demo", ""),
        (".hidden", "100-default"),
    ] {
        let error = store.publish(&run(workspace, run_id)).await.expect_err("refused");
        let rendered = error.to_string();
        assert!(
            rendered.contains("ASCII"),
            "unexpected error for {workspace}/{run_id}: {rendered}",
        );
        assert!(store.get(workspace, run_id).is_err(), "get must refuse {workspace}/{run_id}");
    }
}

#[tokio::test]
async fn concurrent_publications_cannot_replace_the_winner() {
    let root = TempDir::new().unwrap();
    let first_store = PipelineRunStore::new(root.path()).unwrap();
    let second_store = PipelineRunStore::new(root.path()).unwrap();
    let first = run("demo", "100-default");
    let mut second = run("demo", "100-default");
    second.summary = json!({"publisher": "second"});
    let first_publication = first_store.publish(&first);
    let second_publication = second_store.publish(&second);
    let (first_result, second_result) = tokio::join!(first_publication, second_publication);
    assert_ne!(first_result.is_ok(), second_result.is_ok(), "exactly one writer must succeed");
    let expected = if first_result.is_ok() { first.summary } else { second.summary };
    assert_eq!(first_store.get("demo", "100-default").unwrap().unwrap().summary, expected);
    assert!(
        second_store.publish(&run("demo", "100-default")).await.is_err(),
        "later publication must be refused",
    );
    assert_eq!(second_store.get("demo", "100-default").unwrap().unwrap().summary, expected);
}

#[tokio::test]
async fn listing_does_not_parse_records_outside_the_requested_page() {
    let root = TempDir::new().unwrap();
    let store = PipelineRunStore::new(root.path()).unwrap();
    store.publish(&run("demo", "200-default")).await.unwrap();
    std::fs::write(store.run_path("demo", "100-default"), "invalid JSON").unwrap();
    assert_eq!(store.list(Some("demo"), 1).unwrap()[0].run_id, "200-default");
}
