use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, OsString, Path, PathBuf,
    TrustPolicy, assert_eq, fs, io, tempdir, write_file,
};
use crate::{CliLayer, CommandTarget};

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

/// `PNPM_CONFIG_WORKSPACE_DIR` cannot add a project build policy to a global
/// command.
#[test]
fn global_packages_ignore_the_workspace_dir_build_policy() {
    fake_env!();
    let machine = Machine::with_global_config("");
    let workspace = tempdir().expect("workspace tempdir");
    write_file(
        &workspace.path().join("pnpm-workspace.yaml"),
        "dangerouslyAllowAllBuilds: true\nallowBuilds:\n  esbuild: true\n",
    );
    set_fake_env(&[
        ("XDG_CONFIG_HOME", machine.xdg_config_home()),
        ("PNPM_CONFIG_WORKSPACE_DIR", workspace.path().to_str().unwrap()),
    ]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert!(!config.dangerously_allow_all_builds);
    assert!(config.allow_builds.is_empty());
}

#[test]
fn global_packages_keep_the_pnpm_home_build_policy_during_deprecation() {
    fake_env!();
    let machine = Machine::with_global_config("");
    write_file(
        &machine.pnpm_home.path().join("pnpm-workspace.yaml"),
        "allowBuilds:\n  esbuild: true\n",
    );
    set_fake_env(&[
        (
            "PNPM_HOME",
            machine.pnpm_home
                .path()
                .to_str()
                .unwrap(),
        ),
        ("XDG_CONFIG_HOME", machine.xdg_config_home()),
    ]);

    let config =
        global_packages_load().current::<FakeEnv>(machine.pnpm_home.path()).expect("load config");

    assert_eq!(config.allow_builds.get("esbuild"), Some(&true));
    assert!(
        config.npmrc_warnings
            .iter()
            .any(|warning| {
                warning.contains("allowBuilds") && warning.contains("globalPackagesConfig")
            })
    );
}
