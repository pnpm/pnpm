use super::{OriginGates, OriginPermit};
use crate::{NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig};
use std::time::Duration;

const SLOW_ORIGIN: &str = "https://a.example";
const OTHER_ORIGIN: &str = "https://b.example";

async fn is_granted_soon(gates: &OriginGates, origin: &str) -> bool {
    tokio::time::timeout(Duration::from_millis(50), gates.acquire(origin)).await.is_ok()
}

async fn start(gates: &OriginGates, origin: &str) -> OriginPermit {
    let mut permit = gates.acquire(origin).await;
    permit.mark_active();
    permit
}

fn downscale(gates: &OriginGates, origin: &str) -> bool {
    gates.get(origin).is_some_and(|gate| gate.downscale_while_peers_active())
}

#[tokio::test]
async fn a_timeout_with_peers_on_the_same_origin_holds_that_origin_to_one_request() {
    let gates = OriginGates::default();
    let first = start(&gates, SLOW_ORIGIN).await;
    let second = start(&gates, SLOW_ORIGIN).await;
    let third = start(&gates, SLOW_ORIGIN).await;
    assert!(downscale(&gates, SLOW_ORIGIN));
    assert!(!downscale(&gates, SLOW_ORIGIN));

    assert!(is_granted_soon(&gates, OTHER_ORIGIN).await, "other origins keep their concurrency");
    assert!(!is_granted_soon(&gates, SLOW_ORIGIN).await);
    drop(third);
    drop(second);
    assert!(!is_granted_soon(&gates, SLOW_ORIGIN).await, "one request is still in flight");

    drop(first);
    let next = gates.acquire(SLOW_ORIGIN).await;
    assert!(!is_granted_soon(&gates, SLOW_ORIGIN).await, "the origin stays at one request");
    drop(next);
    assert!(is_granted_soon(&gates, SLOW_ORIGIN).await);
}

#[tokio::test]
async fn a_lone_request_on_its_origin_does_not_downscale() {
    let gates = OriginGates::default();
    let only = start(&gates, SLOW_ORIGIN).await;
    let _other_origin = start(&gates, OTHER_ORIGIN).await;
    assert!(!downscale(&gates, SLOW_ORIGIN));
    drop(only);
    let _first = gates.acquire(SLOW_ORIGIN).await;
    assert!(is_granted_soon(&gates, SLOW_ORIGIN).await);
}

#[tokio::test]
async fn a_request_still_queued_for_global_admission_is_not_a_peer() {
    let gates = OriginGates::default();
    let _active = start(&gates, SLOW_ORIGIN).await;
    let mut queued = gates.acquire(SLOW_ORIGIN).await;
    assert!(!downscale(&gates, SLOW_ORIGIN));
    queued.mark_active();
    assert!(downscale(&gates, SLOW_ORIGIN));
}

#[tokio::test]
async fn a_request_waiting_for_a_global_slot_is_not_a_peer_of_a_timed_out_one() {
    let client = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings { network_concurrency: 1, ..NetworkSettings::default() },
    )
    .expect("client builds");
    let url = "https://registry.example/pkg";
    let active = client.acquire_for_url(url).await;
    let queued = client.acquire_for_url(url);
    tokio::pin!(queued);
    let waiting = tokio::time::timeout(Duration::from_millis(50), &mut queued).await;
    assert!(waiting.is_err(), "the only global slot is taken");
    assert!(!client.downscale_while_peers_active(url));
    drop(active);
    drop(queued.await);
}
