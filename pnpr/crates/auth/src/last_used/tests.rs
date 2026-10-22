use super::{LAST_USED_GRANULARITY_SECS, LastUsedWriter, MIN_CLAIMS_BEFORE_PRUNE};
use crate::{NewToken, Result, TokenBackend, TokenRecord};
use async_trait::async_trait;
use pnpr_error::RegistryError;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// A token store that only remembers which keys were touched, and when.
#[derive(Default)]
struct TouchLog {
    touches: Mutex<Vec<(String, u64)>>,
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

fn writer() -> (LastUsedWriter, Arc<TouchLog>) {
    let log = Arc::new(TouchLog::default());
    (LastUsedWriter::new(Arc::clone(&log) as Arc<dyn TokenBackend>), log)
}

const STALE: u64 = 1_000;
const NOW: u64 = STALE + LAST_USED_GRANULARITY_SECS;

#[test]
fn a_fresh_stored_time_is_not_written_again() {
    let (writer, _) = writer();
    assert!(!writer.claim("k", NOW - 1, NOW));
    assert!(!writer.claim("k", STALE, STALE + LAST_USED_GRANULARITY_SECS - 1));
}

#[test]
fn a_stale_stored_time_is_written_once_per_interval_per_token() {
    let (writer, _) = writer();
    assert!(writer.claim("k", STALE, NOW));
    // The stored time is still stale until the write lands (or a replica
    // syncs it back), so the claim alone must hold the interval.
    assert!(!writer.claim("k", STALE, NOW));
    assert!(!writer.claim("k", STALE, NOW + LAST_USED_GRANULARITY_SECS - 1));
    assert!(writer.claim("other", STALE, NOW));
    assert!(writer.claim("k", STALE, NOW + LAST_USED_GRANULARITY_SECS));
}

#[test]
fn expired_claims_are_pruned() {
    let (writer, _) = writer();
    for index in 0..MIN_CLAIMS_BEFORE_PRUNE {
        assert!(writer.claim(&format!("old-{index}"), STALE, NOW));
    }
    assert!(writer.claim("new", STALE, NOW + LAST_USED_GRANULARITY_SECS));
    let claims = writer.claims.lock().unwrap();
    assert_eq!(claims.written_at.len(), 1);
    assert!(claims.written_at.contains_key("new"));
}

#[tokio::test]
async fn a_recorded_use_reaches_the_store_off_the_request_path() {
    let (writer, log) = writer();
    writer.record("k", STALE);
    tokio::time::timeout(Duration::from_secs(5), async {
        while log.touches.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the spawned write lands");
    let touches = log.touches.lock().unwrap();
    assert_eq!(touches.len(), 1);
    assert_eq!(touches[0].0, "k");
    assert!(touches[0].1 > STALE, "written with the current time, not the stored one");
}
