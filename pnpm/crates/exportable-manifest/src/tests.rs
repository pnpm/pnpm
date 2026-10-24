//! Tests for the workspace-protocol rewrite. Rather than run a full
//! install and read from the resulting `node_modules`, these
//! materialize the `node_modules` layout directly with
//! `tempfile::TempDir` so they exercise `replace_workspace_protocol`
//! and `replace_workspace_protocol_peer_dependency` in isolation.

use std::{collections::HashMap, fs, path::Path};

use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use serde_json::Value;
use tempfile::TempDir;

use super::{
    CannotResolveReason, CannotResolveWorkspaceProtocolError, CreateExportableManifestOptions,
    ReplaceWorkspaceProtocolError, WorkspacePackageManifest, WorkspacePackageManifests,
    create_exportable_manifest, replace_workspace_protocol,
    replace_workspace_protocol_peer_dependency,
};

/// Materialize the install tree the workspace-protocol rewrite case
/// needs. Returns `(temp_root, project_dir)`:
/// `project_dir = <temp>/workspace-protocol-package` so the relative
/// `workspace:../xerox` resolves to a sibling at `<temp>/xerox`.
fn workspace_fixture() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().expect("tempdir");
    let project = temp.path().join("workspace-protocol-package");
    let modules = project.join("node_modules");
    fs::create_dir_all(&modules).unwrap();

    // The local alias `bar` resolves to the workspace project named
    // `@foo/bar`; `node_modules/bar/package.json` carries the resolved
    // manifest (same name, copied / linked there by `pnpm install`).
    write_dep(&modules.join("bar"), "@foo/bar", "3.2.1");
    write_dep(&modules.join("baz"), "baz", "1.2.3");
    write_dep(&modules.join("foo"), "foo", "4.5.6");
    write_dep(&modules.join("qux"), "qux", "1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay");
    write_dep(&modules.join("quux"), "quux", "7.8.9");
    write_dep(&modules.join("waldo"), "waldo", "1.9.0");

    // `workspace:../xerox` reads the sibling project directly, not from
    // `node_modules`.
    write_dep(&temp.path().join("xerox"), "xerox", "4.5.6");
    (temp, project)
}

fn write_dep(dir: &Path, name: &str, version: &str) {
    fs::create_dir_all(dir).unwrap();
    let manifest = serde_json::json!({ "name": name, "version": version });
    fs::write(dir.join("package.json"), serde_json::to_string(&manifest).unwrap()).unwrap();
}

fn rewrite(dep_name: &str, dep_spec: &str, dir: &Path) -> String {
    replace_workspace_protocol(dep_name, dep_spec, dir, None, None).expect("replace succeeds")
}

fn rewrite_peer(dep_name: &str, dep_spec: &str, dir: &Path) -> String {
    replace_workspace_protocol_peer_dependency(dep_name, dep_spec, dir, None, None)
        .expect("replace succeeds")
}

#[test]
fn passes_through_non_workspace_specs() {
    let dir = TempDir::new().unwrap();
    assert_eq!(rewrite("foo", "^1.0.0", dir.path()), "^1.0.0");
    assert_eq!(rewrite("foo", "npm:bar@1", dir.path()), "npm:bar@1");
}

#[test]
fn workspace_dep_rewrites_match_upstream() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(rewrite("bar", "workspace:@foo/bar@*", dir), "npm:@foo/bar@3.2.1");
    assert_eq!(rewrite("baz", "workspace:baz@^", dir), "^1.2.3");
    assert_eq!(rewrite("foo", "workspace:*", dir), "4.5.6");
    assert_eq!(
        rewrite("qux", "workspace:^", dir),
        "^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay",
    );
    assert_eq!(rewrite("quux", "workspace:", dir), "7.8.9");
    assert_eq!(rewrite("waldo", "workspace:^", dir), "^1.9.0");
    assert_eq!(rewrite("xerox", "workspace:../xerox", dir), "4.5.6");
    assert_eq!(rewrite("xeroxAlias", "workspace:../xerox", dir), "npm:xerox@4.5.6");
    assert_eq!(rewrite("corge", "workspace:1.0.0", dir), "1.0.0");
    assert_eq!(rewrite("grault", "workspace:^1.0.0", dir), "^1.0.0");
    assert_eq!(rewrite("garply", "workspace:plugh@2.0.0", dir), "npm:plugh@2.0.0");
}

#[test]
fn peer_workspace_dep_rewrites_match_upstream() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(rewrite_peer("foo", "workspace:>= || ^3.9.0", dir), ">=4.5.6 || ^3.9.0");
    assert_eq!(rewrite_peer("baz", "^1.0.0 || workspace:>", dir), "^1.0.0 || >1.2.3");
    assert_eq!(rewrite_peer("bar", "workspace:^3.0.0", dir), "^3.0.0");
    assert_eq!(
        rewrite_peer("qux", "workspace:^", dir),
        "^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay",
    );
    assert_eq!(rewrite_peer("waldo", "workspace:^1.x", dir), "^1.x");
}

/// Only the first `workspace:` occurrence is stripped.
#[test]
fn peer_workspace_strip_only_removes_first_occurrence() {
    let (_fixture, project) = workspace_fixture();
    let dir = project.as_path();

    assert_eq!(
        rewrite_peer("baz", "workspace:^1.0.0 || workspace:^2.0.0", dir),
        "^1.0.0 || workspace:^2.0.0",
    );
}

#[test]
fn missing_dependency_surfaces_cannot_resolve_error() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    fs::create_dir_all(dir.join("node_modules")).unwrap();

    let err = replace_workspace_protocol("ghost", "workspace:*", dir, None, None).unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
            reason: CannotResolveReason::NotInstalled,
            ..
        }) if dep_name == "ghost"
    ));
}

#[test]
fn missing_dependency_surfaces_cannot_resolve_error_for_peer() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    fs::create_dir_all(dir.join("node_modules")).unwrap();

    let err = replace_workspace_protocol_peer_dependency("ghost", "workspace:^", dir, None, None)
        .unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
            reason: CannotResolveReason::NotInstalled,
            ..
        }) if dep_name == "ghost"
    ));
}

#[test]
fn missing_version_on_dependency_is_not_reported_as_not_installed() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    let dep_dir = dir.join("node_modules/pkg-b");
    fs::create_dir_all(&dep_dir).unwrap();
    fs::write(dep_dir.join("package.json"), r#"{ "name": "pkg-b" }"#).unwrap();

    let err = replace_workspace_protocol("pkg-b", "workspace:*", dir, None, None).unwrap_err();
    match err {
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
            reason: CannotResolveReason::MissingVersion,
            ..
        }) => assert_eq!(dep_name, "pkg-b"),
        other => panic!("expected MissingVersion, got {other:?}"),
    }

    let err = replace_workspace_protocol_peer_dependency("pkg-b", "workspace:*", dir, None, None)
        .unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
            reason: CannotResolveReason::MissingVersion,
            ..
        }) if dep_name == "pkg-b"
    ));
}

#[test]
fn scoped_peer_workspace_spec_resolves_from_workspace_packages() {
    let dir = TempDir::new().unwrap();
    let mut ws_pkgs = std::collections::HashMap::new();
    ws_pkgs.insert(
        "@scope/prettier-config".to_string(),
        vec![WorkspacePackageManifest {
            name: "@scope/prettier-config".to_string(),
            version: "2.0.0".to_string(),
            root_dir: None,
            registry_prefix: None,
        }],
    );

    let res = replace_workspace_protocol_peer_dependency(
        "@scope/prettier-config",
        "workspace:*",
        dir.path(),
        None,
        Some(&ws_pkgs),
    )
    .expect("resolves scoped peer from workspace_packages");
    assert_eq!(res, "2.0.0");
}

#[test]
fn workspace_package_without_version_reports_missing_version() {
    let dir = TempDir::new().unwrap();
    let mut ws_pkgs = std::collections::HashMap::new();
    ws_pkgs.insert(
        "@scope/eslint-config".to_string(),
        vec![WorkspacePackageManifest {
            name: "@scope/eslint-config".to_string(),
            version: String::new(),
            root_dir: None,
            registry_prefix: None,
        }],
    );

    let err = replace_workspace_protocol_peer_dependency(
        "@scope/eslint-config",
        "workspace:~",
        dir.path(),
        None,
        Some(&ws_pkgs),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            dep_name,
            reason: CannotResolveReason::MissingVersion,
            ..
        }) if dep_name == "@scope/eslint-config"
    ));
}

#[test]
fn missing_version_error_help_names_the_package() {
    let err = CannotResolveWorkspaceProtocolError {
        dep_name: "alias".to_string(),
        package_name: "pkg-b".to_string(),
        reason: CannotResolveReason::MissingVersion,
    };
    let help = err
        .help()
        .map(|help| help.to_string())
        .expect("help for MissingVersion");
    assert_eq!(help, r#"Add a "version" field to the package.json of "pkg-b"."#);
}

#[test]
fn missing_version_help_uses_the_workspace_package_name() {
    let dir = TempDir::new().unwrap();
    let mut ws_pkgs = std::collections::HashMap::new();
    ws_pkgs.insert(
        "pkg-b".to_string(),
        vec![WorkspacePackageManifest {
            name: "pkg-b".to_string(),
            version: String::new(),
            root_dir: None,
            registry_prefix: None,
        }],
    );

    let err =
        replace_workspace_protocol("alias", "workspace:pkg-b@*", dir.path(), None, Some(&ws_pkgs))
            .unwrap_err();
    let ReplaceWorkspaceProtocolError::CannotResolve(err) = err else {
        panic!("expected CannotResolveWorkspaceProtocolError");
    };
    assert_eq!(err.package_name, "pkg-b");
    assert_eq!(
        err.help()
            .map(|help| help.to_string())
            .expect("help for MissingVersion"),
        r#"Add a "version" field to the package.json of "pkg-b"."#,
    );
}

#[test]
fn installed_manifest_without_name_keeps_the_missing_name_reason() {
    let dir = TempDir::new().unwrap();
    let dep_dir = dir.path().join("node_modules/pkg-b");
    fs::create_dir_all(&dep_dir).unwrap();
    fs::write(dep_dir.join("package.json"), r#"{ "version": "1.0.0" }"#).unwrap();
    let mut ws_pkgs = std::collections::HashMap::new();
    ws_pkgs.insert(
        "pkg-b".to_string(),
        vec![WorkspacePackageManifest {
            name: "pkg-b".to_string(),
            version: String::new(),
            root_dir: None,
            registry_prefix: None,
        }],
    );

    let err = replace_workspace_protocol("pkg-b", "workspace:*", dir.path(), None, Some(&ws_pkgs))
        .unwrap_err();
    assert!(matches!(
        err,
        ReplaceWorkspaceProtocolError::CannotResolve(CannotResolveWorkspaceProtocolError {
            reason: CannotResolveReason::MissingName,
            ..
        })
    ));
}

/// `workspace:*` doesn't reach into the manifest for the version
/// token (the version comes from the lookup), so when the dep name
/// doesn't match the resolved manifest's name the output is an
/// `npm:`-aliased reference.
#[test]
fn dep_name_mismatch_routes_to_npm_alias() {
    let fixture = TempDir::new().unwrap();
    let dir = fixture.path();
    let modules = dir.join("node_modules");
    write_dep(&modules.join("local-name"), "actual-name", "1.2.3");

    assert_eq!(rewrite("local-name", "workspace:*", dir), "npm:actual-name@1.2.3");
}

/// The packed manifest is the input to the tarball hash, so a
/// dependency map that reorders between runs makes an unchanged
/// package pack to different bytes.
#[test]
fn published_dependencies_keep_declaration_order() {
    let (_fixture, project) = workspace_fixture();
    let catalogs = Catalogs::default();
    let manifest = serde_json::json!({
        "name": "workspace-protocol-package",
        "version": "1.0.0",
        "dependencies": {
            "waldo": "workspace:*",
            "baz": "workspace:*",
            "quux": "workspace:*",
            "foo": "workspace:*",
        },
    });

    let published = create_exportable_manifest(
        &project,
        &manifest,
        &CreateExportableManifestOptions {
            catalogs: &catalogs,
            workspace_dir: None,
            modules_dir: None,
            skip_manifest_obfuscation: false,
            embed_readme: false,
            workspace_packages: None,
        },
    )
    .expect("manifest is exportable");

    let dependencies = published
        .get("dependencies")
        .and_then(Value::as_object)
        .expect("dependencies survive");
    assert_eq!(
        dependencies.iter().collect::<Vec<_>>(),
        vec![
            (&"waldo".to_string(), &Value::from("1.9.0")),
            (&"baz".to_string(), &Value::from("1.2.3")),
            (&"quux".to_string(), &Value::from("7.8.9")),
            (&"foo".to_string(), &Value::from("4.5.6")),
        ],
    );
}

#[test]
fn resolves_workspace_protocol_from_workspace_packages_when_node_modules_is_absent() {
    let dir = TempDir::new().unwrap();
    let mut ws_pkgs = std::collections::HashMap::new();
    ws_pkgs.insert(
        "dep-a".to_string(),
        vec![WorkspacePackageManifest {
            name: "dep-a".to_string(),
            version: "1.2.3".to_string(),
            root_dir: None,
            registry_prefix: None,
        }],
    );
    ws_pkgs.insert(
        "dep-b".to_string(),
        vec![WorkspacePackageManifest {
            name: "dep-b".to_string(),
            version: "2.3.4".to_string(),
            root_dir: None,
            registry_prefix: None,
        }],
    );

    let res = replace_workspace_protocol("dep-a", "workspace:^", dir.path(), None, Some(&ws_pkgs))
        .expect("resolves from workspace_packages");
    assert_eq!(res, "^1.2.3");

    let res = replace_workspace_protocol(
        "my-alias",
        "workspace:dep-b@~",
        dir.path(),
        None,
        Some(&ws_pkgs),
    )
    .expect("resolves aliased dep from workspace_packages");
    assert_eq!(res, "npm:dep-b@~2.3.4");

    let res = replace_workspace_protocol_peer_dependency(
        "dep-a",
        "workspace:>=1.0.0",
        dir.path(),
        None,
        Some(&ws_pkgs),
    )
    .expect("resolves peer dep");
    assert_eq!(res, ">=1.0.0");

    let res = replace_workspace_protocol_peer_dependency(
        "dep-a",
        "workspace:^",
        dir.path(),
        None,
        Some(&ws_pkgs),
    )
    .expect("resolves peer dep with sentinel");
    assert_eq!(res, "^1.2.3");
}

fn prefixed(name: &str, version: &str) -> WorkspacePackageManifest {
    WorkspacePackageManifest {
        name: name.to_string(),
        version: version.to_string(),
        root_dir: None,
        registry_prefix: Some("work".to_string()),
    }
}

fn prefixed_workspace() -> WorkspacePackageManifests {
    HashMap::from([
        ("dep-a".to_string(), vec![prefixed("dep-a", "1.2.3")]),
        ("dep-b".to_string(), vec![prefixed("dep-b", "2.3.4")]),
    ])
}

/// Two projects of one name, each publishing through its own registry.
fn two_versions_of_dup() -> WorkspacePackageManifests {
    let project = |version: &str, prefix: &str| WorkspacePackageManifest {
        name: "dup".to_string(),
        version: version.to_string(),
        root_dir: None,
        registry_prefix: Some(prefix.to_string()),
    };
    HashMap::from([("dup".to_string(), vec![project("1.0.0", "old"), project("2.0.0", "work")])])
}

#[test]
fn workspace_dep_carries_the_registry_prefix_of_its_target() {
    let dir = TempDir::new().unwrap();
    let ws_pkgs = prefixed_workspace();
    let rewrite = |dep_name: &str, spec: &str| {
        replace_workspace_protocol(dep_name, spec, dir.path(), None, Some(&ws_pkgs))
            .expect("replace succeeds")
    };

    assert_eq!(rewrite("dep-a", "workspace:*"), "work:1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:^"), "work:^1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:~"), "work:~1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:"), "work:1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:1.2.3"), "work:1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:^1.0.0"), "work:^1.0.0");
    assert_eq!(rewrite("alias", "workspace:dep-b@*"), "work:dep-b@2.3.4");
    assert_eq!(rewrite("alias", "workspace:dep-b@^"), "work:dep-b@^2.3.4");
    assert_eq!(rewrite("alias", "workspace:dep-b@2.3.4"), "work:dep-b@2.3.4");
}

/// The installed copy decides the version, the workspace package of its name
/// the prefix.
#[test]
fn installed_workspace_dep_carries_the_prefix_of_the_workspace_package_of_its_name() {
    let dir = TempDir::new().unwrap();
    write_dep(&dir.path().join("node_modules/dep-a"), "dep-a", "1.0.0");
    let ws_pkgs = prefixed_workspace();

    let spec = replace_workspace_protocol("dep-a", "workspace:^", dir.path(), None, Some(&ws_pkgs))
        .expect("replace succeeds");

    assert_eq!(spec, "work:^1.0.0");
}

#[test]
fn explicit_version_of_a_package_outside_the_workspace_stays_plain() {
    let dir = TempDir::new().unwrap();
    let ws_pkgs = prefixed_workspace();

    assert_eq!(rewrite("other", "workspace:1.0.0", dir.path()), "1.0.0");
    assert_eq!(rewrite("alias", "workspace:other@1.0.0", dir.path()), "npm:other@1.0.0");
    assert_eq!(
        replace_workspace_protocol("other", "workspace:1.0.0", dir.path(), None, Some(&ws_pkgs))
            .expect("replace succeeds"),
        "1.0.0",
    );
}

#[test]
fn peer_ranges_omit_the_registry_prefix() {
    let dir = TempDir::new().unwrap();
    let ws_pkgs = prefixed_workspace();
    let rewrite = |dep_name: &str, spec: &str| {
        replace_workspace_protocol_peer_dependency(dep_name, spec, dir.path(), None, Some(&ws_pkgs))
            .expect("replace succeeds")
    };

    assert_eq!(rewrite("dep-a", "workspace:*"), "1.2.3");
    assert_eq!(rewrite("dep-a", "workspace:^"), "^1.2.3");
    assert_eq!(rewrite("dep-a", "^1.0.0 || workspace:~"), "^1.0.0 || ~1.2.3");
    assert_eq!(rewrite("alias", "workspace:dep-b@^"), "npm:dep-b@*");
}

#[test]
fn installed_copy_takes_the_prefix_of_the_project_at_its_version() {
    let dir = TempDir::new().unwrap();
    write_dep(&dir.path().join("node_modules/dup"), "dup", "2.0.0");
    let ws_pkgs = two_versions_of_dup();

    let spec = replace_workspace_protocol("dup", "workspace:^", dir.path(), None, Some(&ws_pkgs))
        .expect("replace succeeds");

    assert_eq!(spec, "work:^2.0.0");
}

#[test]
fn explicit_version_takes_the_prefix_of_the_project_at_that_version() {
    let dir = TempDir::new().unwrap();
    let ws_pkgs = two_versions_of_dup();
    let rewrite = |dep_name: &str, spec: &str| {
        replace_workspace_protocol(dep_name, spec, dir.path(), None, Some(&ws_pkgs))
            .expect("replace succeeds")
    };

    assert_eq!(rewrite("dup", "workspace:2.0.0"), "work:2.0.0");
    assert_eq!(rewrite("dup", "workspace:1.0.0"), "old:1.0.0");
    assert_eq!(rewrite("alias", "workspace:dup@1.0.0"), "old:dup@1.0.0");
    // A range names neither project, so no registry is guessed.
    assert_eq!(rewrite("dup", "workspace:^1.0.0"), "^1.0.0");
}

#[test]
fn uninstalled_dep_falls_back_on_the_first_project_with_a_version() {
    let dir = TempDir::new().unwrap();
    let ws_pkgs = two_versions_of_dup();

    let spec = replace_workspace_protocol("dup", "workspace:*", dir.path(), None, Some(&ws_pkgs))
        .expect("replace succeeds");

    assert_eq!(spec, "old:1.0.0");
}

#[test]
fn relative_path_takes_the_prefix_of_the_project_in_that_directory() {
    let workspace = TempDir::new().unwrap();
    let first = workspace.path().join("first");
    let second = workspace.path().join("second");
    write_dep(&first, "dup", "1.0.0");
    write_dep(&second, "dup", "1.0.0");
    let project = |root_dir: &Path, prefix: &str| WorkspacePackageManifest {
        name: "dup".to_string(),
        version: "1.0.0".to_string(),
        root_dir: Some(root_dir.to_path_buf()),
        registry_prefix: Some(prefix.to_string()),
    };
    let ws_pkgs = HashMap::from([(
        "dup".to_string(),
        vec![project(&first, "old"), project(&second, "work")],
    )]);
    let consumer = workspace.path().join("consumer");
    fs::create_dir_all(&consumer).unwrap();

    let rewrite = |spec: &str| {
        replace_workspace_protocol("dup", spec, &consumer, None, Some(&ws_pkgs))
            .expect("replace succeeds")
    };

    assert_eq!(rewrite("workspace:../second"), "work:1.0.0");
    assert_eq!(rewrite("workspace:../first"), "old:1.0.0");
}

#[test]
fn several_projects_at_one_version_select_no_prefix() {
    let dir = TempDir::new().unwrap();
    write_dep(&dir.path().join("node_modules/dup"), "dup", "1.0.0");
    let project = |prefix: &str| WorkspacePackageManifest {
        name: "dup".to_string(),
        version: "1.0.0".to_string(),
        root_dir: None,
        registry_prefix: Some(prefix.to_string()),
    };
    let ws_pkgs = HashMap::from([("dup".to_string(), vec![project("old"), project("work")])]);
    let rewrite = |dep_name: &str, spec: &str| {
        replace_workspace_protocol(dep_name, spec, dir.path(), None, Some(&ws_pkgs))
            .expect("replace succeeds")
    };

    assert_eq!(rewrite("dup", "workspace:1.0.0"), "1.0.0");
    assert_eq!(rewrite("dup", "workspace:^"), "^1.0.0");
}
