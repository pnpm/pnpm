use std::path::PathBuf;

use pnpm_registry::Package;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use pnpm_network::MetadataCacheScope;

use super::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, decode_registry_name,
    encode_pkg_name, get_pkg_mirror_path, get_registry_name, load_meta, load_meta_headers,
    load_meta_with_hold_cap, save_meta_indexed, scoped_meta_dir,
};

#[test]
fn scoped_meta_dir_public_is_unchanged() {
    assert_eq!(
        scoped_meta_dir(&MetadataCacheScope::Public, ABBREVIATED_META_DIR),
        ABBREVIATED_META_DIR,
    );
    assert_eq!(scoped_meta_dir(&MetadataCacheScope::Public, FULL_META_DIR), FULL_META_DIR);
}

#[test]
fn scoped_meta_dir_private_namespaces_by_descriptor() {
    let scope = MetadataCacheScope::Private { descriptor_id: "abc123".to_string() };
    assert_eq!(
        scoped_meta_dir(&scope, ABBREVIATED_META_DIR),
        "v11/metadata-private/abc123/metadata",
    );
    assert_eq!(scoped_meta_dir(&scope, FULL_META_DIR), "v11/metadata-private/abc123/metadata-full");
    assert_eq!(
        scoped_meta_dir(&scope, FULL_FILTERED_META_DIR),
        "v11/metadata-private/abc123/metadata-full-filtered",
    );
    // Distinct descriptors never share a directory.
    let other = MetadataCacheScope::Private { descriptor_id: "def456".to_string() };
    assert_ne!(scoped_meta_dir(&scope, FULL_META_DIR), scoped_meta_dir(&other, FULL_META_DIR));
}

#[test]
fn encode_pkg_name_passes_lowercase_through() {
    assert_eq!(encode_pkg_name("lodash"), "lodash");
    assert_eq!(encode_pkg_name("@scope/foo"), "@scope/foo");
}

#[test]
fn encode_pkg_name_hash_suffix_for_mixed_case() {
    let got = encode_pkg_name("LRUCache");
    assert!(got.starts_with("LRUCache_"), "got: {got}");
    let suffix = got.trim_start_matches("LRUCache_");
    assert_eq!(suffix.len(), 64, "sha256 hex is 64 chars");
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
}

#[test]
fn get_registry_name_default_scheme() {
    let got = get_registry_name("https://registry.npmjs.org/").expect("encode");
    assert_eq!(got, "https%3A+registry.npmjs.org");
}

#[test]
fn get_registry_name_with_port() {
    let got = get_registry_name("https://npm.example:8443/").expect("encode");
    assert_eq!(got, "https%3A+npm.example+8443");
}

#[test]
fn get_registry_name_default_port_omitted() {
    let got = get_registry_name("https://npm.example:443/").expect("encode");
    assert_eq!(got, "https%3A+npm.example");
}

#[test]
fn get_registry_name_with_path() {
    let got =
        get_registry_name("https://releases.jfrog.io/artifactory/api/npm/coding-agents-npm-a/")
            .expect("encode");
    assert_eq!(got, "https%3A+releases.jfrog.io%2Fartifactory+api+npm+coding-agents-npm-a");
}

/// Two teams on one Artifactory host each get their own metadata
/// directory; sharing one would let a package resolved from either answer
/// with the other's versions, integrity and tarball URLs.
#[test]
fn get_registry_name_separates_same_host_paths() {
    let team_a =
        get_registry_name("https://releases.jfrog.io/artifactory/api/npm/team-a/").expect("encode");
    let team_b =
        get_registry_name("https://releases.jfrog.io/artifactory/api/npm/team-b/").expect("encode");
    assert_ne!(team_a, team_b);
}

#[test]
fn get_registry_name_ignores_trailing_slash() {
    assert_eq!(
        get_registry_name("https://npm.example/registry").expect("encode"),
        get_registry_name("https://npm.example/registry/").expect("encode"),
    );
}

#[test]
fn get_registry_name_ipv6() {
    let got = get_registry_name("http://[::1]:8080/").expect("encode");
    assert_eq!(got, "http%3A+%5B%3A%3A1%5D+8080");
}

/// A key is one directory name, so a `+`, `_`, `/` or `:` that came from
/// the URL has to be escaped: leaving it raw would let it read as one of
/// the delimiters and merge two registries onto one cache.
#[test]
fn get_registry_name_escapes_delimiters() {
    let distinct = [
        "https://repo.example/foo-bar/",
        "https://repo.example-foo/bar/",
        "https://repo.example/foo/",
        "https://repo.example_foo/",
        "https://nexus_npm/",
        "https://nexus/npm/",
        "https://npm.example/team/a/",
        "https://npm.example/team+a/",
        "https://npm.example/a%2Fb/",
        "https://npm.example/a%3Ab/",
    ];
    let mut keys: Vec<String> =
        distinct.iter().map(|url| get_registry_name(url).expect("encode")).collect();
    keys.sort();
    let key_count = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), key_count, "every registry must get its own directory");

    assert_eq!(
        get_registry_name("https://npm.example/team+a/").expect("encode"),
        "https%3A+npm.example%2Fteam%2Ba",
    );
}

/// Every key pnpm wrote before the scheme joined it was a bare URL host,
/// which can never contain a `%`. A hostname may contain `_`, so without
/// that guarantee `https://nexus/npm/` would land on the directory left
/// behind for `https://nexus_npm/`.
#[test]
fn get_registry_name_cannot_collide_with_an_earlier_pnpm_version() {
    for registry in [
        "https://registry.npmjs.org/",
        "http://localhost:4873/",
        "https://nexus_npm/",
        "https://nexus/npm/",
    ] {
        let got = get_registry_name(registry).expect("encode");
        assert!(got.contains('%'), "got: {got}");
    }
    assert_eq!(get_registry_name("https://nexus_npm/").expect("encode"), "https%3A+nexus_npm");
    assert_eq!(get_registry_name("https://nexus/npm/").expect("encode"), "https%3A+nexus%2Fnpm");
}

/// `http` metadata can be rewritten in transit and must never be handed to
/// a resolution configured for `https`.
#[test]
fn get_registry_name_separates_schemes() {
    assert_ne!(
        get_registry_name("http://registry.example/repo/").expect("encode"),
        get_registry_name("https://registry.example/repo/").expect("encode"),
    );
}

/// A repeated slash reaches the registry as a distinct request path, so it
/// must not be normalized away the way a single trailing slash is:
/// `https://npm.example/`, `//` and `///` ask for `/lodash`, `//lodash` and
/// `///lodash`.
#[test]
fn get_registry_name_keeps_a_repeated_slash() {
    assert_eq!(get_registry_name("https://npm.example/").expect("encode"), "https%3A+npm.example");
    assert_eq!(
        get_registry_name("https://npm.example//").expect("encode"),
        "https%3A+npm.example%2F",
    );
    assert_eq!(
        get_registry_name("https://npm.example///").expect("encode"),
        "https%3A+npm.example%2F+",
    );
    assert_eq!(
        get_registry_name("https://npm.example/a//b/").expect("encode"),
        "https%3A+npm.example%2Fa++b",
    );
    assert_ne!(
        get_registry_name("https://npm.example/repo//").expect("encode"),
        get_registry_name("https://npm.example/repo/").expect("encode"),
    );
    assert_ne!(
        get_registry_name("https://npm.example//repo/").expect("encode"),
        get_registry_name("https://npm.example/repo/").expect("encode"),
    );
}

/// Win32 strips a trailing period, which would alias the two directories.
#[test]
fn get_registry_name_escapes_a_trailing_period() {
    assert_eq!(
        get_registry_name("https://npm.example/foo./").expect("encode"),
        "https%3A+npm.example%2Ffoo%2E",
    );
    assert_eq!(
        get_registry_name("https://npm.example/foo/").expect("encode"),
        "https%3A+npm.example%2Ffoo",
    );
}

/// Windows rejects these in a filename, and the cache commands feed the
/// key to a glob whose matches `pnpm cache delete` removes.
#[test]
fn get_registry_name_escapes_filesystem_and_glob_metacharacters() {
    assert_eq!(
        get_registry_name("https://npm.example/a*b/").expect("encode"),
        "https%3A+npm.example%2Fa%2Ab",
    );
    assert_eq!(
        get_registry_name("https://npm.example/a|b/").expect("encode"),
        "https%3A+npm.example%2Fa%7Cb",
    );
    // The URL parser reads a backslash in a special-scheme path as a separator.
    assert_eq!(
        get_registry_name(r"https://npm.example/a\b/").expect("encode"),
        "https%3A+npm.example%2Fa+b",
    );
    assert_eq!(
        get_registry_name("https://npm.example/a%5Cb/").expect("encode"),
        "https%3A+npm.example%2Fa%255Cb%5F\
         f323ed1d3ec091d56df73b036cd0f4b4b20aa1bc06272a13cfd84f36894ed440",
    );
}

/// HFS+ and NTFS would otherwise merge the two directories.
#[test]
fn get_registry_name_hashes_mixed_case_path() {
    assert_eq!(
        get_registry_name("https://npm.example:8443/registry/A/").expect("encode"),
        "https%3A+npm.example+8443%2Fregistry+A%5F\
         f5296609e0eaab0d2f8fe3c4503600ed349a61793535d2c95505f0710b272e65",
    );
    assert_eq!(
        get_registry_name("https://npm.example:8443/registry/a/").expect("encode"),
        "https%3A+npm.example+8443%2Fregistry+a",
    );
}

/// Past the 255-byte filename limit the key is its own hash, which is
/// still one directory per registry.
#[test]
fn get_registry_name_hashes_an_oversized_key() {
    let long_path = "a".repeat(300);
    let got = get_registry_name(&format!("https://npm.example/{long_path}/")).expect("encode");
    assert_eq!(got.len(), 64);
    assert!(got.chars().all(|character| character.is_ascii_hexdigit()));
    let other = get_registry_name(&format!("https://npm.example/{long_path}b/")).expect("encode");
    assert_ne!(got, other);
}

#[test]
fn decode_registry_name_restores_scheme_host_port_and_path() {
    assert_eq!(decode_registry_name("https%3A+registry.npmjs.org"), "https://registry.npmjs.org/");
    assert_eq!(decode_registry_name("http%3A+localhost+4873"), "http://localhost:4873/");
    assert_eq!(decode_registry_name("http%3A+%5B%3A%3A1%5D+8080"), "http://[::1]:8080/");
    assert_eq!(
        decode_registry_name("https%3A+releases.jfrog.io%2Fartifactory+api+npm+team-a"),
        "https://releases.jfrog.io/artifactory/api/npm/team-a/",
    );
    assert_eq!(
        decode_registry_name("https%3A+npm.example%2Fteam%2Ba"),
        "https://npm.example/team+a/",
    );
    assert_eq!(
        decode_registry_name(
            "https%3A+npm.example+8443%2Fregistry+A%5F\
             f5296609e0eaab0d2f8fe3c4503600ed349a61793535d2c95505f0710b272e65"
        ),
        "https://npm.example:8443/registry/A/",
    );
    // Directories written before the scheme joined the key still label
    // sensibly.
    assert_eq!(decode_registry_name("registry.npmjs.org"), "registry.npmjs.org");
    assert_eq!(decode_registry_name("localhost+4873"), "localhost:4873");
}

/// The guard that keeps the two halves in step: whatever the encoder can
/// spell, the decoder names again exactly.
#[test]
fn decode_registry_name_is_the_exact_inverse_of_get_registry_name() {
    for registry in [
        "https://registry.npmjs.org/",
        "http://localhost:4873/",
        "http://[::1]:8080/",
        "https://npm.example:8443/registry/a/",
        "https://releases.jfrog.io/artifactory/api/npm/team-a/",
        "https://npm.example/team+a/",
        "https://npm.example//",
        "https://npm.example///",
        "https://npm.example/a//b/",
        "https://nexus_npm/",
    ] {
        let key = get_registry_name(registry).expect("encode");
        assert_eq!(decode_registry_name(&key), registry, "key: {key}");
    }
}

/// pnpm v11's `decodeRegistry` hands back a key it cannot decode, so this
/// must too rather than render the undecodable bytes lossily.
#[test]
fn decode_registry_name_passes_through_an_undecodable_key() {
    assert_eq!(decode_registry_name("%FF"), "%FF");
    assert_eq!(decode_registry_name("https%3A+npm.example%2F%FF"), "https%3A+npm.example%2F%FF");
    assert_eq!(decode_registry_name("%not-a-key"), "%not-a-key");
    // A malformed escape must not decode further just because the `+` was
    // already swapped: pnpm v11's `decodeURIComponent` throws on this.
    assert_eq!(decode_registry_name("%not-a-key+8443"), "%not-a-key+8443");
}

/// Callers (notably the cached fetcher) downgrade to a cache-less
/// fetch on this error instead of failing the install.
#[test]
fn get_registry_name_rejects_malformed_url() {
    let err = get_registry_name("not a url").expect_err("malformed url must error");
    assert!(matches!(err, super::EncodeRegistryError::ParseUrl { .. }), "got: {err:?}");
}

#[test]
fn get_registry_name_rejects_a_url_without_a_host() {
    let err = get_registry_name("file:///tmp/registry").expect_err("hostless url must error");
    assert!(matches!(err, super::EncodeRegistryError::MissingHost { .. }), "got: {err:?}");
}

/// A registry can carry `user:pass@` credentials, which must not reach a
/// CI log through the diagnostic.
#[test]
fn get_registry_name_redacts_credentials_in_its_error() {
    let err = get_registry_name("https://user:secret@").expect_err("malformed url must error");
    assert!(!format!("{err}").contains("secret"), "got: {err}");
}

#[test]
fn get_pkg_mirror_path_composes_full_path() {
    let dir = PathBuf::from("/cache");
    let got = get_pkg_mirror_path(&dir, FULL_META_DIR, "https://registry.npmjs.org/", "lodash")
        .expect("compose");
    assert_eq!(
        got,
        PathBuf::from("/cache/v11/metadata-full/https%3A+registry.npmjs.org/lodash.jsonl"),
    );
}

/// Constants match upstream's `core/constants/src/index.ts` slugs.
/// Any drift would silently fork the cache layout from pnpm's.
#[test]
fn constants_match_upstream() {
    assert_eq!(FULL_META_DIR, "v11/metadata-full");
    assert_eq!(FULL_FILTERED_META_DIR, "v11/metadata-full-filtered");
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

#[test]
fn load_meta_survives_mirror_rewrite() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let loaded = load_meta(&mirror).expect("read full back");
    // The fatter `0.9.0` fragment shifts `1.0.0`'s offset, so a loader
    // that re-read the path instead of the pinned inode parses garbage.
    let newer: Package = serde_json::from_value(serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "0.9.0": {
                "name": "acme",
                "version": "0.9.0",
                "deprecated": "superseded by 1.0.0; padding padding padding padding",
                "dist": {
                    "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                    "shasum": "1111111111111111111111111111111111111111",
                    "tarball": "https://registry/acme-0.9.0.tgz"
                }
            },
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0-rebuilt.tgz"
                }
            }
        }
    }))
    .expect("deserialize rewritten Package");
    save_meta_indexed(&mirror, &newer, None).expect("overwrite");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate after rewrite");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

#[test]
fn load_meta_past_the_hold_cap_buffers_fragments_instead_of_missing() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from buffered fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
    assert_eq!(loaded.etag.as_deref(), Some(r#"W/"abc""#));
}

#[test]
fn load_meta_rejects_oversized_declared_record_lengths() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    std::fs::write(&mirror, "pacquet-meta-v1 128 999999999999\n{}{}").expect("write");
    assert!(load_meta(&mirror).is_none());
}

#[test]
fn load_meta_past_the_hold_cap_ignores_a_sparse_tail() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    let size = file.metadata().expect("metadata").len();
    file.set_len(size + 64 * 1024 * 1024).expect("extend sparsely");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from buffered fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

#[test]
fn load_meta_past_the_hold_cap_skips_a_sparse_gap_between_spans() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let headers = "{}";
    let fragment = r#"{"name":"acme","version":"1.0.0","dist":{"integrity":"sha512-A","shasum":"0","tarball":"https://registry/acme-1.0.0.tgz"}}"#;
    let far_offset: u64 = 512 * 1024 * 1024;
    let index = format!(
        r#"{{"name":"acme","distTags":{{}},"versions":[["1.0.0",0,{}],["2.0.0",{far_offset},16]]}}"#,
        fragment.len(),
    );
    let contents =
        format!("pacquet-meta-v1 {} {}\n{headers}{index}{fragment}", headers.len(), index.len());
    std::fs::write(&mirror, &contents).expect("write");
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    file.set_len(contents.len() as u64 + far_offset + 16).expect("extend sparsely");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate the near fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
    // The far span reads zeroes out of the sparse hole — not JSON, so
    // the version is absent; the gap itself must never be buffered.
    assert!(loaded.versions.get("2.0.0").is_none());
}

#[test]
fn load_meta_treats_an_oversized_fragment_span_as_absent() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let headers = "{}";
    let fragment = r#"{"name":"acme","version":"1.0.0","dist":{"integrity":"sha512-A","shasum":"0","tarball":"https://registry/acme-1.0.0.tgz"}}"#;
    let index = format!(
        r#"{{"name":"acme","distTags":{{}},"versions":[["1.0.0",0,{}],["9.9.9",{},{}]]}}"#,
        fragment.len(),
        fragment.len(),
        32 * 1024 * 1024,
    );
    let contents =
        format!("pacquet-meta-v1 {} {}\n{headers}{index}{fragment}", headers.len(), index.len());
    std::fs::write(&mirror, &contents).expect("write");
    // A sparse tail makes the file size cover the declared span
    // without paying for the bytes, like a corrupt mirror would.
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    file.set_len(contents.len() as u64 + 64 * 1024 * 1024).expect("extend sparsely");
    let loaded = load_meta(&mirror).expect("read full back");
    assert!(loaded.versions.get("9.9.9").is_none(), "oversized span must read as absent");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate the in-bounds fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

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

/// pnpm and pacquet must share the same on-disk metadata mirror.
#[test]
fn pnpm_ndjson_format_reads_as_cache_hit() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let mut pkg = fixture_package();
    pkg.modified = None;
    std::fs::write(
        &mirror,
        format!(
            "{{\"etag\":\"W/abc\",\"modified\":\"2025-01-15T12:00:00.000Z\"}}\n{}",
            serde_json::to_string(&pkg).expect("serialize fixture"),
        ),
    )
    .expect("write pnpm format");
    let headers = load_meta_headers(&mirror).expect("read headers");
    assert_eq!(headers.etag.as_deref(), Some("W/abc"));
    let meta = load_meta(&mirror).expect("read meta");
    assert_eq!(meta.etag.as_deref(), Some("W/abc"));
    assert_eq!(meta.modified.as_deref(), Some("2025-01-15T12:00:00.000Z"));
    assert_eq!(meta.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
}

#[test]
fn load_helpers_return_none_on_missing_file() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("does-not-exist.jsonl");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

#[test]
fn load_helpers_return_none_on_malformed_mirror() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("bad.jsonl");
    std::fs::write(&mirror, "no-newline-only-header").expect("write garbage");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

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
