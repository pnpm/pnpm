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

    exclude_linked_dependencies(&mut manifest);
    assert_eq!(
        manifest
            .dependencies([DependencyGroup::Prod])
            .collect::<std::collections::BTreeMap<_, _>>(),
        std::collections::BTreeMap::from([("direct", "1.0.0")]),
    );
    assert_eq!(
        manifest.dependencies([DependencyGroup::Dev]).collect::<Vec<_>>(),
        vec![("declared-peer", "2.0.0")],
    );
}
