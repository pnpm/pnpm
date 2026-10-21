use crate::{ProjectManifests, peer_issues_for_lockfile};
use pnpm_config::PeerDependencyRules;
use pnpm_package_manifest::PackageManifest;

#[test]
fn linked_projects_are_checked_from_all_manifest_formats() {
    for (basename, lib_manifest, provider_manifest) in [
        (
            "package.json",
            r#"{"name":"lib","version":"1.0.0","peerDependencies":{"provider":"^1.0.0"}}"#,
            r#"{"name":"provider","version":"2.0.0"}"#,
        ),
        (
            "package.json5",
            "{name: 'lib', version: '1.0.0', peerDependencies: {provider: '^1.0.0'}}",
            "{name: 'provider', version: '2.0.0'}",
        ),
        (
            "package.yaml",
            "name: lib\nversion: 1.0.0\npeerDependencies:\n  provider: ^1.0.0\n",
            "name: provider\nversion: 2.0.0\n",
        ),
    ] {
        eprintln!("manifest format: {basename}");
        let temp = tempfile::tempdir().unwrap();
        for dir in ["app", "lib", "provider"] {
            std::fs::create_dir_all(temp.path().join(dir)).unwrap();
        }
        std::fs::write(temp.path().join("lib").join(basename), lib_manifest).unwrap();
        std::fs::write(
            temp.path()
                .join("provider")
                .join(basename),
            provider_manifest,
        )
        .unwrap();
        let lockfile = serde_json::from_value(serde_json::json!({
            "lockfileVersion": "9.0",
            "importers": {
                "app": { "dependencies": {
                    "lib": { "specifier": "workspace:*", "version": "link:../lib" },
                    "provider": { "specifier": "workspace:*", "version": "link:../provider" },
                } },
                "lib": {},
                "provider": {},
            },
        }))
        .expect("parse lockfile");
        let report = peer_issues_for_lockfile(
            &lockfile,
            temp.path(),
            &["app".to_string()],
            &PeerDependencyRules::default(),
            None,
            true,
            &ProjectManifests::new(),
        )
        .expect("inspect peers")
        .expect("the provider's version is outside the peer range");
        let issue = &report.issues()["app"].bad["provider"][0];
        assert_eq!(issue.found_version, "2.0.0");
        assert_eq!(issue.parents[0].name, "lib");
        assert_eq!(issue.parents[0].version, "1.0.0");
        assert_eq!(
            super::resolve_link_version(temp.path(), temp.path(), "provider"),
            Some("2.0.0".to_string()),
        );
    }
}

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
