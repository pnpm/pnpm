use super::{
    Config, EnvGuard, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe,
    NodePackageMapType, OsString, Path, PathBuf, WorkspaceSettings, assert_eq, capture_warnings,
    config_from_workspace_yaml, env, fs, io, load_with_project_and_user, repo_on_branch,
    safe_host_var, tempdir, write_file,
};

/// `pnpm config get` reads the explicit-settings record, which therefore has
/// to answer both spellings with the value the install will use — whichever
/// of the two the file was written in.
#[test]
pub fn explicit_settings_report_the_spelling_that_won() {
    for (yaml, expected) in [
        ("virtualStoreType: project\nenableGlobalVirtualStore: true\n", false),
        ("virtualStoreType: global\nenableGlobalVirtualStore: false\n", true),
        ("virtualStoreType: project\n", false),
        ("enableGlobalVirtualStore: true\n", true),
    ] {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml)
            .expect("write to pnpm-workspace.yaml");
        let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
        assert_eq!(
            config.explicit_settings.get("enableGlobalVirtualStore"),
            Some(&serde_json::Value::Bool(expected)),
            "yaml: {yaml}",
        );
        assert_eq!(
            config.explicit_settings.get("virtualStoreType").and_then(serde_json::Value::as_str),
            Some(if expected { "global" } else { "project" }),
            "yaml: {yaml}",
        );
        assert_eq!(config.enable_global_virtual_store, expected, "yaml: {yaml}");
    }
}

/// `audit.level` supersedes the deprecated `auditLevel` spelling, and
/// `pnpm config get audit-level` reads the explicit-settings record — so the
/// record has to carry the level under the deprecated name too, whichever
/// spelling the file was written in.
#[test]
pub fn explicit_settings_mirror_audit_level_from_the_audit_section() {
    for yaml in ["audit:\n  level: high\n", "audit:\n  level: high\nauditLevel: low\n"] {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml)
            .expect("write to pnpm-workspace.yaml");
        let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
        assert_eq!(
            config.explicit_settings.get("auditLevel").and_then(serde_json::Value::as_str),
            Some("high"),
            "yaml: {yaml}",
        );
    }
}

/// Real-process-environment smoke test for the proxy env-var
/// fallback through `Config::current`. The injected-`EnvVar` tests
/// in `npmrc_auth/tests.rs` cover the cascade branches
/// exhaustively; this one only proves the wiring through
/// `Host::var` reaches `std::env::var` and that the cascade
/// fires even with no `.npmrc` present.
#[test]
pub fn proxy_env_fallback_applies_through_current() {
    // Snapshot every proxy var the cascade might read so peer tests
    // can't observe our mutations and so the env restores cleanly.
    let _g = EnvGuard::snapshot([
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "PROXY",
        "proxy",
        "NO_PROXY",
        "no_proxy",
        "NPM_CONFIG_WORKSPACE_DIR",
        "npm_config_workspace_dir",
    ]);
    let tmp = tempdir().unwrap();
    // SAFETY: EnvGuard above serializes the test against other
    // env-mutating tests in this process; no other thread reads
    // these vars concurrently. The other proxy vars are removed so
    // a host-set value can't leak in and skew the assertion.
    unsafe {
        env::remove_var("HTTPS_PROXY");
        env::remove_var("https_proxy");
        env::remove_var("HTTP_PROXY");
        env::remove_var("http_proxy");
        env::remove_var("PROXY");
        env::remove_var("proxy");
        env::remove_var("NO_PROXY");
        env::remove_var("no_proxy");
        env::remove_var("NPM_CONFIG_WORKSPACE_DIR");
        env::remove_var("npm_config_workspace_dir");
        env::set_var("HTTPS_PROXY", "http://env.example:8080");
    }
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://env.example:8080"));
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://env.example:8080"),
        "http side cascades through resolved https",
    );
}

/// Drives the [`EnvVar`] + [`GetHomeDir`] DI seams: the fake
/// returns the test's tempdir for `XDG_CONFIG_HOME`, so
/// [`crate::defaults::default_config_dir`] resolves to
/// `<tempdir>/pnpm/config.yaml` rather than touching the
/// developer's real config dir.
#[test]
pub fn global_config_yaml_enables_gvs() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "enableGlobalVirtualStore: true\n")
        .expect("write to global config.yaml");

    static XDG_CONFIG_HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    XDG_CONFIG_HOME_PATH.set(xdg.path().to_path_buf()).expect("set once");

    struct HostWithXdgConfigHome;
    impl EnvVar for HostWithXdgConfigHome {
        fn var(name: &str) -> Option<String> {
            if name == "XDG_CONFIG_HOME" {
                return XDG_CONFIG_HOME_PATH.get().map(|path| path.to_string_lossy().into_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithXdgConfigHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithXdgConfigHome {
        fn home_dir() -> Option<PathBuf> {
            // XDG_CONFIG_HOME short-circuits the home_dir lookup
            // inside `default_config_dir`, but `Config::current`'s
            // home-dir `.npmrc` fallback still consults it. The
            // fallback gracefully tolerates `None`, so returning
            // `None` keeps the test hermetic without forcing a
            // tempdir for the unrelated `.npmrc` path.
            None
        }
    }
    inert_link_probe!(HostWithXdgConfigHome);
    host_current_dir!(HostWithXdgConfigHome);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithXdgConfigHome>(tmp.path()).expect("config loads");
    assert!(
        config.enable_global_virtual_store,
        "enableGlobalVirtualStore from global config.yaml must apply",
    );
}

#[test]
pub fn pnpm_config_env_var_enables_gvs() {
    struct HostWithPnpmConfigEnv;
    impl EnvVar for HostWithPnpmConfigEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE" {
                return Some("true".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithPnpmConfigEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPnpmConfigEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPnpmConfigEnv);
    host_current_dir!(HostWithPnpmConfigEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(config.enable_global_virtual_store);
}

#[test]
pub fn pnpm_config_env_var_lowercase_works() {
    struct HostWithLowercaseEnv;
    impl EnvVar for HostWithLowercaseEnv {
        fn var(name: &str) -> Option<String> {
            if name == "pnpm_config_enable_global_virtual_store" {
                return Some("true".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithLowercaseEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithLowercaseEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithLowercaseEnv);
    host_current_dir!(HostWithLowercaseEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithLowercaseEnv>(tmp.path()).expect("loads");
    assert!(config.enable_global_virtual_store);
}

#[test]
pub fn self_update_config_honors_trusted_release_age_env_override() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 4320\nminimumReleaseAgeStrict: true\n",
    )
    .expect("write to pnpm-workspace.yaml");

    struct HostWithReleaseAgeEnv;
    impl EnvVar for HostWithReleaseAgeEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_MINIMUM_RELEASE_AGE" => Some("0".to_owned()),
                "PNPM_CONFIG_MINIMUM_RELEASE_AGE_STRICT" => Some("false".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithReleaseAgeEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithReleaseAgeEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithReleaseAgeEnv);
    host_current_dir!(HostWithReleaseAgeEnv);

    let config = Config::new()
        .current_for_self_update::<HostWithReleaseAgeEnv>(tmp.path())
        .expect("config loads");

    assert_eq!(config.minimum_release_age, Some(0));
    assert_eq!(config.minimum_release_age_strict, Some(false));
}

#[test]
pub fn patches_dir_reads_from_env_overlay() {
    struct HostWithPatchesDirEnv;
    impl EnvVar for HostWithPatchesDirEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_PATCHES_DIR" {
                return Some("custom-patches".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithPatchesDirEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPatchesDirEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPatchesDirEnv);
    host_current_dir!(HostWithPatchesDirEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPatchesDirEnv>(tmp.path()).expect("loads");
    assert_eq!(config.patches_dir.as_deref(), Some("custom-patches"));
}

#[test]
pub fn max_sockets_accepts_both_spellings_from_the_environment() {
    fake_env!(load_with_fake_env);
    let tmp = tempdir().unwrap();

    set_fake_env(&[("PNPM_CONFIG_MAXSOCKETS", "7")]);
    assert_eq!(load_with_fake_env(tmp.path()).max_sockets, Some(7));

    // The pnpm spelling wins over npm's when a shell exports both.
    set_fake_env(&[("PNPM_CONFIG_MAXSOCKETS", "7"), ("PNPM_CONFIG_MAX_SOCKETS", "9")]);
    assert_eq!(load_with_fake_env(tmp.path()).max_sockets, Some(9));
}

#[test]
pub fn virtual_store_dir_max_length_env_var_overrides_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "virtualStoreDirMaxLength: 90\n")
        .expect("write to pnpm-workspace.yaml");

    struct HostWithEnvOverride;
    impl EnvVar for HostWithEnvOverride {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH" {
                return Some("50".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEnvOverride {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithEnvOverride {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEnvOverride);
    host_current_dir!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(
        config.virtual_store_dir_max_length, 50,
        "env var must win over pnpm-workspace.yaml",
    );
}

/// `PNPM_CONFIG_LOCKFILE_DIR` overrides the yaml setting, and a relative
/// value resolves against the directory the config was loaded for.
#[test]
pub fn lockfile_dir_env_var_overrides_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "lockfileDir: from-yaml
",
    )
    .expect("write to pnpm-workspace.yaml");

    struct HostWithLockfileDirEnv;
    impl EnvVar for HostWithLockfileDirEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_LOCKFILE_DIR" {
                return Some("from-env".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithLockfileDirEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithLockfileDirEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithLockfileDirEnv);
    host_current_dir!(HostWithLockfileDirEnv);

    let config = Config::new().current::<HostWithLockfileDirEnv>(tmp.path()).expect("loads");
    assert_eq!(config.lockfile_dir.as_deref(), Some(tmp.path().join("from-env").as_path()));
    assert_eq!(config.modules_dir, tmp.path().join("from-env").join("node_modules"));
}

#[test]
pub fn package_map_settings_load_from_env() {
    struct HostWithPackageMapEnv;
    impl EnvVar for HostWithPackageMapEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_NODE_EXPERIMENTAL_PACKAGE_MAP" => Some("true".to_owned()),
                "PNPM_CONFIG_NODE_PACKAGE_MAP_TYPE" => Some("loose".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithPackageMapEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPackageMapEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPackageMapEnv);
    host_current_dir!(HostWithPackageMapEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPackageMapEnv>(tmp.path()).expect("loads");
    assert!(config.node_experimental_package_map);
    assert_eq!(config.node_package_map_type, NodePackageMapType::Loose);
}

#[test]
pub fn peers_suffix_max_length_env_var_overrides_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "peersSuffixMaxLength: 10\n")
        .expect("write to pnpm-workspace.yaml");

    struct HostWithEnvOverride;
    impl EnvVar for HostWithEnvOverride {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_PEERS_SUFFIX_MAX_LENGTH" {
                return Some("25".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEnvOverride {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithEnvOverride {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEnvOverride);
    host_current_dir!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 25, "env var must win over pnpm-workspace.yaml");
}

#[test]
fn an_explicit_strict_setting_wins_over_the_release_age_default() {
    let config =
        config_from_workspace_yaml("minimumReleaseAge: 4320\nminimumReleaseAgeStrict: false\n");
    assert!(!config.resolved_minimum_release_age_strict());

    let config = config_from_workspace_yaml("minimumReleaseAgeStrict: true\n");
    assert!(config.resolved_minimum_release_age_strict(), "strict mode stands on its own");
}

#[test]
fn a_release_age_env_var_turns_on_strict_mode() {
    struct HostWithReleaseAgeEnv;
    impl EnvVar for HostWithReleaseAgeEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_MINIMUM_RELEASE_AGE" => Some("1440".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithReleaseAgeEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithReleaseAgeEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithReleaseAgeEnv);
    host_current_dir!(HostWithReleaseAgeEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithReleaseAgeEnv>(tmp.path()).expect("config loads");

    assert!(config.resolved_minimum_release_age_strict());
}

/// A project `.npmrc` redirecting the default registry still drives normal
/// installs, but must NOT steer package-manager bootstrap: that resolves
/// through the trusted user-level registry instead. Regression test for
/// GHSA-j2hc-m6cf-6jm8 (`packageManager` auto-switch registry confusion).
#[test]
pub fn package_manager_bootstrap_ignores_project_npmrc_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(
        config.registry, "https://attacker.example.com/",
        "project registry drives normal installs",
    );
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://trusted.example.com/",
        "package-manager bootstrap ignores the repository-controlled project .npmrc registry",
    );
    assert_eq!(
        config.package_manager_bootstrap.resolved_registries().get("default").map(String::as_str),
        Some("https://trusted.example.com/"),
    );
}

/// `PNPM_CONFIG_REGISTRY` is user-controlled (not repository-controlled), so
/// it overrides the package-manager bootstrap default registry too,
/// mirroring pnpm's env/CLI `registry` handling.
#[test]
pub fn package_manager_bootstrap_honors_env_registry() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "registry=https://attacker.example.com/\n");
    set_fake_env(&[("PNPM_CONFIG_REGISTRY", "https://env.example.com/")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://env.example.com/", "env registry drives normal installs");
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://env.example.com/",
        "env registry overrides the package-manager bootstrap default",
    );
}

/// When `PNPM_CONFIG_REGISTRY` is set without a trailing slash, the env
/// override normalizes it before storing — matching pnpm, which treats
/// `https://r` and `https://r/` as the same registry. The slash must be
/// appended consistently to `config.registry`, `config.registries_by_scope`, and
/// the bootstrap map so downstream lookups (auth-header pinning,
/// `package_manager_bootstrap`) all key against the normalized form.
#[test]
pub fn env_registry_override_appends_missing_trailing_slash() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("PNPM_CONFIG_REGISTRY", "https://env.example.com")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://env.example.com/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://env.example.com/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://env.example.com/"),
    );
}

/// A key spelled in kebab-case in the global `config.yaml` is read by
/// nothing, since only camelCase reaches the settings, so it is reported
/// naming the spelling that works.
#[test]
pub fn global_config_yaml_kebab_case_key_is_reported() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(&config_file, "store-dir: /kebab-store\nstoreDir: /camel-store\n")
        .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        let settings = WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
        assert_eq!(settings.store_dir.as_deref(), Some("/camel-store"));
    });

    assert_eq!(
        warnings,
        [format!(
            r#"The following settings in the global config file ("{}") were ignored because they are not written in camelCase: "store-dir" (use "storeDir")."#,
            config_file.display(),
        )],
    );
}

/// A workspace-only setting and a key that is no setting at all are both
/// dropped from the global `config.yaml`, so both are reported.
#[test]
pub fn global_config_yaml_keys_it_cannot_set_are_reported() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(&config_file, "nodeLinker: hoisted\npackages:\n  - lib/*\n")
        .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        let settings = WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
        assert_eq!(settings.node_linker, None);
    });

    assert_eq!(
        warnings,
        [format!(
            r#"The following settings cannot be set in the global config file ("{}") and were ignored: "nodeLinker", "packages". Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies"#,
            config_file.display(),
        )],
    );
}

/// A key no configuration file may set is reported with the route pnpm
/// offers for it, not with the move-to-workspace advice.
#[test]
pub fn global_config_yaml_keys_settable_nowhere_are_reported_with_their_route() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(&config_file, "configDir: /elsewhere\nbin: /usr/local/bin\ndir: /work\n")
        .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
    });

    assert_eq!(
        warnings,
        [format!(
            r#"The following settings cannot be set in the global config file ("{}") and were ignored: "configDir" (This is not a pnpm setting), "bin" (Set it for the machine instead: pnpm config set --global global-bin-dir), "dir" (Pass --dir on the command line instead)."#,
            config_file.display(),
        )],
    );
}

/// A key that names no setting of any supported pnpm gets the
/// "not recognized by this version" warning — with the closest real setting
/// when the key looks like a typo of one — instead of the move-to-workspace
/// advice, which would send the user to a file that ignores it too.
#[test]
pub fn global_config_yaml_unrecognized_keys_are_reported_with_a_suggestion() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(&config_file, "minimumReleaseAg: 100\nzzzNotASettingZzz: true\n")
        .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
    });

    assert_eq!(
        warnings,
        [format!(
            r#"The following settings in the global config file ("{}") are not recognized by this version of pnpm and were ignored: "minimumReleaseAg" (did you mean "minimumReleaseAge"?), "zzzNotASettingZzz"."#,
            config_file.display(),
        )],
    );
}

/// A `$schema` key is an editor's schema association, not a setting.
#[test]
pub fn global_config_yaml_schema_directive_is_silent() {
    let config_dir = tempdir().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.yaml"),
        "$schema: https://json.schemastore.org/pnpm-workspace.json\n",
    )
    .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
    });

    assert_eq!(warnings, Vec::<String>::new());
}

/// A dropped key pnpm honors in this file gets no warning; the rationale
/// lives on `warn_about_dropped_keys`.
#[test]
pub fn global_config_yaml_key_pnpm_honors_stays_silent() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(&config_file, "globalPath: /usr/local/pnpm\n").expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
    });

    assert_eq!(warnings, Vec::<String>::new());
}

/// An explicit null sets nothing, so it is not reported. A file carrying
/// every kind of dropped key gets all three warnings, in pnpm's order.
#[test]
pub fn global_config_yaml_null_key_is_silent_and_the_warnings_are_ordered() {
    let config_dir = tempdir().expect("config tempdir");
    let config_file = config_dir.path().join("config.yaml");
    fs::write(
        &config_file,
        "scriptShell: null\nstore-dir: /kebab-store\nzzzNotASettingZzz: true\nconfigDir: /elsewhere\nnodeLinker: hoisted\n",
    )
    .expect("write global config.yaml");

    let warnings = capture_warnings(|| {
        WorkspaceSettings::load_global(config_dir.path())
            .expect("load global config.yaml")
            .expect("global config.yaml is present");
    });

    assert_eq!(
        warnings,
        [
            format!(
                r#"The following settings cannot be set in the global config file ("{}") and were ignored: "nodeLinker". Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies"#,
                config_file.display(),
            ),
            format!(
                r#"The following settings in the global config file ("{}") are not recognized by this version of pnpm and were ignored: "zzzNotASettingZzz"."#,
                config_file.display(),
            ),
            format!(
                r#"The following settings cannot be set in the global config file ("{}") and were ignored: "configDir" (This is not a pnpm setting)."#,
                config_file.display(),
            ),
            format!(
                r#"The following settings in the global config file ("{}") were ignored because they are not written in camelCase: "store-dir" (use "storeDir")."#,
                config_file.display(),
            ),
        ],
    );
}

/// pnpm consults the pattern only when `mergeGitBranchLockfiles` is not
/// set outright, so an explicit `false` survives a matching branch.
#[test]
pub fn an_explicit_merge_setting_wins_over_the_branch_pattern() {
    let repo = repo_on_branch("ref: refs/heads/main\n");
    fs::write(
        repo.path().join("pnpm-workspace.yaml"),
        "mergeGitBranchLockfiles: false\nmergeGitBranchLockfilesBranchPattern:\n  - main\n",
    )
    .unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostExplicitlyNotMerging);

    let config =
        Config::new().current::<HostExplicitlyNotMerging>(repo.path()).expect("yaml is valid");
    assert!(!config.merge_git_branch_lockfiles);
}
