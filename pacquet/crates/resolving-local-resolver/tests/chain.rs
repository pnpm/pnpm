//! Verify that [`LocalResolver`] composes into the
//! [`pacquet_resolving_default_resolver::DefaultResolver`] chain the
//! same way upstream's local resolver composes into pnpm's
//! [`createResolver`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/default-resolver/src/index.ts#L97-L173).

use pacquet_lockfile::LockfileResolution;
use pacquet_resolving_default_resolver::{DefaultResolver, SpecNotSupportedByAnyResolverError};
use pacquet_resolving_local_resolver::{LocalResolver, LocalResolverContext};
use pacquet_resolving_resolver_base::{ResolveOptions, WantedDependency};
use std::{fs, path::PathBuf};
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
        project_dir: project_dir.clone(),
        lockfile_dir: project_dir.clone(),
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
        project_dir: project_dir.clone(),
        lockfile_dir: project_dir,
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

#[tokio::test]
async fn resolve_latest_claims_local_scheme_specifiers() {
    let (_tmp, project_dir) = setup_project();
    let resolver =
        DefaultResolver::new(vec![Box::new(LocalResolver::new(LocalResolverContext::default()))]);
    let opts = ResolveOptions {
        project_dir: project_dir.clone(),
        lockfile_dir: project_dir,
        ..ResolveOptions::default()
    };
    let query = pacquet_resolving_resolver_base::LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("parent".to_string()),
            bare_specifier: Some("link:..".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
    };
    let info = resolver.resolve_latest(&query, &opts).await.expect("resolve_latest");
    assert!(info.is_some(), "local resolver should claim link: specs in resolve_latest");
}
