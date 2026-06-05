use std::sync::Mutex;

use super::{
    GitProbe, PartialSpec, ProbeFuture, correct_url, parse_bare_specifier, parse_git_params,
};

struct Fake {
    head_ok: bool,
    ls_ok: bool,
    calls: Mutex<Vec<String>>,
}

impl GitProbe for Fake {
    fn https_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("head {url}"));
            self.head_ok
        })
    }
    fn ls_remote_exit_code<'a>(&'a self, repo: &'a str) -> ProbeFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("ls {repo}"));
            self.ls_ok
        })
    }
}

fn fake() -> Fake {
    Fake { head_ok: true, ls_ok: true, calls: Mutex::new(Vec::new()) }
}

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

#[tokio::test]
async fn finalize_direct_returns_spec_unchanged() {
    let kind = parse_bare_specifier("git+https://example.com/repo.git#abc").expect("direct");
    let probe = fake();
    let spec = kind.finalize(&probe).await;
    assert_eq!(spec.fetch_spec, "https://example.com/repo.git");
    assert_eq!(spec.git_committish.as_deref(), Some("abc"));
    // Direct spec shouldn't probe.
    assert!(probe.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn finalize_hosted_prefers_https_when_public() {
    let kind = parse_bare_specifier("zkochan/is-negative").expect("hosted");
    let probe = fake();
    let spec = kind.finalize(&probe).await;
    assert_eq!(spec.fetch_spec, "https://github.com/zkochan/is-negative.git");
    assert!(spec.hosted.is_some());
}

#[tokio::test]
async fn finalize_hosted_falls_back_to_ssh_when_private() {
    let kind = parse_bare_specifier("foo/private-repo").expect("hosted");
    let probe = Fake { head_ok: false, ls_ok: false, calls: Mutex::new(Vec::new()) };
    let spec = kind.finalize(&probe).await;
    assert_eq!(spec.fetch_spec, "git+ssh://git@github.com/foo/private-repo.git");
}

// Ported `parsePref.test.ts` SCP-style URL repair cases. Each row
// is `(input, expected_fetch_spec)`.
#[tokio::test]
async fn fetch_spec_for_scp_style_inputs() {
    let probe = fake();
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
        let spec = kind.finalize(&probe).await;
        assert_eq!(
            spec.fetch_spec,
            *expected,
            "input {input}: expected fetch_spec {expected}, got {got}",
            got = spec.fetch_spec,
        );
    }
}

// Ported `parsePref.test.ts` path-extraction cases.
#[tokio::test]
async fn path_extracted_from_scp_style_inputs() {
    let probe = fake();
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
        let spec = kind.finalize(&probe).await;
        assert_eq!(spec.path.as_deref(), *expected_path, "input {input}: path mismatch");
    }
}

// Ported "plain http/https URLs ending in .git should be recognized" suite.
#[tokio::test]
async fn plain_http_dot_git_recognized() {
    let probe = fake();
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
        let spec = kind.finalize(&probe).await;
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
