use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};

use pacquet_hooks::{CustomResolver, HookError};
use pacquet_lockfile::{Lockfile, PackageKey};

use super::check_custom_resolver_force_resolve;

/// Observable state shared between a [`MockResolver`] and the test that
/// built it. The resolver is passed into the checker by value, so the
/// test keeps an `Arc` handle to assert on the calls it recorded.
#[derive(Clone, Default)]
struct Recorder {
    call_count: Arc<AtomicUsize>,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

struct MockResolver {
    has_should_refresh_resolution: bool,
    refresh_outcome: Result<bool, HookError>,
    recorder: Recorder,
}

impl MockResolver {
    fn refreshing(refresh: bool) -> Self {
        MockResolver {
            has_should_refresh_resolution: true,
            refresh_outcome: Ok(refresh),
            recorder: Recorder::default(),
        }
    }

    fn without_hook() -> Self {
        MockResolver { has_should_refresh_resolution: false, ..Self::refreshing(false) }
    }

    fn failing(message: &str) -> Self {
        MockResolver {
            refresh_outcome: Err(HookError::Execution {
                pnpmfile: ".pnpmfile.cjs".to_string(),
                message: message.to_string(),
            }),
            ..Self::refreshing(false)
        }
    }
}

impl CustomResolver for MockResolver {
    fn has_should_refresh_resolution(&self) -> bool {
        self.has_should_refresh_resolution
    }

    async fn can_resolve(&self, _: Value) -> Result<bool, HookError> {
        Ok(false)
    }

    async fn resolve(&self, _: Value, _: Value) -> Result<Value, HookError> {
        Ok(Value::Null)
    }

    async fn should_refresh_resolution(
        &self,
        dep_path: &PackageKey,
        pkg_snapshot: Value,
    ) -> Result<bool, HookError> {
        self.recorder.call_count.fetch_add(1, Ordering::SeqCst);
        self.recorder.calls.lock().unwrap().push((dep_path.to_string(), pkg_snapshot));
        self.refresh_outcome.clone()
    }
}

fn lockfile(value: Value) -> Lockfile {
    serde_json::from_value(value).expect("valid lockfile JSON")
}

fn lockfile_with_one_package() -> Lockfile {
    lockfile(json!({
        "lockfileVersion": "9.0",
        "packages": {
            "test-pkg@1.0.0": {
                "resolution": {
                    "tarball": "http://example.com/test-pkg-1.0.0.tgz",
                    "integrity": "sha512-7vPSqv9MKvOTcGZSjm9ZcMnxiTYbXrAJZHzAi3GUv/tSZdLZE6richEuZ+EHAJRm6q2eOBBdc6TIuQfRGCQTzg==",
                },
            },
        },
        "snapshots": {
            "test-pkg@1.0.0": {},
        },
    }))
}

#[tokio::test]
async fn returns_false_when_no_custom_resolvers() {
    let result =
        check_custom_resolver_force_resolve::<MockResolver>(&[], &lockfile_with_one_package())
            .await
            .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn returns_false_when_lockfile_has_no_snapshots() {
    let empty = lockfile(json!({ "lockfileVersion": "9.0" }));
    let result = check_custom_resolver_force_resolve(&[MockResolver::refreshing(true)], &empty)
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn skips_resolvers_without_the_hook() {
    let resolver = MockResolver::without_hook();
    let recorder = resolver.recorder.clone();

    let result = check_custom_resolver_force_resolve(&[resolver], &lockfile_with_one_package())
        .await
        .unwrap();

    assert!(!result);
    assert_eq!(recorder.call_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn returns_false_when_hook_returns_false() {
    let result = check_custom_resolver_force_resolve(
        &[MockResolver::refreshing(false)],
        &lockfile_with_one_package(),
    )
    .await
    .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn returns_true_when_hook_returns_true() {
    let result = check_custom_resolver_force_resolve(
        &[MockResolver::refreshing(true)],
        &lockfile_with_one_package(),
    )
    .await
    .unwrap();
    assert!(result);
}

#[tokio::test]
async fn returns_true_when_any_resolver_among_multiple_returns_true() {
    let result = check_custom_resolver_force_resolve(
        &[MockResolver::refreshing(false), MockResolver::refreshing(true)],
        &lockfile_with_one_package(),
    )
    .await
    .unwrap();
    assert!(result);
}

#[tokio::test]
async fn propagates_hook_errors() {
    let result = check_custom_resolver_force_resolve(
        &[MockResolver::refreshing(false), MockResolver::failing("resolver crashed")],
        &lockfile_with_one_package(),
    )
    .await;
    let err = result.expect_err("hook error must propagate");
    assert!(err.to_string().contains("resolver crashed"), "got: {err}");
}

#[tokio::test]
async fn passes_dep_path_and_merged_package_snapshot() {
    let resolver = MockResolver::refreshing(false);
    let recorder = resolver.recorder.clone();
    let lockfile = lockfile(json!({
        "lockfileVersion": "9.0",
        "packages": {
            "test-pkg@1.0.0": {
                "resolution": {
                    "tarball": "http://example.com/test-pkg-1.0.0.tgz",
                    "integrity": "sha512-7vPSqv9MKvOTcGZSjm9ZcMnxiTYbXrAJZHzAi3GUv/tSZdLZE6richEuZ+EHAJRm6q2eOBBdc6TIuQfRGCQTzg==",
                },
            },
        },
        "snapshots": {
            "test-pkg@1.0.0(peer@2.0.0)": {
                "dependencies": { "peer": "2.0.0" },
            },
        },
    }));

    check_custom_resolver_force_resolve(&[resolver], &lockfile).await.unwrap();

    let calls = recorder.calls.lock().unwrap();
    let (dep_path, snapshot) = calls.first().expect("hook called once");
    assert_eq!(dep_path, "test-pkg@1.0.0(peer@2.0.0)");
    assert_eq!(snapshot["resolution"]["tarball"], json!("http://example.com/test-pkg-1.0.0.tgz"));
    assert_eq!(snapshot["dependencies"]["peer"], json!("2.0.0"));
}
