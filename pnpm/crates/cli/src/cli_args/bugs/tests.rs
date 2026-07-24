use std::{cell::RefCell, fs, io, path::Path};

use pacquet_config::Config;
use pacquet_network_web_auth::OpenUrl;
use serde_json::json;

use super::{
    BugsArgs, is_http_url, parse_package_spec, pick_bugs_url, repository_to_issues_url,
    try_hosted_git_shorthand,
};

#[test]
fn pick_bugs_url_returns_bugs_url_from_object() {
    let manifest = json!({
        "bugs": { "url": "https://github.com/test/pkg/issues" }
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_returns_bugs_url_from_string() {
    let manifest = json!({
        "bugs": "https://github.com/test/pkg/issues"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_falls_back_to_repository_issues_url() {
    let manifest = json!({
        "repository": "https://github.com/test/pkg"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/test/pkg/issues"));
}

#[test]
fn pick_bugs_url_prefers_bugs_over_repository() {
    let manifest = json!({
        "bugs": { "url": "https://github.com/other/issues" },
        "repository": "https://github.com/test/pkg"
    });
    assert_eq!(pick_bugs_url(&manifest).as_deref(), Some("https://github.com/other/issues"));
}

#[test]
fn pick_bugs_url_returns_none_when_no_bugs_url() {
    let manifest = json!({ "name": "test-pkg" });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn pick_bugs_url_returns_none_for_non_http_bugs() {
    let manifest = json!({
        "bugs": "ftp://example.com/bugs"
    });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn pick_bugs_url_returns_none_for_empty_bugs() {
    let manifest = json!({
        "bugs": {}
    });
    assert_eq!(pick_bugs_url(&manifest), None);
}

#[test]
fn repo_url_normalizes_git_https_with_dot_git() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_trailing_slash() {
    assert_eq!(
        repository_to_issues_url("https://github.com/test/pkg/"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_shorthand_owner_repo() {
    assert_eq!(
        repository_to_issues_url("test/pkg"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_github_shorthand() {
    assert_eq!(
        repository_to_issues_url("github:test/pkg"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_git_ssh_url() {
    assert_eq!(
        repository_to_issues_url("git+ssh://git@github.com/test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_gitlab_shorthand() {
    assert_eq!(
        repository_to_issues_url("gitlab:test/pkg"),
        Some("https://gitlab.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_falls_back_for_self_hosted_git_server() {
    assert_eq!(
        repository_to_issues_url("git+https://git.example.com/test/pkg.git"),
        Some("https://git.example.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_handles_dot_git_with_trailing_slash() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git/"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("git+https://github.com/test/pkg.git#main"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_scp_style_ssh() {
    assert_eq!(
        repository_to_issues_url("git@github.com:test/pkg.git"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_scp_style_ssh_with_dot_git_and_trailing_slash() {
    assert_eq!(
        repository_to_issues_url("git@github.com:test/pkg.git/"),
        Some("https://github.com/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_resolves_bitbucket_shorthand() {
    assert_eq!(
        repository_to_issues_url("bitbucket:test/pkg"),
        Some("https://bitbucket.org/test/pkg/issues".to_string()),
    );
}

#[test]
fn repo_url_returns_none_for_invalid_url() {
    assert_eq!(repository_to_issues_url("not-a-url"), None);
}

#[test]
fn is_http_url_accepts_https() {
    assert!(is_http_url("https://example.com"));
}

#[test]
fn is_http_url_accepts_http() {
    assert!(is_http_url("http://example.com"));
}

#[test]
fn is_http_url_rejects_ftp() {
    assert!(!is_http_url("ftp://example.com"));
}

#[test]
fn is_http_url_rejects_invalid() {
    assert!(!is_http_url("not-a-url"));
}

#[test]
fn hosted_shorthand_recognises_github_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("github:owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_gitlab_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("gitlab:owner/repo"),
        Some("https://gitlab.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_bitbucket_prefix() {
    assert_eq!(
        try_hosted_git_shorthand("bitbucket:owner/repo"),
        Some("https://bitbucket.org/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_recognises_bare_owner_repo() {
    assert_eq!(
        try_hosted_git_shorthand("owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn hosted_shorthand_returns_none_for_urls() {
    assert_eq!(try_hosted_git_shorthand("https://github.com/owner/repo"), None);
}

#[test]
fn hosted_shorthand_returns_none_for_git_urls() {
    assert_eq!(try_hosted_git_shorthand("git@github.com:owner/repo.git"), None);
}

#[test]
fn parse_spec_bare_name() {
    assert_eq!(parse_package_spec("foo"), ("foo", None));
}

#[test]
fn parse_spec_name_with_version() {
    assert_eq!(parse_package_spec("foo@1.0.0"), ("foo", Some("1.0.0")));
}

#[test]
fn parse_spec_scoped_package() {
    assert_eq!(parse_package_spec("@scope/foo"), ("@scope/foo", None));
}

#[test]
fn parse_spec_scoped_with_version() {
    assert_eq!(parse_package_spec("@scope/foo@1.0.0"), ("@scope/foo", Some("1.0.0")));
}

#[test]
fn parse_spec_scoped_with_tag() {
    assert_eq!(parse_package_spec("@scope/foo@latest"), ("@scope/foo", Some("latest")));
}

#[test]
fn parse_spec_trims_whitespace() {
    assert_eq!(parse_package_spec("  foo  "), ("foo", None));
}

#[test]
fn parse_spec_strips_version_from_name_with_version() {
    let (name, tag) = parse_package_spec("react@18.2.0");
    assert_eq!(name, "react");
    assert_eq!(tag, Some("18.2.0"));
}

#[test]
fn repo_url_resolves_github_dot_com_shorthand_without_scheme() {
    assert_eq!(
        repository_to_issues_url("github.com/owner/repo"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_retains_ssh_port() {
    assert_eq!(
        repository_to_issues_url("ssh://git@git.example.com:2222/owner/repo.git"),
        Some("https://git.example.com:2222/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_shorthand_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("github:owner/repo#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
    assert_eq!(
        repository_to_issues_url("owner/repo.git#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

#[test]
fn repo_url_strips_scp_fragment_and_query() {
    assert_eq!(
        repository_to_issues_url("git@github.com:owner/repo.git#main"),
        Some("https://github.com/owner/repo/issues".to_string()),
    );
}

// Per-test browser fake recording the URLs `pnpm bugs` opens. Its buffer is
// fn-local, so each `#[test]` records into its own and concurrent tests never
// share it. Each test names the `run_bugs_*` helper it drives, so every emitted
// helper is used and none needs a `dead_code` allow.
macro_rules! recording_browser {
    ($($helper:ident),* $(,)?) => {
        thread_local! {
            static OPENED_URLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }

        // [`OpenUrl`] fake standing in for the user's browser; mirrors the
        // mocked `open` module in the TypeScript tests
        // (`pnpm11/deps/inspection/commands/test/bugs.ts`).
        struct RecordingBrowser;

        impl OpenUrl for RecordingBrowser {
            fn open_url(url: &str) -> io::Result<()> {
                OPENED_URLS.with(|urls| urls.borrow_mut().push(url.to_owned()));
                Ok(())
            }
        }

        fn opened_urls() -> Vec<String> {
            OPENED_URLS.with(RefCell::take)
        }

        $( recording_browser!(@helper $helper); )*
    };

    (@helper run_bugs_in_project) => {
        async fn run_bugs_in_project(manifest: &str) -> miette::Result<()> {
            let dir = tempfile::tempdir().expect("create temp project dir");
            fs::write(dir.path().join("package.json"), manifest).expect("write package.json");
            let args = BugsArgs { registry: None, packages: Vec::new() };
            args.run::<RecordingBrowser>(&Config::default(), dir.path()).await
        }
    };
    (@helper run_bugs_against_registry) => {
        async fn run_bugs_against_registry(registry: String, package: &str) -> miette::Result<()> {
            let config = Config { registry, ..Config::default() };
            let args = BugsArgs { registry: None, packages: vec![package.to_owned()] };
            args.run::<RecordingBrowser>(&config, Path::new(".")).await
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `recording_browser!` helper `",
            stringify!($unknown),
            "`; expected one of: run_bugs_in_project, run_bugs_against_registry",
        ));
    };
}

#[tokio::test]
async fn run_opens_bugs_url_from_local_manifest_bugs_object() {
    recording_browser!(run_bugs_in_project);
    run_bugs_in_project(
        r#"{"name":"test-pkg","bugs":{"url":"https://github.com/test/pkg/issues"}}"#,
    )
    .await
    .expect("bugs must succeed");
    assert_eq!(opened_urls(), ["https://github.com/test/pkg/issues"]);
}

#[tokio::test]
async fn run_opens_bugs_url_from_local_manifest_bugs_string() {
    recording_browser!(run_bugs_in_project);
    run_bugs_in_project(r#"{"name":"test-pkg","bugs":"https://github.com/test/pkg/issues"}"#)
        .await
        .expect("bugs must succeed");
    assert_eq!(opened_urls(), ["https://github.com/test/pkg/issues"]);
}

#[tokio::test]
async fn run_opens_repository_issues_url_when_bugs_is_missing() {
    recording_browser!(run_bugs_in_project);
    run_bugs_in_project(r#"{"name":"test-pkg","repository":"https://github.com/test/pkg"}"#)
        .await
        .expect("bugs must succeed");
    assert_eq!(opened_urls(), ["https://github.com/test/pkg/issues"]);
}

#[tokio::test]
async fn run_normalizes_git_plus_https_repository_url_with_dot_git() {
    recording_browser!(run_bugs_in_project);
    run_bugs_in_project(
        r#"{"name":"test-pkg","repository":{"url":"git+https://github.com/test/pkg.git"}}"#,
    )
    .await
    .expect("bugs must succeed");
    assert_eq!(opened_urls(), ["https://github.com/test/pkg/issues"]);
}

fn version_response(name: &str, extra_fields: serde_json::Value) -> String {
    let mut version = json!({
        "name": name,
        "version": "1.0.0",
        "dist": {
            "tarball": "https://example.com/pkg.tgz",
        },
    });
    let fields = version.as_object_mut().expect("version response is an object");
    let serde_json::Value::Object(extra) = extra_fields else {
        panic!("extra fields must be an object");
    };
    fields.extend(extra);
    version.to_string()
}

#[tokio::test]
async fn run_opens_bugs_url_of_registry_package() {
    recording_browser!(run_bugs_against_registry);
    let mut server = mockito::Server::new_async().await;
    let body = version_response(
        "is-negative",
        json!({ "bugs": { "url": "https://github.com/kevva/is-negative/issues" } }),
    );
    let mock = server
        .mock("GET", "/is-negative/latest")
        .with_status(200)
        .with_body(&body)
        .create_async()
        .await;

    run_bugs_against_registry(server.url(), "is-negative").await.expect("bugs must succeed");

    mock.assert_async().await;
    assert_eq!(opened_urls(), ["https://github.com/kevva/is-negative/issues"]);
}

#[tokio::test]
async fn run_opens_repository_issues_url_of_registry_package() {
    recording_browser!(run_bugs_against_registry);
    let mut server = mockito::Server::new_async().await;
    let body = version_response(
        "test-pkg",
        json!({ "repository": { "url": "git+https://github.com/test/pkg.git" } }),
    );
    let mock = server
        .mock("GET", "/test-pkg/latest")
        .with_status(200)
        .with_body(&body)
        .create_async()
        .await;

    run_bugs_against_registry(server.url(), "test-pkg").await.expect("bugs must succeed");

    mock.assert_async().await;
    assert_eq!(opened_urls(), ["https://github.com/test/pkg/issues"]);
}

#[tokio::test]
async fn run_encodes_scoped_package_name_in_registry_request() {
    recording_browser!(run_bugs_against_registry);
    let mut server = mockito::Server::new_async().await;
    let body = version_response(
        "@scope/pkg",
        json!({ "bugs": { "url": "https://github.com/scope/pkg/issues" } }),
    );
    let mock = server
        .mock("GET", "/@scope%2Fpkg/latest")
        .with_status(200)
        .with_body(&body)
        .create_async()
        .await;

    run_bugs_against_registry(server.url(), "@scope/pkg").await.expect("bugs must succeed");

    mock.assert_async().await;
    assert_eq!(opened_urls(), ["https://github.com/scope/pkg/issues"]);
}
