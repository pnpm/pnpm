use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::CustomFetcherPicker;
use crate::{CustomFetcher, HookError};

struct ScriptedFetcher {
    can_fetch: bool,
    response: Value,
    can_fetch_calls: AtomicUsize,
    fetch_calls: AtomicUsize,
    seen_pkg_ids: Mutex<Vec<String>>,
    seen_resolutions: Mutex<Vec<Value>>,
    seen_opts: Mutex<Vec<Value>>,
}

impl ScriptedFetcher {
    fn answering(response: Value) -> Self {
        ScriptedFetcher {
            can_fetch: true,
            response,
            can_fetch_calls: AtomicUsize::new(0),
            fetch_calls: AtomicUsize::new(0),
            seen_pkg_ids: Mutex::new(Vec::new()),
            seen_resolutions: Mutex::new(Vec::new()),
            seen_opts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CustomFetcher for ScriptedFetcher {
    async fn can_fetch(&self, pkg_id: &str, resolution: Value) -> Result<bool, HookError> {
        self.can_fetch_calls.fetch_add(1, Ordering::SeqCst);
        self.seen_pkg_ids.lock().unwrap().push(pkg_id.to_string());
        self.seen_resolutions.lock().unwrap().push(resolution);
        Ok(self.can_fetch)
    }

    async fn fetch(
        &self,
        _pkg_id: &str,
        _resolution: Value,
        opts: Value,
    ) -> Result<Value, HookError> {
        self.fetch_calls.fetch_add(1, Ordering::SeqCst);
        self.seen_opts.lock().unwrap().push(opts);
        Ok(self.response.clone())
    }
}

fn fetch_result() -> Value {
    json!({
        "filesIndex": {
            "package.json": { "integrity": "sha512-abc123", "mode": 420 },
            "index.js": { "integrity": "sha512-def456", "mode": 420 },
        }
    })
}

#[tokio::test]
async fn returns_result_from_first_matching_fetcher() {
    let fetcher = Arc::new(ScriptedFetcher::answering(fetch_result()));
    let picker = CustomFetcherPicker::new(vec![Arc::clone(&fetcher) as Arc<dyn CustomFetcher>]);

    let resolution = json!({ "type": "@custom/registry", "url": "https://example.com/pkg" });
    let opts = json!({ "manifest": { "name": "foo", "version": "1.0.0" } });

    let result = picker.try_fetch("foo@1.0.0", &resolution, &opts).await.unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap(), fetch_result());
    assert_eq!(fetcher.can_fetch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fetcher.fetch_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn returns_none_when_no_fetcher_claims_package() {
    let fetcher = Arc::new(ScriptedFetcher {
        can_fetch: false,
        ..ScriptedFetcher::answering(fetch_result())
    });
    let picker = CustomFetcherPicker::new(vec![Arc::clone(&fetcher) as Arc<dyn CustomFetcher>]);

    let resolution = json!({ "type": "@custom/registry" });
    let opts = json!({});

    let result = picker.try_fetch("foo@1.0.0", &resolution, &opts).await.unwrap();

    assert!(result.is_none());
    assert_eq!(fetcher.can_fetch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fetcher.fetch_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tries_fetchers_in_order_stops_at_first_match() {
    let first =
        Arc::new(ScriptedFetcher { can_fetch: false, ..ScriptedFetcher::answering(json!({})) });
    let second = Arc::new(ScriptedFetcher::answering(fetch_result()));
    let third = Arc::new(ScriptedFetcher::answering(json!({"other": true})));

    let picker = CustomFetcherPicker::new(vec![
        Arc::clone(&first) as Arc<dyn CustomFetcher>,
        Arc::clone(&second) as Arc<dyn CustomFetcher>,
        Arc::clone(&third) as Arc<dyn CustomFetcher>,
    ]);

    let resolution = json!({ "tarball": "https://example.com/foo.tgz" });
    let opts = json!({});

    let result = picker.try_fetch("foo@1.0.0", &resolution, &opts).await.unwrap();

    assert_eq!(result.unwrap(), fetch_result());
    assert_eq!(first.can_fetch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.fetch_calls.load(Ordering::SeqCst), 0);
    assert_eq!(second.can_fetch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second.fetch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        third.can_fetch_calls.load(Ordering::SeqCst),
        0,
        "must not consult fetchers after a match"
    );
}

#[tokio::test]
async fn skips_fetcher_without_can_fetch() {
    let fetcher = Arc::new(NoCanFetchFetcher);
    let picker = CustomFetcherPicker::new(vec![fetcher]);

    let result = picker.try_fetch("foo@1.0.0", &json!({}), &json!({})).await.unwrap();

    assert!(result.is_none());
}

struct NoCanFetchFetcher;

#[async_trait]
impl CustomFetcher for NoCanFetchFetcher {
    fn has_can_fetch(&self) -> bool {
        false
    }

    async fn can_fetch(&self, _: &str, _: Value) -> Result<bool, HookError> {
        panic!("should not be called");
    }

    async fn fetch(&self, _: &str, _: Value, _: Value) -> Result<Value, HookError> {
        panic!("should not be called");
    }
}

#[tokio::test]
async fn empty_picker_returns_none() {
    let picker = CustomFetcherPicker::new(vec![]);
    assert!(picker.is_empty());

    let result = picker.try_fetch("foo@1.0.0", &json!({}), &json!({})).await.unwrap();
    assert!(result.is_none());
}
