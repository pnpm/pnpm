//! A pnpr that hosts `@acme/*`, shared by the tests that publish to it.

use super::PnprServer;
use pnpr::{
    AccessList, Ecosystem, HostedConfig, PackagePattern, PackageRules, Registries, Registry,
    TokenBackend as _, UserBackend as _,
};

/// Start a pnpr that hosts `@acme/*`, readable by anyone, and return its
/// npm URL and a token that may publish there.
pub(super) fn start_hosted_pnpr() -> (String, String) {
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
            tokens
                .issue(
                    &users
                        .account_id("pacquet-test")
                        .await
                        .unwrap()
                        .unwrap(),
                )
                .await
                .expect("issue test token")
        });
    let addr = server.serve(move |config| {
        config.identity.auth.users.file = Some(htpasswd_path);
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
pub(super) fn host_targets() -> Vec<String> {
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
