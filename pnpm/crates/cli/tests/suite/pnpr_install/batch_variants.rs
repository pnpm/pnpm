//! `pnpm -r publish --batch` of a workspace with a package that has
//! platform variants, round-tripped through an in-process pnpr: every
//! package's archives are staged, the batch commits them by reference, and
//! pnpm installs the host's variant.

use super::{
    PnprServer, configure_pnpr_auth, declare_pnpr_registries, pacquet_at, point_npmrc_registry_at,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pnpr::{
    AccessList, Ecosystem, HostedConfig, PackagePattern, PackageRules, Registries, Registry,
    TokenBackend as _, UserBackend as _,
};
use serde_json::json;
use std::{fs, path::Path};

/// Start a pnpr that hosts `@acme/*`, readable by anyone, and return its
/// npm URL and a token that may publish there.
fn start_hosted_pnpr() -> (String, String) {
    let server = PnprServer::bind("pnpr-hosted");
    let tokens_path = server.storage.join("tokens.db");
    let htpasswd_path = server.storage.join("htpasswd");
    let token = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("token setup runtime")
        .block_on(async {
            let users = pnpr::UserStore::open(htpasswd_path.clone(), pnpr::MaxUsers::Disabled)
                .expect("open user store");
            users.create_user("pacquet-test", "password").await.expect("store test account");
            let tokens = pnpr::TokenStore::open(tokens_path.clone()).expect("open token store");
            tokens.issue("pacquet-test").await.expect("issue test token")
        });
    let addr = server.serve(move |config| {
        config.identity.auth.htpasswd.file = Some(htpasswd_path);
        config.identity.auth.tokens.file = Some(tokens_path);
        config.routing.hosted.insert(
            "acme".to_string(),
            HostedConfig::new(
                "acme",
                PackageRules::new(Vec::new(), Some(AccessList::from_tokens(["$all"]))),
            ),
        );
        // The hosted registry is the default, so its URL both publishes and
        // installs.
        let graph = [
            ("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }),
            (
                "acme".to_string(),
                Registry::Hosted {
                    patterns: vec![
                        PackagePattern::parse("@acme/*", Ecosystem::Npm).expect("pattern parses"),
                    ],
                },
            ),
        ];
        config.routing.registries =
            Registries::new(graph.into_iter().collect(), Some("acme".to_string()));
    });
    (format!("http://{addr}/npm/"), token)
}

/// The targets of a variant this machine installs: its OS and CPU, with
/// both C libraries on Linux.
fn host_targets() -> Vec<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let cpu = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let tag = format!("{os}-{cpu}");
    if os == "linux" { vec![tag.clone(), format!("{tag}-musl")] } else { vec![tag] }
}

/// Write `files` under `dir`, each holding its own path.
fn write_files(dir: &Path, files: &[&str]) {
    for file in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().expect("a file in a directory")).expect("create dir");
        fs::write(&path, *file).expect("write file");
    }
}

/// Publish a workspace of `@acme/cli@1.0.0`, with an `any` archive and a
/// variant for the host, and `@acme/lib@1.0.0`, in one batch.
fn batch_publish(registry: &str, token: &str) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    let cli = workspace.join("packages/cli");
    fs::create_dir_all(&cli).expect("create cli");
    fs::write(
        cli.join("package.json"),
        json!({
            "name": "@acme/cli",
            "version": "1.0.0",
            "files": [],
            "publishConfig": {
                "variants": [
                    { "targets": ["any"], "files": ["portable/any"] },
                    { "targets": host_targets(), "files": ["native/host"] },
                ],
            },
        })
        .to_string(),
    )
    .expect("write cli package.json");
    write_files(&cli, &["portable/any", "native/host"]);
    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create lib");
    fs::write(
        lib.join("package.json"),
        json!({ "name": "@acme/lib", "version": "1.0.0", "files": ["index.js"] }).to_string(),
    )
    .expect("write lib package.json");
    write_files(&lib, &["index.js"]);
    let authority = registry.trim_start_matches("http:");
    fs::write(
        workspace.join(".npmrc"),
        format!("registry={registry}\n{authority}:_authToken={token}\n"),
    )
    .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\nregistries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
    pacquet_at(&workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .with_args(["-r", "publish", "--batch", "--no-git-checks", "--publish-wait-timeout=0"])
        .assert()
        .success();
    drop(root);
}

/// A project on `registry` with `dependencies`, its `.npmrc` and workspace
/// manifest declaring the registry `serverType: pnpr`.
fn consumer(
    registry: &str,
    token: &str,
    dependencies: &serde_json::Value,
) -> CommandTempCwd<pnpm_testing_utils::bin::AddMockedRegistry> {
    let command = CommandTempCwd::init().add_mocked_registry();
    point_npmrc_registry_at(&command.npmrc_info.npmrc_path, registry);
    configure_pnpr_auth(&command.npmrc_info.npmrc_path, registry, token);
    fs::write(command.workspace.join("package.json"), dependencies.to_string())
        .expect("write package.json");
    declare_pnpr_registries(&command.workspace, &[registry]);
    command
}

#[test]
fn a_batch_publishes_a_package_with_variants_beside_one_without() {
    let (registry, token) = start_hosted_pnpr();
    batch_publish(&registry, &token);
    let dependencies = json!({ "dependencies": { "@acme/cli": "1.0.0", "@acme/lib": "1.0.0" } });
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token, &dependencies);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let installed = |file: &str| {
        workspace
            .join("node_modules/@acme")
            .join(file)
            .exists()
    };
    assert!(installed("cli/native/host"), "the host's variant is installed");
    assert!(!installed("cli/portable/any"), "not the archive of every other host");
    assert!(installed("lib/index.js"));
    drop((root, npmrc_info));
}
