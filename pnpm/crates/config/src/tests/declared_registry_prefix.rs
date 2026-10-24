use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, OsString, Path, PathBuf,
    assert_eq, fs, io, tempdir,
};

const WORK: &str = "https://npm.work.example.com/";

fn config_with_prefixes(prefixes: &[(&str, &str)]) -> Config {
    let mut config = Config::new();
    config.registries_by_prefix = prefixes
        .iter()
        .map(|(prefix, url)| (prefix.to_string(), url.to_string()))
        .collect();
    config.workspace_registry_prefixes = prefixes
        .iter()
        .map(|(prefix, _)| prefix.to_string())
        .collect();
    config
}

#[test]
fn a_registry_matches_its_prefix_regardless_of_trailing_slash_and_host_case() {
    let config = config_with_prefixes(&[("work", "https://NPM.Work.Example.com")]);
    assert_eq!(config.declared_registry_prefix(WORK), Some("work"));
    assert_eq!(config.declared_registry_prefix("HTTPS://npm.work.example.com"), Some("work"));
    assert_eq!(config.declared_registry_prefix("https://npm.other.example.com/"), None);
}

#[test]
fn a_registry_path_keeps_its_case() {
    let config = config_with_prefixes(&[("work", "https://registry.example.com/Work/")]);
    assert_eq!(config.declared_registry_prefix("https://registry.example.com/work/"), None);
    assert_eq!(config.declared_registry_prefix("https://registry.example.com/Work"), Some("work"));
}

#[test]
fn the_alphabetically_first_of_several_prefixes_on_one_registry_is_answered() {
    let config = config_with_prefixes(&[("zeta", WORK), ("alpha", WORK), ("mid", WORK)]);
    assert_eq!(config.declared_registry_prefix(WORK), Some("alpha"));
}

#[test]
fn the_built_in_prefixes_are_not_declared() {
    assert_eq!(Config::new().declared_registry_prefix("https://registry.npmjs.org/"), None);
    assert_eq!(Config::new().declared_registry_prefix("https://npm.pkg.github.com/"), None);
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

    assert_eq!(config.declared_registry_prefix(WORK), Some("work"));
    assert_eq!(config.declared_registry_prefix("https://npm.legacy.example.com/"), Some("legacy"));
    assert_eq!(config.declared_registry_prefix("https://registry.npmjs.org/"), None);
    assert_eq!(config.declared_registry_prefix("https://npm.mine.example.com/"), None);
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

    assert_eq!(config.registries_by_prefix.get("work").map(String::as_str), Some(WORK));
    assert_eq!(config.declared_registry_prefix(WORK), Some("work"));
    assert_eq!(config.declared_registry_prefix("https://registry.npmjs.org/"), None);
}
