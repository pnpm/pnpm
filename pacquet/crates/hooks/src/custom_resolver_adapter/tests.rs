use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};

use async_trait::async_trait;
use pacquet_resolving_resolver_base::{
    CurrentPkg, PkgResolutionId, ResolveOptions, Resolver, WantedDependency,
};

use super::CustomResolverAdapter;
use crate::{CustomResolver, HookError};

struct ScriptedResolver {
    can_resolve: bool,
    response: Value,
    can_resolve_calls: AtomicUsize,
    seen_wanted: Mutex<Vec<Value>>,
    seen_opts: Mutex<Vec<Value>>,
}

impl ScriptedResolver {
    fn answering(response: Value) -> Self {
        ScriptedResolver {
            can_resolve: true,
            response,
            can_resolve_calls: AtomicUsize::new(0),
            seen_wanted: Mutex::new(Vec::new()),
            seen_opts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CustomResolver for ScriptedResolver {
    async fn can_resolve(&self, wanted_dependency: Value) -> Result<bool, HookError> {
        self.can_resolve_calls.fetch_add(1, Ordering::SeqCst);
        self.seen_wanted.lock().unwrap().push(wanted_dependency);
        Ok(self.can_resolve)
    }

    async fn resolve(&self, _: Value, opts: Value) -> Result<Value, HookError> {
        self.seen_opts.lock().unwrap().push(opts);
        Ok(self.response.clone())
    }

    async fn should_refresh_resolution(
        &self,
        _: &pacquet_lockfile::PackageKey,
        _: Value,
    ) -> Result<bool, HookError> {
        Ok(false)
    }
}

fn wanted(alias: &str, bare_specifier: &str) -> WantedDependency {
    WantedDependency {
        alias: Some(alias.to_string()),
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    }
}

fn valid_response() -> Value {
    json!({
        "id": "foo@1.0.0",
        "resolution": { "tarball": "https://example.com/foo-1.0.0.tgz" },
    })
}

#[tokio::test]
async fn returns_typed_result_for_valid_response() {
    let adapter =
        CustomResolverAdapter::new(Arc::new(ScriptedResolver::answering(valid_response())));

    let result = adapter
        .resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default())
        .await
        .unwrap()
        .expect("custom resolver claims the dependency");

    assert_eq!(result.id, PkgResolutionId::from("foo@1.0.0"));
    assert_eq!(result.resolved_via, "custom-resolver");
    assert_eq!(result.alias.as_deref(), Some("foo"));
    assert!(result.manifest.is_none());
}

#[tokio::test]
async fn returns_none_when_can_resolve_is_false() {
    let resolver = Arc::new(ScriptedResolver {
        can_resolve: false,
        ..ScriptedResolver::answering(valid_response())
    });
    let adapter = CustomResolverAdapter::new(Arc::clone(&resolver) as Arc<dyn CustomResolver>);

    let result =
        adapter.resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default()).await.unwrap();

    assert!(result.is_none());
    assert!(resolver.seen_opts.lock().unwrap().is_empty(), "resolve must not be called");
}

#[tokio::test]
async fn caches_can_resolve_per_alias_and_specifier() {
    let resolver = Arc::new(ScriptedResolver::answering(valid_response()));
    let adapter = CustomResolverAdapter::new(Arc::clone(&resolver) as Arc<dyn CustomResolver>);
    let opts = ResolveOptions::default();

    adapter.resolve(&wanted("foo", "custom:foo"), &opts).await.unwrap();
    adapter.resolve(&wanted("foo", "custom:foo"), &opts).await.unwrap();
    assert_eq!(resolver.can_resolve_calls.load(Ordering::SeqCst), 1);

    adapter.resolve(&wanted("foo", "custom:other"), &opts).await.unwrap();
    assert_eq!(resolver.can_resolve_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn errors_when_id_is_missing() {
    let adapter = CustomResolverAdapter::new(Arc::new(ScriptedResolver::answering(json!({
        "resolution": { "tarball": "https://example.com/foo-1.0.0.tgz" },
    }))));

    let err = adapter
        .resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default())
        .await
        .expect_err("missing id must fail");
    assert!(err.to_string().contains("'id'"), "got: {err}");
}

#[tokio::test]
async fn errors_when_resolution_is_missing() {
    let adapter = CustomResolverAdapter::new(Arc::new(ScriptedResolver::answering(
        json!({ "id": "foo@1.0.0" }),
    )));

    let err = adapter
        .resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default())
        .await
        .expect_err("missing resolution must fail");
    assert!(err.to_string().contains("'resolution'"), "got: {err}");
}

#[tokio::test]
async fn errors_when_resolution_has_invalid_shape() {
    let adapter = CustomResolverAdapter::new(Arc::new(ScriptedResolver::answering(json!({
        "id": "foo@1.0.0",
        "resolution": "not-an-object",
    }))));

    let err = adapter
        .resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default())
        .await
        .expect_err("invalid resolution must fail");
    assert!(err.to_string().contains("invalid resolution"), "got: {err}");
}

#[tokio::test]
async fn manifest_passes_through() {
    let adapter = CustomResolverAdapter::new(Arc::new(ScriptedResolver::answering(json!({
        "id": "foo@1.0.0",
        "resolution": { "tarball": "https://example.com/foo-1.0.0.tgz" },
        "manifest": { "name": "foo", "version": "1.0.0" },
    }))));

    let result = adapter
        .resolve(&wanted("foo", "custom:foo"), &ResolveOptions::default())
        .await
        .unwrap()
        .expect("resolved");

    let manifest = result.manifest.expect("manifest survives the adapter");
    assert_eq!(*manifest, json!({ "name": "foo", "version": "1.0.0" }));
}

#[tokio::test]
async fn sends_upstream_payload_shapes() {
    let resolver = Arc::new(ScriptedResolver::answering(valid_response()));
    let adapter = CustomResolverAdapter::new(Arc::clone(&resolver) as Arc<dyn CustomResolver>);
    let wanted_dependency = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("custom:foo".to_string()),
        injected: Some(true),
        prev_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        project_dir: "/repo/pkg".into(),
        lockfile_dir: "/repo".into(),
        current_pkg: Some(CurrentPkg {
            id: PkgResolutionId::from("foo@1.0.0"),
            name: Some("foo".to_string()),
            version: Some("1.0.0".to_string()),
            resolution: serde_json::from_value(
                json!({ "tarball": "https://example.com/foo-1.0.0.tgz" }),
            )
            .unwrap(),
            published_at: None,
        }),
        ..ResolveOptions::default()
    };

    adapter.resolve(&wanted_dependency, &opts).await.unwrap();

    let seen_wanted = resolver.seen_wanted.lock().unwrap();
    assert_eq!(
        seen_wanted.first().unwrap(),
        &json!({
            "alias": "foo",
            "bareSpecifier": "custom:foo",
            "injected": true,
            "optional": null,
            "prevSpecifier": "^1.0.0",
        }),
    );
    let seen_opts = resolver.seen_opts.lock().unwrap();
    assert_eq!(
        seen_opts.first().unwrap(),
        &json!({
            "lockfileDir": "/repo",
            "projectDir": "/repo/pkg",
            "preferredVersions": {},
            "currentPkg": {
                "id": "foo@1.0.0",
                "name": "foo",
                "version": "1.0.0",
                "resolution": { "tarball": "https://example.com/foo-1.0.0.tgz" },
            },
        }),
    );
}
