use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pacquet binary").with_current_dir(workspace)
}

fn nerf(registry: &str) -> String {
    let without_scheme = registry
        .strip_prefix("http://")
        .or_else(|| registry.strip_prefix("https://"))
        .unwrap_or(registry);
    format!("//{}/", without_scheme.trim_end_matches('/'))
}

fn configure(root: &Path, workspace: &Path, registry: &str, auth_file_contents: &str) -> PathBuf {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write project .npmrc");
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, auth_file_contents).expect("write auth .npmrc");
    auth_file
}

fn run_dist_tag(workspace: &Path, auth_file: &Path, args: &[&str]) -> std::process::Output {
    run_dist_tag_command(workspace, auth_file, "dist-tag", args)
}

fn run_dist_tag_command(
    workspace: &Path,
    auth_file: &Path,
    command: &str,
    args: &[&str],
) -> std::process::Output {
    pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg(command)
        .with_args(args)
        .output()
        .expect("spawn pacquet dist-tag")
}

#[test]
fn lists_dist_tags_sorted() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"2.0.0","beta":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag ls must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "beta: 1.0.0\nlatest: 2.0.0\n");
    drop((root, server));
}

#[test]
fn ls_sanitizes_registry_control_chars() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"\u001b[31mlatest":"1.0.0\u001b[0m"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag ls must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains('\u{1b}'), "stdout must not include raw escape bytes: {stdout:?}");
    assert_eq!(stdout, "[31mlatest: 1.0.0[0m\n");
    drop((root, server));
}

#[test]
fn lists_no_output_when_no_dist_tags_exist() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock =
        server.mock("GET", "/-/package/pkg/dist-tags").with_status(200).with_body("{}").create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag ls must succeed for empty tags (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    drop((root, server));
}

#[test]
fn lists_dist_tags_without_subcommand() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag default ls must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn list_alias_lists_dist_tags() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["list", "pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag list alias must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn ls_uses_package_name_from_versioned_spec_for_url() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg@1.0.0"]);

    mock.assert();
    assert!(
        output.status.success(),
        "versioned ls package specs must query the package name (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn ls_uses_clean_package_name_for_versioned_scoped_auth() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/@scope%2fpkg/dist-tags")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file_contents = format!(
        "{}:_authToken=default-token\n{}:@scope:_authToken=scoped-token\n",
        nerf(&registry),
        nerf(&registry),
    );
    let auth_file = configure(root.path(), &workspace, &registry, &auth_file_contents);

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "@scope/pkg@1.0.0"]);

    mock.assert();
    assert!(
        output.status.success(),
        "versioned scoped ls specs must use scoped auth (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn dist_tags_alias_lists_dist_tags() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag_command(&workspace, &auth_file, "dist-tags", &["ls", "pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tags alias must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn dist_tags_alias_defaults_to_listing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag_command(&workspace, &auth_file, "dist-tags", &["pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tags alias default ls must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn registry_option_overrides_config_registry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg", "--registry", &registry]);

    mock.assert();
    assert!(
        output.status.success(),
        "--registry must override config registry (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn ls_uses_package_scoped_auth() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/@scope%2fpkg/dist-tags")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file_contents = format!(
        "{}:_authToken=default-token\n{}:@scope:_authToken=scoped-token\n",
        nerf(&registry),
        nerf(&registry),
    );
    let auth_file = configure(root.path(), &workspace, &registry, &auth_file_contents);

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "@scope/pkg"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag ls must succeed with scoped auth (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    drop((root, server));
}

#[test]
fn ls_redacts_registry_credentials_on_network_error() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", "");

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_FETCH_RETRIES", "0")
        .with_env("PNPM_CONFIG_FETCH_TIMEOUT", "100")
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("dist-tag")
        .with_args(["ls", "pkg", "--registry", "http://user:pass@127.0.0.1:1/"])
        .output()
        .expect("spawn pacquet dist-tag");

    assert!(!output.status.success(), "transport failure must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_REGISTRY_ERROR"), "stderr:\n{stderr}");
    assert!(!stderr.contains("user:pass"), "credentials leaked into stderr:\n{stderr}");
    assert!(!stderr.contains("pass@"), "credentials leaked into stderr:\n{stderr}");
    drop(root);
}

#[test]
fn add_sets_a_dist_tag_with_otp() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("authorization", "Bearer token")
        .match_header("content-type", Matcher::Regex(r"^application/json\b".to_string()))
        .match_header("npm-auth-type", "legacy")
        .match_header("npm-otp", "123456")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let auth_file_contents = format!("{}:_authToken=token\n", nerf(&registry));
    let auth_file = configure(root.path(), &workspace, &registry, &auth_file_contents);

    let output =
        run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta", "--otp", "123456"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag add must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "+beta: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn add_uses_scoped_auth_for_scoped_package() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/@scope%2fpkg/dist-tags/beta")
        .match_header("authorization", "Bearer scoped-token")
        .match_header("npm-auth-type", "web")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let auth_file_contents = format!(
        "{}:_authToken=default-token\n{}:@scope:_authToken=scoped-token\n",
        nerf(&registry),
        nerf(&registry),
    );
    let auth_file = configure(root.path(), &workspace, &registry, &auth_file_contents);

    let output = run_dist_tag(&workspace, &auth_file, &["add", "@scope/pkg@1.0.0", "beta"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag add must use scoped auth for scoped packages (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "+beta: @scope/pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn add_normalizes_v_prefixed_versions() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@v1.0.0", "beta"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag add must normalize v-prefixed versions (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "+beta: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn add_defaults_to_latest_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/latest")
        .match_header("npm-auth-type", "web")
        .match_header("npm-otp", Matcher::Missing)
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag add latest must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "+latest: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn add_encodes_dist_tag_path_segment() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/release%2Fcandidate")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "release/candidate"]);

    mock.assert();
    assert!(
        output.status.success(),
        "dist-tag add must encode reserved tag characters (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "+release/candidate: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn add_reports_web_otp_challenge() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .with_status(401)
        .with_body(
            r#"{"authUrl":"https://auth.example/login","doneUrl":"https://auth.example/done"}"#,
        )
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta"]);

    mock.assert();
    assert!(!output.status.success(), "web OTP challenge must fail with guidance");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DIST_TAG_WEB_OTP_REQUIRED")
            && stderr.contains("https://auth.example/login")
            && stderr.contains("--otp"),
        "stderr must include web OTP guidance; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn add_rejects_unsafe_web_otp_challenge_urls() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .with_status(401)
        .with_body(r#"{"authUrl":"javascript:alert(1)","doneUrl":"https://auth.example/done"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta"]);

    mock.assert();
    assert!(!output.status.success(), "unsafe web OTP challenge must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_UNAUTHORIZED"), "stderr:\n{stderr}");
    assert!(
        !stderr.contains("ERR_PNPM_DIST_TAG_WEB_OTP_REQUIRED"),
        "unsafe challenge must not be printed as web OTP guidance:\n{stderr}",
    );
    assert!(!stderr.contains("Open javascript:"), "stderr:\n{stderr}");
    drop((root, server));
}

#[test]
fn add_rejects_web_otp_challenge_urls_with_control_chars() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .with_status(401)
        .with_body(
            r#"{"authUrl":"https://auth.example/login\nspoof","doneUrl":"https://auth.example/done"}"#,
        )
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta"]);

    mock.assert();
    assert!(!output.status.success(), "unsafe web OTP challenge must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_UNAUTHORIZED"), "stderr:\n{stderr}");
    assert!(
        !stderr.contains("ERR_PNPM_DIST_TAG_WEB_OTP_REQUIRED"),
        "unsafe challenge must not be printed as web OTP guidance:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn rm_removes_an_existing_dist_tag() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0","beta":"1.0.0"}"#)
        .create();
    let delete_mock = server
        .mock("DELETE", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .with_status(200)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["rm", "pkg", "beta"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(
        output.status.success(),
        "dist-tag rm must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "-beta: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn rm_encodes_dist_tag_path_segment() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0","release/candidate":"1.0.0"}"#)
        .create();
    let delete_mock = server
        .mock("DELETE", "/-/package/pkg/dist-tags/release%2Fcandidate")
        .with_status(200)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["rm", "pkg", "release/candidate"]);

    get_mock.assert();
    delete_mock.assert();
    assert!(
        output.status.success(),
        "dist-tag rm must encode reserved tag characters (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "-release/candidate: pkg@1.0.0\n");
    drop((root, server));
}

#[test]
fn rm_refuses_latest() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", "");

    let output = run_dist_tag(&workspace, &auth_file, &["rm", "pkg", "latest"]);

    assert!(
        !output.status.success(),
        "dist-tag rm latest must fail (stdout: {})",
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DIST_TAG_RM_LATEST")
            && stderr.contains(r#"Removing the "latest" dist-tag is not allowed"#),
        "stderr must name the refusal; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn rm_fails_when_tag_is_missing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["rm", "pkg", "beta"]);

    mock.assert();
    assert!(!output.status.success(), "missing tag must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DIST_TAG_NOT_FOUND")
            && stderr.contains(r#"dist-tag "beta" is not set on package "pkg""#),
        "stderr must name the missing tag; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn ls_fails_when_package_is_missing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let mock = server.mock("GET", "/-/package/missing/dist-tags").with_status(404).create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "missing"]);

    mock.assert();
    assert!(!output.status.success(), "missing package must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_PACKAGE_NOT_FOUND")
            && stderr.contains(r#"Package "missing" not found in registry"#),
        "stderr must name the missing package; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn ls_rejects_oversized_dist_tags_response() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let body = format!(r#"{{"latest":"{}"}}"#, "1".repeat(1024 * 1024));
    let mock =
        server.mock("GET", "/-/package/pkg/dist-tags").with_status(200).with_body(body).create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["ls", "pkg"]);

    mock.assert();
    assert!(!output.status.success(), "oversized dist-tags responses must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_REGISTRY_RESPONSE_TOO_LARGE"),
        "stderr must name the oversized response; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn add_rejects_non_exact_semver_versions() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@^1.0.0", "beta"]);

    assert!(!output.status.success(), "range version must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DIST_TAG_ADD_INVALID_VERSION")
            && stderr.contains("Version must be an exact semver version"),
        "stderr must name the invalid version; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn add_sanitizes_and_truncates_registry_error_body() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let body = format!("{}{}", "x".repeat(70 * 1024), "\u{1b}[31m");
    let mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .with_status(400)
        .with_body(body)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");

    let output = run_dist_tag(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta"]);

    mock.assert();
    assert!(!output.status.success(), "registry write errors must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_REGISTRY_ERROR")
            && stderr.contains("response body truncated")
            && !stderr.contains('\u{1b}'),
        "stderr must sanitize and truncate the registry body; got:\n{stderr}",
    );
    drop((root, server));
}
