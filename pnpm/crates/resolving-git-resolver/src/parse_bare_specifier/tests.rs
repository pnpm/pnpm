use super::{PartialSpec, correct_url, parse_bare_specifier, parse_git_params};

#[test]
fn rejects_non_git_url() {
    assert!(parse_bare_specifier("1.2.3").is_none());
    assert!(parse_bare_specifier("https://example.com/package.tar.gz").is_none());
    assert!(parse_bare_specifier("https://example.com/file").is_none());
}

#[test]
fn parses_github_shortcut_to_hosted() {
    let kind = parse_bare_specifier("zkochan/is-negative#1.0.0").expect("hosted");
    assert!(matches!(kind, PartialSpec::Hosted(_)));
}

#[test]
fn parses_plain_https_dot_git_to_direct() {
    let kind = parse_bare_specifier("https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git#abc")
        .expect("direct");
    match kind {
        PartialSpec::Direct(spec) => {
            assert_eq!(spec.fetch_spec, "https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git");
            assert_eq!(spec.git_committish.as_deref(), Some("abc"));
        }
        PartialSpec::Hosted(_) => panic!("expected Direct"),
    }
}

#[test]
fn parse_git_params_splits_semver_path_committish() {
    let params = parse_git_params(Some("semver:^1.0.0"));
    assert_eq!(params.git_range.as_deref(), Some("^1.0.0"));
    assert!(params.git_committish.is_none());

    let params = parse_git_params(Some("path:/sub"));
    assert_eq!(params.path.as_deref(), Some("/sub"));

    let params = parse_git_params(Some("beta&path:/packages/x"));
    assert_eq!(params.git_committish.as_deref(), Some("beta"));
    assert_eq!(params.path.as_deref(), Some("/packages/x"));
}

#[test]
fn correct_url_rewrites_scp_style_colon() {
    assert_eq!(
        correct_url("ssh://username:password@example.com:repo.git"),
        "ssh://username:password@example.com/repo.git",
    );
    assert_eq!(
        correct_url("git+ssh://username:password@example.com:repo.git"),
        "git+ssh://username:password@example.com/repo.git",
    );
}

#[test]
fn correct_url_keeps_numeric_port() {
    assert_eq!(
        correct_url("ssh://username:password@example.com:22/repo/@foo.git"),
        "ssh://username:password@example.com:22/repo/@foo.git",
    );
}

#[test]
fn correct_url_keeps_bracketed_ipv6_host() {
    assert_eq!(correct_url("ssh://[::1]/repo.git"), "ssh://[::1]/repo.git");
    assert_eq!(
        correct_url("ssh://[2001:db8::1]/team/repo.git"),
        "ssh://[2001:db8::1]/team/repo.git",
    );
    assert_eq!(correct_url("ssh://[::1]:2222/repo.git"), "ssh://[::1]:2222/repo.git");
    assert_eq!(correct_url("ssh://[::1]:team/repo.git"), "ssh://[::1]/team/repo.git");
    assert_eq!(correct_url("ssh://git@[::1]/repo.git"), "ssh://git@[::1]/repo.git");
    assert_eq!(correct_url("ssh://git@[::1]:team/repo.git"), "ssh://git@[::1]/team/repo.git");
}

#[test]
fn finalize_direct_returns_spec_unchanged() {
    let kind = parse_bare_specifier("git+https://example.com/repo.git#abc").expect("direct");
    let spec = kind.finalize();
    assert_eq!(spec.fetch_spec, "https://example.com/repo.git");
    assert_eq!(spec.git_committish.as_deref(), Some("abc"));
}

#[test]
fn finalize_hosted_uses_canonical_https() {
    let kind = parse_bare_specifier("zkochan/is-negative").expect("hosted");
    let spec = kind.finalize();
    assert_eq!(spec.fetch_spec, "https://github.com/zkochan/is-negative.git");
    assert_eq!(spec.normalized_bare_specifier, "github:zkochan/is-negative");
    assert!(spec.hosted.is_some());
}

#[test]
fn finalize_hosted_ssh_input_becomes_the_same_https_identity() {
    let kind = parse_bare_specifier("git+ssh://git@github.com/foo/bar.git").expect("hosted");
    let spec = kind.finalize();
    assert_eq!(spec.fetch_spec, "https://github.com/foo/bar.git");
    assert_eq!(spec.normalized_bare_specifier, "github:foo/bar");
    assert!(spec.hosted.is_some());
}

#[test]
fn finalize_hosted_scp_style_input_becomes_the_same_https_identity() {
    let kind = parse_bare_specifier("git@github.com:foo/bar.git#v1.0.0").expect("hosted");
    let spec = kind.finalize();
    assert_eq!(spec.fetch_spec, "https://github.com/foo/bar.git");
    assert_eq!(spec.normalized_bare_specifier, "github:foo/bar#v1.0.0");
    assert_eq!(spec.git_committish.as_deref(), Some("v1.0.0"));
}

#[test]
fn finalize_hosted_auth_url_kept_verbatim_without_archive_eligibility() {
    let kind = parse_bare_specifier("git+https://token:x-oauth-basic@github.com/foo/bar.git")
        .expect("hosted");
    let spec = kind.finalize();
    assert_eq!(spec.fetch_spec, "https://token:x-oauth-basic@github.com/foo/bar.git");
    assert_eq!(
        spec.normalized_bare_specifier,
        "git+https://token:x-oauth-basic@github.com/foo/bar.git",
    );
    assert!(spec.hosted.is_none(), "credentialed URL must never resolve to a host archive");
}

// [pnpm/pnpm#13999](https://github.com/pnpm/pnpm/issues/13999). Each row
// is `(input, normalized_bare_specifier, git_committish, git_range)`.
#[test]
fn every_representation_of_a_hosted_specifier_keeps_its_committish() {
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    let auth_url = |committish: &str| {
        format!("git+https://token:x-oauth-basic@github.com/foo/bar.git#{committish}")
    };
    let cases: [(String, String, Option<&str>, Option<&str>); 7] = [
        ("foo/bar#develop".into(), "github:foo/bar#develop".into(), Some("develop"), None),
        ("foo/bar#v1.0.0".into(), "github:foo/bar#v1.0.0".into(), Some("v1.0.0"), None),
        (
            "foo/bar#semver:^1.0.0".into(),
            "github:foo/bar#semver:^1.0.0".into(),
            None,
            Some("^1.0.0"),
        ),
        (format!("foo/bar#{SHA}"), format!("github:foo/bar#{SHA}"), Some(SHA), None),
        (auth_url("develop"), auth_url("develop"), Some("develop"), None),
        (auth_url(SHA), auth_url(SHA), Some(SHA), None),
        (auth_url("semver:^1.0.0"), auth_url("semver:^1.0.0"), None, Some("^1.0.0")),
    ];
    for (input, expected_specifier, expected_committish, expected_range) in &cases {
        let spec = parse_bare_specifier(input).expect("hosted").finalize();
        assert!(!spec.fetch_spec.contains('#'), "ls-remote target keeps a committish: {input}");
        assert_eq!(
            (
                spec.normalized_bare_specifier.as_str(),
                spec.git_committish.as_deref(),
                spec.git_range.as_deref(),
            ),
            (expected_specifier.as_str(), *expected_committish, *expected_range),
            "input: {input}",
        );
    }
}

// Ported `parsePref.test.ts` SCP-style URL repair cases. Each row
// is `(input, expected_fetch_spec)`.
#[test]
fn fetch_spec_for_scp_style_inputs() {
    let cases: &[(&str, &str)] = &[
        (
            "ssh://username:password@example.com:repo.git",
            "ssh://username:password@example.com/repo.git",
        ),
        (
            "ssh://username:password@example.com:repo/@foo.git",
            "ssh://username:password@example.com/repo/@foo.git",
        ),
        (
            "ssh://username:password@example.com:22/repo/@foo.git",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        (
            "ssh://username:password@example.com:22repo/@foo.git",
            "ssh://username:password@example.com/22repo/@foo.git",
        ),
        (
            "ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        (
            "ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        (
            "git+ssh://username:password@example.com:repo.git",
            "ssh://username:password@example.com/repo.git",
        ),
        (
            "git+ssh://username:password@example.com:repo/@foo.git",
            "ssh://username:password@example.com/repo/@foo.git",
        ),
        (
            "git+ssh://username:password@example.com:22/repo/@foo.git",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        (
            "git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        (
            "git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev",
            "ssh://username:password@example.com:22/repo/@foo.git",
        ),
        ("git+https://github.com/pnpm/pnpm.git", "https://github.com/pnpm/pnpm.git"),
        (
            "git+ssh://git@sub.domain.tld:internal-app/sub-path/service-name.git",
            "ssh://git@sub.domain.tld/internal-app/sub-path/service-name.git",
        ),
    ];
    for (input, expected) in cases {
        let kind = parse_bare_specifier(input).expect("parse claims input");
        let spec = kind.finalize();
        assert_eq!(
            spec.fetch_spec,
            *expected,
            "input {input}: expected fetch_spec {expected}, got {got}",
            got = spec.fetch_spec,
        );
    }
}

#[test]
fn fetch_spec_for_inputs_without_user_info() {
    let cases: &[(&str, &str)] = &[
        ("ssh://git.example.com/team/repo.git", "ssh://git.example.com/team/repo.git"),
        ("ssh://git.example.com:2222/team/repo.git", "ssh://git.example.com:2222/team/repo.git"),
        ("ssh://git.example.com:team/repo.git", "ssh://git.example.com/team/repo.git"),
        ("ssh://git.example.com:repo.git", "ssh://git.example.com/repo.git"),
        ("git+ssh://git.example.com/team/repo.git", "ssh://git.example.com/team/repo.git"),
        ("git+ssh://git.example.com:team/repo.git", "ssh://git.example.com/team/repo.git"),
    ];
    for (input, expected) in cases {
        let kind = parse_bare_specifier(input).expect("parse claims input");
        let spec = kind.finalize();
        assert_eq!(spec.fetch_spec, *expected, "input {input}");
    }
}

#[test]
fn fetch_spec_for_bracketed_ipv6_hosts() {
    let cases: &[(&str, &str)] = &[
        ("ssh://[::1]/repo.git", "ssh://[::1]/repo.git"),
        ("ssh://[2001:db8::1]/team/repo.git", "ssh://[2001:db8::1]/team/repo.git"),
        ("ssh://[::1]:2222/repo.git", "ssh://[::1]:2222/repo.git"),
        ("ssh://[::1]:team/repo.git", "ssh://[::1]/team/repo.git"),
        ("ssh://git@[::1]/repo.git", "ssh://git@[::1]/repo.git"),
        ("ssh://git@[::1]:team/repo.git", "ssh://git@[::1]/team/repo.git"),
    ];
    for (input, expected) in cases {
        let kind = parse_bare_specifier(input).expect("parse claims input");
        let spec = kind.finalize();
        assert_eq!(spec.fetch_spec, *expected, "input {input}");
    }
}

#[test]
fn path_extracted_from_scp_style_inputs() {
    let cases: &[(&str, Option<&str>)] = &[
        ("ssh://username:password@example.com:repo.git#path:/a/@b", Some("/a/@b")),
        ("ssh://username:password@example.com:repo/@foo.git#path:/a/@b", Some("/a/@b")),
        ("ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b", Some("/a/@b")),
        ("ssh://username:password@example.com:22repo/@foo.git#path:/a/@b", Some("/a/@b")),
        ("ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev", Some("/a/@b")),
        ("git+ssh://username:password@example.com:repo.git#path:/a/@b", Some("/a/@b")),
        ("git+ssh://username:password@example.com:repo/@foo.git#path:/a/@b", Some("/a/@b")),
        ("git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b", Some("/a/@b")),
        ("git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev", Some("/a/@b")),
        ("ssh://username:password@example.com:repo.git", None),
        ("ssh://username:password@example.com:22/repo/@foo.git#dev", None),
        ("git+ssh://username:password@example.com:repo.git", None),
        ("git+ssh://username:password@example.com:22/repo/@foo.git#dev", None),
    ];
    for (input, expected_path) in cases {
        let kind = parse_bare_specifier(input).expect("parse claims input");
        let spec = kind.finalize();
        assert_eq!(spec.path.as_deref(), *expected_path, "input {input}: path mismatch");
    }
}

// Ported "plain http/https URLs ending in .git should be recognized" suite.
#[test]
fn plain_http_dot_git_recognized() {
    let cases: &[(&str, &str)] = &[
        (
            "https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git",
            "https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git",
        ),
        (
            "https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git#6daccff309fca1e7561a43984d42fa4f829ce06d",
            "https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git",
        ),
        ("http://example.com/repo.git", "http://example.com/repo.git"),
        ("http://example.com/repo.git#main", "http://example.com/repo.git"),
    ];
    for (input, expected) in cases {
        let kind = parse_bare_specifier(input).expect("claim");
        let spec = kind.finalize();
        assert_eq!(spec.fetch_spec, *expected, "input {input}");
    }
}

#[test]
fn plain_http_non_dot_git_declined() {
    for input in [
        "https://example.com/package.tar.gz",
        "https://example.com/package.tgz",
        "https://example.com/file",
    ] {
        assert!(parse_bare_specifier(input).is_none(), "input {input}");
    }
}
