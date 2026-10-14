use super::Singleflight;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn singleflight_coalesces_concurrent_calls() {
    let singleflight = Arc::new(Singleflight::<String, String>::new());
    let calls = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..10 {
        let flight_clone = Arc::clone(&singleflight);
        let calls_clone = Arc::clone(&calls);
        handles.push(tokio::spawn(async move {
            flight_clone.work("package-a".to_string(), || async move {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(50)).await;
                "result-a".to_string()
            })
            .await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, "result-a");
    }

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn singleflight_independent_keys_run_independently() {
    let singleflight = Arc::new(Singleflight::<String, String>::new());
    let calls = Arc::new(AtomicUsize::new(0));

    let flight_first = Arc::clone(&singleflight);
    let calls_first = Arc::clone(&calls);
    let handle_first = tokio::spawn(async move {
        flight_first.work("key-1".to_string(), || async move {
            calls_first.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(20)).await;
            "val-1".to_string()
        })
        .await
    });

    let flight_second = Arc::clone(&singleflight);
    let calls_second = Arc::clone(&calls);
    let handle_second = tokio::spawn(async move {
        flight_second.work("key-2".to_string(), || async move {
            calls_second.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(20)).await;
            "val-2".to_string()
        })
        .await
    });

    assert_eq!(handle_first.await.unwrap(), "val-1");
    assert_eq!(handle_second.await.unwrap(), "val-2");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn singleflight_subsequent_call_runs_after_previous_finished() {
    let singleflight = Singleflight::<String, usize>::new();
    let calls = Arc::new(AtomicUsize::new(0));

    let calls_first = Arc::clone(&calls);
    let first_result = singleflight.work("key".to_string(), || async move {
        calls_first.fetch_add(1, Ordering::SeqCst);
        100
    })
    .await;
    assert_eq!(first_result, 100);

    let calls_second = Arc::clone(&calls);
    let second_result = singleflight.work("key".to_string(), || async move {
        calls_second.fetch_add(1, Ordering::SeqCst);
        200
    })
    .await;
    assert_eq!(second_result, 200);

    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn singleflight_leader_cancellation_lets_follower_take_over() {
    let singleflight = Arc::new(Singleflight::<String, String>::new());
    let calls = Arc::new(AtomicUsize::new(0));

    // Start a leader that takes long
    let flight_leader = Arc::clone(&singleflight);
    let calls_leader = Arc::clone(&calls);
    let leader_handle = tokio::spawn(async move {
        flight_leader.work("key".to_string(), || async move {
            calls_leader.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_secs(10)).await;
            "leader".to_string()
        })
        .await
    });

    // Wait until leader has started
    sleep(Duration::from_millis(20)).await;

    // Follower joins while leader is in-flight
    let flight_follower = Arc::clone(&singleflight);
    let calls_follower = Arc::clone(&calls);
    let follower_handle = tokio::spawn(async move {
        flight_follower.work("key".to_string(), || async move {
            calls_follower.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(20)).await;
            "follower".to_string()
        })
        .await
    });

    // Cancel the leader
    sleep(Duration::from_millis(10)).await;
    leader_handle.abort();

    // Follower should wake up and complete successfully as new leader
    let follower_res = follower_handle.await.unwrap();
    assert_eq!(follower_res, "follower");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
