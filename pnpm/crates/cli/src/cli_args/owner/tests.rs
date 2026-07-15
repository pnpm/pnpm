use pacquet_config::Config;
use serde_json::json;

use super::{OwnerArgs, OwnerError};

#[test]
fn owner_entry_deserializes() {
    let json = r#"{"username": "alice", "email": "alice@example.com"}"#;
    let entry: super::OwnerEntry = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(entry.username, "alice");
    assert_eq!(entry.email, "alice@example.com");
}

#[test]
fn owner_entry_deserializes_array() {
    let json = r#"[
        {"username": "alice", "email": "alice@example.com"},
        {"username": "bob", "email": "bob@example.com"}
    ]"#;
    let entries: Vec<super::OwnerEntry> = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].username, "alice");
    assert_eq!(entries[1].username, "bob");
}

#[test]
fn error_add_args_required_display() {
    let err = OwnerError::AddArgsRequired;
    assert!(err.to_string().contains("Package name and owner are required"));
}

#[test]
fn error_rm_args_required_display() {
    let err = OwnerError::RmArgsRequired;
    assert!(err.to_string().contains("Package name and owner are required"));
}

#[test]
fn error_package_not_found_display() {
    let err = OwnerError::PackageNotFound { package_name: "my-pkg".to_string() };
    assert!(err.to_string().contains("my-pkg"));
    assert!(err.to_string().contains("not found"));
}

#[test]
fn error_unauthorized_display() {
    let err = OwnerError::Unauthorized {
        action: "add owner to".to_string(),
        body: "token expired".to_string(),
    };
    assert!(err.to_string().contains("logged in"));
    assert!(err.to_string().contains("token expired"));
}

#[test]
fn error_forbidden_display() {
    let err = OwnerError::Forbidden {
        action: "remove owner from".to_string(),
        body: "not allowed".to_string(),
    };
    assert!(err.to_string().contains("permission"));
    assert!(err.to_string().contains("not allowed"));
}

#[test]
fn error_registry_write_failed_includes_package() {
    let err = OwnerError::RegistryWriteFailed {
        action: r#"add owner "bob" to"#.to_string(),
        status: 500,
        status_text: "Internal Server Error".to_string(),
        body: "something broke".to_string(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("package"),
        "error message must include 'package' for parity with TypeScript: {msg}",
    );
    assert!(msg.contains(r#"add owner "bob" to"#));
    assert!(msg.contains("500"));
}

fn config_with_registry(registry: &str) -> Config {
    let url = if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    Config { registry: url, ..Config::default() }
}

fn config_with_registry_no_retries(registry: &str) -> Config {
    let url = if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") };
    Config { registry: url, fetch_retries: 0, ..Config::default() }
}

fn owner_args(subcommand: &str, params: &[&str]) -> OwnerArgs {
    let mut all_params = vec![subcommand.to_string()];
    all_params.extend(params.iter().map(ToString::to_string));
    OwnerArgs { registry: None, otp: None, params: all_params }
}

fn owner_args_with_registry(registry: &str, subcommand: &str, params: &[&str]) -> OwnerArgs {
    let mut all_params = vec![subcommand.to_string()];
    all_params.extend(params.iter().map(ToString::to_string));
    OwnerArgs { registry: Some(registry.to_string()), otp: None, params: all_params }
}

fn owner_args_with_otp(otp: &str, subcommand: &str, params: &[&str]) -> OwnerArgs {
    let mut all_params = vec![subcommand.to_string()];
    all_params.extend(params.iter().map(ToString::to_string));
    OwnerArgs { registry: None, otp: Some(otp.to_string()), params: all_params }
}

// ── owner ls HTTP-flow tests ──────────────────────────────────────────

#[tokio::test]
async fn owner_ls_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/my-pkg/owners")
        .with_status(200)
        .with_body(
            json!([
                {"username": "alice", "email": "alice@example.com"},
                {"username": "bob", "email": "bob@example.com"}
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("ls", &["my-pkg"]);
    let result = args.run(&config).await.expect("owner ls must succeed");

    mock.assert_async().await;
    eprintln!("OWNERS:\n{}\n", result.as_deref().unwrap_or_default());
    assert_eq!(result.as_deref(), Some("alice <alice@example.com>\nbob <bob@example.com>"));
}

#[tokio::test]
async fn owner_ls_404_returns_package_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("GET", "/-/package/unknown-pkg/owners").with_status(404).create_async().await;

    let config = config_with_registry(&server.url());
    let args = owner_args("ls", &["unknown-pkg"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("not found"), "expected PackageNotFound, got: {formatted}");
}

// The `ls` fetch path shares the `add`/`rm` write path's status mapping (TS
// `fetchOwners` delegates to `throwRegistryError`), so a 401/403 surfaces the
// registry's response body as an Unauthorized/Forbidden error rather than a
// bare status line.
#[tokio::test]
async fn owner_ls_401_returns_unauthorized() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/private-pkg/owners")
        .with_status(401)
        .with_body("unauthorized access")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("ls", &["private-pkg"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    // Assert single tokens: miette's Debug output word-wraps long lines, so a
    // multi-word substring can straddle a line break.
    assert!(
        formatted.contains("logged in") && formatted.contains("unauthorized"),
        "expected Unauthorized error with body, got: {formatted}",
    );
}

#[tokio::test]
async fn owner_ls_403_returns_forbidden() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/locked-pkg/owners")
        .with_status(403)
        .with_body("forbidden detail")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("ls", &["locked-pkg"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(
        formatted.contains("permission") && formatted.contains("forbidden"),
        "expected Forbidden error with body, got: {formatted}",
    );
}

#[tokio::test]
async fn owner_ls_500_returns_registry_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/broken-pkg/owners")
        .with_status(500)
        .with_body("teapot")
        .create_async()
        .await;

    let config = config_with_registry_no_retries(&server.url());
    let args = owner_args("ls", &["broken-pkg"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(
        formatted.contains("fetch owners")
            && formatted.contains("500")
            && formatted.contains("teapot"),
        "expected registry error with body, got: {formatted}",
    );
}

#[tokio::test]
async fn owner_ls_scoped_package_encodes_correctly() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/@scope%2Fpkg/owners")
        .with_status(200)
        .with_body(json!([{"username": "alice", "email": "a@b.com"}]).to_string())
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("ls", &["@scope/pkg"]);
    let result = args.run(&config).await.expect("owner ls must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("alice <a@b.com>"));
}

#[tokio::test]
async fn owner_ls_list_alias() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/my-pkg/owners")
        .with_status(200)
        .with_body(json!([{"username": "carol", "email": "c@d.com"}]).to_string())
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("list", &["my-pkg"]);
    let result = args.run(&config).await.expect("owner list must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("carol <c@d.com>"));
}

#[tokio::test]
async fn owner_ls_no_params_returns_package_required() {
    let config = config_with_registry("http://unused/");
    let args = OwnerArgs { registry: None, otp: None, params: vec!["ls".to_string()] };
    let err = args.run(&config).await.unwrap_err();
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("Package name is required"));
}

// ── owner add HTTP-flow tests ─────────────────────────────────────────

#[tokio::test]
async fn owner_add_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/my-pkg/owners")
        .match_body(mockito::Matcher::Json(json!({"user": "alice"})))
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("add", &["my-pkg", "alice"]);
    let result = args.run(&config).await.expect("owner add must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("+alice: my-pkg"));
}

#[tokio::test]
async fn owner_add_401_returns_unauthorized() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/my-pkg/owners")
        .with_status(401)
        .with_body("token expired")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("add", &["my-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("logged in"), "expected Unauthorized, got: {formatted}");
}

#[tokio::test]
async fn owner_add_403_returns_forbidden() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/my-pkg/owners")
        .with_status(403)
        .with_body("not allowed")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("add", &["my-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("permission"), "expected Forbidden, got: {formatted}");
}

// The write path (add/rm) mirrors the TypeScript `throwRegistryError`, whose
// 404 message is `Package not found in registry. {body}` — distinct from the
// `ls` fetch path, which quotes the package name instead.
#[tokio::test]
async fn owner_add_404_returns_package_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/missing-pkg/owners")
        .with_status(404)
        .with_body("no such package")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("add", &["missing-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(
        formatted.contains("Package not found in registry")
            && formatted.contains("no such package"),
        "expected write-path PackageNotFound with body, got: {formatted}",
    );
}

#[tokio::test]
async fn owner_add_500_returns_registry_error_with_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/my-pkg/owners")
        .with_status(500)
        .with_body("something went wrong")
        .create_async()
        .await;

    let config = config_with_registry_no_retries(&server.url());
    let args = owner_args("add", &["my-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("500"), "expected status code in error, got: {formatted}");
    assert!(formatted.contains("package"), "expected 'package' in error message for TS parity");
}

#[tokio::test]
async fn owner_add_sends_otp_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/package/my-pkg/owners")
        .match_header("npm-otp", "123456")
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args_with_otp("123456", "add", &["my-pkg", "alice"]);
    let result = args.run(&config).await.expect("owner add with OTP must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("+alice: my-pkg"));
}

#[tokio::test]
async fn owner_add_too_few_args() {
    let config = config_with_registry("http://unused/");
    let args = owner_args("add", &["my-pkg"]);
    let err = args.run(&config).await.unwrap_err();
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("Package name and owner are required"));
}

// ── owner rm HTTP-flow tests ──────────────────────────────────────────

#[tokio::test]
async fn owner_rm_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/my-pkg/owners/alice")
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("rm", &["my-pkg", "alice"]);
    let result = args.run(&config).await.expect("owner rm must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("-alice: my-pkg"));
}

#[tokio::test]
async fn owner_rm_401_returns_unauthorized() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/my-pkg/owners/alice")
        .with_status(401)
        .with_body("unauthorized")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("rm", &["my-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("logged in"), "expected Unauthorized, got: {formatted}");
}

#[tokio::test]
async fn owner_rm_403_returns_forbidden() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/my-pkg/owners/alice")
        .with_status(403)
        .with_body("not allowed")
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("rm", &["my-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("permission"), "expected Forbidden, got: {formatted}");
}

#[tokio::test]
async fn owner_rm_404_returns_package_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/missing-pkg/owners/alice")
        .with_status(404)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("rm", &["missing-pkg", "alice"]);
    let err = args.run(&config).await.unwrap_err();

    mock.assert_async().await;
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("not found"), "expected PackageNotFound, got: {formatted}");
}

#[tokio::test]
async fn owner_rm_sends_otp_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/my-pkg/owners/bob")
        .match_header("npm-otp", "654321")
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args_with_otp("654321", "rm", &["my-pkg", "bob"]);
    let result = args.run(&config).await.expect("owner rm with OTP must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("-bob: my-pkg"));
}

#[tokio::test]
async fn owner_rm_too_few_args() {
    let config = config_with_registry("http://unused/");
    let args = owner_args("rm", &["my-pkg"]);
    let err = args.run(&config).await.unwrap_err();
    let report: miette::Report = err;
    let formatted = format!("{report:?}");
    assert!(formatted.contains("Package name and owner are required"));
}

#[tokio::test]
async fn owner_rm_encodes_owner_in_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/package/my-pkg/owners/user%40example.com")
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = owner_args("rm", &["my-pkg", "user@example.com"]);
    let result = args.run(&config).await.expect("owner rm must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("-user@example.com: my-pkg"));
}

// ── default subcommand (no subcommand → ls) ──────────────────────────

#[tokio::test]
async fn owner_no_subcommand_defaults_to_ls() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/package/my-pkg/owners")
        .with_status(200)
        .with_body(json!([{"username": "dave", "email": "dave@example.com"}]).to_string())
        .create_async()
        .await;

    let config = config_with_registry(&server.url());
    let args = OwnerArgs { registry: None, otp: None, params: vec!["my-pkg".to_string()] };
    let result = args.run(&config).await.expect("default ls must succeed");

    mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("dave <dave@example.com>"));
}

// ── --registry override ──────────────────────────────────────────────

#[tokio::test]
async fn owner_ls_registry_override() {
    let mut default_server = mockito::Server::new_async().await;
    let _default_mock = default_server
        .mock("GET", "/-/package/my-pkg/owners")
        .with_status(200)
        .with_body(json!([{"username": "wrong", "email": "wrong@example.com"}]).to_string())
        .create_async()
        .await;

    let mut override_server = mockito::Server::new_async().await;
    let override_mock = override_server
        .mock("GET", "/-/package/my-pkg/owners")
        .with_status(200)
        .with_body(json!([{"username": "correct", "email": "correct@example.com"}]).to_string())
        .create_async()
        .await;

    let config = config_with_registry(&default_server.url());
    let args = owner_args_with_registry(&override_server.url(), "ls", &["my-pkg"]);
    let result = args.run(&config).await.expect("owner ls with registry override must succeed");

    override_mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("correct <correct@example.com>"));
}

#[tokio::test]
async fn owner_add_registry_override() {
    let mut default_server = mockito::Server::new_async().await;
    let _default_mock = default_server
        .mock("PUT", "/-/package/my-pkg/owners")
        .with_status(200)
        .create_async()
        .await;

    let mut override_server = mockito::Server::new_async().await;
    let override_mock = override_server
        .mock("PUT", "/-/package/my-pkg/owners")
        .match_body(mockito::Matcher::Json(json!({"user": "alice"})))
        .with_status(200)
        .create_async()
        .await;

    let config = config_with_registry(&default_server.url());
    let args = owner_args_with_registry(&override_server.url(), "add", &["my-pkg", "alice"]);
    let result = args.run(&config).await.expect("owner add with registry override must succeed");

    override_mock.assert_async().await;
    assert_eq!(result.as_deref(), Some("+alice: my-pkg"));
}

// ── normalize_registry_url ───────────────────────────────────────────

#[test]
fn normalize_registry_url_adds_trailing_slash() {
    assert_eq!(
        super::normalize_registry_url("https://registry.example.com"),
        "https://registry.example.com/",
    );
}

#[test]
fn normalize_registry_url_preserves_trailing_slash() {
    assert_eq!(
        super::normalize_registry_url("https://registry.example.com/"),
        "https://registry.example.com/",
    );
}
