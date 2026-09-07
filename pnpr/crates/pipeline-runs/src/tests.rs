use super::{PipelineRunStore, PublishPipelineRun};
use pnpr_config::HostedStoreConfig;
use pnpr_storage::Storage;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn run(workspace: &str, run_id: &str) -> PublishPipelineRun {
    PublishPipelineRun {
        workspace: workspace.to_string(),
        run_id: run_id.to_string(),
        summary: json!({ "pipeline": "default", "runId": run_id }),
        events: vec![json!({ "event": "taskStarted", "task": "packages/a#build" })],
    }
}

fn local_store(root: &TempDir) -> PipelineRunStore {
    PipelineRunStore::new(storage_in(&HostedStoreConfig::Fs, root))
}

fn storage_in(hosted: &HostedStoreConfig, root: &TempDir) -> Storage {
    Storage::new(hosted, root.path().join("storage"), root.path().join("cache"))
        .expect("open storage")
}

#[tokio::test]
async fn publish_then_get_roundtrips_the_record() {
    let root = TempDir::new().expect("create storage root");
    let store = local_store(&root);
    store.publish(&run("demo-1234", "100-default")).await.expect("publish");

    let stored = store.get("demo-1234", "100-default").await.expect("get").expect("run exists");
    assert_eq!(stored.summary["pipeline"], "default");
    assert_eq!(stored.events.len(), 1);
    assert!(store.get("demo-1234", "999-missing").await.expect("get").is_none());
}

#[tokio::test]
async fn list_returns_newest_first_and_honors_the_workspace_filter() {
    let root = TempDir::new().expect("create storage root");
    let store = local_store(&root);
    store.publish(&run("ws-a", "100-default")).await.expect("publish");
    store.publish(&run("ws-a", "200-default")).await.expect("publish");
    store.publish(&run("ws-b", "150-default")).await.expect("publish");

    let all = store.list(&["ws-a", "ws-b"], 10).await.expect("list");
    let ids: Vec<&str> = all.iter().map(|entry| entry.run_id.as_str()).collect();
    assert_eq!(ids, ["200-default", "150-default", "100-default"]);

    let only_a = store.list(&["ws-a"], 10).await.expect("list");
    assert_eq!(only_a.len(), 2);
    assert!(only_a.iter().all(|entry| entry.workspace == "ws-a"));

    let limited = store.list(&["ws-a", "ws-b"], 1).await.expect("list");
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].run_id, "200-default");
}

#[tokio::test]
async fn a_run_id_is_append_only() {
    let root = TempDir::new().expect("create storage root");
    let store = local_store(&root);
    store.publish(&run("demo", "100-default")).await.expect("publish");

    let error = store.publish(&run("demo", "100-default")).await.expect_err("re-publish refused");
    let rendered = error.to_string();
    assert!(rendered.contains("append-only"), "unexpected error: {rendered}");
}

#[tokio::test]
async fn path_shaped_identifiers_are_refused() {
    let root = TempDir::new().expect("create storage root");
    let store = local_store(&root);
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
        assert!(
            store.get(workspace, run_id).await.is_err(),
            "get must refuse {workspace}/{run_id}",
        );
    }
}

#[tokio::test]
async fn concurrent_publications_cannot_replace_the_winner() {
    let root = TempDir::new().unwrap();
    let first_store = local_store(&root);
    let second_store = local_store(&root);
    let first = run("demo", "100-default");
    let mut second = run("demo", "100-default");
    second.summary = json!({"publisher": "second"});
    let first_publication = first_store.publish(&first);
    let second_publication = second_store.publish(&second);
    let (first_result, second_result) = tokio::join!(first_publication, second_publication);
    assert_ne!(first_result.is_ok(), second_result.is_ok(), "exactly one writer must succeed");
    let expected = if first_result.is_ok() { first.summary } else { second.summary };
    let winner = first_store.get("demo", "100-default").await.unwrap().unwrap();
    assert_eq!(winner.summary, expected);
    assert!(
        second_store.publish(&run("demo", "100-default")).await.is_err(),
        "later publication must be refused",
    );
    let seen_by_second = second_store.get("demo", "100-default").await.unwrap().unwrap();
    assert_eq!(seen_by_second.summary, expected);
}

#[tokio::test]
async fn listing_does_not_parse_records_outside_the_requested_page() {
    let root = TempDir::new().unwrap();
    let storage = storage_in(&HostedStoreConfig::Fs, &root);
    let store = PipelineRunStore::new(storage.clone());
    store.publish(&run("demo", "200-default")).await.unwrap();
    storage.create_pipeline_run("demo", "100-default.json", b"invalid JSON").await.unwrap();
    assert_eq!(store.list(&["demo"], 1).await.unwrap()[0].run_id, "200-default");
}

/// A listing costs what the workspaces asked about hold, not what the
/// deployment holds.
#[tokio::test]
async fn a_listing_is_scoped_to_the_workspaces_it_was_given() {
    let root = TempDir::new().unwrap();
    let storage = storage_in(&HostedStoreConfig::Fs, &root);
    let store = PipelineRunStore::new(storage.clone());
    store.publish(&run("wanted", "100-default")).await.unwrap();
    storage.create_pipeline_run("ignored", "999-default.json", b"invalid JSON").await.unwrap();

    let listed = store.list(&["wanted"], 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].run_id, "100-default");
}

/// The state every deployment starts in: a configured workspace nothing has
/// reported a run for yet.
#[tokio::test]
async fn a_workspace_with_no_runs_lists_empty() {
    let root = TempDir::new().unwrap();
    let store = local_store(&root);
    assert!(store.list(&["never-run"], 10).await.expect("list").is_empty());
    assert!(store.get("never-run", "100-default").await.expect("get").is_none());
}

/// An operator should hear that a record cannot be read, and be told which.
#[tokio::test]
async fn a_corrupt_record_on_the_page_is_named() {
    let root = TempDir::new().unwrap();
    let storage = storage_in(&HostedStoreConfig::Fs, &root);
    let store = PipelineRunStore::new(storage.clone());
    storage.create_pipeline_run("demo", "100-default.json", b"invalid JSON").await.unwrap();

    let error = store.list(&["demo"], 10).await.expect_err("the listing fails");
    let rendered = error.to_string();
    assert!(rendered.contains("demo/100-default"), "unexpected error: {rendered}");
}

#[tokio::test]
async fn a_key_the_store_did_not_write_is_passed_over() {
    let root = TempDir::new().unwrap();
    let storage = storage_in(&HostedStoreConfig::Fs, &root);
    let store = PipelineRunStore::new(storage.clone());
    store.publish(&run("demo", "100-default")).await.unwrap();
    std::fs::create_dir_all(root.path().join("storage/.pipeline-runs/v0/demo/nested")).unwrap();
    std::fs::write(
        root.path().join("storage/.pipeline-runs/v0/demo/nested/deeper.json"),
        b"invalid JSON",
    )
    .unwrap();
    std::fs::write(root.path().join("storage/.pipeline-runs/v0/demo/notes.txt"), b"notes").unwrap();

    let listed = store.list(&["demo"], 10).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].run_id, "100-default");
}

/// Run records belong to the deployment, not to the replica that was asked.
#[tokio::test]
async fn a_run_recorded_on_one_replica_is_served_by_another() {
    let bucket: Arc<dyn object_store::ObjectStore> =
        Arc::new(object_store::memory::InMemory::new());
    let hosted =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&bucket), prefix: String::new() };
    let recording_root = TempDir::new().unwrap();
    let serving_root = TempDir::new().unwrap();
    let recording = PipelineRunStore::new(storage_in(&hosted, &recording_root));
    let serving = PipelineRunStore::new(storage_in(&hosted, &serving_root));

    recording.publish(&run("demo", "100-default")).await.expect("publish");

    let stored = serving.get("demo", "100-default").await.expect("get").expect("run exists");
    assert_eq!(stored.summary["runId"], "100-default");
    let listed = serving.list(&["demo"], 10).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].run_id, "100-default");
    assert!(
        serving.publish(&run("demo", "100-default")).await.is_err(),
        "a run recorded on one replica is append-only on every replica",
    );
}
