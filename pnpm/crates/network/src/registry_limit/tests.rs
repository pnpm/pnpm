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
