use super::{configure, nerf, pacquet_at};
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Output};

const NPM_ID_TOKEN: &str = "header.eyJhdWQiOiJucG06MTI3LjAuMC4xIn0.signature";

fn run_with_oidc(
    workspace: &Path,
    auth_file: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Output {
    let mut command = pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("dist-tag")
        .with_args(args)
        .without_env("NPM_ID_TOKEN")
        .without_env("GITLAB_CI")
        .without_env("GITHUB_ACTIONS")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL");
    command.envs(env.iter().copied());
    command.output().expect("spawn pnpm dist-tag with OIDC token")
}

#[test]
fn oidc_authenticates_add_rm_and_ls_with_registry_override() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/npm/private", server.url());
    let identity_url = format!("{}/identity", server.url());
    let identity = server
        .mock("GET", "/identity")
        .match_query(Matcher::Any)
        .expect(0)
        .create();
    let exchange = server
        .mock("POST", "/npm/private/-/npm/v1/oidc/token/exchange/package/@scope%2fpkg")
        .match_header("authorization", Matcher::Exact(format!("Bearer {NPM_ID_TOKEN}")))
        .with_status(200)
        .with_body(r#"{"token":"oidc-registry-token"}"#)
        .expect(3)
        .create();
    let add = server
        .mock("PUT", "/npm/private/-/package/@scope%2fpkg/dist-tags/beta")
        .match_header("authorization", "Bearer oidc-registry-token")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(201)
        .create();
    let tags = server
        .mock("GET", "/npm/private/-/package/@scope%2fpkg/dist-tags")
        .match_header("authorization", "Bearer oidc-registry-token")
        .with_status(200)
        .with_body(r#"{"beta":"1.0.0"}"#)
        .expect(2)
        .create();
    let remove = server
        .mock("DELETE", "/npm/private/-/package/@scope%2fpkg/dist-tags/beta")
        .match_header("authorization", "Bearer oidc-registry-token")
        .with_status(200)
        .create();
    let visibility = server
        .mock("GET", "/npm/private/-/package/@scope%2fpkg/visibility")
        .expect(0)
        .create();
    let auth_contents = format!("{}:_authToken=configured-token\n", nerf(&registry));
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", &auth_contents);
    let project_npmrc = fs::read(workspace.join(".npmrc")).expect("read project npmrc");

    for (args, expected) in [
        (vec!["add", "@scope/pkg@1.0.0", "beta"], "+beta: @scope/pkg@1.0.0\n"),
        (vec!["rm", "@scope/pkg", "beta"], "-beta: @scope/pkg@1.0.0\n"),
        (vec!["ls", "@scope/pkg"], "beta: 1.0.0\n"),
    ] {
        let mut args = args;
        args.extend(["--registry", &registry]);
        let output = run_with_oidc(
            &workspace,
            &auth_file,
            &args,
            &[
                ("NPM_ID_TOKEN", NPM_ID_TOKEN),
                ("GITHUB_ACTIONS", "true"),
                ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "request-token"),
                ("ACTIONS_ID_TOKEN_REQUEST_URL", &identity_url),
            ],
        );
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    exchange.assert();
    identity.assert();
    add.assert();
    tags.assert();
    remove.assert();
    visibility.assert();
    assert_eq!(fs::read(workspace.join(".npmrc")).unwrap(), project_npmrc);
    assert_eq!(fs::read_to_string(&auth_file).unwrap(), auth_contents);
    drop((root, server));
}

#[test]
fn oidc_exchange_rejection_falls_back_to_configured_token() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let exchange = server
        .mock("POST", "/-/npm/v1/oidc/token/exchange/package/pkg")
        .match_header("authorization", Matcher::Exact(format!("Bearer {NPM_ID_TOKEN}")))
        .with_status(401)
        .with_body(r#"{"body":{"message":"exchange rejected"}}"#)
        .create();
    let add = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("authorization", "Bearer configured-token")
        .with_status(201)
        .create();
    let auth_contents = format!("{}:_authToken=configured-token\n", nerf(&registry));
    let auth_file = configure(root.path(), &workspace, &registry, &auth_contents);

    let output = run_with_oidc(
        &workspace,
        &auth_file,
        &["add", "pkg@1.0.0", "beta"],
        &[("NPM_ID_TOKEN", NPM_ID_TOKEN)],
    );

    exchange.assert();
    add.assert();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).ends_with("+beta: pkg@1.0.0\n"));
    assert_eq!(fs::read_to_string(&auth_file).unwrap(), auth_contents);
    drop((root, server));
}

#[test]
fn github_oidc_uses_the_packages_scoped_registry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let identity = server
        .mock("GET", "/identity")
        .match_query(Matcher::UrlEncoded("audience".into(), "npm:127.0.0.1".into()))
        .match_header("authorization", "Bearer request-token")
        .with_status(200)
        .with_body(r#"{"value":"test-id-token"}"#)
        .create();
    let exchange = server
        .mock("POST", "/-/npm/v1/oidc/token/exchange/package/@scope%2fpkg")
        .match_header("authorization", "Bearer test-id-token")
        .with_status(200)
        .with_body(r#"{"token":"oidc-registry-token"}"#)
        .create();
    let tags = server
        .mock("GET", "/-/package/@scope%2fpkg/dist-tags")
        .match_header("authorization", "Bearer oidc-registry-token")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let visibility = server
        .mock("GET", "/-/package/@scope%2fpkg/visibility")
        .expect(0)
        .create();
    let auth_file = configure(root.path(), &workspace, "http://127.0.0.1:1/", "");
    fs::write(
        workspace.join(".npmrc"),
        format!("registry=http://127.0.0.1:1/\n@scope:registry={registry}\n"),
    )
    .expect("write scoped registry");

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_args(["dist-tag", "ls", "@scope/pkg"])
        .without_env("NPM_ID_TOKEN")
        .without_env("GITLAB_CI")
        .env("GITHUB_ACTIONS", "true")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "request-token")
        .env("ACTIONS_ID_TOKEN_REQUEST_URL", format!("{}/identity", server.url()))
        .output()
        .expect("spawn pnpm dist-tag with GitHub OIDC");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("latest: 1.0.0"));
    identity.assert();
    exchange.assert();
    tags.assert();
    visibility.assert();
    assert_eq!(fs::read(&auth_file).unwrap(), b"");
    drop((root, server));
}

#[test]
fn gitlab_uses_a_forwarded_token_without_provenance() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let exchange = server
        .mock("POST", "/-/npm/v1/oidc/token/exchange/package/pkg")
        .match_header("authorization", Matcher::Exact(format!("Bearer {NPM_ID_TOKEN}")))
        .with_status(200)
        .with_body(r#"{"token":"oidc-registry-token"}"#)
        .create();
    let tags = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .match_header("authorization", "Bearer oidc-registry-token")
        .with_status(200)
        .with_body(r#"{"latest":"1.0.0"}"#)
        .create();
    let visibility = server
        .mock("GET", "/-/package/pkg/visibility")
        .expect(0)
        .create();
    let auth_file = configure(root.path(), &workspace, &registry, "");
    let output = run_with_oidc(
        &workspace,
        &auth_file,
        &["ls", "pkg"],
        &[("GITLAB_CI", "true"), ("NPM_ID_TOKEN", NPM_ID_TOKEN)],
    );
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "latest: 1.0.0\n");
    exchange.assert();
    tags.assert();
    visibility.assert();
    assert_eq!(fs::read(&auth_file).unwrap(), b"");
    drop((root, server));
}

#[test]
fn unavailable_oidc_uses_configured_credentials_without_network_overhead() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let identity_url = format!("{}/identity", server.url());
    let identity = server
        .mock("GET", "/identity")
        .match_query(Matcher::Any)
        .expect(0)
        .create();
    let exchange = server
        .mock("POST", "/-/npm/v1/oidc/token/exchange/package/pkg")
        .expect(0)
        .create();
    let add = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("authorization", "Bearer configured-token")
        .with_status(201)
        .expect(4)
        .create();
    let auth_contents = format!("{}:_authToken=configured-token\n", nerf(&registry));
    let auth_file = configure(root.path(), &workspace, &registry, &auth_contents);
    let cases: &[&[(&str, &str)]] = &[
        &[],
        &[("GITHUB_ACTIONS", "true")],
        &[("GITHUB_ACTIONS", "true"), ("ACTIONS_ID_TOKEN_REQUEST_URL", &identity_url)],
        &[("GITLAB_CI", "true")],
    ];
    for env in cases {
        let output = run_with_oidc(&workspace, &auth_file, &["add", "pkg@1.0.0", "beta"], env);
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    }
    identity.assert();
    exchange.assert();
    add.assert();
    assert_eq!(fs::read_to_string(&auth_file).unwrap(), auth_contents);
    drop((root, server));
}

#[test]
fn rejected_github_identity_uses_configured_credentials() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let identity_url = format!("{}/identity", server.url());
    let identity = server
        .mock("GET", "/identity")
        .match_query(Matcher::UrlEncoded("audience".into(), "npm:127.0.0.1".into()))
        .match_header("authorization", "Bearer request-token")
        .with_status(403)
        .with_body("forbidden")
        .create();
    let exchange = server
        .mock("POST", "/-/npm/v1/oidc/token/exchange/package/pkg")
        .expect(0)
        .create();
    let add = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("authorization", "Bearer configured-token")
        .with_status(201)
        .create();
    let auth_contents = format!("{}:_authToken=configured-token\n", nerf(&registry));
    let auth_file = configure(root.path(), &workspace, &registry, &auth_contents);
    let output = run_with_oidc(
        &workspace,
        &auth_file,
        &["add", "pkg@1.0.0", "beta"],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "request-token"),
            ("ACTIONS_ID_TOKEN_REQUEST_URL", &identity_url),
        ],
    );
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    identity.assert();
    exchange.assert();
    add.assert();
    assert_eq!(fs::read_to_string(&auth_file).unwrap(), auth_contents);
    drop((root, server));
}
