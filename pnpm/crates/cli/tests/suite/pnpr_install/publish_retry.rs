//! A publish to pnpr retried after the version was published: pnpm reads
//! the version's release record and dist-tags, and reports it as published
//! only when they are the release the retry would have made.

use super::{PnprServer, pacquet_at};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pnpr::{
    AccessList, Ecosystem, HostedConfig, PackagePattern, PackageRules, Registries, Registry,
    TokenBackend as _, UserBackend as _,
};
use serde_json::json;
use std::{fs, path::Path, process::Output};

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

/// A package `@acme/cli@1.0.0` with an `any` archive and a variant for the
/// host, set up to publish to `registry` with `token`.
fn write_package(workspace: &Path, registry: &str, token: &str) {
    fs::write(
        workspace.join("package.json"),
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
    .expect("write package.json");
    for file in ["portable/any", "native/host"] {
        let path = workspace.join(file);
        fs::create_dir_all(path.parent().expect("a file in a directory")).expect("create dir");
        fs::write(&path, file).expect("write file");
    }
    let authority = registry.trim_start_matches("http:");
    fs::write(
        workspace.join(".npmrc"),
        format!("registry={registry}\n{authority}:_authToken={token}\n"),
    )
    .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("registries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
}

fn publish(workspace: &Path, args: &[&str]) -> Output {
    pacquet_at(workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .with_args(["publish", "--no-git-checks", "--publish-wait-timeout=0"])
        .with_args(args)
        .output()
        .expect("run publish")
}

/// A retry that sets another dist-tag is a new request, which pnpr refuses
/// as already published. Its archives are the recorded ones, so pnpm names
/// the dist-tag as what differs, and once the dist-tag points at the
/// version, the retry succeeds.
#[test]
fn a_retried_publish_is_published_only_when_the_release_matches() {
    let (registry, token) = start_hosted_pnpr();
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_package(&workspace, &registry, &token);
    let first = publish(&workspace, &[]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));

    let retry = publish(&workspace, &["--tag", "next"]);
    assert!(!retry.status.success());
    let stderr = String::from_utf8_lossy(&retry.stderr);
    assert!(stderr.contains("ERR_PNPM_PUBLISHED_RELEASE_DIFFERS"), "{stderr}");
    // The message wraps at the terminal's width, so its words are checked.
    assert!(stderr.contains("`next`") && stderr.contains("is not set"), "{stderr}");

    let tagged = pacquet_at(&workspace)
        .with_args(["dist-tag", "add", "@acme/cli@1.0.0", "next"])
        .output()
        .expect("run dist-tag add");
    assert!(tagged.status.success(), "{}", String::from_utf8_lossy(&tagged.stderr));
    let retry = publish(&workspace, &["--tag", "next"]);
    let stdout = String::from_utf8_lossy(&retry.stdout);
    assert!(retry.status.success(), "{}{stdout}", String::from_utf8_lossy(&retry.stderr));
    assert!(stdout.contains("already published as this release"), "{stdout}");
    drop(root);
}
