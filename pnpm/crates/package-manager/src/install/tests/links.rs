use crate::install::lockfile_freshness::manifest::exclude_linked_dependencies;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};

#[test]
fn exclude_linked_dependencies_drops_link_deps_from_every_group() {
    let mut manifest = PackageManifest::from_value(
        std::path::PathBuf::from("package.json"),
        serde_json::json!({
            "dependencies": {
                "direct": "1.0.0",
                "linked": "link:../linked"
            },
            "devDependencies": {
                "declared-peer": "2.0.0",
                "linked-dev": "link:../dev"
            }
        }),
    );

    exclude_linked_dependencies(&mut manifest, None);
    assert_eq!(
        manifest
            .dependencies([DependencyGroup::Prod])
            .collect::<std::collections::BTreeMap<_, _>>(),
        std::collections::BTreeMap::from([("direct", "1.0.0")]),
    );
    assert_eq!(
        manifest
            .dependencies([DependencyGroup::Dev])
            .collect::<Vec<_>>(),
        vec![("declared-peer", "2.0.0")],
    );
}

#[test]
fn exclude_linked_dependencies_drops_matching_workspace_ranges() {
    let mut manifest = PackageManifest::from_value(
        std::path::PathBuf::from("package.json"),
        serde_json::json!({
            "dependencies": {
                "explicit-workspace": "workspace:^1.0.0",
                "linked": "^1.0.0",
                "registry": "^2.0.0"
            }
        }),
    );
    let workspace_version = std::collections::BTreeMap::from([(
        "1.1.0".to_string(),
        pnpm_resolving_resolver_base::WorkspacePackage {
            root_dir: std::path::PathBuf::from("linked"),
            manifest: serde_json::json!({ "name": "linked", "version": "1.1.0" }),
        },
    )]);
    let workspace_packages = pnpm_resolving_resolver_base::WorkspacePackages::from([
        ("explicit-workspace".to_string(), workspace_version.clone()),
        ("linked".to_string(), workspace_version),
    ]);

    exclude_linked_dependencies(&mut manifest, Some(&workspace_packages));

    assert_eq!(
        manifest
            .dependencies([DependencyGroup::Prod])
            .collect::<std::collections::BTreeMap<_, _>>(),
        std::collections::BTreeMap::from([
            ("explicit-workspace", "workspace:^1.0.0"),
            ("registry", "^2.0.0"),
        ]),
    );
}
