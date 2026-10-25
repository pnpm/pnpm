use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, OsString, Path, PathBuf,
    assert_eq, fs, io, tempdir,
};

const WORK: &str = "https://npm.work.example.com/";

#[test]
fn the_built_in_prefixes_are_not_declared() {
    let declared = Config::new().workspace_declared_registry_prefixes();
    assert_eq!(declared.prefix_for("https://registry.npmjs.org/"), None);
    assert_eq!(declared.prefix_for("https://npm.pkg.github.com/"), None);
}

#[test]
fn only_prefixes_from_the_workspace_yaml_are_declared() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "namedRegistries:\n  personal: https://registry.npmjs.org/\n  mine: https://npm.mine.example.com/\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        format!("registries:\n  {WORK}:\n    prefix: work\nnamedRegistries:\n  legacy: https://npm.legacy.example.com/\n"),
    )
    .expect("write workspace yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    let declared = config.workspace_declared_registry_prefixes();

    assert_eq!(declared.prefix_for(WORK), Some("work"));
    assert_eq!(declared.prefix_for("https://npm.legacy.example.com/"), Some("legacy"));
    assert_eq!(declared.prefix_for("https://registry.npmjs.org/"), None);
    assert_eq!(declared.prefix_for("https://npm.mine.example.com/"), None);
    assert!(config.registries_by_prefix.contains_key("mine"), "global aliases still resolve");
}

#[test]
fn a_workspace_alias_overrides_a_global_alias_of_the_same_name() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "namedRegistries:\n  work: https://registry.npmjs.org/\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        format!("namedRegistries:\n  work: {WORK}\n"),
    )
    .expect("write workspace yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    let declared = config.workspace_declared_registry_prefixes();

    assert_eq!(config.registries_by_prefix.get("work").map(String::as_str), Some(WORK));
    assert_eq!(declared.prefix_for(WORK), Some("work"));
    assert_eq!(declared.prefix_for("https://registry.npmjs.org/"), None);
}

#[test]
fn a_workspace_alias_that_is_not_applied_does_not_declare_the_global_alias() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "namedRegistries:\n  work: https://registry.npmjs.org/\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "registries:\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\nnamedRegistries:\n  work: https://pypi.example.com/simple/\n",
    )
    .expect("write workspace yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    let declared = config.workspace_declared_registry_prefixes();

    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(declared.prefix_for("https://registry.npmjs.org/"), None);
}
