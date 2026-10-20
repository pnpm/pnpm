use super::{Config, HostNoHome, LoadWorkspaceYamlError, TrustPolicy, assert_eq, fs, tempdir};
use std::path::Path;

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
        fs::write(path, text).expect("write file");
    }
}

fn load(files: &[(&str, &str)]) -> Result<Config, LoadWorkspaceYamlError> {
    let tmp = tempdir().expect("workspace tempdir");
    write(tmp.path(), files);
    Config::new().current::<HostNoHome>(tmp.path())
}

const POLICY_ROOT: &str =
    "configDependencies:\n  policy: 1.0.0\ninclude:\n  policy: [minimumReleaseAge, trustPolicy]\n";

fn installed_policy(config: &str) -> [(&'static str, String); 2] {
    [
        (
            "node_modules/.pnpm-config/policy/package.json",
            r#"{"name":"policy","pnpm":{"config":"config.yaml"}}"#.to_string(),
        ),
        ("node_modules/.pnpm-config/policy/config.yaml", config.to_string()),
    ]
}

fn load_with_policy(root: &str, policy: &str, extra: &[(&str, &str)]) -> Config {
    let policy = installed_policy(policy);
    let mut files: Vec<(&str, &str)> = vec![("pnpm-workspace.yaml", root)];
    files.extend(
        policy
            .iter()
            .map(|(name, text)| (*name, text.as_str())),
    );
    files.extend_from_slice(extra);
    load(&files).expect("config loads")
}

#[test]
fn settings_from_included_files_apply() {
    let config = load(&[
        (
            "pnpm-workspace.yaml",
            "include:\n  ./config/security.yaml: '*'\n  ./config/deploy.yml: [nodeLinker]\n",
        ),
        ("config/security.yaml", "minimumReleaseAge: 10080\ntrustPolicy: no-downgrade\n"),
        ("config/deploy.yml", "nodeLinker: hoisted\n"),
    ])
    .expect("config loads");
    assert_eq!(config.minimum_release_age, Some(10080));
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.node_linker, super::NodeLinker::Hoisted);
    assert!(config.workspace_includes.is_some());
}

#[test]
fn an_unknown_delegated_setting_is_an_error() {
    let error = load(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [minimumReleaseAgee]\n"),
        ("a.yaml", ""),
    ])
    .expect_err("unknown setting");
    assert!(
        matches!(&error, LoadWorkspaceYamlError::UnknownDelegatedSetting { setting, .. } if setting == "minimumReleaseAgee"),
        "{error:?}",
    );
}

#[test]
fn an_invalid_included_file_names_that_file() {
    let error = load(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: '*'\n"),
        ("a.yaml", "minimumReleaseAge: soon\n"),
    ])
    .expect_err("invalid value");
    let LoadWorkspaceYamlError::ParseYaml { path, .. } = &error else {
        panic!("unexpected error: {error:?}");
    };
    assert!(path.ends_with("a.yaml"), "{path:?}");
}

#[test]
fn an_ownership_conflict_fails_config_loading() {
    let error = load(&[
        ("pnpm-workspace.yaml", "include:\n  ./a.yaml: [nodeLinker]\nnodeLinker: hoisted\n"),
        ("a.yaml", ""),
    ])
    .expect_err("conflict");
    assert!(matches!(error, LoadWorkspaceYamlError::Include(_)), "{error:?}");
}

#[test]
fn an_omitted_delegated_setting_keeps_its_inherited_value() {
    let config = load(&[
        ("pnpm-workspace.yaml", "include:\n  ./security.yaml: [minimumReleaseAge]\n"),
        ("security.yaml", ""),
    ])
    .expect("config loads");
    assert_eq!(config.minimum_release_age, Config::default().minimum_release_age);
}

#[test]
fn null_in_an_included_file_keeps_its_meaning() {
    let config = load(&[
        ("pnpm-workspace.yaml", "include:\n  ./layout.yaml: '*'\n"),
        ("layout.yaml", "hoistPattern: null\nminimumReleaseAge: null\n"),
    ])
    .expect("config loads");
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.minimum_release_age, Config::default().minimum_release_age);
}

#[test]
fn an_installed_config_dependency_supplies_its_settings() {
    let config =
        load_with_policy(POLICY_ROOT, "minimumReleaseAge: 1\ntrustPolicy: no-downgrade\n", &[]);
    assert_eq!(config.minimum_release_age, Some(1));
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
}

#[test]
fn a_config_dependency_not_installed_contributes_nothing() {
    let config = load(&[("pnpm-workspace.yaml", POLICY_ROOT)]).expect("config loads");
    assert_eq!(config.minimum_release_age, Config::default().minimum_release_age);
    let includes = config.workspace_includes.expect("has includes");
    assert_eq!(includes.unavailable_packages().collect::<Vec<_>>(), ["policy"]);
}

#[test]
fn boot_phase_ignores_the_policy_of_a_config_dependency() {
    let config =
        load_with_policy(POLICY_ROOT, "minimumReleaseAge: 1\ntrustPolicy: no-downgrade\n", &[]);
    let boot = config.for_boot();
    assert_eq!(boot.minimum_release_age, Config::default().minimum_release_age);
    assert_eq!(boot.trust_policy, Config::default().trust_policy);
}

#[test]
fn boot_phase_unsets_a_policy_setting_only_a_config_dependency_sets() {
    let root = "configDependencies:\n  policy: 1.0.0\ninclude:\n  policy: [trustPolicyExclude]\n";
    let config = load_with_policy(root, "trustPolicyExclude: ['*']\n", &[]);
    assert_eq!(config.trust_policy_exclude, Some(vec!["*".to_string()]));
    assert_eq!(config.for_boot().trust_policy_exclude, None);
}

#[test]
fn boot_config_governs_the_boot_phase_only() {
    let root = format!(
        "{POLICY_ROOT}bootConfig:\n  minimumReleaseAge: 0\n  trustPolicyExclude: ['policy']\n",
    );
    let config = load_with_policy(&root, "minimumReleaseAge: 1\n", &[]);
    assert_eq!(config.minimum_release_age, Some(1));
    let boot = config.for_boot();
    assert_eq!(boot.minimum_release_age, Some(0));
    assert_eq!(boot.trust_policy_exclude, Some(vec!["policy".to_string()]));
}

#[test]
fn boot_phase_keeps_repository_settings_without_boot_config() {
    let config = load(&[("pnpm-workspace.yaml", "minimumReleaseAge: 10080\n")]).expect("loads");
    assert_eq!(config.for_boot().minimum_release_age, Some(10080));
}

#[test]
fn null_in_boot_config_falls_through() {
    let config = load(&[(
        "pnpm-workspace.yaml",
        "minimumReleaseAge: 10080\nbootConfig:\n  minimumReleaseAge: null\n",
    )])
    .expect("loads");
    assert_eq!(config.for_boot().minimum_release_age, Some(10080));
}

#[test]
fn boot_config_rejects_other_settings() {
    let error = load(&[("pnpm-workspace.yaml", "bootConfig:\n  nodeLinker: hoisted\n")])
        .expect_err("unknown boot setting");
    assert!(matches!(error, LoadWorkspaceYamlError::ParseYaml { .. }), "{error:?}");
}

#[test]
fn command_line_settings_win_over_boot_config() {
    let tmp = tempdir().expect("workspace tempdir");
    write(tmp.path(), &[("pnpm-workspace.yaml", "bootConfig:\n  minimumReleaseAge: 0\n")]);
    let config = Config {
        cli: crate::CliLayer {
            setting_values: [("minimumReleaseAge".to_string(), "10080".to_string())].into(),
            ..crate::CliLayer::default()
        },
        ..Config::default()
    }
    .current::<HostNoHome>(tmp.path())
    .expect("config loads");
    assert_eq!(config.for_boot().minimum_release_age, Some(10080));
}

#[test]
fn self_update_ignores_boot_config() {
    let tmp = tempdir().expect("workspace tempdir");
    write(tmp.path(), &[("pnpm-workspace.yaml", "bootConfig:\n  minimumReleaseAge: 99999\n")]);
    let config =
        Config::new().current_for_self_update::<HostNoHome>(tmp.path()).expect("config loads");
    assert_eq!(config.for_boot().minimum_release_age, Config::default().minimum_release_age);
}
