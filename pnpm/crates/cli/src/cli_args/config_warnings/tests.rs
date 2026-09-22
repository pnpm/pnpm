use super::{
    shared_workspace_lockfile_outside_workspace_warning,
    unapplied_package_configs_warning,
    unmatched_registry_options_warning,
};
use indexmap::IndexMap;
use pnpm_config::{
    Config,
    ProjectConfig,
};
use pnpm_lockfile::{
    RegistryOptions,
    RegistryServerType,
};
use pretty_assertions::assert_eq;

fn config_with(registries: &[(&str, &str)], registry_options_by_url: &[&str]) -> Config {
    let mut config = Config::new();
    config.registries_by_scope = registries
        .iter()
        .map(|(scope, url)| ((*scope).to_string(), (*url).to_string()))
        .collect();
    config.registry_options_by_url = registry_options_by_url
        .iter()
        .map(|registry| {
            (
                (*registry).to_string(),
                RegistryOptions {
                    server_type: Some(RegistryServerType::Artifactory),
                    supports_time_field: None,
                },
            )
        })
        .collect();
    config
}

#[test]
fn no_warning_when_every_entry_matches_a_configured_registry() {
    let config = config_with(
        &[("default", "https://npm.example.com/"), ("@acme", "https://acme.example.com/")],
        &["https://npm.example.com/", "https://acme.example.com/"],
    );
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

#[test]
fn no_warning_without_any_registry_options() {
    let config = config_with(&[("default", "https://npm.example.com/")], &[]);
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

/// A built-in named registry is a legitimate target even though the user never
/// declared it, so an entry for it must not be reported as unmatched.
#[test]
fn no_warning_for_a_builtin_named_registry() {
    let config =
        config_with(&[("default", "https://npm.example.com/")], &["https://npm.pkg.github.com/"]);
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

#[test]
fn warns_about_an_entry_matching_no_configured_registry() {
    let config = config_with(
        &[("default", "https://npm.example.com/")],
        &["https://npm.example.com/", "https://typo.example.com/"],
    );
    let received = unmatched_registry_options_warning(&config).expect("a warning");
    assert!(
        received.contains(r#"were ignored: "https://typo.example.com/"."#),
        "the unmatched entry must be named: {received}",
    );
    assert!(
        received.contains(r#""https://npm.example.com/""#),
        "the configured registries must be listed: {received}",
    );
}

/// A registry URL can carry `user:pass@` credentials, and this warning names
/// every configured registry, so it must not echo one into a CI log.
#[test]
fn redacts_credentials_in_the_warning() {
    let config = config_with(
        &[("default", "https://ci-user-6e42:hunter2@npm.example.com/")],
        &["https://typo.example.com/"],
    );
    let received = unmatched_registry_options_warning(&config).expect("a warning");
    assert!(!received.contains("hunter2"), "the password must not be echoed: {received}");
    assert!(!received.contains("ci-user-6e42"), "the username must not be echoed: {received}");
    assert!(received.contains("npm.example.com"), "the host is still named: {received}");
}

mod workspace_key_issues {
    use super::super::{
        non_camel_case_workspace_keys_warning,
        refused_workspace_keys_warning,
        report_workspace_key_issues,
    };
    use pnpm_config::{
        UnrecognizedTaskSettings,
        WorkspaceKeyIssues,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn refused_keys_name_where_each_belongs() {
        assert_eq!(
            refused_workspace_keys_warning(&["configDir".to_string(), "bin".to_string()]),
            "The following settings cannot be set in a project's pnpm-workspace.yaml and were \
             ignored: \"configDir\" (This is not a pnpm setting), \"bin\" (Set it for the machine \
             instead: pnpm config set --global global-bin-dir).",
        );
    }

    #[test]
    fn non_camel_case_keys_name_the_spelling_that_works() {
        assert_eq!(
            non_camel_case_workspace_keys_warning(&["store-dir".to_string()]),
            "The following settings in pnpm-workspace.yaml were ignored because they are not \
             written in camelCase: \"store-dir\" (use \"storeDir\").",
        );
    }

    #[test]
    fn unrecognized_keys_error_only_when_strict() {
        let issues = WorkspaceKeyIssues {
            unrecognized: vec!["minimumReleaseAg".to_string()],
            ..WorkspaceKeyIssues::default()
        };
        report_workspace_key_issues(&issues, false).expect("a warning, not an error");
        let error =
            report_workspace_key_issues(&issues, true).expect_err("strict must fail").to_string();
        assert_eq!(
            error,
            "The following settings in pnpm-workspace.yaml are not recognized by this version of \
             pnpm: \"minimumReleaseAg\" (did you mean \"minimumReleaseAge\"?).",
        );
    }

    #[test]
    fn unrecognized_task_settings_error_only_when_strict() {
        let issues = WorkspaceKeyIssues {
            unrecognized_task_settings: UnrecognizedTaskSettings {
                named: vec!["tasks['build'].dependson".to_string()],
                total: 1,
            },
            ..WorkspaceKeyIssues::default()
        };
        report_workspace_key_issues(&issues, false).expect("a warning, not an error");
        let error =
            report_workspace_key_issues(&issues, true).expect_err("strict must fail").to_string();
        assert_eq!(
            error,
            "The following task settings in pnpm-workspace.yaml are not recognized by this \
             version of pnpm: \"tasks['build'].dependson\".",
        );
    }

    /// A file may name more unrecognized task settings than a message can
    /// usefully carry, and the count is what tells the reader so.
    #[test]
    fn a_report_says_how_many_task_settings_it_did_not_name() {
        let issues = WorkspaceKeyIssues {
            unrecognized_task_settings: UnrecognizedTaskSettings {
                named: vec!["tasks['build'].one".to_string()],
                total: 4,
            },
            ..WorkspaceKeyIssues::default()
        };
        let error =
            report_workspace_key_issues(&issues, true).expect_err("strict must fail").to_string();
        assert_eq!(
            error,
            "The following task settings in pnpm-workspace.yaml are not recognized by this \
             version of pnpm: \"tasks['build'].one\", and 3 more.",
        );
    }

    /// The error is the top-level key's, not the task settings'. That the
    /// task settings still reach the user is
    /// `a_task_setting_is_reported_when_an_unrecognized_key_takes_the_error`
    /// in the end-to-end suite, which can read what was written to stderr.
    #[test]
    fn an_unrecognized_key_takes_the_error_over_task_settings() {
        let issues = WorkspaceKeyIssues {
            unrecognized: vec!["minimumReleaseAg".to_string()],
            unrecognized_task_settings: UnrecognizedTaskSettings {
                named: vec!["tasks['build'].dependson".to_string()],
                total: 1,
            },
            ..WorkspaceKeyIssues::default()
        };
        let error =
            report_workspace_key_issues(&issues, true).expect_err("strict must fail").to_string();
        assert!(error.contains("minimumReleaseAg"), "unexpected error: {error}");
    }

    #[test]
    fn a_clean_file_reports_nothing_even_when_strict() {
        report_workspace_key_issues(&WorkspaceKeyIssues::default(), true)
            .expect("nothing to report");
    }
}

fn config_with_package_configs(shared_workspace_lockfile: bool) -> Config {
    let mut config = Config::new();
    config.shared_workspace_lockfile = shared_workspace_lockfile;
    config.package_configs = Some(IndexMap::from([(
        "a".to_string(),
        ProjectConfig {
            overrides: Some(IndexMap::from([("ms".to_string(), "2.0.0".to_string())])),
            save_exact: Some(true),
            ..ProjectConfig::default()
        },
    )]));
    config
}

#[test]
fn no_warning_when_each_project_has_its_own_lockfile() {
    assert_eq!(unapplied_package_configs_warning(&config_with_package_configs(false)), None);
}

#[test]
fn no_warning_without_any_package_configs() {
    let mut config = Config::new();
    config.shared_workspace_lockfile = true;
    assert_eq!(unapplied_package_configs_warning(&config), None);
}

#[test]
fn warns_about_every_setting_a_shared_lockfile_ignores() {
    let received =
        unapplied_package_configs_warning(&config_with_package_configs(true)).expect("a warning");
    println!("{received}");
    assert_eq!(
        received,
        r#"The following "packageConfigs" settings were ignored: "a.overrides", "a.saveExact". They apply only when each project has its own lockfile ("sharedWorkspaceLockfile: false")."#,
    );
}

/// Project names reach the warning from `pnpm-workspace.yaml`, so a name
/// carrying terminal control characters must not reach the terminal.
#[test]
fn sanitizes_the_project_name() {
    let mut config = Config::new();
    config.shared_workspace_lockfile = true;
    config.package_configs = Some(IndexMap::from([(
        "a\u{1b}[2Kb".to_string(),
        ProjectConfig { save_exact: Some(true), ..ProjectConfig::default() },
    )]));
    let received = unapplied_package_configs_warning(&config).expect("a warning");
    println!("{received}");
    assert!(!received.contains('\u{1b}'), "{received}");
}

#[test]
fn warns_when_shared_workspace_lockfile_is_set_outside_a_workspace() {
    let received =
        shared_workspace_lockfile_outside_workspace_warning(Some(true), None).expect("a warning");
    assert_eq!(
        received,
        r#"The "shared-workspace-lockfile" option was ignored because no "pnpm-workspace.yaml" was found."#,
    );
}

#[test]
fn no_warning_when_in_workspace_or_not_set_on_cli() {
    assert_eq!(shared_workspace_lockfile_outside_workspace_warning(None, None), None);
    assert_eq!(
        shared_workspace_lockfile_outside_workspace_warning(
            Some(true),
            Some(std::path::Path::new("/workspace")),
        ),
        None,
    );
}
