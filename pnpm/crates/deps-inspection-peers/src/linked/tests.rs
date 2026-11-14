use crate::{
    LockfilePeerCheck, ProjectManifests, check_peer_dependencies_from_lockfile,
    peer_issues_for_lockfile,
};
use pnpm_config::PeerDependencyRules;
use pnpm_package_manifest::PackageManifest;
use std::sync::Arc;

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
            super::resolve_link_version(
                temp.path(),
                &super::LinkRoots::same(temp.path()),
                "provider",
            ),
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

#[test]
fn dedicated_sibling_lockfile_provides_tarball_peer_versions() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("packages/app");
    let lib_dir = temp.path().join("packages/lib");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("package.json"),
        r#"{"name":"lib","version":"1.0.0","peerDependencies":{"provider":"^1.0.0"}}"#,
    )
    .unwrap();

    let app_lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "lib": { "specifier": "workspace:*", "version": "link:../lib" },
                },
            },
        },
    }))
    .expect("parse app lockfile");

    let lib_lockfile: pnpm_lockfile::Lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "provider": {
                        "specifier": "https://example.com/provider.tgz",
                        "version": "https://example.com/provider.tgz",
                    },
                },
            },
        },
        "packages": {
            "provider@https://example.com/provider.tgz": {
                "version": "2.0.0",
                "resolution": { "tarball": "https://example.com/provider.tgz" },
            },
        },
    }))
    .expect("parse lib lockfile");

    let canonical_lib = dunce::canonicalize(&lib_dir).unwrap();
    let project_dirs = [app_dir.clone()];
    let mut load_sibling = |dir: &std::path::Path, name: Option<&str>| {
        assert_eq!(dir, canonical_lib.as_path());
        assert_eq!(name, Some("lib"));
        Some(Arc::new(lib_lockfile.clone()))
    };
    let issues = check_peer_dependencies_from_lockfile(
        LockfilePeerCheck {
            lockfile: &app_lockfile,
            lockfile_dir: &app_dir,
            workspace_dir: Some(temp.path()),
            project_dirs: &project_dirs,
            catalogs: None,
            resolve_peers_from_workspace_root: false,
        },
        Some(&mut load_sibling),
    )
    .expect("inspect dedicated lockfile peers");

    assert!(issues["."].missing.is_empty());
    assert_eq!(issues["."].bad["provider"][0].found_version, "2.0.0");
}
