use std::path::PathBuf;

use pacquet_registry::Package;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::{
    ABBREVIATED_META_DIR, FULL_META_DIR, encode_pkg_name, get_pkg_mirror_path, get_registry_name,
    load_meta, load_meta_headers, save_meta_indexed,
};

/// Lower-case names pass through unchanged. Matches upstream's
/// `pkgName !== pkgName.toLowerCase()` short-circuit.
#[test]
fn encode_pkg_name_passes_lowercase_through() {
    assert_eq!(encode_pkg_name("lodash"), "lodash");
    assert_eq!(encode_pkg_name("@scope/foo"), "@scope/foo");
}

/// Names containing any uppercase letter get a `_<sha256-hex>` suffix
/// so case-insensitive filesystems can't collide them with a lowercase
/// sibling. The prefix is the original name; the suffix is the sha256
/// hex of the original name.
#[test]
fn encode_pkg_name_hash_suffix_for_mixed_case() {
    let got = encode_pkg_name("LRUCache");
    assert!(got.starts_with("LRUCache_"), "got: {got}");
    let suffix = got.trim_start_matches("LRUCache_");
    assert_eq!(suffix.len(), 64, "sha256 hex is 64 chars");
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
}

/// `https://registry.npmjs.org/` → `registry.npmjs.org`. No port,
/// no escaping needed.
#[test]
fn get_registry_name_default_scheme() {
    let got = get_registry_name("https://registry.npmjs.org/").expect("encode");
    assert_eq!(got, "registry.npmjs.org");
}

/// Explicit non-default port encodes as `host+port`.
#[test]
fn get_registry_name_with_port() {
    let got = get_registry_name("https://npm.example:8443/").expect("encode");
    assert_eq!(got, "npm.example+8443");
}

/// Default scheme port is **not** included in the slug — the URL
/// parser strips it.
#[test]
fn get_registry_name_default_port_omitted() {
    let got = get_registry_name("https://npm.example:443/").expect("encode");
    assert_eq!(got, "npm.example");
}

/// Malformed registry URL surfaces as the dedicated [`super::EncodeRegistryError`]
/// rather than panicking — callers (notably the cached fetcher) downgrade
/// to a cache-less fetch instead of failing the install.
#[test]
fn get_registry_name_rejects_malformed_url() {
    let err = get_registry_name("not a url").expect_err("malformed url must error");
    assert!(matches!(err, super::EncodeRegistryError::ParseUrl { .. }), "got: {err:?}");
}

/// The mirror path is `<cache_dir>/<meta_dir>/<registry-slug>/<encoded-name>.jsonl`.
#[test]
fn get_pkg_mirror_path_composes_full_path() {
    let dir = PathBuf::from("/cache");
    let got = get_pkg_mirror_path(&dir, FULL_META_DIR, "https://registry.npmjs.org/", "lodash")
        .expect("compose");
    assert_eq!(got, PathBuf::from("/cache/v11/metadata-full/registry.npmjs.org/lodash.jsonl"));
}

/// Constants match upstream's `core/constants/src/index.ts` slugs.
/// Any drift would silently fork the cache layout from pnpm's.
#[test]
fn constants_match_upstream() {
    assert_eq!(FULL_META_DIR, "v11/metadata-full");
    assert_eq!(ABBREVIATED_META_DIR, "v11/metadata");
}

/// Build a minimal `Package` fixture for the round-trip tests.
fn fixture_package() -> Package {
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    });
    serde_json::from_value(body).expect("deserialize fixture Package")
}

/// Save → `load_meta_headers` reads back only the headers record
/// (etag, modified) without touching the index or fragments — fast
/// path for the conditional GET decision.
#[test]
fn load_meta_headers_round_trip() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("nested").join("lodash.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let headers = load_meta_headers(&mirror).expect("read headers back");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"abc""#));
    assert_eq!(headers.modified.as_deref(), Some("2025-01-15T12:00:00.000Z"));
}

/// Save → `load_meta` reconstructs the Package: scalars from the
/// index record, etag back-filled from the headers record, and
/// version manifests hydrating from their byte spans. The cached
/// fetcher uses this on a 304 response.
#[test]
fn load_meta_round_trip_hydrates_versions_from_spans() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let loaded = load_meta(&mirror).expect("read full back");
    assert_eq!(loaded.name, "acme");
    assert_eq!(loaded.etag.as_deref(), Some(r#"W/"abc""#));
    assert_eq!(loaded.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    assert_eq!(loaded.dist_tag("latest"), Some("1.0.0"));
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from file span");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

/// A truncated mirror — spans pointing past the end of the file —
/// reads as a cache miss instead of handing out garbage fragments.
#[test]
fn load_meta_rejects_truncated_fragments() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let full = std::fs::read(&mirror).expect("read mirror");
    std::fs::write(&mirror, &full[..full.len() - 10]).expect("truncate");
    assert!(load_meta(&mirror).is_none());
}

/// Files in the previous two-line NDJSON format read as cache misses
/// (the fetcher then refetches and rewrites in the indexed format).
#[test]
fn previous_ndjson_format_reads_as_cache_miss() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    std::fs::write(&mirror, "{\"etag\":\"W/abc\"}\n{\"name\":\"acme\",\"versions\":{}}")
        .expect("write old format");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

/// Missing file → `None` from both readers. The fetcher's lookup
/// chain catches `None` as "cache cold" and proceeds with an
/// unconditional GET.
#[test]
fn load_helpers_return_none_on_missing_file() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("does-not-exist.jsonl");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

/// Malformed mirror (no newline separator) → `None`.
#[test]
fn load_helpers_return_none_on_malformed_mirror() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("bad.jsonl");
    std::fs::write(&mirror, "no-newline-only-header").expect("write garbage");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

/// `save_meta_indexed` overwrites an existing mirror atomically — an
/// observer sees either the old contents or the new ones, never a
/// torn record.
#[test]
fn save_meta_overwrites_existing_mirror() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();

    save_meta_indexed(&mirror, &pkg, Some(r#"W/"old""#)).expect("first save");
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"new""#)).expect("second save");

    let headers = load_meta_headers(&mirror).expect("read headers");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"new""#));
}
