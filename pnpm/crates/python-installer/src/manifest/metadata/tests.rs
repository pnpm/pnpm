use super::Manifest;
use crate::manifest::DependencySelection;
use pnpm_config::Config;

#[test]
fn dynamic_extras_keep_platform_markers_and_dependency_groups() {
    let mut manifest = Manifest::parse("[project]\nname = 'app'\ndynamic = ['version', 'dependencies', 'optional-dependencies']\n[dependency-groups]\ndev = ['pytest']\n").unwrap();
    let metadata = serde_json::from_value(serde_json::json!({
        "name": "app", "version": "1.0", "requires_python": null,
        "provides_extra": ["cli"], "dist_info": "app-1.0.dist-info", "purelib": true,
        "requires_dist": ["alpha", "beta; extra == 'cli' and sys_platform == 'linux'"]
    }))
    .unwrap();
    manifest.set_metadata(metadata, None).unwrap();
    let mut config = Config::new();
    let requirements = |config: &Config, selection| {
        manifest
            .requirements(config, selection)
            .unwrap()
            .into_iter()
            .map(|requirement| requirement.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(requirements(&config, DependencySelection::ALL), ["alpha", "pytest"]);
    config.python.extras.push("cli".to_string());
    assert_eq!(
        requirements(&config, DependencySelection::ALL),
        ["alpha", "beta ; sys_platform == 'linux'", "pytest"],
    );
    assert_eq!(
        requirements(&config, DependencySelection { production: false, development: true }),
        ["pytest"],
    );
    assert_eq!(
        manifest.project
            .as_ref()
            .unwrap()
            .version
            .as_ref()
            .unwrap()
            .to_string(),
        "1.0",
    );
}

#[test]
fn static_extras_select_using_their_normalized_names() {
    let mut manifest = Manifest::parse("[project]\nname = 'app'\ndynamic = ['version']\n[project.optional-dependencies]\ndev_tools = ['alpha']\n").unwrap();
    let metadata = serde_json::from_value(serde_json::json!({
        "name": "app", "version": "1.0", "requires_python": null,
        "provides_extra": ["dev-tools"], "dist_info": "app-1.0.dist-info", "purelib": true,
        "requires_dist": ["alpha; extra == 'dev-tools'"]
    }))
    .unwrap();
    manifest.validate_static_metadata(&metadata).unwrap();
    manifest.set_metadata(metadata, None).unwrap();
    let mut config = Config::new();
    config.python.extras.push("dev_tools".to_string());
    let requirements = manifest.requirements(&config, DependencySelection::ALL).unwrap();
    assert_eq!(
        requirements
            .into_iter()
            .map(|requirement| requirement.to_string())
            .collect::<Vec<_>>(),
        ["alpha"],
    );
}

#[test]
fn dynamic_extras_use_project_overrides_and_skip_missing_workspace_defaults() {
    let contents =
        "[project]\nname = 'app'\ndynamic = ['version', 'dependencies', 'optional-dependencies']\n";
    let metadata = serde_json::from_value(serde_json::json!({
        "name": "app", "version": "1.0", "requires_python": null,
        "provides_extra": ["web"], "dist_info": "app-1.0.dist-info", "purelib": true,
        "requires_dist": ["alpha", "beta; extra == 'web'"]
    }))
    .unwrap();
    let mut manifest = Manifest::parse(contents).unwrap();
    manifest.set_metadata(metadata, None).unwrap();
    let mut config = Config::new();
    config.python.extras = vec!["cli".into()];
    assert_eq!(
        manifest
            .requirements(&config, DependencySelection::ALL)
            .unwrap()
            .len(),
        1,
    );
    let metadata = manifest.metadata.clone().unwrap();
    let mut manifest =
        Manifest::parse(&format!("{contents}\n[tool.pnpm.python]\nextras = ['web']\n")).unwrap();
    manifest.set_metadata(metadata, None).unwrap();
    assert_eq!(
        manifest
            .requirements(&config, DependencySelection::ALL)
            .unwrap()
            .len(),
        2,
    );
}
