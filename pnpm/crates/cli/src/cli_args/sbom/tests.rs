use super::{
    LockfileResolution, base64_to_hex, build_purl, classify_license, confined_importer_dir,
    extract_author, extract_repository, integrity_string, normalize_link_path,
    peer_names_from_manifest, platform_incompatible_optional,
};
use crate::cli_args::sbom::{
    cyclonedx::split_scoped_name,
    metadata::{encode_purl_name, extract_bugs_url, url_without_credentials},
    spdx::sanitize_spdx_id,
};
use pnpm_lockfile::{PackageMetadata, RegistryResolution, StringOrList};
use pnpm_package_is_installable::InstallabilityOptions;

fn registry_package(
    os: Option<Vec<String>>,
    cpu: Option<Vec<String>>,
    libc: Option<StringOrList>,
) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .parse()
                .expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu,
        os,
        libc,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn host_darwin(current_cpu: &str) -> InstallabilityOptions<'_> {
    InstallabilityOptions {
        current_os: "darwin",
        current_cpu,
        current_libc: "unknown",
        ..Default::default()
    }
}

#[test]
fn platform_incompatible_optional_skips_optional_package_for_another_platform() {
    let pkg = registry_package(
        Some(vec!["linux".to_string()]),
        Some(vec!["x64".to_string()]),
        Some(StringOrList::String("glibc".to_string())),
    );
    assert!(platform_incompatible_optional(
        "@scope/binding",
        true,
        Some(&pkg),
        &host_darwin("arm64"),
    ));
}

#[test]
fn platform_incompatible_optional_keeps_optional_package_for_current_platform() {
    let pkg =
        registry_package(Some(vec!["darwin".to_string()]), Some(vec!["x64".to_string()]), None);
    assert!(!platform_incompatible_optional(
        "@scope/binding",
        true,
        Some(&pkg),
        &host_darwin("x64"),
    ));
}

#[test]
fn platform_incompatible_optional_ignores_non_optional_packages() {
    let pkg =
        registry_package(Some(vec!["linux".to_string()]), Some(vec!["x64".to_string()]), None);
    assert!(!platform_incompatible_optional("plain-dep", false, Some(&pkg), &host_darwin("x64")));
}

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
fn extract_author_blank_string() {
    assert_eq!(extract_author(&serde_json::json!({ "author": "" })), None);
    assert_eq!(extract_author(&serde_json::json!({ "author": " \t\n" })), None);
}

#[test]
fn extract_author_blank_object_name() {
    let manifest = serde_json::json!({ "author": { "name": "", "email": "jane@example.com" } });
    assert_eq!(extract_author(&manifest), None);
    let manifest = serde_json::json!({ "author": { "name": "   " } });
    assert_eq!(extract_author(&manifest), None);
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
fn extract_repository_expands_the_shorthands_hosted_git_info_knows() {
    for (value, expected) in [
        ("vercel/ms", "git+https://github.com/vercel/ms.git"),
        ("acme/widgets.git", "git+https://github.com/acme/widgets.git"),
        ("  vercel/ms  ", "git+https://github.com/vercel/ms.git"),
        ("github:vercel/ms", "git+https://github.com/vercel/ms.git"),
        ("gitlab:acme/widgets", "git+https://gitlab.com/acme/widgets.git"),
        ("bitbucket:acme/widgets", "git+https://bitbucket.org/acme/widgets.git"),
        ("gitlab:foo/bar/baz", "git+https://gitlab.com/foo/bar/baz.git"),
        ("owner/repo#main", "git+https://github.com/owner/repo.git#main"),
        ("git@github.com:foo/bar.git", "git+https://github.com/foo/bar.git"),
    ] {
        let manifest = serde_json::json!({ "repository": value });
        assert_eq!(extract_repository(&manifest), Some(expected.to_string()), "value: {value:?}");
    }
}

#[test]
fn extract_repository_expands_shorthand_in_object() {
    let manifest = serde_json::json!({ "repository": { "type": "git", "url": "acme/widgets" } });
    assert_eq!(
        extract_repository(&manifest),
        Some("git+https://github.com/acme/widgets.git".to_string()),
    );
}

#[test]
fn extract_repository_keeps_non_http_absolute_urls() {
    for url in [
        "git://github.com/foo/bar.git",
        "git+https://github.com/foo/bar.git",
        "git+ssh://git@github.com/foo/bar.git",
        "ssh://git@github.com/foo/bar.git",
    ] {
        let manifest = serde_json::json!({ "repository": url });
        assert_eq!(extract_repository(&manifest), Some(url.to_string()));
    }
}

#[test]
fn extract_repository_completes_a_url_missing_a_slash() {
    let manifest = serde_json::json!({ "repository": "https:/github.com/foo/bar.git" });
    assert_eq!(extract_repository(&manifest), Some("https://github.com/foo/bar.git".to_string()));
}

#[test]
fn extract_repository_strips_credentials() {
    let manifest = serde_json::json!({ "repository": "https://user:token@github.com/foo/bar" });
    assert_eq!(extract_repository(&manifest), Some("https://github.com/foo/bar".to_string()));
}

#[test]
fn extract_repository_keeps_an_ssh_login() {
    for url in ["ssh://git@github.com/foo/bar.git", "git+ssh://git@github.com/foo/bar.git"] {
        let manifest = serde_json::json!({ "repository": url });
        assert_eq!(extract_repository(&manifest), Some(url.to_string()));
    }
}

#[test]
fn extract_repository_strips_a_username_only_authority() {
    for (url, expected) in [
        ("https://token@github.com/foo/bar", "https://github.com/foo/bar"),
        ("git+https://token@github.com/foo/bar.git", "git+https://github.com/foo/bar.git"),
        ("ssh://git:token@github.com/foo/bar.git", "ssh://github.com/foo/bar.git"),
        ("not-ssh://token@example.com/foo/bar", "not-ssh://example.com/foo/bar"),
    ] {
        let manifest = serde_json::json!({ "repository": url });
        assert_eq!(extract_repository(&manifest), Some(expected.to_string()), "url: {url:?}");
    }
}

#[test]
fn extract_repository_drops_values_that_name_no_repository() {
    for value in [
        "foo@example.com",
        "a/b/c",
        "/abs/path",
        ".hidden/repo",
        "owner/",
        "owner",
        "owner /repo",
        // Only a project name, with no owner.
        "github:owner",
        "mailto:bugs@example.com",
        "git+file:/tmp/repo",
        "",
        "   ",
    ] {
        let manifest = serde_json::json!({ "repository": value });
        assert_eq!(extract_repository(&manifest), None, "value: {value:?}");
    }
}

/// Expanding the shorthand in one pnpm version alone would split the two.
#[test]
fn extract_repository_keeps_a_gist_url_and_drops_the_gist_shorthand() {
    let shorthand = serde_json::json!({ "repository": "gist:11081aaa281" });
    assert_eq!(extract_repository(&shorthand), None);
    let url = serde_json::json!({ "repository": "https://gist.github.com/11081aaa281" });
    assert_eq!(extract_repository(&url), Some("https://gist.github.com/11081aaa281".to_string()));
}

#[test]
fn extract_repository_keeps_at_sign_in_path() {
    let manifest = serde_json::json!({ "repository": "https://github.com/foo/bar/baz@qux" });
    assert_eq!(
        extract_repository(&manifest),
        Some("https://github.com/foo/bar/baz@qux".to_string()),
    );
}

#[test]
fn extract_repository_does_not_treat_query_userinfo_lookalikes_as_credentials() {
    let manifest =
        serde_json::json!({ "repository": "https://github.com?x=user:pass@evil.example/repo" });
    assert_eq!(
        extract_repository(&manifest),
        Some("https://github.com/?x=user:pass@evil.example/repo".to_string()),
    );
}

#[test]
fn extract_repository_percent_encodes_whitespace_in_urls() {
    let manifest = serde_json::json!({ "repository": "https://example.com/a b" });
    assert_eq!(extract_repository(&manifest), Some("https://example.com/a%20b".to_string()));
}

#[test]
fn extract_repository_drops_an_incomplete_percent_escape() {
    // The hosted parser decodes a shorthand's committish, so the `%251` of
    // the last value reaches the derived URL as a stray `%1`.
    for value in ["https://example.com/%zz", "https://example.com/%", "owner/repo#release%251"] {
        let manifest = serde_json::json!({ "repository": value });
        assert_eq!(extract_repository(&manifest), None, "value: {value:?}");
    }
}

#[test]
fn extract_repository_drops_unparsable_absolute_urls() {
    for value in ["https://", "http://user:pass@"] {
        let manifest = serde_json::json!({ "repository": value });
        assert_eq!(extract_repository(&manifest), None, "value: {value:?}");
    }
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
fn classify_license_spdx_ids() {
    for (license, id) in [
        ("MIT", "MIT"),
        ("mit", "MIT"),
        (" MIT ", "MIT"),
        ("GPL-2.0", "GPL-2.0"),
        ("gpl-2.0", "GPL-2.0"),
        ("GFDL-1.1-invariants-only", "GFDL-1.1-invariants-only"),
        ("WTFPL", "WTFPL"),
    ] {
        assert_eq!(classify_license(license), serde_json::json!({ "license": { "id": id } }));
    }
}

#[test]
fn classify_license_spdx_2_3_expressions() {
    for expression in [
        "MIT OR Apache-2.0",
        "mit OR apache-2.0",
        "MIT AND ISC",
        "GPL-2.0+",
        "LicenseRef-Proprietary",
        "DocumentRef-doc:LicenseRef-Custom",
        "GPL-2.0-only WITH Classpath-exception-2.0",
        "gpl-2.0-only WITH classpath-exception-2.0",
        "(MIT AND Apache-2.0) OR ISC",
    ] {
        assert_eq!(classify_license(expression), serde_json::json!({ "expression": expression }));
    }
}

#[test]
fn classify_license_free_form_names() {
    for license in [
        "BDS-3-Clause",
        "UNLICENSED",
        "Proprietary License",
        "SEE LICENSE IN LICENSE.md",
        "LLVM-exception",
        "GFDL-1.1-invariants",
        "GFDL-1.1-invariants OR MIT",
        "NONE",
        "NOASSERTION",
        "NOASSERTION OR MIT",
        "MIT OR BDS-3-Clause",
        "MIT or Apache-2.0",
        "MIT WITH AdditionRef-Custom",
        "MIT WITH Unknown-exception",
        "MIT/Apache-2.0",
        "MIT OR",
    ] {
        assert_eq!(
            classify_license(license),
            serde_json::json!({ "license": { "name": license } }),
        );
    }
}

#[test]
fn url_without_credentials_removes_userinfo() {
    assert_eq!(
        url_without_credentials("https://user:token@github.com/foo/bar").map(|u| u.to_string()),
        Some("https://github.com/foo/bar".to_string()),
    );
}

#[test]
fn url_without_credentials_keeps_an_ssh_login() {
    assert_eq!(
        url_without_credentials("git+ssh://git@github.com/foo/bar.git").map(|u| u.to_string()),
        Some("git+ssh://git@github.com/foo/bar.git".to_string()),
    );
}

#[test]
fn url_without_credentials_removes_a_username_only_authority() {
    for (url, expected) in [
        ("https://token@github.com/foo/bar", "https://github.com/foo/bar"),
        ("not-ssh://token@example.com/foo/bar", "not-ssh://example.com/foo/bar"),
    ] {
        assert_eq!(
            url_without_credentials(url).map(|u| u.to_string()),
            Some(expected.to_string()),
            "url: {url:?}",
        );
    }
}

#[test]
fn url_without_credentials_no_credentials() {
    assert_eq!(
        url_without_credentials("https://github.com/foo/bar").map(|u| u.to_string()),
        Some("https://github.com/foo/bar".to_string()),
    );
}

#[test]
fn url_without_credentials_ignores_userinfo_lookalikes_in_query() {
    // The query, not the authority, carries the `@`: the URL must come out
    // unchanged, not re-pointed at the query's host.
    let url = "https://github.com?x=user:pass@evil.example/repo";
    assert_eq!(
        url_without_credentials(url).map(|u| u.to_string()),
        Some("https://github.com/?x=user:pass@evil.example/repo".to_string()),
    );
}

#[test]
fn url_without_credentials_percent_encodes_whitespace() {
    assert_eq!(
        url_without_credentials("https://example.com/a b").map(|u| u.to_string()),
        Some("https://example.com/a%20b".to_string()),
    );
}

#[test]
fn url_without_credentials_rejects_unparsable_values() {
    for value in ["https://", "http://user:pass@"] {
        assert_eq!(url_without_credentials(value), None, "value: {value:?}");
    }
}

#[test]
fn extract_bugs_url_keeps_http_urls_in_both_manifest_shapes() {
    let string_form = serde_json::json!({ "bugs": "https://tracker.example.com/issues" });
    assert_eq!(
        extract_bugs_url(&string_form),
        Some("https://tracker.example.com/issues".to_string()),
    );
    let object_form =
        serde_json::json!({ "bugs": { "url": "https://tracker.example.com/issues" } });
    assert_eq!(
        extract_bugs_url(&object_form),
        Some("https://tracker.example.com/issues".to_string()),
    );
}

#[test]
fn extract_bugs_url_strips_password_bearing_userinfo() {
    let manifest = serde_json::json!({ "bugs": "https://user:token@tracker.example.com/issues" });
    assert_eq!(extract_bugs_url(&manifest), Some("https://tracker.example.com/issues".to_string()));
}

#[test]
fn extract_bugs_url_strips_bare_username() {
    let manifest = serde_json::json!({ "bugs": "https://user@tracker.example.com/issues" });
    assert_eq!(extract_bugs_url(&manifest), Some("https://tracker.example.com/issues".to_string()));
}

#[test]
fn extract_bugs_url_does_not_treat_query_userinfo_lookalikes_as_credentials() {
    let manifest =
        serde_json::json!({ "bugs": "https://github.com?x=user:pass@evil.example/repo" });
    assert_eq!(
        extract_bugs_url(&manifest),
        Some("https://github.com/?x=user:pass@evil.example/repo".to_string()),
    );
}

#[test]
fn extract_bugs_url_drops_non_http_schemes_and_unparsable_values() {
    for value in
        ["mailto:bugs@example.com", "git+https://github.com/foo/bar.git", "https://user:pass@"]
    {
        let manifest = serde_json::json!({ "bugs": value });
        assert_eq!(extract_bugs_url(&manifest), None, "value: {value:?}");
    }
}

#[test]
fn normalize_link_path_escape_returns_none() {
    assert_eq!(normalize_link_path(".", ".."), None);
}

/// An SBOM checksum reads as an assurance that the artifact was verified
/// against it. Only the shapes pnpm actually checks the downloaded bytes
/// against may supply one.
#[test]
fn integrity_string_publishes_only_verified_hashes() {
    const HASH: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";
    let hash = || HASH.parse::<ssri::Integrity>().expect("parse integrity");

    let registry = LockfileResolution::Registry(pnpm_lockfile::RegistryResolution {
        integrity: hash(),
        revision: None,
    });
    assert_eq!(integrity_string(&registry).as_deref(), Some(HASH));

    let binary = LockfileResolution::Binary(pnpm_lockfile::BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz".to_string(),
        integrity: hash(),
        bin: pnpm_lockfile::BinarySpec::Single("bin/node".to_string()),
        archive: pnpm_lockfile::BinaryArchive::Tarball,
        prefix: None,
    });
    assert_eq!(integrity_string(&binary).as_deref(), Some(HASH));

    // Nothing verifies a git checkout against a hash, so one recorded on
    // the entry must not be republished as a checksum.
    let git = LockfileResolution::Git(pnpm_lockfile::GitResolution {
        repo: "https://github.com/foo/bar.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: Some(HASH.to_string()),
        path: None,
    });
    assert_eq!(integrity_string(&git), None);
}
