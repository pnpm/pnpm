use super::{default_graph, seed_default_graph};
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::Storage;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;

fn storage_config(dir: &TempDir) -> pnpr_config::StorageConfig {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    pnpr_config::Config::proxy(listen, dir.path().join("storage")).storage
}

fn storage(dir: &TempDir) -> Storage {
    Storage::from_config(&storage_config(dir)).unwrap()
}

async fn stored_document(storage: &Storage) -> serde_json::Value {
    let stored = storage
        .read_registry_graph()
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&stored.document).unwrap()
}

#[tokio::test]
async fn an_empty_database_stores_the_default_registries_once() {
    let dir = TempDir::new().unwrap();
    let storage = storage(&dir);
    seed_default_graph(&storage).await.unwrap();
    seed_default_graph(&storage).await.unwrap();
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(1));
    let default = serde_json::to_value(default_graph().unwrap()).unwrap();
    assert_eq!(stored_document(&storage).await, default);
    assert!(
        default["registries"]["npmjs"].is_object() && default["registries"]["local"].is_object(),
    );
}

#[tokio::test]
async fn a_stored_graph_is_never_replaced_by_the_default() {
    let dir = TempDir::new().unwrap();
    let storage = storage(&dir);
    let stored = pnpr_config::RegistryGraphDocument::from_yaml("defaultRegistry: null\n").unwrap();
    pnpr_fixtures::write_document(&storage_config(&dir), &stored).await;
    seed_default_graph(&storage).await.unwrap();
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(1));
    assert_eq!(stored_document(&storage).await, serde_json::to_value(&stored).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replicas_starting_together_store_one_revision() {
    for _ in 0..10 {
        let dir = TempDir::new().unwrap();
        let replicas: Vec<Storage> = (0..4).map(|_| storage(&dir)).collect();
        let seeds = replicas.iter().map(seed_default_graph);
        for seeded in futures_util::future::join_all(seeds).await {
            seeded.unwrap();
        }
        let revisions = replicas[0].list_registry_graph_revisions(None, 10).await.unwrap();
        assert_eq!(revisions.len(), 1, "{revisions:?}");
    }
}
