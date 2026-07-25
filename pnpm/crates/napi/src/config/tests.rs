use std::{collections::BTreeMap, path::Path};

use super::{ConfigOverlay, cache_key, overlay_default_registry, pin_unkeyed_header};

/// Two independently constructed overlays with identical map contents must
/// produce the same cache key. If the map fields were `HashMap` (random
/// per-instance iteration order) their `Debug` output would differ, the intern
/// cache would miss every call, and a fresh `Config` would leak each time.
#[test]
fn cache_key_is_stable_across_equal_overlays_with_map_fields() {
    let build = || {
        let allow_builds = BTreeMap::from([("a".to_string(), true), ("b".to_string(), false)]);
        let auth_header_by_uri = BTreeMap::from([
            ("//r1/".to_string(), "Bearer x".to_string()),
            ("//r2/".to_string(), "Bearer y".to_string()),
        ]);
        ConfigOverlay {
            allow_builds: Some(allow_builds),
            auth_header_by_uri: Some(auth_header_by_uri),
            ..ConfigOverlay::default()
        }
    };
    let dir = Path::new("/pnpm-napi-cache-key-test-does-not-exist");
    assert_eq!(cache_key(dir, &build()), cache_key(dir, &build()));
}

#[test]
fn unkeyed_header_pins_to_the_registry_the_same_overlay_declared() {
    let overlay = ConfigOverlay {
        registries: Some(BTreeMap::from([(
            "default".to_string(),
            "https://trusted.example.com/".to_string(),
        )])),
        ..ConfigOverlay::default()
    };
    let headers = BTreeMap::from([(String::new(), "Bearer host-secret".to_string())]);

    let by_uri = pin_unkeyed_header(&headers, &overlay_default_registry(&overlay));

    assert_eq!(
        by_uri.get("//trusted.example.com/").map(String::as_str),
        Some("Bearer host-secret"),
    );
    assert_eq!(by_uri.len(), 1);
}

/// An overlay that declares no registry of its own gets the npmjs default,
/// mirroring how a `.npmrc` without a `registry=` pins its unscoped
/// credentials.
#[test]
fn unkeyed_header_falls_back_to_npmjs_when_the_overlay_declares_no_registry() {
    let headers = BTreeMap::from([(String::new(), "Bearer host-secret".to_string())]);

    let by_uri = pin_unkeyed_header(&headers, &overlay_default_registry(&ConfigOverlay::default()));

    assert_eq!(by_uri.get("//registry.npmjs.org/").map(String::as_str), Some("Bearer host-secret"));
}

/// A default registry that is not a parseable URL leaves the unkeyed header
/// nowhere safe to go, so it is dropped rather than keyed at `""` — where it
/// would match every lookup.
#[test]
fn unkeyed_header_is_dropped_when_the_default_registry_is_unparsable() {
    let overlay =
        ConfigOverlay { registry: Some("not-a-url".to_string()), ..ConfigOverlay::default() };
    let headers = BTreeMap::from([(String::new(), "Bearer host-secret".to_string())]);

    let by_uri = pin_unkeyed_header(&headers, &overlay_default_registry(&overlay));

    assert!(by_uri.is_empty(), "{by_uri:?} should not carry the unkeyed header");
}

/// "Explicit wins" has to survive a host key spelled without the trailing
/// slash: left un-normalized it would stay a separate entry here and only
/// collide once `AuthHeaders` canonicalizes, letting map order pick the
/// winner.
#[test]
fn an_explicit_key_missing_its_trailing_slash_still_wins() {
    let overlay = ConfigOverlay {
        registry: Some("https://trusted.example.com/".to_string()),
        ..ConfigOverlay::default()
    };
    let headers = BTreeMap::from([
        (String::new(), "Bearer unkeyed".to_string()),
        ("//trusted.example.com".to_string(), "Bearer explicit".to_string()),
    ]);

    let by_uri = pin_unkeyed_header(&headers, &overlay_default_registry(&overlay));

    assert_eq!(by_uri.get("//trusted.example.com/").map(String::as_str), Some("Bearer explicit"));
    assert_eq!(by_uri.len(), 1);
}

#[test]
fn a_header_the_host_keyed_explicitly_wins_over_the_unkeyed_one() {
    let overlay = ConfigOverlay {
        registry: Some("https://trusted.example.com/".to_string()),
        ..ConfigOverlay::default()
    };
    let headers = BTreeMap::from([
        (String::new(), "Bearer unkeyed".to_string()),
        ("//trusted.example.com/".to_string(), "Bearer explicit".to_string()),
    ]);

    let by_uri = pin_unkeyed_header(&headers, &overlay_default_registry(&overlay));

    assert_eq!(by_uri.get("//trusted.example.com/").map(String::as_str), Some("Bearer explicit"));
}
