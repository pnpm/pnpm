use super::{
    Config,
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    LinkProbe,
    NPM_DEFAULT_REGISTRY,
    OsString,
    Path,
    PathBuf,
    TrustPolicy,
    assert_eq,
    fs,
    io,
    tempdir,
    write_file,
};
use crate::{
    CliLayer,
    CommandTarget,
};

const GLOBAL_CONFIG: &str = "\
trustPolicy: no-downgrade
trustPolicyExclude:
  - from-the-top-level
globalPackagesConfig:
  trustPolicyExclude:
    - from-global-packages
  permissions:
    esbuild:
      build: true
";

/// A pnpm home and a global config dir holding `config_yaml`, with
/// `XDG_CONFIG_HOME` pointed at the config dir's parent.
struct Machine {
    pnpm_home: tempfile::TempDir,
    xdg: tempfile::TempDir,
}

impl Machine {
    fn with_global_config(config_yaml: &str) -> Self {
        let machine = Self {
            pnpm_home: tempdir().expect("pnpm home tempdir"),
            xdg: tempdir().expect("config tempdir"),
        };
        let config_dir = machine.xdg.path().join("pnpm");
        fs::create_dir_all(&config_dir).expect("create global config dir");
        write_file(&config_dir.join("config.yaml"), config_yaml);
        machine
    }

    fn xdg_config_home(&self) -> &str {
        self.xdg
            .path()
            .to_str()
            .expect("UTF-8 tempdir")
    }
}

/// A config seeded for a command that acts on the global packages.
fn global_packages_load() -> Config {
    Config {
        cli: CliLayer { command_target: CommandTarget::GlobalPackages, ..CliLayer::default() },
        ..Config::default()
    }
}

#[test]
fn global_packages_settings_apply_to_a_global_packages_load() {
    fake_env!();
    let machine = Machine::with_global_config(GLOBAL_CONFIG);
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(config.trust_policy_exclude, Some(vec!["from-global-packages".to_string()]));
    assert_eq!(config.allow_builds.get("esbuild"), Some(&true));
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
}

#[test]
fn global_packages_settings_do_not_apply_to_a_project_load() {
    fake_env!();
    let machine = Machine::with_global_config(GLOBAL_CONFIG);
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        Config::default().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(config.trust_policy_exclude, Some(vec!["from-the-top-level".to_string()]));
    assert_eq!(config.allow_builds.get("esbuild"), None);
    assert!(config.global_packages_config.is_some());
}

#[test]
fn environment_overrides_global_packages_settings() {
    fake_env!();
    let machine = Machine::with_global_config("globalPackagesConfig:\n  trustPolicy: off\n");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", machine.xdg_config_home()),
        ("PNPM_CONFIG_TRUST_POLICY", "no-downgrade"),
    ]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
}

#[test]
fn a_project_manifest_cannot_set_global_packages_settings() {
    fake_env!();
    let machine = Machine::with_global_config("");
    write_file(
        &machine.pnpm_home.path().join("pnpm-workspace.yaml"),
        "globalPackagesConfig:\n  trustPolicyExclude:\n    - from-a-project\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(config.trust_policy_exclude, None);
    assert_eq!(config.workspace_key_issues.refused, ["globalPackagesConfig"]);
}

#[test]
fn unsupported_global_packages_settings_are_reported() {
    fake_env!();
    let machine = Machine::with_global_config(
        "globalPackagesConfig:\n  overrides:\n    foo: 1.0.0\n  trustPolicyExclude:\n    - bar\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(
        config.npmrc_warnings,
        ["These globalPackagesConfig settings are not supported and are ignored: overrides"],
    );
}

#[test]
fn environment_overrides_the_global_packages_build_policy() {
    fake_env!();
    let machine =
        Machine::with_global_config("globalPackagesConfig:\n  dangerouslyAllowAllBuilds: true\n");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", machine.xdg_config_home()),
        ("PNPM_CONFIG_DANGEROUSLY_ALLOW_ALL_BUILDS", "false"),
    ]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert!(!config.dangerously_allow_all_builds);
}

/// The workspace layer of a global command may be a repository's, so its
/// build policy does not reach the global packages.
#[test]
fn a_global_packages_load_drops_the_build_policy_of_the_workspace_layer() {
    fake_env!();
    let machine = Machine::with_global_config("");
    write_file(
        &machine.pnpm_home.path().join("pnpm-workspace.yaml"),
        "dangerouslyAllowAllBuilds: true\nallowBuilds:\n  esbuild: true\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert!(!config.dangerously_allow_all_builds);
    assert!(config.allow_builds.is_empty());
}

#[test]
fn a_global_packages_load_drops_the_skill_policy_of_the_workspace_layer() {
    fake_env!();
    let machine = Machine::with_global_config("");
    write_file(
        &machine.pnpm_home.path().join("pnpm-workspace.yaml"),
        "permissions:\n  some-skills: {skills: true}\nskills:\n  dirs:\n    - .claude/skills\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert!(config.allow_skills.is_empty());
    assert_eq!(config.skills_dirs, None);
}

#[test]
fn a_global_packages_load_keeps_the_top_level_build_policy() {
    fake_env!();
    let machine = Machine::with_global_config("dangerouslyAllowAllBuilds: true\n");
    write_file(
        &machine.pnpm_home.path().join("pnpm-workspace.yaml"),
        "dangerouslyAllowAllBuilds: false\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert!(config.dangerously_allow_all_builds);
    assert_eq!(config.explicit_settings.get("dangerouslyAllowAllBuilds"), Some(&true.into()));
}

#[test]
fn a_global_packages_load_ignores_workspace_env_vars_and_parent_workspaces() {
    fake_env!();
    let outer = tempdir().expect("outer tempdir");
    write_file(&outer.path().join("pnpm-workspace.yaml"), "packages: [\n");
    let nested_home = outer.path().join("nested-pnpm-home");
    fs::create_dir_all(&nested_home).expect("create nested pnpm home");

    let env_workspace = tempdir().expect("env workspace tempdir");
    write_file(
        &env_workspace.path().join("pnpm-workspace.yaml"),
        "pnpmfile: ./evil.cjs\nregistry: http://127.0.0.1:1/\ntrustPolicyExclude:\n  - evil@1.0.0\n",
    );

    let machine = Machine::with_global_config("");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", machine.xdg_config_home()),
        (
            "PNPM_CONFIG_WORKSPACE_DIR",
            env_workspace
                .path()
                .to_str()
                .expect("UTF-8 path"),
        ),
    ]);

    let project = Config::default().current::<FakeEnv>(&nested_home).expect("load project config");
    assert_eq!(project.workspace_dir.as_deref(), Some(env_workspace.path()));
    assert_eq!(project.registry, "http://127.0.0.1:1/");

    let config = global_packages_load().current::<FakeEnv>(&nested_home).expect("load config");

    assert_eq!(config.workspace_dir, None);
    assert_eq!(config.pnpmfile, None);
    assert_eq!(config.registry, NPM_DEFAULT_REGISTRY);
    assert_eq!(config.trust_policy_exclude, None);
}

#[test]
fn global_packages_auth_applies_to_a_global_packages_load() {
    fake_env!();
    let machine = Machine::with_global_config(
        "_auth:\n  \"https://shared.example.com\":\n    \"@\":\n      authToken: top-level-token\n\
         globalPackagesConfig:\n  _auth:\n    \"https://shared.example.com\":\n      \"@\":\n        authToken: global-packages-token\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let global = global_packages_load()
        .current::<FakeEnv>(machine.pnpm_home.path())
        .expect("load global packages config");
    let project =
        Config::default().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    let header = |config: &Config| config.auth_headers.for_url("https://shared.example.com/pkg");
    assert_eq!(header(&global).as_deref(), Some("Bearer global-packages-token"));
    assert_eq!(header(&project).as_deref(), Some("Bearer top-level-token"));
}

#[test]
fn global_packages_npmrc_auth_file_applies_to_a_global_packages_load() {
    fake_env!();
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("global-npmrc");
    write_file(&auth_file, "//registry.example.com/:_authToken=global-packages-token\n");
    let machine = Machine::with_global_config(&format!(
        "globalPackagesConfig:\n  npmrcAuthFile: {}\n",
        auth_file.to_str().expect("UTF-8 path"),
    ));
    set_fake_env(&[("XDG_CONFIG_HOME", machine.xdg_config_home())]);

    let global = global_packages_load()
        .current::<FakeEnv>(machine.pnpm_home.path())
        .expect("load global packages config");
    let project =
        Config::default().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    let header = |config: &Config| config.auth_headers.for_url("https://registry.example.com/pkg");
    assert_eq!(header(&global).as_deref(), Some("Bearer global-packages-token"));
    assert_eq!(header(&project), None);
}
