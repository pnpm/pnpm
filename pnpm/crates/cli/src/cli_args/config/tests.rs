//! pnpm's tests inject a `_config` / `_context`; pacquet builds a [`Config`]
//! and sets the public fields the command reads ([`Config::config_dir`],
//! [`Config::explicit_settings`], [`Config::raw_auth_config`]) — the same
//! injection, expressed against pacquet's loaded-config shape.

use super::{ConfigFlags, ConfigLocation, config_get, config_list, config_set, ini};
use indexmap::IndexMap;
use pacquet_config::Config;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn config_with_dir(config_dir: &Path) -> Config {
    Config { config_dir: Some(config_dir.to_path_buf()), ..Config::default() }
}

fn flags(global: bool, location: Option<ConfigLocation>, json: bool) -> ConfigFlags {
    ConfigFlags { global, location, json }
}

fn read_yaml(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_saphyr::from_str(&text).expect("parse yaml"))
}

fn read_ini(path: &Path) -> IndexMap<String, String> {
    ini::read(path).expect("read ini")
}

//  config set: INI routing --------------------------------------------

#[test]
fn set_registry_global_writes_auth_ini() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "registry",
        Some("https://npm-registry.example.com/".to_string()),
    )
    .unwrap();

    let ini = read_ini(&config_dir.join("auth.ini"));
    assert_eq!(ini.get("registry").map(String::as_str), Some("https://npm-registry.example.com/"));
}

#[test]
fn set_cafile_global_writes_auth_ini() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(&config, tmp.path(), flags(true, None, false), "cafile", Some("some-cafile".into()))
        .unwrap();

    assert_eq!(
        read_ini(&config_dir.join("auth.ini")).get("cafile").map(String::as_str),
        Some("some-cafile"),
    );
}

#[test]
fn set_scoped_registry_project_creates_npmrc() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "@myorg:registry",
        Some("https://test-registry.example.com/".to_string()),
    )
    .unwrap();

    assert_eq!(
        read_ini(&tmp.path().join(".npmrc")).get("@myorg:registry").map(String::as_str),
        Some("https://test-registry.example.com/"),
    );
    assert!(!tmp.path().join("pnpm-workspace.yaml").exists());
}

#[test]
fn set_per_registry_auth_project_creates_npmrc() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "//registry.example.com/:_auth",
        Some("test-auth-value".to_string()),
    )
    .unwrap();

    assert_eq!(
        read_ini(&tmp.path().join(".npmrc"))
            .get("//registry.example.com/:_auth")
            .map(String::as_str),
        Some("test-auth-value"),
    );
}

//  config set: YAML routing -------------------------------------------

#[test]
fn set_pnpm_key_global_writes_config_yaml_as_number() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = config_with_dir(&config_dir);

    config_set(&config, tmp.path(), flags(true, None, false), "fetch-retries", Some("1".into()))
        .unwrap();

    assert_eq!(read_yaml(&config_dir.join("config.yaml")).unwrap(), json!({ "fetchRetries": 1 }));
}

#[test]
fn set_registries_and_named_registries_global_writes_config_yaml() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = config_with_dir(&config_dir);

    let registries = json!({
        "default": "https://registry.example.com/",
        "@scope": "https://scope.example.com/",
    });
    config_set(
        &config,
        tmp.path(),
        flags(true, None, true),
        "registries",
        Some(registries.to_string()),
    )
    .unwrap();

    let named_registries = json!({ "work": "https://work.example.com/" });
    config_set(
        &config,
        tmp.path(),
        flags(true, None, true),
        "named-registries",
        Some(named_registries.to_string()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&config_dir.join("config.yaml")).unwrap(),
        json!({ "registries": registries, "namedRegistries": named_registries }),
    );
}

#[test]
fn set_camel_key_location_global() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Global), false),
        "fetchRetries",
        Some("1".into()),
    )
    .unwrap();

    assert_eq!(read_yaml(&config_dir.join("config.yaml")).unwrap(), json!({ "fetchRetries": 1 }));
}

#[test]
fn set_pnpm_key_project_writes_workspace_yaml() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "virtual-store-dir",
        Some(".pnpm".into()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap(),
        json!({ "virtualStoreDir": ".pnpm" }),
    );
}

#[test]
fn set_global_https_proxy_writes_config_yaml_not_auth_ini() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "https-proxy",
        Some("http://proxy.example.com:8443".into()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&config_dir.join("config.yaml")).unwrap(),
        json!({ "httpsProxy": "http://proxy.example.com:8443" }),
    );
    assert!(!config_dir.join("auth.ini").exists());
}

#[test]
fn set_global_http_proxy_writes_config_yaml() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "httpProxy",
        Some("http://proxy.example.com:8080".into()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&config_dir.join("config.yaml")).unwrap(),
        json!({ "httpProxy": "http://proxy.example.com:8080" }),
    );
}

#[test]
fn set_global_no_proxy_writes_config_yaml() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "no-proxy",
        Some("localhost,127.0.0.1".into()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&config_dir.join("config.yaml")).unwrap(),
        json!({ "noProxy": "localhost,127.0.0.1" }),
    );
}

#[test]
fn set_key_equals_value_form() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    // value with an embedded `=` keeps everything after the first `=`.
    super::split_set_params(Some("lockfile-dir=foo=bar".into()), None, "set")
        .map(|(k, v)| {
            config_set(
                &config,
                tmp.path(),
                flags(false, Some(ConfigLocation::Project), false),
                &k,
                Some(v),
            )
        })
        .unwrap()
        .unwrap();

    assert_eq!(
        read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap(),
        json!({ "lockfileDir": "foo=bar" }),
    );
}

#[test]
fn set_dot_leading_and_subscripted_keys() {
    for key in [".fetchRetries", r#"["fetch-retries"]"#] {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("global-config");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config = config_with_dir(&config_dir);

        config_set(&config, tmp.path(), flags(true, None, false), key, Some("1".into())).unwrap();

        assert_eq!(
            read_yaml(&config_dir.join("config.yaml")).unwrap(),
            json!({ "fetchRetries": 1 }),
            "key {key}",
        );
    }
}

#[test]
fn set_object_value_with_json_writes_workspace_yaml() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    let extensions = json!({
        "@babel/parser": { "peerDependencies": { "@babel/types": "*" } },
        "jest-circus": { "dependencies": { "slash": "3" } },
    });
    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), true),
        "packageExtensions",
        Some(extensions.to_string()),
    )
    .unwrap();

    assert_eq!(
        read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap(),
        json!({ "packageExtensions": extensions }),
    );
}

//  config set: validation errors --------------------------------------

#[test]
fn set_rejects_deep_property_path() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));
    let err = config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        ".catalog.react",
        Some("19".into()),
    )
    .unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "ERR_PNPM_CONFIG_SET_DEEP_KEY");
}

#[test]
fn set_refuses_workspace_key_in_global_config() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);
    let err = config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Global), true),
        "catalog",
        Some(r#"{"react":"19"}"#.into()),
    )
    .unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY");
}

#[test]
fn set_refuses_kebab_workspace_key() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));
    let err = config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), true),
        "package-extensions",
        Some("{}".into()),
    )
    .unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "ERR_PNPM_CONFIG_SET_UNSUPPORTED_WORKSPACE_KEY");
}

//  config delete ------------------------------------------------------

#[test]
fn delete_last_yaml_key_removes_file() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "virtual-store-dir",
        Some(".pnpm".into()),
    )
    .unwrap();
    assert!(tmp.path().join("pnpm-workspace.yaml").exists());

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "virtual-store-dir",
        None,
    )
    .unwrap();
    assert!(!tmp.path().join("pnpm-workspace.yaml").exists());
}

#[test]
fn delete_auth_key_set_and_unset() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = config_with_dir(&config_dir);

    // set, not present → file untouched (no registry key)
    std::fs::write(
        config_dir.join("auth.ini"),
        "@my-company:registry=https://registry.my-company.example.com/\n",
    )
    .unwrap();
    config_set(&config, tmp.path(), flags(true, None, false), "registry", None).unwrap();
    assert_eq!(
        read_ini(&config_dir.join("auth.ini")).get("@my-company:registry").map(String::as_str),
        Some("https://registry.my-company.example.com/"),
    );

    // set present → removed
    std::fs::write(
        config_dir.join("auth.ini"),
        "registry=https://registry.my-company.example.com/\n",
    )
    .unwrap();
    config_set(&config, tmp.path(), flags(true, None, false), "registry", None).unwrap();
    assert!(read_ini(&config_dir.join("auth.ini")).is_empty());
}

#[test]
fn delete_missing_params_errors() {
    // No key → NoParams. Exercised through the param-splitting shape the
    // dispatch uses.
    let (key, value) = (None::<String>, None::<String>);
    let err = super::split_set_params(key, value, "set").unwrap_err();
    assert_eq!(miette::Diagnostic::code(&err).unwrap().to_string(), "ERR_PNPM_CONFIG_NO_PARAMS");
}

//  config get / list --------------------------------------------------

fn config_for_get(explicit: &[(&str, Value)], auth: &[(&str, &str)]) -> Config {
    let mut config = Config { config_dir: Some(PathBuf::from("/config")), ..Config::default() };
    for (key, value) in explicit {
        config.explicit_settings.insert((*key).to_string(), value.clone());
    }
    for (key, value) in auth {
        config.raw_auth_config.insert((*key).to_string(), (*value).to_string());
    }
    config
}

#[test]
fn get_scalar_string_and_camel() {
    let config = config_for_get(&[("storeDir", json!("~/store"))], &[]);
    assert_eq!(config_get(&config, flags(true, None, false), "store-dir").unwrap(), "~/store");
    assert_eq!(config_get(&config, flags(true, None, false), "storeDir").unwrap(), "~/store");
}

#[test]
fn get_boolean_and_array_and_object() {
    let config = config_for_get(
        &[
            ("updateNotifier", json!(true)),
            ("publicHoistPattern", json!(["*eslint*", "*prettier*"])),
            ("packageExtensions", json!({ "a": { "dependencies": { "b": "1" } } })),
        ],
        &[],
    );
    assert_eq!(config_get(&config, flags(true, None, false), "update-notifier").unwrap(), "true");
    assert_eq!(
        serde_json::from_str::<Value>(
            &config_get(&config, flags(true, None, false), "public-hoist-pattern").unwrap()
        )
        .unwrap(),
        json!(["*eslint*", "*prettier*"]),
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &config_get(&config, flags(true, None, false), "package-extensions").unwrap()
        )
        .unwrap(),
        json!({ "a": { "dependencies": { "b": "1" } } }),
    );
}

#[test]
fn get_unknown_key_is_undefined() {
    let config = config_for_get(&[], &[]);
    assert_eq!(
        config_get(&config, flags(true, None, false), "no-such-setting").unwrap(),
        "undefined",
    );
    // prototype-chain names must not resolve
    for key in ["constructor", "__proto__", "hasOwnProperty"] {
        assert_eq!(
            config_get(&config, flags(true, None, false), key).unwrap(),
            "undefined",
            "{key}",
        );
    }
}

#[test]
fn get_scoped_registry_from_auth_and_merged() {
    let from_auth =
        config_for_get(&[], &[("@scope:registry", "https://custom-registry.example.com/")]);
    assert_eq!(
        config_get(&from_auth, flags(false, None, false), "@scope:registry").unwrap(),
        "https://custom-registry.example.com/",
    );

    // merged `registries` block wins over the raw .npmrc value (pnpm/pnpm#11492)
    let mut merged = config_for_get(&[], &[("@scope:registry", "https://from-npmrc.example.com/")]);
    merged
        .registries
        .insert("@scope".to_string(), "https://from-workspace-yaml.example.com/".to_string());
    assert_eq!(
        config_get(&merged, flags(false, None, false), "@scope:registry").unwrap(),
        "https://from-workspace-yaml.example.com/",
    );

    let absent = config_for_get(&[], &[]);
    assert_eq!(
        config_get(&absent, flags(false, None, false), "@scope:registry").unwrap(),
        "undefined",
    );
}

#[test]
fn get_globalconfig_path() {
    let config = config_for_get(&[], &[]);
    assert_eq!(
        config_get(&config, flags(true, None, false), "globalconfig").unwrap(),
        Path::new("/config").join("config.yaml").to_string_lossy(),
    );
}

#[test]
fn get_property_path_into_object() {
    let config = config_for_get(
        &[
            ("trustPolicyExclude", json!(["foo", "bar"])),
            (
                "packageExtensions",
                json!({ "@babel/parser": { "peerDependencies": { "@babel/types": "*" } } }),
            ),
        ],
        &[],
    );
    assert_eq!(
        config_get(&config, flags(false, None, false), "trustPolicyExclude[0]").unwrap(),
        "foo",
    );
    assert_eq!(
        config_get(
            &config,
            flags(false, None, false),
            r#"packageExtensions["@babel/parser"].peerDependencies["@babel/types"]"#
        )
        .unwrap(),
        "*",
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &config_get(&config, flags(false, None, false), "package-extensions").unwrap()
        )
        .unwrap(),
        json!({ "@babel/parser": { "peerDependencies": { "@babel/types": "*" } } }),
    );
}

#[test]
fn list_includes_settings_and_censors_protected() {
    let config = config_for_get(
        &[("storeDir", json!("~/store")), ("fetchRetries", json!(2))],
        &[
            ("username", "general-username"),
            ("@my-org:registry", "https://my-org.example.com/registry"),
            ("//my-org.example.com:username", "my-username-in-my-org"),
        ],
    );
    let listed: Value = serde_json::from_str(&config_list(&config)).unwrap();
    assert_eq!(listed["storeDir"], json!("~/store"));
    assert_eq!(listed["fetchRetries"], json!(2));
    assert_eq!(listed["@my-org:registry"], json!("https://my-org.example.com/registry"));
    assert_eq!(listed["username"], json!("(protected)"));
    assert_eq!(listed["//my-org.example.com:username"], json!("(protected)"));

    // `get` with no key equals `list`.
    let got = config_get(&config, flags(false, None, false), "").unwrap();
    // empty key is a property path → whole record
    let got_value: Value = serde_json::from_str(&got).unwrap();
    assert_eq!(got_value["storeDir"], json!("~/store"));
}

//  security hardening for the INI write path --------------------------

#[test]
fn set_ini_value_with_control_char_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    let err = config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "//registry.example.com/:_authToken",
        Some("token\ninjected=evil".to_string()),
    )
    .unwrap_err();

    assert_eq!(
        err.code().unwrap().to_string(),
        "ERR_PNPM_CLI_CONFIG_SET_INVALID_CONTROL_CHARACTER",
    );
    // The file must not have been written.
    assert!(!tmp.path().join(".npmrc").exists());
}

#[cfg(unix)]
#[test]
fn set_preserves_existing_npmrc_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new().unwrap();
    let npmrc = tmp.path().join(".npmrc");
    std::fs::write(&npmrc, "@local:registry=https://localhost/\n").unwrap();
    std::fs::set_permissions(&npmrc, std::fs::Permissions::from_mode(0o644)).unwrap();

    let config = config_with_dir(&tmp.path().join("global-config"));
    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "registry",
        Some("https://example.com/".to_string()),
    )
    .unwrap();

    let mode = std::fs::metadata(&npmrc).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "existing .npmrc mode must be preserved, got {mode:o}");
}

#[cfg(unix)]
#[test]
fn set_does_not_follow_symlinked_npmrc_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new().unwrap();
    // A repo-controlled symlinked `.npmrc` pointing at a permissive (0644) file.
    let target = tmp.path().join("target-npmrc");
    std::fs::write(&target, "@local:registry=https://localhost/\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    let npmrc = tmp.path().join(".npmrc");
    std::os::unix::fs::symlink(&target, &npmrc).unwrap();

    let config = config_with_dir(&tmp.path().join("global-config"));
    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "//registry.example.com/:_authToken",
        Some("secret-token".to_string()),
    )
    .unwrap();

    // The rename replaced the symlink with a fresh regular file that must keep
    // the conservative 0600 default, not the link target's 0644.
    let meta = std::fs::symlink_metadata(&npmrc).unwrap();
    assert!(!meta.file_type().is_symlink(), "symlink should be replaced by a regular file");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "credentials written through a symlinked .npmrc must stay 0600, got {mode:o}",
    );
}
