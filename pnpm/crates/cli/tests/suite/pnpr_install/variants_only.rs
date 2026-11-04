//! A version published with platform variants and no archive that targets
//! `any`, round-tripped through an in-process pnpr: pnpm installs the
//! host's variant, a host none fits fails listing the published platforms,
//! and an optional dependency none of whose variants fits is skipped.

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

/// Publish `@acme/<name>@1.0.0` with a variant for each of `entries`, a
/// list of targets and the file its archive holds, and no `any` entry.
fn publish(registry: &str, token: &str, name: &str, entries: &[(Vec<String>, &str)]) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let variants: Vec<_> = entries
        .iter()
        .map(|(targets, file)| json!({ "targets": targets, "files": [file] }))
        .collect();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": format!("@acme/{name}"),
            "version": "1.0.0",
            "files": [],
            "publishConfig": { "variants": variants },
        })
        .to_string(),
    )
    .expect("write package.json");
    for (_, file) in entries {
        let path = workspace.join(file);
        fs::create_dir_all(path.parent().expect("a file in a directory")).expect("create dir");
        fs::write(&path, *file).expect("write archive file");
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
    pacquet_at(&workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .with_args(["publish", "--no-git-checks", "--publish-wait-timeout=0"])
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

fn installed(workspace: &Path, file: &str) -> bool {
    workspace
        .join("node_modules/@acme")
        .join(file)
        .exists()
}

#[test]
fn a_version_without_an_any_archive_installs_the_hosts_variant() {
    let (registry, token) = start_hosted_pnpr();
    let foreign = vec!["freebsd-x64".to_string()];
    publish(
        &registry,
        &token,
        "native-cli",
        &[(host_targets(), "native/host"), (foreign, "native/freebsd")],
    );
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token, &json!({ "dependencies": { "@acme/native-cli": "1.0.0" } }));

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(installed(&workspace, "native-cli/native/host"));
    assert!(!installed(&workspace, "native-cli/native/freebsd"));

    // The lockfile installs it again, frozen, with no archive that targets
    // every host.
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("type: variations"), "{lockfile}");
    assert!(!lockfile.contains("any: true"), "no archive targets every host: {lockfile}");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(installed(&workspace, "native-cli/native/host"));
    drop((root, npmrc_info));
}

#[test]
fn a_host_no_variant_fits_fails_unless_the_dependency_is_optional() {
    let (registry, token) = start_hosted_pnpr();
    publish(
        &registry,
        &token,
        "freebsd-only",
        &[(vec!["freebsd-x64".to_string()], "native/freebsd")],
    );

    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        consumer(&registry, &token, &json!({ "dependencies": { "@acme/freebsd-only": "1.0.0" } }));
    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run install");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PACKAGE_MANAGER_NO_MATCHING_PLATFORM_VARIANT"), "{stderr}");
    assert!(stderr.contains("freebsd"), "{stderr}");
    drop((root, npmrc_info));

    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(
        &registry,
        &token,
        &json!({ "optionalDependencies": { "@acme/freebsd-only": "1.0.0" } }),
    );
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(!installed(&workspace, "freebsd-only"));
    drop((root, npmrc_info));
}
