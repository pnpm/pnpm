use crate::RouteContext;
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::path::PathBuf;

#[test]
fn local_package_uses_the_serving_path() {
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://registry.example/base".to_string();
    let context = RouteContext::from_config(&config);
    for (path, package) in [
        ("npm/widget", "widget"),
        ("npm/~internal/widget/-/widget-1.0.0.tgz?download=1", "widget"),
        ("npm/@scope/widget", "@scope/widget"),
        ("npm/~internal/@scope%2Fwidget@1.0.0+r0", "@scope/widget"),
        ("npm/~internal/@scope/widget/-/widget-1.0.0.tgz", "@scope/widget"),
    ] {
        let url = format!("{}/{path}", config.http.public_url);
        assert_eq!(context.local_package(&url).as_deref(), Some(package), "{url}");
    }
    assert_eq!(context.local_package("https://registry.example/base-other/npm/widget"), None);
    assert_eq!(context.local_package("https://other.example/base/npm/widget"), None);
}

#[test]
fn local_fetch_rejects_urls_that_classification_does_not_recognize() {
    use crate::{Footprint, RouteHook};
    use pnpm_network::UpstreamRouteHook;
    use pnpr_policy::Identity;
    use std::sync::{Arc, Mutex};

    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://registry.example/base".to_string();
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.local_package("https://registry.example:443/base/npm/widget").as_deref(),
        Some("widget"),
    );
    let hook = RouteHook::new(
        Arc::new(context),
        Identity::Anonymous,
        Arc::new(Mutex::new(Footprint::default())),
        Arc::from(b"secret".as_slice()),
    )
    .with_local_fetch(Arc::new(|_, _| panic!("unsafe local dispatch")));
    for url in [
        "https://REGISTRY.example/base/npm/widget",
        "https://registry.example/base/npm/~corp/%2e%2e/~other/widget",
        r"https://registry.example/base/npm\~corp/widget",
        "https://registry.example/base/npm/~co\trp/widget",
    ] {
        assert!(
            hook.fetch_local(url, reqwest::header::HeaderMap::new())
                .is_none(),
            "{url}",
        );
    }
}

#[test]
fn denied_own_origin_package_does_not_share_public_or_authorized_metadata_scope() {
    use crate::{Footprint, PrivateAccessDescriptor, RouteHook};
    use pnpm_network::{MetadataCacheScope, UpstreamRouteHook};
    use pnpr_policy::{AccessList, Identity, RuleOverride};
    use std::sync::{Arc, Mutex};

    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://pnpr.example".to_string();
    let context = Arc::new(RouteContext::from_config(&config));
    let secret: Arc<[u8]> = Arc::from(b"server-secret".as_slice());

    let scope_for = |identity: Identity, url: &str, package: &str| {
        RouteHook::new(
            Arc::clone(&context),
            identity,
            Arc::new(Mutex::new(Footprint::default())),
            Arc::clone(&secret),
        )
        .metadata_scope(url, Some(package))
    };

    assert_eq!(
        scope_for(Identity::Anonymous, "https://pnpr.example/npm/ajv", "ajv"),
        MetadataCacheScope::Public,
    );

    let rules = &config.routing.hosted["local"].rules;
    let table = rules.snapshot();
    let alice_only = table
        .rules()
        .iter()
        .map(|rule| {
            let access = Some(AccessList::from_tokens(["alice"]));
            (rule.pattern.clone(), RuleOverride { access, ..RuleOverride::default() })
        })
        .collect();
    rules.replace(
        table
            .with_overrides(Some(AccessList::from_tokens(["alice"])), alice_only)
            .unwrap(),
    );

    let alice_scope = scope_for(Identity::user("alice"), "https://pnpr.example/npm/ajv", "ajv");
    let bob_scope = scope_for(Identity::user("bob"), "https://pnpr.example/npm/ajv", "ajv");
    let anon_scope = scope_for(Identity::Anonymous, "https://pnpr.example/npm/ajv", "ajv");

    assert_eq!(
        alice_scope,
        MetadataCacheScope::Private {
            descriptor_id: PrivateAccessDescriptor::Hosted { policy_id: "local\0ajv".to_string() }
                .digest_id(&secret),
        },
    );
    assert_ne!(bob_scope, MetadataCacheScope::Public);
    assert_ne!(anon_scope, MetadataCacheScope::Public);
    assert_ne!(bob_scope, alice_scope);
    assert_ne!(anon_scope, alice_scope);
}

#[test]
fn footprint_serde_roundtrip_and_extend_preserve_descriptors() {
    use crate::{Footprint, PrivateAccessDescriptor};

    let mut first = Footprint::default();
    first.add(PrivateAccessDescriptor::Hosted { policy_id: "local\0@acme/a".to_string() });
    let mut second = Footprint::default();
    second.add(PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: "deadbeef".to_string(),
        package: Some("@acme/b".to_string()),
    });
    first.extend(&second);

    let encoded = serde_json::to_string(&first).expect("serialize footprint");
    let decoded: Footprint = serde_json::from_str(&encoded).expect("deserialize footprint");
    assert_eq!(decoded, first);
    assert_eq!(decoded.digest(b"secret"), first.digest(b"secret"));
}
