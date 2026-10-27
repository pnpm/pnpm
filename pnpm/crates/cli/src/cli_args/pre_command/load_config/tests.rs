use crate::{
    cli_args::{
        pre_command::{
            ConfigLoad, EntryPoint, Invocation, load_pre_command_config, pre_command_plan,
        },
        verify_deps::install_config_args,
    },
    config_overrides::ConfigOverrides,
};
use std::{cell::RefCell, ffi::OsString, fs, path::PathBuf};
use tempfile::TempDir;

std::thread_local! {
    static TEST_GLOBAL_CONFIG_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(super) fn current_test_global_config_dir() -> Option<PathBuf> {
    TEST_GLOBAL_CONFIG_DIR.with(|cell| cell.borrow().clone())
}

pub(in crate::cli_args) fn with_test_global_config_dir<ResultValue>(
    dir: Option<PathBuf>,
    run: impl FnOnce() -> ResultValue,
) -> ResultValue {
    TEST_GLOBAL_CONFIG_DIR.with(|cell| {
        let prev = cell.replace(dir);
        let result = run();
        cell.replace(prev);
        result
    })
}

#[test]
fn cli_flags_replace_config_dot_tokens_for_spawned_children() {
    let temp = TempDir::new().expect("temp dir");
    let root = dunce::canonicalize(temp.path()).expect("canonicalize root");
    let global_config_dir = root.join("global-config");
    fs::create_dir_all(&global_config_dir).expect("create global-config");

    let raw_argv = vec![
        OsString::from("pnpm"),
        OsString::from("--dir"),
        root.as_os_str().to_os_string(),
        OsString::from("--config.registry=https://stale.example.com/"),
        OsString::from("--registry=https://winning.example.com/"),
        OsString::from("--config.httpsProxy=http://stale-proxy:8080"),
        OsString::from("--https-proxy=http://winning-proxy:8080"),
        OsString::from("--config.noproxy=stale.local"),
        OsString::from("--no-proxy=winning.local"),
        OsString::from("--config.lockfileDir=stale-lf"),
        OsString::from("--lockfile-dir=winning-lf"),
        OsString::from("run"),
        OsString::from("build"),
    ];
    let (overrides, remaining) = ConfigOverrides::extract(raw_argv);
    let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);

    with_test_global_config_dir(Some(global_config_dir), || {
        let config = load_pre_command_config(&invocation, &overrides, &root, ConfigLoad::RECORD)
            .expect("load config");
        let forwarded: Vec<String> = install_config_args(&config).collect();
        let has_stale = forwarded
            .iter()
            .any(|arg| arg.contains("stale"));
        assert!(!has_stale);
        assert!(forwarded.contains(&"--config.registry=https://winning.example.com/".to_string()));
        assert!(forwarded.contains(&"--config.https-proxy=http://winning-proxy:8080".to_string()));
        assert!(forwarded.contains(&"--config.no-proxy=winning.local".to_string()));
        let expected_lockfile =
            format!("--config.lockfile-dir={}", root.join("winning-lf").display());
        assert!(forwarded.contains(&expected_lockfile));
    });
}

#[test]
fn corrupted_lockfile_skipped_when_pin_does_not_consult_lockfile() {
    let temp = TempDir::new().expect("temp dir");
    let root = dunce::canonicalize(temp.path()).expect("canonicalize root");
    let global_config_dir = root.join("global-config");
    fs::create_dir_all(&global_config_dir).expect("create global-config");
    fs::write(root.join("pnpm-lock.yaml"), ": : : not valid yaml [ [ [\n")
        .expect("write malformed pnpm-lock.yaml");
    fs::write(
        root.join("package.json"),
        r#"{"name":"root","devEngines":{"packageManager":{"name":"pnpm","version":"9.1.0","onFail":"download"}}}"#,
    )
    .expect("write package.json");

    with_test_global_config_dir(Some(global_config_dir), || {
        for flag in ["--global", "--pm-on-fail=ignore", "--pm-on-fail=warn"] {
            let argv = vec![
                OsString::from("pnpm"),
                OsString::from("--dir"),
                root.as_os_str().to_os_string(),
                OsString::from("--reporter=silent"),
                OsString::from(flag),
                OsString::from("root"),
            ];
            let (overrides, remaining) = ConfigOverrides::extract(argv);
            let invocation = Invocation::from_argv(&remaining, EntryPoint::Command);
            let outcome = pre_command_plan(&invocation, &overrides)
                .unwrap_or_else(|err| panic!("{flag} should not read malformed lockfile: {err}"));
            assert!(outcome.plan.is_none());
        }
        let unknown_argv = vec![
            OsString::from("pnpm"),
            OsString::from("--dir"),
            root.as_os_str().to_os_string(),
            OsString::from("--pm-on-fail=warn"),
            OsString::from("root"),
            OsString::from("--unknown-flag"),
        ];
        let (overrides, remaining) = ConfigOverrides::extract(unknown_argv);
        let invocation = Invocation::from_argv(&remaining, EntryPoint::UnknownFlag);
        let outcome = pre_command_plan(&invocation, &overrides)
            .expect("unknown-flag with non-switching pin should not read malformed lockfile");
        assert!(outcome.plan.is_none());
    });
}
