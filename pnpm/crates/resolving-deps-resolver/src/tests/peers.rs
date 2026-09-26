mod auto_install;

mod peer_resolution_two_peer_chains_resolve;

mod peer_resolution_cycle_reentry_does_not;

mod behavior;

use std::{collections::HashMap, sync::Mutex};

use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::ResolveOptions;
use pretty_assertions::assert_eq;

use super::{StubResolver, fake_manifest, fake_result};
use crate::{
    resolve_dependency_tree::{ResolveDependencyTreeOptions, resolve_dependency_tree},
    resolve_peers::{ResolvePeersOptions, resolve_peers},
};
use pnpm_deps_path::DepPath;

/// Shared fixture for the `dedupe_peers_*` pair: react@18 plus
/// `@emotion/react@11` (peer: react) plus `@emotion/styled@11`
/// (peers: react, @emotion/react).
async fn resolve_emotion_fixture(
    opts: ResolvePeersOptions,
) -> crate::resolve_peers::ResolvePeersResult {
    let mut table = HashMap::default();
    table.insert(
        ("react".to_string(), "18.0.0".to_string()),
        fake_result("react", "18.0.0", serde_json::json!({ "name": "react", "version": "18.0.0" })),
    );
    table.insert(
        ("@emotion/react".to_string(), "11.0.0".to_string()),
        fake_result(
            "@emotion/react",
            "11.0.0",
            serde_json::json!({
                "name": "@emotion/react",
                "version": "11.0.0",
                "peerDependencies": { "react": ">=16" }
            }),
        ),
    );
    table.insert(
        ("@emotion/styled".to_string(), "11.0.0".to_string()),
        fake_result(
            "@emotion/styled",
            "11.0.0",
            serde_json::json!({
                "name": "@emotion/styled",
                "version": "11.0.0",
                "peerDependencies": {
                    "react": ">=16",
                    "@emotion/react": ">=11"
                }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "react": "18.0.0",
        "@emotion/react": "11.0.0",
        "@emotion/styled": "11.0.0",
    }));
    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();
    resolve_peers(&mut tree, opts)
}
