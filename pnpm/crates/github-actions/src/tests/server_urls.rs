use super::{FakeGitRunner, SilentReporter, find_outdated_with_runner};
use crate::validate_server_url;
use std::fs;

#[test]
fn server_urls_allow_https_and_loopback_http_only() {
    for url in [
        "https://github.com",
        "https://user:secret@github.example.com/base/",
        "http://localhost:1234",
        "http://127.0.0.2",
        "http://[::1]:1234",
    ] {
        assert!(validate_server_url(url).is_ok(), "{url}");
    }
    for url in [
        "http://github.example.com",
        "http://localhost.evil.example",
        "http://127.example.com",
        "file:///tmp/repos",
        "ext::command",
        "https://",
        "http://user:secret@remote.example",
    ] {
        let error = validate_server_url(url).expect_err("unsafe server");
        assert_eq!(
            error.code().expect("code").to_string(),
            "ERR_PNPM_GITHUB_ACTIONS_SERVER_PROTOCOL",
        );
        assert!(!error.to_string().contains("secret"));
    }
}

#[tokio::test]
async fn homepages_do_not_expose_server_credentials() {
    let root = tempfile::tempdir().expect("temp directory");
    let directory = root.path().join(".github/workflows");
    fs::create_dir_all(&directory).expect("workflow directory");
    fs::write(
        directory.join("ci.yml"),
        "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n",
    )
    .expect("workflow");
    let outdated = find_outdated_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://user:secret@github.example.com",
        &FakeGitRunner,
    )
    .await
    .expect("outdated");
    assert_eq!(outdated[0].homepage, "https://github.example.com/actions/checkout");
}
