use super::Source;

#[test]
fn executable_transports_credentials_and_escaping_subdirectories_are_rejected() {
    for source in [
        "git+ext::sh -c id",
        "git+http://example.test/repo@main",
        "git+https://user:password@example.test/repo@main",
        "git+ssh://git:password@example.test/repo@main",
        "git+https://example.test/repo@--upload-pack=command",
        "git+https://example.test/repo@%2D%2Dupload-pack=command",
        "git+https://example.test/repo@main#subdirectory=../outside",
        "git+https://example.test/repo@main#subdirectory=%2Foutside",
        "git+https://example.test/repo@main#subdirectory=ok&subdirectory=outside",
        "https://user:password@example.test/demo.whl",
        "https://example.test/demo.whl#sha256=short",
    ] {
        assert!(Source::parse(source).is_err(), "accepted {source}");
    }
    let Source::Git(git) = Source::parse(
        "git+ssh://git@example.test/repo@feature%2Fbranch#subdirectory=packages%2Fdemo",
    )
    .unwrap() else {
        panic!("git source expected")
    };
    assert_eq!(git.url, "ssh://git@example.test/repo");
    assert_eq!(git.requested_revision, "feature/branch");
    assert_eq!(git.subdirectory.as_deref(), Some("packages/demo"));
}

#[test]
fn wheel_hash_constraints_allow_an_unhashed_declaration_but_reject_different_hashes() {
    let plain = Source::parse("https://example.test/demo.whl").unwrap();
    let pinned =
        Source::parse(&format!("https://example.test/demo.whl#sha256={}", "a".repeat(64))).unwrap();
    let different =
        Source::parse(&format!("https://example.test/demo.whl#sha256={}", "b".repeat(64))).unwrap();
    assert!(plain.compatible_with(&pinned));
    assert!(pinned.compatible_with(&plain));
    assert!(!pinned.compatible_with(&different));
}
