use crate::{
    cli_args::{
        dispatch::{apply_cli_layers, seed_config},
        package_manager::{PinRoots, wanted_package_manager},
        pre_command::{
            ConfigLoad, EntryPoint, Invocation, PinDecision, PinInput, PreCommandPlan,
            SwitchProcessState, SwitchSource, decide,
            load_config::tests::with_test_global_config_dir, load_pre_command_config,
            pre_command_plan_with_state,
        },
        verify_deps::install_config_args,
    },
    config_overrides::ConfigOverrides,
};
use pnpm_config::{Config, Host, PNPM_VERSION};
use pnpm_lockfile::EnvLockfile;
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    cases: Vec<MatrixCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatrixCase {
    id: String,
    pin_source: String,
    pin_version: String,
    setting_location: String,
    readability: String,
    #[serde(default)]
    settings: CaseSettings,
    entry_point: String,
    expected: ExpectedOutcome,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaseSettings {
    pm_on_fail: Option<String>,
    lockfile_dir: Option<String>,
    lockfile: Option<bool>,
    global_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedOutcome {
    outcome: String,
    version: Option<String>,
    at: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ActualOutcome {
    Switch { version: String },
    Record { at: String },
    Nothing,
    Error,
}

#[test]
fn pin_decision_matrix() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../../../../../fixtures/pin-decision.json"))
            .expect("parse shared pin-decision fixture");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let actual = run_matrix_case(case);
        let expected = match case.expected.outcome.as_str() {
            "switch" => ActualOutcome::Switch {
                version: case.expected.version.clone().expect("switch version"),
            },
            "record" => {
                ActualOutcome::Record { at: case.expected.at.clone().expect("record path") }
            }
            "nothing" => ActualOutcome::Nothing,
            "error" => ActualOutcome::Error,
            other => panic!("unknown expected outcome {other} in case {}", case.id),
        };
        assert_eq!(actual, expected, "case `{}` failed", case.id);
    }
}

fn run_matrix_case(case: &MatrixCase) -> ActualOutcome {
    let temp = TempDir::new().expect("temp dir");
    let workspace_root = temp.path().join("workspace");
    let global_config_dir = temp.path().join("global-config");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    fs::create_dir_all(&global_config_dir).expect("create global config dir");
    let workspace_root = dunce::canonicalize(&workspace_root).expect("canonicalize workspace root");

    let version = if case.pin_version == "running" {
        PNPM_VERSION.to_string()
    } else {
        case.pin_version.clone()
    };

    let run_dir = setup_pin_source(&workspace_root, &case.pin_source, &version);
    let cli_flags = setup_settings(
        &workspace_root,
        &global_config_dir,
        &case.pin_source,
        &case.setting_location,
        &case.readability,
        &case.settings,
    );

    with_test_global_config_dir(Some(global_config_dir), || {
        execute_entry_point(&workspace_root, &run_dir, &case.entry_point, &cli_flags)
    })
}

fn setup_pin_source(workspace_root: &Path, pin_source: &str, version: &str) -> PathBuf {
    match pin_source {
        "packageManager" => {
            fs::write(
                workspace_root.join("package.json"),
                serde_json::to_string(&json!({
                    "name": "root",
                    "packageManager": format!("pnpm@{version}"),
                }))
                .expect("serialize package.json"),
            )
            .expect("write package.json");
            workspace_root.to_path_buf()
        }
        "devEngines" => {
            fs::write(
                workspace_root.join("package.json"),
                serde_json::to_string(&json!({
                    "name": "root",
                    "devEngines": {
                        "packageManager": {
                            "name": "pnpm",
                            "version": version,
                        }
                    }
                }))
                .expect("serialize package.json"),
            )
            .expect("write package.json");
            workspace_root.to_path_buf()
        }
        "workspace-package-yaml" => {
            fs::write(
                workspace_root.join("package.yaml"),
                format!(
                    "name: root\ndevEngines:\n  packageManager:\n    name: pnpm\n    version: '{version}'\n",
                ),
            )
            .expect("write package.yaml");
            workspace_root.to_path_buf()
        }
        "nested-project" => {
            fs::write(
                workspace_root.join("package.json"),
                serde_json::to_string(&json!({
                    "name": "root",
                    "devEngines": {
                        "packageManager": {
                            "name": "pnpm",
                            "version": version,
                        }
                    }
                }))
                .expect("serialize root package.json"),
            )
            .expect("write root package.json");
            let app_dir = workspace_root.join("packages/app");
            fs::create_dir_all(&app_dir).expect("create nested project");
            fs::write(app_dir.join("package.json"), r#"{"name":"app"}"#)
                .expect("write nested package.json");
            dunce::canonicalize(&app_dir).expect("canonicalize nested project")
        }
        other => panic!("unknown pinSource: {other}"),
    }
}

fn setup_settings(
    workspace_root: &Path,
    global_config_dir: &Path,
    pin_source: &str,
    setting_location: &str,
    readability: &str,
    settings: &CaseSettings,
) -> Vec<String> {
    let mut workspace_yaml = default_workspace_yaml(pin_source);
    let mut cli_flags = Vec::new();
    let unreadable = readability == "unreadable";

    match setting_location {
        "global-config-yaml" => write_global_config_yaml(global_config_dir, settings, unreadable),
        "pnpm-workspace-yaml" => {
            append_workspace_yaml_settings(&mut workspace_yaml, settings, unreadable);
        }
        "cli-config" => {
            append_cli_config_settings(&mut cli_flags, &mut workspace_yaml, settings, unreadable);
        }
        "npmrc" => write_npmrc_settings(workspace_root, settings, unreadable),
        other => panic!("unknown settingLocation: {other}"),
    }

    fs::write(workspace_root.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    cli_flags
}

fn default_workspace_yaml(pin_source: &str) -> String {
    if matches!(pin_source, "workspace-package-yaml" | "nested-project") {
        "packages:\n  - packages/*\n".to_string()
    } else {
        "packages:\n  - .\n".to_string()
    }
}

fn write_global_config_yaml(global_config_dir: &Path, settings: &CaseSettings, unreadable: bool) {
    let mut content = format_yaml_settings(settings);
    if unreadable {
        content.push_str("remoteSideEffectsCache: 123\n");
    }
    fs::write(global_config_dir.join("config.yaml"), content).expect("write global config.yaml");
}

fn append_workspace_yaml_settings(
    workspace_yaml: &mut String,
    settings: &CaseSettings,
    unreadable: bool,
) {
    workspace_yaml.push_str(&format_yaml_settings(settings));
    if unreadable {
        workspace_yaml.push_str("remoteSideEffectsCache: 123\n");
    }
}

fn append_cli_config_settings(
    cli_flags: &mut Vec<String>,
    workspace_yaml: &mut String,
    settings: &CaseSettings,
    unreadable: bool,
) {
    if let Some(pm_on_fail) = &settings.pm_on_fail {
        cli_flags.push(format!("--config.pm-on-fail={pm_on_fail}"));
    }
    if let Some(lockfile_dir) = &settings.lockfile_dir {
        cli_flags.push(format!("--config.lockfile-dir={lockfile_dir}"));
    }
    if let Some(lockfile) = settings.lockfile {
        cli_flags.push(format!("--config.lockfile={lockfile}"));
    }
    if let Some(global_dir) = &settings.global_dir {
        cli_flags.push(format!("--config.global-dir={global_dir}"));
    }
    if unreadable {
        workspace_yaml.push_str("remoteSideEffectsCache: 123\n");
    }
}

fn write_npmrc_settings(workspace_root: &Path, settings: &CaseSettings, unreadable: bool) {
    let mut npmrc = String::new();
    if let Some(pm_on_fail) = &settings.pm_on_fail {
        let _ = writeln!(npmrc, "pm-on-fail={pm_on_fail}");
    }
    if let Some(lockfile_dir) = &settings.lockfile_dir {
        let _ = writeln!(npmrc, "lockfile-dir={lockfile_dir}");
    }
    if unreadable {
        npmrc.push_str("//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n");
    }
    fs::write(workspace_root.join(".npmrc"), npmrc).expect("write .npmrc");
}

fn format_yaml_settings(settings: &CaseSettings) -> String {
    let mut out = String::new();
    if let Some(pm_on_fail) = &settings.pm_on_fail {
        let _ = writeln!(out, "pmOnFail: {pm_on_fail}");
    }
    if let Some(lockfile_dir) = &settings.lockfile_dir {
        let _ = writeln!(out, "lockfileDir: {lockfile_dir}");
    }
    if let Some(lockfile) = settings.lockfile {
        let _ = writeln!(out, "lockfile: {lockfile}");
    }
    if let Some(global_dir) = &settings.global_dir {
        let _ = writeln!(out, "globalDir: {global_dir}");
    }
    out
}

fn execute_entry_point(
    workspace_root: &Path,
    run_dir: &Path,
    entry_point: &str,
    cli_flags: &[String],
) -> ActualOutcome {
    let mut raw_argv = vec![
        OsString::from("pnpm"),
        OsString::from("--dir"),
        run_dir.as_os_str().to_os_string(),
        OsString::from("--reporter=silent"),
    ];
    raw_argv.extend(cli_flags.iter().map(OsString::from));

    match entry_point {
        "command" => execute_command_entry_point(workspace_root, run_dir, raw_argv),
        "--version" => execute_version_entry_point(workspace_root, raw_argv),
        "unknown-flag" => execute_unknown_flag_entry_point(raw_argv),
        "self-update" => execute_self_update_entry_point(workspace_root, run_dir, raw_argv),
        other => panic!("unknown entryPoint: {other}"),
    }
}

fn execute_command_entry_point(
    workspace_root: &Path,
    run_dir: &Path,
    mut raw_argv: Vec<OsString>,
) -> ActualOutcome {
    raw_argv.push(OsString::from("root"));
    let (config_overrides, remaining) = ConfigOverrides::extract(raw_argv);
    let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);
    let Ok(outcome) = pre_command_plan_with_state(
        &invocation,
        &config_overrides,
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    ) else {
        return ActualOutcome::Error;
    };
    match outcome.plan {
        Some(PreCommandPlan::Switch(plan)) => ActualOutcome::Switch { version: plan.target.spec },
        Some(PreCommandPlan::SyncEnvLockfile(sync)) => {
            ActualOutcome::Record { at: relative_lockfile_dir(workspace_root, &sync.env_root) }
        }
        None => check_command_full_config(
            &invocation,
            &config_overrides,
            run_dir,
            outcome.preloaded_config,
        ),
    }
}

fn check_command_full_config(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    run_dir: &Path,
    preloaded_config: Option<Result<Config, pnpm_config::ConfigLoadFailure>>,
) -> ActualOutcome {
    let full_config = match preloaded_config {
        Some(preloaded) => preloaded,
        None => seed_config(
            invocation.paths.npmrc_auth_file.as_deref(),
            invocation.paths.ignore_workspace,
            config_overrides,
            invocation.network.registry.as_deref(),
        )
        .current_keeping_warnings::<Host>(run_dir),
    };
    match full_config {
        Ok(config) if !invocation.warnings.fail(&config, run_dir) => ActualOutcome::Nothing,
        _ => ActualOutcome::Error,
    }
}

fn execute_version_entry_point(
    workspace_root: &Path,
    mut raw_argv: Vec<OsString>,
) -> ActualOutcome {
    raw_argv.push(OsString::from("--version"));
    let (config_overrides, remaining) = ConfigOverrides::extract(raw_argv);
    let invocation = Invocation::from_argv(&remaining, EntryPoint::Version);
    let Ok(outcome) = pre_command_plan_with_state(
        &invocation,
        &config_overrides,
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    ) else {
        return ActualOutcome::Error;
    };
    match outcome.plan {
        Some(PreCommandPlan::Switch(plan)) => ActualOutcome::Switch { version: plan.target.spec },
        Some(PreCommandPlan::SyncEnvLockfile(sync)) => {
            ActualOutcome::Record { at: relative_lockfile_dir(workspace_root, &sync.env_root) }
        }
        None => ActualOutcome::Nothing,
    }
}

fn execute_unknown_flag_entry_point(mut raw_argv: Vec<OsString>) -> ActualOutcome {
    raw_argv.push(OsString::from("root"));
    raw_argv.push(OsString::from("--definitely-unknown-flag"));
    let (config_overrides, remaining) = ConfigOverrides::extract(raw_argv);
    let invocation = Invocation::from_argv(&remaining, EntryPoint::UnknownFlag);
    let Ok(outcome) = pre_command_plan_with_state(
        &invocation,
        &config_overrides,
        SwitchProcessState { package_manager_switch_disabled: false, executed_by_corepack: false },
    ) else {
        return ActualOutcome::Error;
    };
    match outcome.plan {
        Some(PreCommandPlan::Switch(plan)) => ActualOutcome::Switch { version: plan.target.spec },
        _ => ActualOutcome::Error,
    }
}

fn execute_self_update_entry_point(
    workspace_root: &Path,
    run_dir: &Path,
    mut raw_argv: Vec<OsString>,
) -> ActualOutcome {
    raw_argv.push(OsString::from("self-update"));
    raw_argv.push(OsString::from("99.0.0"));
    let (config_overrides, remaining) = ConfigOverrides::extract(raw_argv);
    let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);
    let Some(config) = load_self_update_matrix_config(&invocation, &config_overrides, run_dir)
    else {
        return ActualOutcome::Error;
    };
    let roots = PinRoots::of(&config, run_dir);
    let Some((_, mut manifest)) =
        pnpm_workspace::try_read_project_manifest(&roots.manifest).ok().flatten()
    else {
        return ActualOutcome::Nothing;
    };
    let target_version = "99.0.0";
    rewrite_manifest_pin_for_test(manifest.value_mut(), target_version);
    let wanted = wanted_package_manager(manifest.value());
    let locked_env =
        if config.lockfile { EnvLockfile::read(&roots.env).ok().flatten() } else { None };
    let pin = PinInput {
        roots: &roots,
        manifest: Some(manifest.value()),
        wanted: wanted.as_ref(),
        locked_env,
        running_version: target_version,
        process_state: SwitchProcessState {
            package_manager_switch_disabled: false,
            executed_by_corepack: false,
        },
    };
    let self_update_inv = Invocation::for_self_update(roots.manifest.clone());
    match decide(&pin, &config, &self_update_inv) {
        Ok(PinDecision::Record { at, .. }) => {
            ActualOutcome::Record { at: relative_lockfile_dir(workspace_root, &at) }
        }
        Ok(PinDecision::Nothing) => ActualOutcome::Nothing,
        Ok(PinDecision::Switch(target)) => ActualOutcome::Switch { version: target.spec },
        Err(_) => ActualOutcome::Error,
    }
}

fn load_self_update_matrix_config(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    run_dir: &Path,
) -> Option<Config> {
    let mut config = seed_config(
        invocation.paths.npmrc_auth_file.as_deref(),
        invocation.paths.ignore_workspace,
        config_overrides,
        invocation.network.registry.as_deref(),
    )
    .current_for_self_update::<Host>(run_dir)
    .ok()?;
    apply_cli_layers(&mut config, invocation, config_overrides, run_dir).ok()?;
    (!invocation.warnings.fail(&config, run_dir)).then_some(config)
}

fn rewrite_manifest_pin_for_test(manifest: &mut serde_json::Value, target_version: &str) {
    if let Some(pm_obj) = manifest
        .get_mut("devEngines")
        .and_then(|de| de.get_mut("packageManager"))
        .and_then(|pm| pm.as_object_mut())
    {
        pm_obj.insert("version".to_string(), json!(target_version));
    } else if let Some(obj) = manifest.as_object_mut() {
        obj.insert("packageManager".to_string(), json!(format!("pnpm@{target_version}")));
    }
}

fn relative_lockfile_dir(workspace_root: &Path, env_root: &Path) -> String {
    let rel = env_root.strip_prefix(workspace_root).unwrap_or(env_root);
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    }
}

/// A `--config.<key>`, credentials included, has the same value in the
/// switched pnpm, the `verifyDeps` install, the record config, and the
/// bootstrap download.
#[test]
fn cli_config_reaches_every_child() {
    let temp = TempDir::new().expect("temp dir");
    let root = dunce::canonicalize(temp.path()).expect("canonicalize root");
    let global_config_dir = root.join("global-config");
    fs::create_dir_all(&global_config_dir).expect("create global-config");
    fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let raw_argv = vec![
        OsString::from("pnpm"),
        OsString::from("--dir"),
        root.as_os_str().to_os_string(),
        OsString::from("--config.registry=https://mirror.example.com/"),
        OsString::from("--config.//mirror.example.com/:_authToken=secret-token-123"),
        OsString::from("--config.lockfile-dir=shared-lf"),
        OsString::from("--config.store-dir=custom-store"),
        OsString::from("--config.strict-ssl=false"),
        OsString::from("--config.custom-setting=custom-val"),
        OsString::from("run"),
        OsString::from("build"),
    ];

    let child_argv = &raw_argv[1..];
    for expected_flag in [
        "--config.registry=https://mirror.example.com/",
        "--config.//mirror.example.com/:_authToken=secret-token-123",
        "--config.lockfile-dir=shared-lf",
        "--config.store-dir=custom-store",
        "--config.strict-ssl=false",
        "--config.custom-setting=custom-val",
    ] {
        assert!(
            child_argv.contains(&OsString::from(expected_flag)),
            "switched pnpm child_argv must retain {expected_flag}: {child_argv:?}",
        );
    }

    let (config_overrides, remaining) = ConfigOverrides::extract(raw_argv.clone());
    let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);

    with_test_global_config_dir(Some(global_config_dir), || {
        // 1. Switch / bootstrap download config
        fs::write(
            root.join("package.json"),
            r#"{"name":"root","devEngines":{"packageManager":{"name":"pnpm","version":"9.1.0","onFail":"download"}}}"#,
        )
        .expect("write package.json for switch");
        let switch_outcome = pre_command_plan_with_state(
            &invocation,
            &config_overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("switch outcome");
        let Some(PreCommandPlan::Switch(switch_plan)) = switch_outcome.plan else {
            panic!("expected switch plan");
        };
        assert_config_has_cli_layers(&switch_plan.config, &root, "bootstrap download config");
        let SwitchSource::Resolve { env_root, .. } = &switch_plan.target.source else {
            panic!("expected resolve target");
        };
        assert_eq!(env_root, &root.join("shared-lf"));

        // 2. Record config (when pin matches running version)
        fs::write(
            root.join("package.json"),
            format!(
                r#"{{"name":"root","devEngines":{{"packageManager":{{"name":"pnpm","version":"{PNPM_VERSION}","onFail":"download"}}}}}}"#,
            ),
        )
        .expect("write package.json for record");
        let record_outcome = pre_command_plan_with_state(
            &invocation,
            &config_overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("record outcome");
        let Some(PreCommandPlan::SyncEnvLockfile(sync)) = record_outcome.plan else {
            panic!("expected sync env lockfile plan");
        };
        assert_eq!(sync.env_root, root.join("shared-lf"));
        assert_config_has_cli_layers(&sync.config, &root, "record config");

        // 3. verifyDeps install args constructed from the config
        let verify_deps_args: Vec<String> = install_config_args(&sync.config).collect();
        let expected_lockfile_arg =
            format!("--config.lockfile-dir={}", root.join("shared-lf").display());
        let expected_store_arg =
            format!("--config.store-dir={}", sync.config.store_dir.root().display());
        for expected in [
            "--config.registry=https://mirror.example.com/",
            "--config.//mirror.example.com/:_authToken=secret-token-123",
            expected_lockfile_arg.as_str(),
            expected_store_arg.as_str(),
            "--config.strict-ssl=false",
            "--config.custom-setting=custom-val",
        ] {
            assert!(
                verify_deps_args
                    .iter()
                    .any(|arg| arg == expected),
                "verifyDeps install args must include {expected}: {verify_deps_args:?}",
            );
        }

        // And when the spawned verifyDeps install parses those args, its config has the exact same values
        let mut verify_deps_argv = vec![
            OsString::from("pnpm"),
            OsString::from("install"),
            OsString::from("--verify-deps-before-run-install"),
            OsString::from("--dir"),
            root.as_os_str().to_os_string(),
        ];
        verify_deps_argv.extend(verify_deps_args.into_iter().map(OsString::from));
        let (vd_overrides, vd_remaining) = ConfigOverrides::extract(verify_deps_argv);
        let vd_invocation = Invocation::from_argv(&vd_remaining, EntryPoint::Command);
        let vd_config =
            load_pre_command_config(&vd_invocation, &vd_overrides, &root, ConfigLoad::RECORD)
                .expect("verifyDeps child config");
        assert_config_has_cli_layers(&vd_config, &root, "verifyDeps child config");
    });
}

fn assert_config_has_cli_layers(config: &Config, root: &Path, label: &str) {
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://mirror.example.com/",
        "{label}: bootstrap registry",
    );
    assert_eq!(
        config.package_manager_bootstrap.auth_headers
            .for_url("https://mirror.example.com/pnpm/-/pnpm-9.1.0.tgz")
            .as_deref(),
        Some("Bearer secret-token-123"),
        "{label}: bootstrap auth header",
    );
    assert_eq!(
        config.package_manager_bootstrap.tls.strict_ssl,
        Some(false),
        "{label}: bootstrap strict_ssl",
    );
    assert_eq!(config.tls.strict_ssl, Some(false), "{label}: strict_ssl");
    assert_eq!(
        config.lockfile_dir.as_deref(),
        Some(root.join("shared-lf").as_path()),
        "{label}: lockfile_dir",
    );
    assert!(
        config.store_dir
            .root()
            .starts_with(root.join("custom-store")),
        "{label}: store_dir ({})",
        config.store_dir.root().display(),
    );
    assert_eq!(
        config.explicit_settings.get("strictSsl"),
        Some(&json!(false)),
        "{label}: strictSsl in explicit_settings",
    );
    assert!(
        config.cli.npmrc_tokens.contains(&("custom-setting".to_string(), "custom-val".to_string())),
        "{label}: custom-setting in cli.npmrc_tokens",
    );
}

#[test]
fn pin_recorded_only_from_full_config_at_lockfile_dir() {
    let temp = TempDir::new().expect("temp dir");
    let root = dunce::canonicalize(temp.path()).expect("canonicalize root");
    let global_config_dir = root.join("global-config");
    fs::create_dir_all(&global_config_dir).expect("create global-config");
    fs::write(
        root.join("package.json"),
        format!(
            r#"{{"name":"root","devEngines":{{"packageManager":{{"name":"pnpm","version":"{PNPM_VERSION}","onFail":"download"}}}}}}"#,
        ),
    )
    .expect("write package.json");

    with_test_global_config_dir(Some(global_config_dir.clone()), || {
        // 1. Readable full config with lockfileDir in pnpm-workspace.yaml -> records at lockfileDir
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - .\nlockfileDir: custom-lf\n")
            .expect("write readable pnpm-workspace.yaml");
        let argv = vec![
            OsString::from("pnpm"),
            OsString::from("--dir"),
            root.as_os_str().to_os_string(),
            OsString::from("--reporter=silent"),
            OsString::from("run"),
            OsString::from("build"),
        ];
        let (overrides, remaining) = ConfigOverrides::extract(argv);
        let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);
        let outcome = pre_command_plan_with_state(
            &invocation,
            &overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("readable outcome");
        let Some(PreCommandPlan::SyncEnvLockfile(sync)) = outcome.plan else {
            panic!("expected SyncEnvLockfile plan for readable config");
        };
        assert_eq!(sync.env_root, root.join("custom-lf"));
        assert_eq!(sync.config.lockfile_dir, Some(root.join("custom-lf")));

        // 2. Unreadable setting in pnpm-workspace.yaml -> records nothing, caches failure in preloaded_config
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - .\nlockfileDir: custom-lf\nremoteSideEffectsCache: 123\n",
        )
        .expect("write unreadable pnpm-workspace.yaml");
        let outcome = pre_command_plan_with_state(
            &invocation,
            &overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("unreadable workspace outcome");
        assert!(
            outcome.plan.is_none(),
            "must not record pin when full config is unreadable: {:?}",
            outcome.plan,
        );
        assert!(
            matches!(outcome.preloaded_config, Some(Err(_))),
            "failed full config load must be preserved in preloaded_config so the command does not reload it",
        );

        // 3. Unreadable setting in .npmrc -> records nothing, caches failure in preloaded_config
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - .\nlockfileDir: custom-lf\n")
            .expect("restore readable pnpm-workspace.yaml");
        fs::write(root.join(".npmrc"), "//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n")
            .expect("write unreadable .npmrc");
        let outcome = pre_command_plan_with_state(
            &invocation,
            &overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("unreadable npmrc outcome");
        assert!(outcome.plan.is_none());
        assert!(matches!(outcome.preloaded_config, Some(Err(_))));
        fs::remove_file(root.join(".npmrc")).expect("remove .npmrc");

        // 4. Unreadable setting in global config.yaml -> records nothing, caches failure in preloaded_config
        fs::write(global_config_dir.join("config.yaml"), "remoteSideEffectsCache: 123\n")
            .expect("write unreadable global config.yaml");
        let outcome = pre_command_plan_with_state(
            &invocation,
            &overrides,
            SwitchProcessState {
                package_manager_switch_disabled: false,
                executed_by_corepack: false,
            },
        )
        .expect("unreadable global config outcome");
        assert!(outcome.plan.is_none());
        assert!(matches!(outcome.preloaded_config, Some(Err(_))));
    });
}
