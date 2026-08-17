use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

use super::{ResolveDependencyTreeOptions, resolve_dependency_tree};

struct NestedWorkspaceLinkResolver {
    target_dir: std::path::PathBuf,
}

impl Resolver for NestedWorkspaceLinkResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let target_dir = self.target_dir.clone();
        let project_dir = opts.project_dir.clone();
        let alias = wanted.alias.clone().unwrap_or_default();
        Box::pin(async move {
            if alias != "shared" {
                return Ok(None);
            }
            let relative = pathdiff::diff_paths(target_dir, project_dir)
                .expect("target can be relativized")
                .display()
                .to_string()
                .replace('\\', "/");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(format!("link:{relative}")),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: Some(std::sync::Arc::new(
                    serde_json::json!({ "name": "shared", "version": "1.0.0" }),
                )),
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: relative,
                }),
                resolved_via: "workspace".to_string(),
                normalized_bare_specifier: None,
                alias: Some(alias),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
async fn canonical_snapshot_link_id_is_relative_to_lockfile_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest_path = temp.path().join("package.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "shared": "workspace:*" },
        }))
        .expect("serialize manifest"),
    )
    .expect("write manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("parse manifest");
    let project_dir = std::path::PathBuf::from("/repo/apps/nested/app");
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let resolver = NestedWorkspaceLinkResolver { target_dir: lockfile_dir.join("packages/shared") };

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions { project_dir, lockfile_dir, ..ResolveOptions::default() },
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("resolve nested workspace link");

    let direct = tree.direct.first().expect("shared direct dependency");
    assert_eq!(direct.id, "link:packages/shared");
    assert_eq!(direct.node_id, crate::NodeId::leaf("link:packages/shared"));
    assert!(tree.packages.contains_key("link:packages/shared"));
    assert!(!tree.packages.contains_key("link:../../../packages/shared"));
}
