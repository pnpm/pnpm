use std::{collections::BTreeMap, path::Path};

use super::{ConfigOverlay, cache_key};

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
