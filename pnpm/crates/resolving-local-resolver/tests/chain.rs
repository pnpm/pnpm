//! Verify that [`LocalResolver`] composes into the
//! [`pnpm_resolving_default_resolver::DefaultResolver`] chain.

use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_default_resolver::{
    DefaultResolver,
    SpecNotSupportedByAnyResolverError,
};
use pnpm_resolving_local_resolver::{
    LocalResolver,
    LocalResolverContext,
};
use pnpm_resolving_resolver_base::{
    ResolveOptions,
    UnsupportedProtocolError,
    WantedDependency,
    WorkspacePackage,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::Arc,
};
use tempfile::TempDir;

fn setup_project() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let inner = tmp.path().join("inner");
    fs::create_dir_all(&inner).expect("create inner");
    fs::write(
        tmp.path().join("package.json"),
        r#"{"name":"@pnpm/resolving.local-resolver","version":"0.0.0"}"#,
    )
    .expect("write package.json");
    (tmp, inner)
}

#[tokio::test]
async fn dispatcher_routes_link_specifier_through_local_resolver() {
    let (_tmp, project_dir) = setup_project();
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);

    let opts = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: project_dir.clone(),
            lockfile_dir: project_dir.clone(),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wd = WantedDependency {
        alias: Some("parent".to_string()),
        bare_specifier: Some("link:..".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wd, &opts).await.expect("resolve");
    assert_eq!(result.id.as_str(), "link:..");
    assert_eq!(result.alias.as_deref(), Some("parent"));
    assert!(matches!(result.resolution, LockfileResolution::Directory(_)));
    assert_eq!(result.resolved_via, "local-filesystem");
}

#[tokio::test]
async fn dispatcher_falls_through_when_specifier_is_neither_local_nor_npm() {
    let (_tmp, project_dir) = setup_project();
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);
    let opts = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: project_dir.clone(),
            lockfile_dir: project_dir,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wd = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wd, &opts)
        .await
        .expect_err("chain with only the local resolver shouldn't claim a registry-shaped dep");
    assert!(err.downcast_ref::<SpecNotSupportedByAnyResolverError>().is_some(), "got {err}");
}

/// The tree walker recovers `ERR_PNPM_UNSUPPORTED_PROTOCOL` by downcast, so
/// the chain must box the error outermost.
#[tokio::test]
async fn dispatcher_boxes_an_unsupported_protocol_error_outermost() {
    let (_tmp, project_dir) = setup_project();
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);
    let opts = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: project_dir.clone(),
            lockfile_dir: project_dir,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wd = WantedDependency {
        alias: Some("got".to_string()),
        bare_specifier: Some("patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wd, &opts).await.expect_err("unsupported protocol");
    assert!(err.downcast_ref::<UnsupportedProtocolError>().is_some(), "got {err:?}");
}

#[tokio::test]
async fn resolve_latest_claims_local_scheme_specifiers() {
    let (_tmp, project_dir) = setup_project();
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);
    let opts = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: project_dir.clone(),
            lockfile_dir: project_dir,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let query = pnpm_resolving_resolver_base::LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("parent".to_string()),
            bare_specifier: Some("link:..".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
        current_version: None,
    };
    let info = resolver.resolve_latest(&query, &opts).await.expect("resolve_latest");
    assert!(info.is_some(), "local resolver should claim link: specs in resolve_latest");
}

/// A workspace project reached by a path is described by the manifest the
/// resolve options hold for it, as it is when reached by a `workspace:` range.
#[tokio::test]
async fn a_workspace_project_reached_by_path_resolves_with_its_in_memory_manifest() {
    let (tmp, project_dir) = setup_project();
    let in_memory = serde_json::json!({
        "name": "@pnpm/resolving.local-resolver",
        "version": "0.0.0",
        "dependencies": { "dep": "1.0.0" },
    });
    let workspace_packages = BTreeMap::from([(
        "@pnpm/resolving.local-resolver".to_string(),
        BTreeMap::from([(
            "0.0.0".to_string(),
            WorkspacePackage { root_dir: tmp.path().to_path_buf(), manifest: in_memory.clone() },
        )]),
    )]);
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);
    let opts = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: project_dir.clone(),
            lockfile_dir: project_dir,
            workspace_packages: Some(Arc::new(workspace_packages)),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    for bare_specifier in ["link:..", "file:..", "workspace:.."] {
        let wd = WantedDependency {
            alias: Some("parent".to_string()),
            bare_specifier: Some(bare_specifier.to_string()),
            ..WantedDependency::default()
        };
        let result = resolver.resolve(&wd, &opts).await.expect("resolve");
        assert_eq!(result.package.manifest.as_deref(), Some(&in_memory), "{bare_specifier}");
    }
}
