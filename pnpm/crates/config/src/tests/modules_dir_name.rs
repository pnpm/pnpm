use super::{Config, Path, PathBuf, assert_eq};

fn workspace() -> PathBuf {
    std::env::temp_dir().join("workspace")
}

fn config_with_modules_dir(raw: &str) -> Config {
    let mut config = Config::new();
    config.explicit_settings.insert("modulesDir".to_string(), raw.into());
    config.anchor_lockfile_paths(&workspace());
    config
}

#[test]
fn a_nested_modules_dir_keeps_every_segment() {
    for raw in ["www/modules", "./www/modules"] {
        let config = config_with_modules_dir(raw);
        assert_eq!(Path::new(config.modules_dir_name()), Path::new("www/modules"), "{raw}");
        assert_eq!(config.modules_dir_anchor(), Some(workspace().as_path()), "{raw}");
    }
}

#[test]
fn a_modules_dir_outside_the_project_keeps_only_its_last_segment() {
    let elsewhere = std::env::temp_dir().join("elsewhere");
    let absolute = elsewhere.join("modules");
    let cases = [
        ("../modules".to_string(), workspace().join("..")),
        (absolute.to_string_lossy().into_owned(), elsewhere),
    ];
    for (raw, anchor) in cases {
        let config = config_with_modules_dir(&raw);
        assert_eq!(Path::new(config.modules_dir_name()), Path::new("modules"), "{raw}");
        assert_eq!(config.modules_dir_anchor(), Some(anchor.as_path()), "{raw}");
    }
}

#[test]
fn a_modules_dir_moved_after_loading_keeps_only_its_last_segment() {
    let elsewhere = std::env::temp_dir().join("elsewhere");
    let mut config = config_with_modules_dir("www/modules");
    config.modules_dir = elsewhere.join("vendor");
    assert_eq!(Path::new(config.modules_dir_name()), Path::new("vendor"));
    assert_eq!(config.modules_dir_anchor(), Some(elsewhere.as_path()));
}

#[test]
fn a_package_config_nested_modules_dir_keeps_every_segment() {
    let mut config = Config::new();
    let project_dir = workspace().join("packages/member");
    let project_config = crate::package_configs::ProjectConfig {
        modules_dir: Some("www/modules".to_string()),
        ..Default::default()
    };
    project_config.apply_to(&mut config, &project_dir);
    assert_eq!(Path::new(config.modules_dir_name()), Path::new("www/modules"));
    assert_eq!(config.modules_dir_anchor(), Some(project_dir.as_path()));
}
