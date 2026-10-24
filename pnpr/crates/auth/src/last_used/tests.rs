use super::{
    LAST_USED_GRANULARITY_SECS, LastUsedWriter, MAX_WRITES_IN_FLIGHT, MIN_CLAIMS_BEFORE_PRUNE,
};
use crate::{NewToken, Result, TokenBackend, TokenRecord, sha256_hex, unix_seconds};
use async_trait::async_trait;
use pnpr_error::RegistryError;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

/// A token store that only remembers which keys were touched, and when. With
/// `gate` set, a touch takes a permit from it before it is remembered; with
/// `fail` set, every touch fails after that.
#[derive(Default)]
struct TouchLog {
    touches: Mutex<Vec<(String, u64)>>,
    gate: Option<Semaphore>,
    fail: bool,
}

#[async_trait]
impl TokenBackend for TouchLog {
    async fn issue_token(
        &self,
        _username: &str,
        _new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        Err(RegistryError::Internal { reason: "TouchLog cannot issue tokens".to_string() })
    }

    async fn lookup(&self, _raw: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        if self.fail {
            return Err(RegistryError::Internal { reason: "TouchLog is failing".to_string() });
        }
        self.touches
            .lock()
            .unwrap()
            .push((key.to_string(), now));
        Ok(())
    }

    async fn find_by_key(&self, _key: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_for_user(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn revoke_by_key(&self, _key: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }
}

fn writer_over(log: TouchLog) -> (Arc<LastUsedWriter>, Arc<TouchLog>) {
    let log = Arc::new(log);
    let writer = LastUsedWriter::new(Arc::clone(&log) as Arc<dyn TokenBackend>);
    (Arc::new(writer), log)
}

fn writer() -> (Arc<LastUsedWriter>, Arc<TouchLog>) {
    writer_over(TouchLog::default())
}

async fn wait_until(condition: impl Fn() -> bool + Send + Sync) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the spawned writes land");
}

/// A stored time every test treats as due for a write.
const STALE: u64 = 0;
const NOW: u64 = 1_000;

#[test]
fn an_interval_is_claimed_once_per_token() {
    let (writer, _) = writer();
    assert!(writer.claim("k", NOW));
    assert!(!writer.claim("k", NOW));
    assert!(!writer.claim("k", NOW + LAST_USED_GRANULARITY_SECS - 1));
    assert!(writer.claim("other", NOW));
    assert!(writer.claim("k", NOW + LAST_USED_GRANULARITY_SECS));
}

#[test]
fn expired_claims_are_pruned() {
    let (writer, _) = writer();
    for index in 0..MIN_CLAIMS_BEFORE_PRUNE {
        assert!(writer.claim(&format!("old-{index}"), NOW));
    }
    assert!(writer.claim("new", NOW + LAST_USED_GRANULARITY_SECS));
    let claims = writer.claims.lock().unwrap();
    assert_eq!(claims.written_at.len(), 1);
    assert!(claims.written_at.contains_key("new"));
}

#[tokio::test]
async fn a_fresh_stored_time_needs_no_write_and_no_key() {
    let (writer, log) = writer();
    assert!(!writer.record_with(unix_seconds(), || panic!("a fresh time derives no key")));
    assert!(writer.record_with(STALE, || "k".to_string()));
    wait_until(|| !log.touches.lock().unwrap().is_empty()).await;
    let touches = log.touches.lock().unwrap();
    assert_eq!(touches.len(), 1);
    assert_eq!(touches[0].0, "k");
    assert!(touches[0].1 > STALE, "written with the current time, not the stored one");
}

#[tokio::test]
async fn a_raw_token_is_written_under_its_key() {
    let (writer, log) = writer();
    writer.record_raw("raw-token", STALE);
    wait_until(|| !log.touches.lock().unwrap().is_empty()).await;
    assert_eq!(log.touches.lock().unwrap()[0].0, sha256_hex(b"raw-token"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_uses_of_one_token_write_once() {
    let (writer, log) = writer();
    let uses = (0..64).map(|_| {
        let writer = Arc::clone(&writer);
        tokio::spawn(async move { writer.record_with(STALE, || "k".to_string()) })
    });
    let mut started = 0;
    for handle in uses {
        if handle.await.unwrap() {
            started += 1;
        }
    }
    assert_eq!(started, 1);
    wait_until(|| !log.touches.lock().unwrap().is_empty()).await;
    assert_eq!(log.touches.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn writes_in_flight_are_bounded_and_a_refused_token_retries() {
    let (writer, log) =
        writer_over(TouchLog { gate: Some(Semaphore::new(0)), ..TouchLog::default() });
    for index in 0..MAX_WRITES_IN_FLIGHT {
        assert!(writer.record_with(STALE, || format!("in-flight-{index}")));
    }
    assert!(!writer.record_with(STALE, || "refused".to_string()));

    let gate = log.gate.as_ref().unwrap();
    gate.add_permits(MAX_WRITES_IN_FLIGHT);
    wait_until(|| log.touches.lock().unwrap().len() == MAX_WRITES_IN_FLIGHT).await;
    assert!(writer.record_with(STALE, || "refused".to_string()));
    gate.add_permits(1);
    wait_until(|| log.touches.lock().unwrap().len() == MAX_WRITES_IN_FLIGHT + 1).await;
}

#[tokio::test]
async fn a_failed_write_frees_its_slot_but_keeps_its_claim() {
    let (writer, _) = writer_over(TouchLog { fail: true, ..TouchLog::default() });
    assert!(writer.record_with(STALE, || "k".to_string()));
    wait_until(|| writer.writes_in_flight.available_permits() == MAX_WRITES_IN_FLIGHT).await;
    assert!(!writer.record_with(STALE, || "k".to_string()));
}
