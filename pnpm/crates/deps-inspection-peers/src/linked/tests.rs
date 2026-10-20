use crate::{ProjectManifests, peer_issues_for_lockfile};
use pnpm_config::PeerDependencyRules;
use pnpm_package_manifest::PackageManifest;

/// No project directory holds a `package.json`, so the report can only come
/// from the manifests the caller passed.
#[test]
fn linked_projects_are_checked_from_the_given_manifests() {
    let temp = tempfile::tempdir().unwrap();
    for dir in ["packages/app", "packages/lib", "packages/provider"] {
        std::fs::create_dir_all(temp.path().join(dir)).unwrap();
    }
    let manifest = |dir: &str, value: serde_json::Value| {
        (
            dir.to_string(),
            PackageManifest::from_value(
                temp.path()
                    .join(dir)
                    .join("package.json"),
                value,
            ),
        )
    };
    let lib = manifest(
        "packages/lib",
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "provider": "^1.0.0" },
        }),
    );
    let provider = manifest(
        "packages/provider",
        serde_json::json!({ "name": "provider", "version": "2.0.0" }),
    );
    let project_manifests: ProjectManifests<'_> = [(&lib.0, &lib.1), (&provider.0, &provider.1)]
        .into_iter()
        .map(|(importer_id, manifest)| (importer_id.clone(), manifest))
        .collect();
    let lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            "packages/app": { "dependencies": {
                "lib": { "specifier": "workspace:*", "version": "link:../lib" },
                "provider": { "specifier": "workspace:*", "version": "link:../provider" },
            } },
            "packages/lib": {},
            "packages/provider": {},
        },
    }))
    .expect("parse lockfile");

    let report = peer_issues_for_lockfile(
        &lockfile,
        temp.path(),
        &["packages/app".to_string()],
        &PeerDependencyRules::default(),
        None,
        true,
        &project_manifests,
    )
    .expect("inspect peers")
    .expect("the provider's version is outside the peer range");

    let issue = &report.issues()["packages/app"].bad["provider"][0];
    assert_eq!(issue.found_version, "2.0.0");
    assert_eq!(
        issue.parents
            .iter()
            .map(|parent| format!("{}@{}", parent.name, parent.version))
            .collect::<Vec<_>>(),
        ["lib@1.0.0"],
    );
}
