use super::OriginGates;
use std::time::Duration;

const SLOW_ORIGIN: &str = "https://a.example";
const OTHER_ORIGIN: &str = "https://b.example";

async fn is_granted_soon(gates: &OriginGates, origin: &str) -> bool {
    tokio::time::timeout(Duration::from_millis(50), gates.acquire(origin)).await.is_ok()
}

fn downscale(gates: &OriginGates, origin: &str) -> bool {
    gates.get(origin).is_some_and(|gate| gate.downscale_while_peers_active())
}

#[tokio::test]
async fn a_timeout_with_peers_on_the_same_origin_holds_that_origin_to_one_request() {
    let gates = OriginGates::default();
    let first = gates.acquire(SLOW_ORIGIN).await;
    let second = gates.acquire(SLOW_ORIGIN).await;
    let third = gates.acquire(SLOW_ORIGIN).await;
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
    let only = gates.acquire(SLOW_ORIGIN).await;
    let _other_origin = gates.acquire(OTHER_ORIGIN).await;
    assert!(!downscale(&gates, SLOW_ORIGIN));
    drop(only);
    let _first = gates.acquire(SLOW_ORIGIN).await;
    assert!(is_granted_soon(&gates, SLOW_ORIGIN).await);
}
