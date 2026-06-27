use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use pacquet_network::{MetadataCacheScope, UpstreamRouteHook};

use super::{Footprint, PrivateAccessDescriptor, RouteClass, RouteContext, RouteHook};
use crate::{
    config::{Config, PublicRoute, UpstreamAlias},
    policy::{AccessList, Identity, PackagePolicies},
};

fn base_config() -> Config {
    Config::proxy("127.0.0.1:7677".parse::<SocketAddr>().unwrap(), PathBuf::from("/tmp/pnpr-route"))
}

fn anon() -> Identity {
    Identity::Anonymous
}

fn user(name: &str) -> Identity {
    Identity::user(name)
}

fn alias(registry: &str, package: Option<&str>, access: &str, generation: u64) -> UpstreamAlias {
    UpstreamAlias {
        registry: registry.to_string(),
        package: package.map(str::to_string),
        authorization: "Bearer alias-secret".to_string(),
        access: AccessList::parse(access),
        generation,
    }
}

#[test]
fn hmac_sha256_matches_rfc4231_case1() {
    // RFC 4231 test case 1: 20-byte 0x0b key, "Hi There".
    let mac = super::hmac_sha256(&[0x0b; 20], b"Hi There");
    assert_eq!(
        super::hex(&mac),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
    );
}

#[test]
fn unscoped_npmjs_is_public_scoped_is_not() {
    let context = RouteContext::from_config(&base_config());
    assert_eq!(
        context.classify(&user("alice"), "https://registry.npmjs.org/lodash", Some("lodash")),
        RouteClass::Public,
    );
    // npm allows private scoped packages, so a scoped npmjs name is not
    // public under the built-in route — it needs an explicit rule.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://registry.npmjs.org/@babel%2fcore",
            Some("@babel/core"),
        ),
        RouteClass::Unknown,
    );
}

#[test]
fn disabling_the_builtin_makes_unscoped_npmjs_private() {
    let mut config = base_config();
    config.route_policy.npmjs_unscoped_public = false;
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.classify(&user("alice"), "https://registry.npmjs.org/lodash", Some("lodash")),
        RouteClass::Unknown,
    );
}

#[test]
fn custom_registry_is_unknown_until_declared_public() {
    let mut config = base_config();
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.classify(&user("alice"), "https://custom.registry.example/lodash", Some("lodash"),),
        RouteClass::Unknown,
    );

    config.route_policy.public.push(PublicRoute {
        registry: Some("https://custom.registry.example/".to_string()),
        package: None,
    });
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.classify(&user("alice"), "https://custom.registry.example/lodash", Some("lodash"),),
        RouteClass::Public,
    );
}

#[test]
fn operator_declared_public_route_matches_scope() {
    let mut config = base_config();
    config.route_policy.public.push(PublicRoute {
        registry: Some("https://registry.npmjs.org/".to_string()),
        package: Some("@babel/*".to_string()),
    });
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.classify(&anon(), "https://registry.npmjs.org/@babel%2fcore", Some("@babel/core")),
        RouteClass::Public,
    );
}

#[test]
fn proxied_alias_requires_authorization() {
    let mut config = base_config();
    config.upstream_aliases.insert(
        "corp".to_string(),
        alias("https://npm.corp.example/", Some("@acme/*"), "$authenticated", 2),
    );
    let context = RouteContext::from_config(&config);
    let url = "https://npm.corp.example/@acme%2fwidget";

    assert_eq!(
        context.classify(&user("alice"), url, Some("@acme/widget")),
        RouteClass::Proxied { alias: "corp".to_string(), generation: 2 },
    );
    // An unauthorized caller cannot select the alias, so the route is a
    // non-shareable unknown rather than the alias's private namespace.
    assert_eq!(context.classify(&anon(), url, Some("@acme/widget")), RouteClass::Unknown);
    // A package outside the alias's pattern doesn't match it.
    assert_eq!(
        context.classify(&user("alice"), "https://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Unknown,
    );
}

#[test]
fn proxied_alias_accepts_configured_group_identity() {
    let mut config = base_config();
    config.groups.add_user_to_group("alice", "platform");
    config.upstream_aliases.insert(
        "corp".to_string(),
        alias("https://npm.corp.example/", Some("@acme/*"), "platform", 2),
    );
    let context = RouteContext::from_config(&config);
    let url = "https://npm.corp.example/@acme%2fwidget";

    assert_eq!(
        context.classify(&config.identity_for_user("alice"), url, Some("@acme/widget")),
        RouteClass::Proxied { alias: "corp".to_string(), generation: 2 },
    );
    assert_eq!(
        context.classify(&config.identity_for_user("bob"), url, Some("@acme/widget")),
        RouteClass::Unknown,
    );
}

#[test]
fn hosted_route_follows_package_access_policy() {
    let mut config = base_config();
    config.public_url = "https://pnpr.example/".to_string();
    config.policies = PackagePolicies::registry_mock_defaults();
    let context = RouteContext::from_config(&config);

    // `@private/*` requires auth: private+gated for an authorized caller,
    // fail-closed for anonymous.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://pnpr.example/@private%2fpkg",
            Some("@private/pkg")
        ),
        RouteClass::Hosted { policy_id: "@private/pkg".to_string() },
    );
    assert_eq!(
        context.classify(&anon(), "https://pnpr.example/@private%2fpkg", Some("@private/pkg")),
        RouteClass::Unknown,
    );
    // A package the hosted policy opens to everyone is public.
    assert_eq!(
        context.classify(&anon(), "https://pnpr.example/lodash", Some("lodash")),
        RouteClass::Public,
    );
}

#[test]
fn footprint_digest_is_stable_and_namespaced() {
    let mut footprint = Footprint::default();
    assert!(footprint.is_public());
    assert!(footprint.is_shareable());
    assert_eq!(footprint.digest(b"secret"), None);

    footprint.add(PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 1 });
    assert!(!footprint.is_public());
    let digest = footprint.digest(b"secret").expect("private footprint has a digest");

    // Rotating the alias generation moves to a new namespace.
    let mut rotated = Footprint::default();
    rotated.add(PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 2 });
    assert_ne!(digest, rotated.digest(b"secret").unwrap());

    // A different server secret yields a different (non-correlatable) key.
    assert_ne!(digest, footprint.digest(b"other-secret").unwrap());

    // The digest is order-independent (BTreeSet union).
    let mut one_order = Footprint::default();
    one_order.add(PrivateAccessDescriptor::Alias { alias: "x".to_string(), generation: 1 });
    one_order.add(PrivateAccessDescriptor::Hosted { policy_id: "@p/*".to_string() });
    let mut other_order = Footprint::default();
    other_order.add(PrivateAccessDescriptor::Hosted { policy_id: "@p/*".to_string() });
    other_order.add(PrivateAccessDescriptor::Alias { alias: "x".to_string(), generation: 1 });
    assert_eq!(one_order.digest(b"k"), other_order.digest(b"k"));
}

#[test]
fn footprint_unknown_private_is_not_shareable() {
    let mut footprint = Footprint::default();
    footprint.mark_unknown_private();
    assert!(!footprint.is_public());
    assert!(!footprint.is_shareable());
    // No descriptor was recorded, so there is nothing to key on.
    assert_eq!(footprint.digest(b"secret"), None);
}

#[test]
fn route_hook_records_routes_and_returns_alias_credential() {
    let mut config = base_config();
    config
        .upstream_aliases
        .insert("corp".to_string(), alias("https://npm.corp.example/", None, "$authenticated", 1));
    let footprint = std::sync::Arc::new(std::sync::Mutex::new(Footprint::default()));
    let hook = RouteHook::new(
        RouteContext::from_config(&config),
        user("alice"),
        std::sync::Arc::clone(&footprint),
        std::sync::Arc::from(b"secret".as_slice()),
    );

    // Public fetch: no upstream credential, no private footprint entry.
    assert_eq!(hook.authorize("https://registry.npmjs.org/lodash", Some("lodash")), None);
    // Private proxied fetch: the alias credential is returned and recorded.
    assert_eq!(
        hook.authorize("https://npm.corp.example/@acme%2fwidget", Some("@acme/widget")),
        Some("Bearer alias-secret".to_string()),
    );

    let footprint = footprint.lock().unwrap();
    assert!(!footprint.is_public());
    assert!(footprint.digest(b"secret").is_some());
}

#[test]
fn metadata_scope_maps_route_classes() {
    let mut config = base_config();
    config
        .upstream_aliases
        .insert("corp".to_string(), alias("https://npm.corp.example/", None, "$authenticated", 3));
    let footprint = std::sync::Arc::new(std::sync::Mutex::new(Footprint::default()));
    let hook = RouteHook::new(
        RouteContext::from_config(&config),
        user("alice"),
        std::sync::Arc::clone(&footprint),
        std::sync::Arc::from(b"server-secret".as_slice()),
    );

    // Public route → the shared global mirror.
    assert_eq!(
        hook.metadata_scope("https://registry.npmjs.org/lodash", Some("lodash")),
        MetadataCacheScope::Public,
    );
    // Authorized proxied route → a descriptor-scoped private mirror.
    let widget =
        hook.metadata_scope("https://npm.corp.example/@acme%2fwidget", Some("@acme/widget"));
    let MetadataCacheScope::Private { descriptor_id } = widget else {
        panic!("proxied route should be private, got {widget:?}");
    };
    assert!(!descriptor_id.is_empty());
    // The id is the HMAC of the alias descriptor; it must be stable.
    assert_eq!(
        descriptor_id,
        PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 3 }
            .digest_id(b"server-secret"),
    );
    // Unknown/private with no usable credential → bypass (request-local).
    assert_eq!(
        hook.metadata_scope("https://private.unknown.example/secret", Some("secret")),
        MetadataCacheScope::Bypass,
    );

    // metadata_scope is read-only: classifying must not grow the footprint.
    assert!(footprint.lock().unwrap().is_public());
}

#[test]
fn authorized_alias_users_share_metadata_scope() {
    let mut config = base_config();
    config
        .upstream_aliases
        .insert("corp".to_string(), alias("https://npm.corp.example/", None, "$authenticated", 3));
    let context = RouteContext::from_config(&config);
    let secret = Arc::from(b"server-secret".as_slice());
    let alice = RouteHook::new(
        context.clone(),
        user("alice"),
        Arc::new(Mutex::new(Footprint::default())),
        Arc::clone(&secret),
    );
    let bob = RouteHook::new(
        context,
        user("bob"),
        Arc::new(Mutex::new(Footprint::default())),
        Arc::clone(&secret),
    );
    let url = "https://npm.corp.example/@acme%2fwidget";

    let alice_scope = alice.metadata_scope(url, Some("@acme/widget"));
    let bob_scope = bob.metadata_scope(url, Some("@acme/widget"));
    assert_eq!(alice_scope, bob_scope);

    let MetadataCacheScope::Private { descriptor_id } = alice_scope else {
        panic!("authorized alias route should be private");
    };
    assert_eq!(
        descriptor_id,
        PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 3 }
            .digest_id(b"server-secret"),
    );
}

#[test]
fn descriptor_digest_id_depends_on_secret() {
    let descriptor = PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 1 };
    assert_ne!(descriptor.digest_id(b"secret-a"), descriptor.digest_id(b"secret-b"));
    // Generation rotation moves the namespace.
    let rotated = PrivateAccessDescriptor::Alias { alias: "corp".to_string(), generation: 2 };
    assert_ne!(descriptor.digest_id(b"secret-a"), rotated.digest_id(b"secret-a"));
}
