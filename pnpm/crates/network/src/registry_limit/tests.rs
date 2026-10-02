use super::RegistryLimits;
use crate::{NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig};
use std::{collections::BTreeMap, num::NonZeroUsize, time::Duration};

fn client_with_registry_limits(limits: &[(&str, usize)]) -> ThrottledClient {
    let limits: BTreeMap<String, NonZeroUsize> = limits
        .iter()
        .map(|&(registry, limit)| (registry.to_owned(), NonZeroUsize::new(limit).unwrap()))
        .collect();
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings {
            network_concurrency: 16,
            network_concurrency_by_registry: limits,
            ..NetworkSettings::default()
        },
    )
    .expect("client builds")
}

async fn is_granted_soon(client: &ThrottledClient, url: &str) -> bool {
    tokio::time::timeout(Duration::from_millis(50), client.acquire_for_url(url))
        .await
        .is_ok()
}

#[tokio::test]
async fn a_registry_limit_caps_requests_to_that_registry_origin_only() {
    let client = client_with_registry_limits(&[("https://slow.example/npm/", 2)]);
    let _first = client.acquire_for_url("https://slow.example/npm/a").await;
    let second = client.acquire_for_url("https://slow.example/-/b.tgz").await;
    assert!(!is_granted_soon(&client, "https://slow.example/npm/c").await);
    assert!(is_granted_soon(&client, "https://fast.example/c").await);
    drop(second);
    assert!(is_granted_soon(&client, "https://slow.example/npm/c").await);
}

#[tokio::test]
async fn registries_sharing_an_origin_share_the_smallest_limit() {
    let client = client_with_registry_limits(&[
        ("https://corp.example/npm-a/", 3),
        ("https://corp.example/npm-b/", 1),
    ]);
    let _only = client.acquire_for_url("https://corp.example/npm-a/pkg").await;
    assert!(!is_granted_soon(&client, "https://corp.example/npm-a/other").await);
}

#[tokio::test]
async fn a_capped_registry_serves_queued_metadata_before_queued_downloads() {
    let client = std::sync::Arc::new(client_with_registry_limits(&[("https://slow.example/", 1)]));
    let held = client.acquire_for_url("https://slow.example/held").await;
    let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let download = tokio::spawn({
        let (client, order) = (std::sync::Arc::clone(&client), std::sync::Arc::clone(&order));
        async move {
            let _slot =
                client.acquire_for_url_with_priority("https://slow.example/a.tgz", 100).await;
            order.lock().unwrap().push("download");
        }
    });
    wait_for_waiters(&client, "https://slow.example/", 1).await;
    let metadata = tokio::spawn({
        let (client, order) = (std::sync::Arc::clone(&client), std::sync::Arc::clone(&order));
        async move {
            let _slot = client.acquire_for_url("https://slow.example/pkg").await;
            order.lock().unwrap().push("metadata");
        }
    });
    wait_for_waiters(&client, "https://slow.example/", 2).await;
    drop(held);
    metadata.await.unwrap();
    download.await.unwrap();
    assert_eq!(*order.lock().unwrap(), vec!["metadata", "download"]);
}

async fn wait_for_waiters(client: &ThrottledClient, url: &str, count: usize) {
    let slots = client.origin_limits
        .registries()
        .slots_for(url)
        .expect("the registry is capped");
    tokio::time::timeout(Duration::from_secs(5), async {
        while slots.queued_waiters() < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the requests queue at the registry cap");
}

#[test]
fn a_registry_limit_above_the_global_limit_is_held_to_it() {
    let limits = BTreeMap::from([("https://huge.example/".to_owned(), NonZeroUsize::MAX)]);
    let registries = RegistryLimits::new(&limits, 16);
    let slots = registries.slots_for("https://huge.example/pkg").expect("the registry is capped");
    assert_eq!(slots.available_permits(), 16);
}
