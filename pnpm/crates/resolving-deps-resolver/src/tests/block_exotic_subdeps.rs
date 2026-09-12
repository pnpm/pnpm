use std::{collections::HashMap, sync::Mutex};

use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::ResolveOptions;

use super::{StubResolver, fake_manifest, fake_result};
use crate::resolve_dependency_tree::{
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
};

fn git_result(
    name: &str,
    version: &str,
    manifest: serde_json::Value,
) -> pnpm_resolving_resolver_base::ResolveResult {
    let mut result = fake_result(name, version, manifest);
    result.resolved_via = "git-repository".to_string();
    result
}

#[tokio::test]
async fn rejects_exotic_transitive_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "say-hi": "github:zkochan/hi" }
            }),
        ),
    );
    table.insert(
        ("say-hi".to_string(), "github:zkochan/hi".to_string()),
        git_result("say-hi", "1.0.0", serde_json::json!({ "name": "say-hi", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let err = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { block_exotic_subdeps: true, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect_err("exotic subdep must error");
    match err {
        ResolveDependencyTreeError::ExoticSubdep { specifier, resolved_via } => {
            assert_eq!(specifier, "say-hi");
            assert_eq!(resolved_via, "git-repository");
        }
        other => panic!("expected ExoticSubdep, got {other:?}"),
    }
}

#[tokio::test]
async fn allows_exotic_direct_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("is-negative".to_string(), "kevva/is-negative#1.0.0".to_string()),
        git_result(
            "is-negative",
            "1.0.0",
            serde_json::json!({ "name": "is-negative", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "is-negative": "kevva/is-negative#1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { block_exotic_subdeps: true, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("direct exotic dep should resolve");
    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].alias, "is-negative");
}

#[tokio::test]
async fn allows_registry_subdep() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "bar": "^2.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "^2.0.0".to_string()),
        fake_result("bar", "2.0.0", serde_json::json!({ "name": "bar", "version": "2.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { block_exotic_subdeps: true, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("registry subdep must pass");
}

#[tokio::test]
async fn allows_exotic_subdep_when_disabled() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "say-hi": "github:zkochan/hi" }
            }),
        ),
    );
    table.insert(
        ("say-hi".to_string(), "github:zkochan/hi".to_string()),
        git_result("say-hi", "1.0.0", serde_json::json!({ "name": "say-hi", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { block_exotic_subdeps: false, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("exotic subdep must pass when disabled");
    assert!(tree.packages.contains_key("say-hi@1.0.0"));
}

#[tokio::test]
async fn allows_exotic_dep_under_workspace_dep() {
    let mut table = HashMap::default();
    let mut workspace_dep_res = fake_result(
        "workspace-dep",
        "1.0.0",
        serde_json::json!({
            "name": "workspace-dep",
            "version": "1.0.0",
            "dependencies": { "say-hi": "github:zkochan/hi" }
        }),
    );
    workspace_dep_res.resolved_via = "workspace".to_string();
    table.insert(("workspace-dep".to_string(), "workspace:^1.0.0".to_string()), workspace_dep_res);
    table.insert(
        ("say-hi".to_string(), "github:zkochan/hi".to_string()),
        git_result("say-hi", "1.0.0", serde_json::json!({ "name": "say-hi", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "workspace-dep": "workspace:^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { block_exotic_subdeps: true, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("exotic dep under workspace dep must pass");
    assert!(tree.packages.contains_key("say-hi@1.0.0"));
}
