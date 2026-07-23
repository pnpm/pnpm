use super::{
    base64_to_hex, build_purl, classify_license, confined_importer_dir, encode_purl_name,
    extract_author, extract_repository, is_simple_spdx_id, normalize_link_path,
    peer_names_from_manifest, sanitize_spdx_id, split_scoped_name, strip_url_credentials,
};

#[test]
fn confined_importer_dir_accepts_dirs_inside_the_lockfile_root() {
    let root = tempfile::tempdir().expect("create lockfile dir");
    std::fs::create_dir_all(root.path().join("packages/foo")).expect("create importer dir");

    // The root importer and an in-tree sub-importer both resolve inside the
    // lockfile dir, so both are readable.
    let canonical_root = std::fs::canonicalize(root.path()).expect("canonicalize root");
    assert_eq!(confined_importer_dir(root.path(), "."), Some(canonical_root.clone()));
    assert_eq!(
        confined_importer_dir(root.path(), "packages/foo"),
        Some(canonical_root.join("packages/foo")),
    );
}

#[test]
fn confined_importer_dir_rejects_lexical_escapes() {
    let root = tempfile::tempdir().expect("create lockfile dir");
    for id in ["..", "../foo", "/abs/path", "C:/x"] {
        assert!(confined_importer_dir(root.path(), id).is_none(), "expected {id:?} to be rejected");
    }
}

/// A lexically clean importer key whose directory is a symlink pointing
/// outside the workspace must not be read — the canonicalize + containment
/// check catches what the lexical `validate_importer_id` filter cannot.
#[cfg(unix)]
#[test]
fn confined_importer_dir_rejects_symlinked_escape() {
    let root = tempfile::tempdir().expect("create lockfile dir");
    let outside = tempfile::tempdir().expect("create outside dir");
    std::fs::create_dir_all(root.path().join("packages")).expect("create packages dir");
    std::os::unix::fs::symlink(outside.path(), root.path().join("packages/escape"))
        .expect("create escaping symlink");

    assert!(confined_importer_dir(root.path(), "packages/escape").is_none());
}

#[test]
fn encode_purl_name_unscoped() {
    assert_eq!(encode_purl_name("react"), "react");
}

#[test]
fn encode_purl_name_scoped() {
    assert_eq!(encode_purl_name("@babel/core"), "%40babel/core");
}

#[test]
fn build_purl_unscoped() {
    assert_eq!(build_purl("react", "18.2.0"), "pkg:npm/react@18.2.0");
}

#[test]
fn build_purl_scoped() {
    assert_eq!(build_purl("@babel/core", "7.22.0"), "pkg:npm/%40babel/core@7.22.0");
}

#[test]
fn split_scoped_name_unscoped() {
    assert_eq!(split_scoped_name("react"), (None, "react"));
}

#[test]
fn split_scoped_name_scoped() {
    assert_eq!(split_scoped_name("@babel/core"), (Some("@babel"), "core"));
}

#[test]
fn sanitize_spdx_id_preserves_valid_chars() {
    assert_eq!(sanitize_spdx_id("foo-bar.1"), "foo-bar.1");
}

#[test]
fn sanitize_spdx_id_replaces_special_chars() {
    assert_eq!(sanitize_spdx_id("@scope/name"), "-scope-name");
}

#[test]
fn base64_to_hex_sha512() {
    assert_eq!(base64_to_hex("AAAA"), Some("000000".to_string()));
}

#[test]
fn base64_to_hex_invalid_returns_none() {
    assert_eq!(base64_to_hex("!!!"), None);
}

#[test]
fn peer_names_excludes_regular_deps() {
    let manifest = serde_json::json!({
        "dependencies": { "react": "^18.0.0" },
        "peerDependencies": { "react": "^18.0.0", "react-dom": "^18.0.0" },
    });
    let peers = peer_names_from_manifest(&manifest);
    assert!(!peers.contains("react"), "react is both a dep and peer; should be excluded");
    assert!(peers.contains("react-dom"), "react-dom is peer-only; should be included");
}

#[test]
fn peer_names_empty_when_no_peers() {
    let manifest = serde_json::json!({ "dependencies": { "react": "^18.0.0" } });
    assert!(peer_names_from_manifest(&manifest).is_empty());
}

#[test]
fn extract_author_string() {
    let manifest = serde_json::json!({ "author": "Jane Doe" });
    assert_eq!(extract_author(&manifest), Some("Jane Doe".to_string()));
}

#[test]
fn extract_author_object() {
    let manifest =
        serde_json::json!({ "author": { "name": "Jane Doe", "email": "jane@example.com" } });
    assert_eq!(extract_author(&manifest), Some("Jane Doe".to_string()));
}

#[test]
fn extract_author_missing() {
    assert_eq!(extract_author(&serde_json::json!({})), None);
}

#[test]
fn extract_repository_string() {
    let manifest = serde_json::json!({ "repository": "https://github.com/foo/bar" });
    assert_eq!(extract_repository(&manifest), Some("https://github.com/foo/bar".to_string()));
}

#[test]
fn extract_repository_object() {
    let manifest = serde_json::json!({ "repository": { "type": "git", "url": "https://github.com/foo/bar.git" } });
    assert_eq!(extract_repository(&manifest), Some("https://github.com/foo/bar.git".to_string()));
}

#[test]
fn normalize_link_path_simple() {
    assert_eq!(normalize_link_path(".", "packages/foo"), Some("packages/foo".to_string()));
}

#[test]
fn normalize_link_path_relative() {
    assert_eq!(normalize_link_path("packages/a", "../b"), Some("packages/b".to_string()));
}

#[test]
fn normalize_link_path_to_parent() {
    assert_eq!(normalize_link_path("packages/a", ".."), Some("packages".to_string()));
}

#[test]
fn normalize_link_path_to_root() {
    assert_eq!(normalize_link_path("packages/a", "../.."), Some(".".to_string()));
}

#[test]
fn classify_license_spdx_id() {
    let result = classify_license("MIT");
    assert_eq!(result["license"]["id"], "MIT");
}

#[test]
fn classify_license_expression() {
    let result = classify_license("MIT OR Apache-2.0");
    assert_eq!(result["expression"], "MIT OR Apache-2.0");
}

#[test]
fn classify_license_freetext() {
    let result = classify_license("Proprietary License");
    assert_eq!(result["license"]["name"], "Proprietary License");
}

#[test]
fn is_simple_spdx_id_valid() {
    assert!(is_simple_spdx_id("MIT"));
    assert!(is_simple_spdx_id("Apache-2.0"));
    assert!(is_simple_spdx_id("GPL-3.0-or-later"));
}

#[test]
fn is_simple_spdx_id_invalid() {
    assert!(!is_simple_spdx_id("Proprietary License"));
    assert!(!is_simple_spdx_id(""));
}

#[test]
fn strip_url_credentials_removes_userinfo() {
    assert_eq!(
        strip_url_credentials("https://user:token@github.com/foo/bar"),
        "https://github.com/foo/bar",
    );
}

#[test]
fn strip_url_credentials_no_credentials() {
    assert_eq!(strip_url_credentials("https://github.com/foo/bar"), "https://github.com/foo/bar");
}

#[test]
fn normalize_link_path_escape_returns_none() {
    assert_eq!(normalize_link_path(".", ".."), None);
}
