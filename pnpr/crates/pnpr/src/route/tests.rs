use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use pacquet_network::{MetadataCacheScope, UpstreamRouteHook};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::{Footprint, PrivateAccessDescriptor, RouteClass, RouteContext, RouteHook};
use crate::{
    config::{Config, PublicRoute, UplinkConfig},
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

#[test]
fn strip_url_credentials_removes_inline_userinfo() {
    use super::strip_url_credentials;
    assert_eq!(
        strip_url_credentials("https://user:pass@cdn.example/acme-1.0.0.tgz"),
        "https://cdn.example/acme-1.0.0.tgz",
    );
    assert_eq!(
        strip_url_credentials("https://token@cdn.example/x.tgz?a=1"),
        "https://cdn.example/x.tgz?a=1",
    );
    // No userinfo / no scheme: returned unchanged.
    assert_eq!(
        strip_url_credentials("https://registry.npmjs.org/acme/-/acme-1.0.0.tgz"),
        "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz",
    );
    assert_eq!(strip_url_credentials("^1.0.0"), "^1.0.0");
}

#[test]
fn sanitize_registry_tarball_url_drops_userinfo_query_and_fragment() {
    use super::sanitize_registry_tarball_url;
    // A presigned/tokenized upstream URL loses its token.
    assert_eq!(
        sanitize_registry_tarball_url(
            "https://cdn.example/acme-1.0.0.tgz?X-Amz-Signature=secret&token=abc",
        ),
        "https://cdn.example/acme-1.0.0.tgz",
    );
    assert_eq!(
        sanitize_registry_tarball_url("https://user:pass@cdn.example/x.tgz#frag"),
        "https://cdn.example/x.tgz",
    );
    // A clean canonical npm URL is unchanged.
    assert_eq!(
        sanitize_registry_tarball_url("https://registry.npmjs.org/acme/-/acme-1.0.0.tgz"),
        "https://registry.npmjs.org/acme/-/acme-1.0.0.tgz",
    );
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
fn npmjs_host_is_public_including_scoped() {
    let context = RouteContext::from_config(&base_config());
    assert_eq!(
        context.classify(&user("alice"), "https://registry.npmjs.org/lodash", Some("lodash")),
        RouteClass::Public,
    );
    // The npmjs host is public at the host level: an anonymous fetch returns
    // only public content (a private scoped package 404s), so a scoped npmjs
    // name resolves as public and globally shareable too.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://registry.npmjs.org/@babel%2fcore",
            Some("@babel/core"),
        ),
        RouteClass::Public,
    );
}

#[test]
fn allows_registry_is_a_default_deny_allowlist() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    config.route_policy.public.push(PublicRoute {
        registry: Some("https://public.mirror.example/".to_string()),
        package: None,
    });
    let context = RouteContext::from_config(&config);

    // Allowed: the built-in npm host, a declared public route, a uplink
    // origin, and pnpr's own origin (its `/~<uplink>/` endpoints).
    assert!(context.allows_registry("https://registry.npmjs.org/"));
    assert!(context.allows_registry("https://public.mirror.example/@scope/pkg"));
    assert!(context.allows_registry("https://npm.corp.example/@acme/widget"));
    assert!(context.allows_registry(&format!("{}/~corp/@acme/widget", config.public_url)));

    // Rejected: cloud instance metadata and any other off-allowlist host.
    assert!(!context.allows_registry("http://169.254.169.254/"));
    assert!(!context.allows_registry("https://evil.example/"));
    assert!(!context.allows_registry("not a url"));

    // Rejected: a `..` segment that could escape a path-scoped prefix match
    // (here pnpr's own `/~corp/` endpoint prefix) onto a sibling path.
    assert!(!context.allows_registry(&format!("{}/~corp/../admin", config.public_url)));
    assert!(!context.allows_registry("https://npm.corp.example/../etc"));
}

#[test]
fn the_builtin_npmjs_route_is_always_allowlisted_and_public() {
    // Even a deployment that declares no uplinks and no public routes still
    // resolves from the official npm registry: it's a built-in public route.
    let mut config = base_config();
    config.uplinks.clear();
    let context = RouteContext::from_config(&config);

    assert!(context.allows_registry("https://registry.npmjs.org/lodash"));
    assert_eq!(
        context.classify(&user("alice"), "https://registry.npmjs.org/lodash", Some("lodash")),
        RouteClass::Public,
    );
    // Host-level: scoped npmjs names are public too.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://registry.npmjs.org/@babel%2fcore",
            Some("@babel/core"),
        ),
        RouteClass::Public,
    );
}

#[test]
fn custom_registry_is_off_allowlist_until_declared_public() {
    let mut config = base_config();
    let context = RouteContext::from_config(&config);
    assert!(!context.allows_registry("https://custom.registry.example/lodash"));

    config.route_policy.public.push(PublicRoute {
        registry: Some("https://custom.registry.example/".to_string()),
        package: None,
    });
    let context = RouteContext::from_config(&config);
    assert!(context.allows_registry("https://custom.registry.example/lodash"));
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
fn public_route_with_an_invalid_field_fails_closed_instead_of_matching_all() {
    // A typo'd registry URL must not collapse to a match-any public rule
    // that would classify a private registry's packages as Public.
    let mut config = base_config();
    config.uplinks.clear();
    config.route_policy.public.push(PublicRoute {
        registry: Some("not a url".to_string()),
        package: Some("@public/*".to_string()),
    });
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.classify(&anon(), "https://npm.corp.example/@secret%2fpkg", Some("@secret/pkg")),
        RouteClass::Public,
        "the dropped rule leaves classification to the anonymous fall-through, not a match-all",
    );
    // The dropped rule contributes nothing to the allowlist either, so the
    // private registry is rejected at the request boundary.
    assert!(!context.allows_registry("https://npm.corp.example/@secret/pkg"));

    // An invalid package glob drops the rule the same way.
    let mut config = base_config();
    config.uplinks.clear();
    config.route_policy.public.push(PublicRoute {
        registry: Some("https://npm.corp.example/".to_string()),
        package: Some("[".to_string()),
    });
    let context = RouteContext::from_config(&config);
    assert!(!context.allows_registry("https://npm.corp.example/@secret/pkg"));
}

/// The standard bearer token every test uplink uses, and the credential digest
/// it produces — what a `RouteClass::Proxied` / `PrivateAccessDescriptor::Alias`
/// for such an uplink carries.
const UPLINK_TOKEN: &str = "Bearer uplink-secret";

fn corp_credential() -> String {
    super::credential_digest(UPLINK_TOKEN)
}

/// A *different* credential digest, standing in for the same uplink after its
/// upstream credential has been rotated.
fn rotated_credential() -> String {
    super::credential_digest("Bearer rotated-secret")
}

fn uplink_with_access(registry: &str, access: &str) -> UplinkConfig {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer uplink-secret"));
    let mut uplink = UplinkConfig::with_defaults(registry.to_string(), headers);
    uplink.access = Some(AccessList::parse(access));
    uplink
}

#[test]
fn uplink_with_access_is_a_proxied_route_matched_by_origin() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);

    // An authorized caller selects the uplink as a proxied route.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://npm.corp.example/@acme%2fwidget",
            Some("@acme/widget")
        ),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
    // Routing is by registry origin, not a package glob, so any package on
    // that origin matches the same uplink.
    assert_eq!(
        context.classify(&user("alice"), "https://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
    // An unauthorized caller cannot select it: it gets no managed credential
    // (an anonymous public fetch), which the upstream rejects if the package
    // is private.
    assert_eq!(
        context.classify(&anon(), "https://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Public,
    );
}

#[test]
fn uplink_credential_is_not_attached_over_a_mismatched_scheme() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);

    // An https fetch matches the https uplink and gets the managed credential.
    assert_eq!(
        context.classify(&user("alice"), "https://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
    // A plain-http fetch to the same origin must NOT receive the https uplink's
    // server-owned token (nerf-darting strips the scheme); it falls through to
    // an anonymous public fetch instead.
    assert_eq!(
        context.classify(&user("alice"), "http://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Public,
    );
}

#[test]
fn self_uplink_endpoint_url_classifies_as_proxied_for_authorized_caller() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);
    // A request to pnpr's own `/~corp/` endpoint resolves through the corp
    // uplink, using its current credential (the URL carries none).
    let url = format!("{}/~corp/@acme%2fwidget", config.public_url);
    assert_eq!(
        context.classify(&user("alice"), &url, Some("@acme/widget")),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
    // An unauthorized caller gets no managed credential: a `/~<uplink>/` URL
    // is an uplink endpoint, never a hosted package, so it does not fall
    // through to the hosted-package policy; the anonymous fetch the endpoint
    // itself rejects is the fail-closed point.
    assert_eq!(context.classify(&anon(), &url, Some("@acme/widget")), RouteClass::Public);
    // An unknown uplink name is treated the same way.
    let ghost = format!("{}/~ghost/@acme%2fwidget", config.public_url);
    assert_eq!(context.classify(&user("alice"), &ghost, Some("@acme/widget")), RouteClass::Public);
}

#[test]
fn self_endpoint_recognized_when_pnpr_is_served_under_a_path_prefix() {
    let mut config = base_config();
    // pnpr deployed behind a reverse proxy under a `/pnpr/` sub-path.
    config.public_url = "https://host.example/pnpr/".to_string();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);
    // The path-preserving hosted prefix still recognizes the `/pnpr/~corp/`
    // endpoint instead of dropping the `/pnpr` path and misclassifying it.
    assert_eq!(
        context.classify(
            &user("alice"),
            "https://host.example/pnpr/~corp/@acme%2fwidget",
            Some("@acme/widget"),
        ),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
}

#[test]
fn uplink_without_access_is_an_anonymous_route() {
    let mut config = base_config();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer uplink-secret"));
    // A plain proxy uplink that does not declare `access:` is never offered
    // as a resolver private-route credential, but its origin is still
    // allowlisted (a configured registry) and fetched anonymously.
    config.uplinks.insert(
        "mirror".to_string(),
        UplinkConfig::with_defaults("https://npm.corp.example/".to_string(), headers),
    );
    let context = RouteContext::from_config(&config);
    assert!(context.allows_registry("https://npm.corp.example/lodash"));
    assert_eq!(
        context.classify(&user("alice"), "https://npm.corp.example/lodash", Some("lodash")),
        RouteClass::Public,
    );
}

#[test]
fn proxied_alias_accepts_configured_group_identity() {
    let mut config = base_config();
    config.groups.add_user_to_group("alice", "platform");
    config
        .uplinks
        .insert("corp".to_string(), uplink_with_access("https://npm.corp.example/", "platform"));
    let context = RouteContext::from_config(&config);
    let url = "https://npm.corp.example/@acme%2fwidget";

    assert_eq!(
        context.classify(&config.identity_for_user("alice"), url, Some("@acme/widget")),
        RouteClass::Proxied { alias: "corp".to_string(), credential_digest: corp_credential() },
    );
    // A caller outside the alias's group gets no managed credential.
    assert_eq!(
        context.classify(&config.identity_for_user("bob"), url, Some("@acme/widget")),
        RouteClass::Public,
    );
}

#[test]
fn hosted_route_follows_package_access_policy() {
    let mut config = base_config();
    config.public_url = "https://pnpr.example/".to_string();
    config.policies = PackagePolicies::registry_mock_defaults();
    let context = RouteContext::from_config(&config);

    // `@private/*` requires auth: private+gated for an authorized caller. An
    // anonymous caller gets no managed credential (the hosted-serving
    // endpoint re-checks the policy and rejects it), so it never matches the
    // private hosted entry.
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
        RouteClass::Public,
    );
    // A package the hosted policy opens to everyone is public.
    assert_eq!(
        context.classify(&anon(), "https://pnpr.example/lodash", Some("lodash")),
        RouteClass::Public,
    );
}

#[test]
fn overlapping_uplink_access_reuses_only_the_selected_alias() {
    let mut config = base_config();
    // Two uplinks serving the same origin; `primary` is declared first, so
    // `select_alias` picks it for a caller authorized for both.
    config.uplinks.insert(
        "primary".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    config.uplinks.insert(
        "secondary".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);

    let mut via_primary = Footprint::default();
    via_primary.add(PrivateAccessDescriptor::Alias {
        alias: "primary".to_string(),
        credential_digest: corp_credential(),
    });
    assert!(via_primary.allows(&context, &user("alice")));

    // A lockfile routed through `secondary` must NOT be replayed for alice,
    // even though she is authorized for it — she resolves through `primary`.
    let mut via_secondary = Footprint::default();
    via_secondary.add(PrivateAccessDescriptor::Alias {
        alias: "secondary".to_string(),
        credential_digest: corp_credential(),
    });
    assert!(!via_secondary.allows(&context, &user("alice")));
}

#[test]
fn footprint_digest_is_stable_and_namespaced() {
    let mut footprint = Footprint::default();
    assert!(footprint.is_public());
    assert_eq!(footprint.digest(b"secret"), None);

    footprint.add(PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: corp_credential(),
    });
    assert!(!footprint.is_public());
    let digest = footprint.digest(b"secret").expect("private footprint has a digest");

    // Rotating the alias's credential moves to a new namespace.
    let mut rotated = Footprint::default();
    rotated.add(PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: rotated_credential(),
    });
    assert_ne!(digest, rotated.digest(b"secret").unwrap());

    // A different server secret yields a different (non-correlatable) key.
    assert_ne!(digest, footprint.digest(b"other-secret").unwrap());

    // The digest is order-independent (BTreeSet union).
    let mut one_order = Footprint::default();
    one_order.add(PrivateAccessDescriptor::Alias {
        alias: "x".to_string(),
        credential_digest: corp_credential(),
    });
    one_order.add(PrivateAccessDescriptor::Hosted { policy_id: "@p/*".to_string() });
    let mut other_order = Footprint::default();
    other_order.add(PrivateAccessDescriptor::Hosted { policy_id: "@p/*".to_string() });
    other_order.add(PrivateAccessDescriptor::Alias {
        alias: "x".to_string(),
        credential_digest: corp_credential(),
    });
    assert_eq!(one_order.digest(b"k"), other_order.digest(b"k"));
}

#[test]
fn route_hook_records_routes_and_returns_alias_credential() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let footprint = std::sync::Arc::new(std::sync::Mutex::new(Footprint::default()));
    let hook = RouteHook::new(
        Arc::new(RouteContext::from_config(&config)),
        user("alice"),
        std::sync::Arc::clone(&footprint),
        std::sync::Arc::from(b"secret".as_slice()),
    );

    // Public fetch: no upstream credential, no private footprint entry.
    assert_eq!(hook.authorize("https://registry.npmjs.org/lodash", Some("lodash")), None);
    // Private proxied fetch: the uplink credential is returned and recorded.
    assert_eq!(
        hook.authorize("https://npm.corp.example/@acme%2fwidget", Some("@acme/widget")),
        Some("Bearer uplink-secret".to_string()),
    );

    let footprint = footprint.lock().unwrap();
    assert!(!footprint.is_public());
    assert!(footprint.digest(b"secret").is_some());
}

#[test]
fn metadata_scope_maps_route_classes() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let footprint = std::sync::Arc::new(std::sync::Mutex::new(Footprint::default()));
    let hook = RouteHook::new(
        Arc::new(RouteContext::from_config(&config)),
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
        PrivateAccessDescriptor::Alias {
            alias: "corp".to_string(),
            credential_digest: corp_credential()
        }
        .digest_id(b"server-secret"),
    );
    // metadata_scope is read-only: classifying must not grow the footprint.
    assert!(footprint.lock().unwrap().is_public());
}

#[test]
fn authorized_alias_users_share_metadata_scope() {
    let mut config = base_config();
    config.uplinks.insert(
        "corp".to_string(),
        uplink_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = Arc::new(RouteContext::from_config(&config));
    let secret = Arc::from(b"server-secret".as_slice());
    let alice = RouteHook::new(
        Arc::clone(&context),
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
        PrivateAccessDescriptor::Alias {
            alias: "corp".to_string(),
            credential_digest: corp_credential()
        }
        .digest_id(b"server-secret"),
    );
}

#[test]
fn descriptor_digest_id_depends_on_secret() {
    let descriptor = PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: corp_credential(),
    };
    assert_ne!(descriptor.digest_id(b"secret-a"), descriptor.digest_id(b"secret-b"));
    // Generation rotation moves the namespace.
    let rotated = PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: rotated_credential(),
    };
    assert_ne!(descriptor.digest_id(b"secret-a"), rotated.digest_id(b"secret-a"));
}
