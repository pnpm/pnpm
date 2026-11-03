use super::project_structure_matches;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, LinkWorkspacePackages, PackageExtension};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Whether the structure check accepts `app`, which declares `app_fields`,
/// beside `lib`, recorded at `1.0.0` and now at `2.0.0`.
fn accepts_lib_bump(app_fields: Value, config: &Config, catalogs: &Catalogs) -> bool {
    accepts_lib_bump_with_pnpmfiles(app_fields, config, catalogs, Vec::new())
}

fn accepts_lib_bump_with_pnpmfiles(
    app_fields: Value,
    config: &Config,
    catalogs: &Catalogs,
    pnpmfiles: Vec<String>,
) -> bool {
    let mut app = json!({ "name": "app", "version": "1.0.0" });
    let Value::Object(app_fields) = app_fields else { panic!("the fields must be an object") };
    app.as_object_mut()
        .unwrap()
        .extend(app_fields);
    let app = PackageManifest::from_value(PathBuf::from("/ws/app/package.json"), app);
    let lib = PackageManifest::from_value(
        PathBuf::from("/ws/lib/package.json"),
        json!({ "name": "lib", "version": "2.0.0" }),
    );
    let project_manifests = [(PathBuf::from("/ws/app"), &app), (PathBuf::from("/ws/lib"), &lib)];
    let entry = |name: &str| ProjectEntry {
        name: Some(name.to_string()),
        version: Some("1.0.0".to_string()),
        has_modules_dir: true,
    };
    let state = WorkspaceState {
        projects: [("/ws/app".to_string(), entry("app")), ("/ws/lib".to_string(), entry("lib"))]
            .into_iter()
            .collect(),
        pnpmfiles,
        ..WorkspaceState::default()
    };
    project_structure_matches(&state, &project_manifests, config, catalogs)
}

#[test]
fn ignores_the_version_of_a_project_nothing_depends_on() {
    let dependencies = json!({ "dependencies": { "library": "1.0.0", "@scope/lib": "1.0.0" } });
    assert!(accepts_lib_bump(dependencies, &Config::new(), &Catalogs::new()));
}

#[test]
fn compares_the_version_of_a_project_a_dependency_names() {
    for group in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        let fields = json!({ group: { "lib": "workspace:*" } });
        assert!(!accepts_lib_bump(fields, &Config::new(), &Catalogs::new()), "{group}");
    }
}

#[test]
fn compares_the_version_of_a_project_an_alias_names() {
    for spec in ["npm:lib@^2.0.0", "npm:lib", "workspace:lib@*"] {
        let fields = json!({ "dependencies": { "renamed": spec } });
        assert!(!accepts_lib_bump(fields, &Config::new(), &Catalogs::new()), "{spec}");
    }
}

#[test]
fn compares_every_version_when_a_dependency_is_a_local_path() {
    for spec in ["workspace:../lib", "file:../lib", "link:../lib", "../lib"] {
        let fields = json!({ "dependencies": { "renamed": spec } });
        assert!(!accepts_lib_bump(fields, &Config::new(), &Catalogs::new()), "{spec}");
    }
}

#[test]
fn compares_the_version_of_a_project_a_catalog_entry_names() {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        [("renamed".to_string(), "npm:lib@^2.0.0".to_string())].into(),
    )]);
    let fields = json!({ "dependencies": { "renamed": "catalog:" } });
    assert!(!accepts_lib_bump(fields, &Config::new(), &catalogs));
}

#[test]
fn compares_the_version_of_a_project_a_setting_mentions() {
    let mut overridden = Config::new();
    overridden.overrides =
        Some(std::iter::once(("lib@1>dep".to_string(), "1.0.0".to_string())).collect());
    let mut extended = Config::new();
    let extension = PackageExtension {
        dependencies: Some([("renamed".to_string(), "npm:lib@^2.0.0".to_string())].into()),
        ..PackageExtension::default()
    };
    extended.package_extensions = Some(std::iter::once(("app".to_string(), extension)).collect());
    let mut patched = Config::new();
    patched.patched_dependencies =
        Some(std::iter::once(("lib@1.0.0".to_string(), "patches/lib.patch".to_string())).collect());
    for config in [&overridden, &extended, &patched] {
        assert!(!accepts_lib_bump(json!({}), config, &Catalogs::new()));
    }
}

#[test]
fn compares_every_version_under_deep_linking() {
    let mut config = Config::new();
    config.link_workspace_packages = LinkWorkspacePackages::Deep;
    assert!(!accepts_lib_bump(json!({}), &config, &Catalogs::new()));
}

#[test]
fn compares_every_version_when_a_pnpmfile_is_loaded() {
    let pnpmfiles = vec!["/ws/.pnpmfile.cjs".to_string()];
    assert!(!accepts_lib_bump_with_pnpmfiles(
        json!({}),
        &Config::new(),
        &Catalogs::new(),
        pnpmfiles
    ));
}
