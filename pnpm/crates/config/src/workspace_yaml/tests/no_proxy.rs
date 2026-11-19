use super::{
    Config, EnvVar, GetHomeDir, LoadWorkspaceYamlError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings, fs,
};
use crate::api::{EnvVarOs, GetCurrentDir, LinkProbe};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[test]
fn rejects_invalid_no_proxy_types() {
    for (value, expected_type) in
        [("123", "number"), ("[]", "array"), ("{host: localhost}", "object")]
    {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), format!("noProxy: {value}\n"))
            .unwrap();
        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
        dbg!(&error);
        let prefix = r#"The "noProxy" setting should be a boolean or string, but got"#;
        assert_eq!(error.to_string(), format!("{prefix} {expected_type}"));
    }
}

#[test]
fn accepts_no_proxy_strings_booleans_and_null() {
    for value in ["true", "false", "''", "'null'", "'localhost,127.0.0.1'", "null"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), format!("noProxy: {value}\n"))
            .unwrap();
        let settings = WorkspaceSettings::load_at(dir.path()).unwrap().unwrap();
        let expected = serde_saphyr::from_str::<Option<serde_json::Value>>(value).unwrap();
        assert_eq!(settings.no_proxy, expected);
    }
}

#[test]
fn skips_invalid_no_proxy_without_dropping_other_settings() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str("noProxy: [localhost]\nregistry: https://registry.example/\n")
            .unwrap();
    settings.drop_invalid_sections();
    assert_eq!(settings.no_proxy, None);
    assert_eq!(settings.registry.as_deref(), Some("https://registry.example/"));
}

#[test]
fn global_no_proxy_is_validated_and_can_be_skipped() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(crate::workspace_yaml::GLOBAL_CONFIG_YAML_FILENAME),
        "noProxy: [localhost]\nregistry: https://registry.example/\n",
    )
    .unwrap();
    let error = WorkspaceSettings::load_global(dir.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, LoadWorkspaceYamlError::InvalidNoProxy { received_type: "array" }));
    let settings =
        WorkspaceSettings::load_global_reading(dir.path(), crate::SettingsRead::Readable)
            .unwrap()
            .unwrap();
    assert_eq!(settings.no_proxy, None);
    assert_eq!(settings.registry.as_deref(), Some("https://registry.example/"));
}

#[test]
fn env_workspace_override_validates_no_proxy() {
    let env_workspace = tempfile::tempdir().unwrap();
    let cwd_dir = tempfile::tempdir().unwrap();
    fs::write(
        env_workspace.path().join(WORKSPACE_MANIFEST_FILENAME),
        "noProxy: [localhost]\nregistry: https://registry.example/\n",
    )
    .unwrap();
    static ENV_WORKSPACE_PATH: OnceLock<OsString> = OnceLock::new();
    ENV_WORKSPACE_PATH
        .set(
            env_workspace
                .path()
                .as_os_str()
                .to_owned(),
        )
        .expect("set once");
    struct EnvWorkspaceHost;
    impl EnvVar for EnvWorkspaceHost {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    impl EnvVarOs for EnvWorkspaceHost {
        fn var_os(name: &str) -> Option<OsString> {
            (name == "PNPM_CONFIG_WORKSPACE_DIR").then(|| {
                ENV_WORKSPACE_PATH
                    .get()
                    .expect("initialized")
                    .clone()
            })
        }
    }
    impl GetHomeDir for EnvWorkspaceHost {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    impl GetCurrentDir for EnvWorkspaceHost {
        fn current_dir() -> io::Result<PathBuf> {
            std::env::current_dir()
        }
    }
    impl LinkProbe for EnvWorkspaceHost {
        fn can_link_between_dirs(_: &Path, _: &Path) -> bool {
            false
        }
    }

    let error = Config::default().current::<EnvWorkspaceHost>(cwd_dir.path()).unwrap_err();
    assert!(matches!(error, LoadWorkspaceYamlError::InvalidNoProxy { received_type: "array" }));

    let readable = Config { settings_read: crate::SettingsRead::Readable, ..Config::default() }
        .current::<EnvWorkspaceHost>(cwd_dir.path())
        .unwrap();
    assert_eq!(readable.proxy.no_proxy, None);
    assert_eq!(readable.registry, "https://registry.example/");
}
