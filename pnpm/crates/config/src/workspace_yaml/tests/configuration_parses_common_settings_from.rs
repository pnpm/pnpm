use super::{
    AuditLevel, BTreeMap, CASE, ColorMode, Config, ConfigDependency, ConfigDependencyDetail,
    EnvVar, LoadWorkspaceYamlError, NodeLinker, NodePackageMapType, Path, StoreDir, TrustPolicy,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs,
};

#[test]
fn parses_common_settings_from_yaml() {
    let yaml = r"
storeDir: ../my-store
registry: https://reg.example
lockfile: false
autoInstallPeers: true
dedupePeers: true
preferWorkspacePackages: true
nodeLinker: hoisted
nodeExperimentalPackageMap: true
nodePackageMapType: loose
packages:
  - packages/*
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.store_dir.as_deref(), Some("../my-store"));
    assert_eq!(settings.registry.as_deref(), Some("https://reg.example"));
    assert_eq!(settings.lockfile, Some(false));
    assert_eq!(settings.auto_install_peers, Some(true));
    assert_eq!(settings.dedupe_peers, Some(true));
    assert_eq!(settings.prefer_workspace_packages, Some(true));
    assert!(matches!(settings.node_linker, Some(NodeLinker::Hoisted)));
    assert_eq!(settings.node_experimental_package_map, Some(true));
    assert_eq!(settings.node_package_map_type, Some(NodePackageMapType::Loose));
}

#[test]
fn parity_settings_parse_and_apply() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
bail: false
color: never
embedReadme: true
ignoreWorkspaceRootCheck: true
optional: false
packageLock: false
pending: true
recursiveInstall: false
reverse: true
shellEmulator: true
skipManifestObfuscation: true
sort: false
useBetaCli: true
",
    )
    .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.bail);
    assert_eq!(config.color, ColorMode::Never);
    assert!(config.embed_readme);
    assert!(config.ignore_workspace_root_check);
    assert!(!config.optional);
    assert!(!config.package_lock);
    assert!(config.pending);
    assert!(!config.recursive_install);
    assert!(config.reverse);
    assert!(config.shell_emulator);
    assert!(config.skip_manifest_obfuscation);
    assert!(!config.sort);
    assert!(config.use_beta_cli);
}

#[test]
fn parity_settings_follow_global_config_key_routing() {
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
bail: false
color: never
embedReadme: true
ignoreWorkspaceRootCheck: true
optional: false
packageLock: false
pending: true
recursiveInstall: false
reverse: true
shellEmulator: true
skipManifestObfuscation: true
sort: false
useBetaCli: true
",
    )
    .unwrap();
    settings.clear_workspace_only_fields();

    assert_eq!(settings.bail, Some(false));
    assert_eq!(settings.color, Some(ColorMode::Never));
    assert_eq!(settings.optional, Some(false));
    assert_eq!(settings.package_lock, Some(false));
    assert_eq!(settings.shell_emulator, Some(true));
    assert_eq!(settings.use_beta_cli, Some(true));
    assert_eq!(settings.embed_readme, None);
    assert_eq!(settings.ignore_workspace_root_check, None);
    assert_eq!(settings.pending, None);
    assert_eq!(settings.recursive_install, None);
    assert_eq!(settings.reverse, None);
    assert_eq!(settings.skip_manifest_obfuscation, None);
    assert_eq!(settings.sort, None);
}

#[test]
fn apply_overrides_npmrc_defaults() {
    let yaml = r"
storeDir: /absolute/store
lockfile: false
registry: https://reg.example
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    config.lockfile = true;
    let before_registry = config.registry.clone();

    settings.apply_to(&mut config, Path::new("/irrelevant-for-absolute-paths"));

    assert_eq!(config.store_dir, StoreDir::from(Path::new("/absolute/store").to_path_buf()));
    assert!(!config.lockfile);
    assert_eq!(config.registry, "https://reg.example/");
    assert_ne!(before_registry, config.registry);
}

/// pnpm reads `fetchRetries` / `fetchRetryFactor` /
/// `fetchRetryMintimeout` / `fetchRetryMaxtimeout` from
/// `pnpm-workspace.yaml` as camelCase keys (mirrors of the kebab-case
/// `.npmrc` form). Confirm both deserialization and `apply_to` push
/// the overrides onto the `Config`, since pacquet has to honour them
/// for parity with pnpm and for the install-time retry plumbing in
/// crates/tarball.
#[test]
fn parses_fetch_retry_settings_from_yaml_and_applies() {
    let yaml = r"
fetchRetries: 5
fetchRetryFactor: 3
fetchRetryMintimeout: 1000
fetchRetryMaxtimeout: 4000
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.fetch_retries, Some(5));
    assert_eq!(settings.fetch_retry_factor, Some(3));
    assert_eq!(settings.fetch_retry_mintimeout, Some(1000));
    assert_eq!(settings.fetch_retry_maxtimeout, Some(4000));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.fetch_retries, 5);
    assert_eq!(config.fetch_retry_factor, 3);
    assert_eq!(config.fetch_retry_mintimeout, 1000);
    assert_eq!(config.fetch_retry_maxtimeout, 4000);
}

#[test]
fn parses_network_settings_from_yaml_and_applies() {
    let yaml = r"
networkConcurrency: 8
fetchTimeout: 120000
fetchWarnTimeoutMs: 20000
fetchMinSpeedKiBps: 100
userAgent: my-agent/2.0
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.network_concurrency, Some(8));
    assert_eq!(settings.fetch_timeout, Some(120_000));
    assert_eq!(settings.fetch_warn_timeout_ms, Some(20_000));
    assert_eq!(settings.fetch_min_speed_ki_bps, Some(100));
    assert_eq!(settings.user_agent.as_deref(), Some("my-agent/2.0"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.network_concurrency, 8);
    assert_eq!(config.fetch_timeout, 120_000);
    assert_eq!(config.fetch_warn_timeout_ms, 20_000);
    assert_eq!(config.fetch_min_speed_ki_bps, 100);
    assert_eq!(config.user_agent, "my-agent/2.0");
}

#[test]
fn trusted_settings_expand_env_vars_inside_request_destination_values() {
    struct EnvWithHost;
    impl EnvVar for EnvWithHost {
        fn var(name: &str) -> Option<String> {
            (name == "WORK_HOST").then(|| "internal.example.com".to_owned())
        }
    }

    let yaml = r"
pnprServer: https://${WORK_HOST}/pnpr/
registry: https://${WORK_HOST}/npm/
httpsProxy: http://${WORK_HOST}:8080/
httpProxy: http://${WORK_HOST}:8081/
noProxy: ${WORK_HOST}
namedRegistries:
  stable: https://registry.example.com/npm/
  work: https://${WORK_HOST}/work/
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_trusted::<EnvWithHost>();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.pnpr_server.as_deref(), Some("https://internal.example.com/pnpr/"));
    assert_eq!(config.registry, "https://internal.example.com/npm/");
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://internal.example.com:8080/"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://internal.example.com:8081/"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec!["internal.example.com".to_string()])),
    );
    assert_eq!(
        config.registries_by_prefix.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://internal.example.com/work/"),
    );
}

/// `configDependencies` is a map of package name → version-with-integrity
/// spec. pacquet records it into the workspace-state file so pnpm's
/// `checkDepsStatus` doesn't treat the install as stale on the next
/// `pnpm run` / `pnpm node`. Guards the camelCase rename, optionality,
/// and `apply_to` wiring.
#[test]
fn parses_config_dependencies_from_yaml_and_applies() {
    let yaml = r#"
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let expected = settings.config_dependencies.clone();
    assert_eq!(
        expected.as_ref().and_then(|m| m.get("@pnpm/pacquet")),
        Some(&ConfigDependency::VersionWithIntegrity("0.2.2-14".to_string())),
    );

    let mut config = Config::new();
    assert!(config.config_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.config_dependencies, expected);
}

/// pnpm's `configDependencies` value can also be the `{ tarball?, integrity }`
/// object form. It must parse (not error) and round-trip, otherwise an
/// upstream-supported manifest becomes a hard config-load failure.
#[test]
fn parses_object_form_config_dependencies() {
    let yaml = r#"
configDependencies:
  "@scope/dep":
    integrity: sha512-abc
    tarball: https://example.test/dep.tgz
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.config_dependencies.expect("field present");
    assert_eq!(
        map.get("@scope/dep"),
        Some(&ConfigDependency::Detailed(ConfigDependencyDetail {
            integrity: "sha512-abc".to_string(),
            tarball: Some("https://example.test/dep.tgz".to_string()),
        })),
    );
}

#[test]
fn cargo_settings_reject_unknown_fields() {
    let error = serde_saphyr::from_str::<WorkspaceSettings>(
        r"
cargo:
  enabled: true
  registry: https://example.com
",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unknown field `registry`"), "{error}");
}

/// The variables are named for the setting they configure, and the names that
/// matched the older spelling keep working — a machine set up before the rename
/// is not something a `pnpm install` should start ignoring.
///
/// Every suffix is covered because they do not share a parsing path: `PUBLISH`
/// is a boolean, `BUILD_ENV` and `TRUSTED_KEYS` are JSON, the rest are strings.
#[test]
fn the_remote_tier_reads_both_environment_spellings() {
    const CANONICAL: &str = "PNPM_SIDE_EFFECTS_CACHE_REMOTE_";
    const OLDER: &str = "PNPM_REMOTE_SIDE_EFFECTS_CACHE_";

    fn read(prefixes: &[&'static str], suffix: &'static str, value: &'static str) -> Config {
        // A thread-local rather than a generic per case: `EnvVar` is a trait
        // with an associated function, so the case has to reach it somehow.
        CASE.with(|case| *case.borrow_mut() = Some((prefixes.to_vec(), suffix, value)));
        struct Env;
        impl crate::EnvVar for Env {
            fn var(key: &str) -> Option<String> {
                CASE.with(|case| {
                    let borrowed = case.borrow();
                    let (prefixes, suffix, value) = borrowed.as_ref()?;
                    prefixes
                        .iter()
                        .any(|prefix| key == format!("{prefix}{suffix}"))
                        .then(|| (*value).to_string())
                })
            }
        }
        let mut config = Config::new();
        config.apply_remote_side_effects_cache_env::<Env>();
        config
    }

    for suffix in ["KEY_ID", "BUILDER_ID", "IMAGE_DIGEST", "ARCHITECTURE_BASELINE", "PRIVATE_KEY"] {
        for prefixes in [vec![CANONICAL], vec![OLDER], vec![CANONICAL, OLDER]] {
            let config = read(&prefixes, suffix, "value");
            let shared = config.remote_side_effects_cache.expect("shared cache config");
            let read_back = match suffix {
                "KEY_ID" => shared.key_id,
                "BUILDER_ID" => shared.builder_id,
                "IMAGE_DIGEST" => shared.image_digest,
                "ARCHITECTURE_BASELINE" => shared.architecture_baseline,
                _ => shared.private_key,
            };
            assert_eq!(read_back.as_deref(), Some("value"), "{suffix} under {prefixes:?}");
        }
    }

    for prefixes in [vec![CANONICAL], vec![OLDER], vec![CANONICAL, OLDER]] {
        let config = read(&prefixes, "PUBLISH", "true");
        assert_eq!(
            config.remote_side_effects_cache.expect("shared cache config").publish,
            Some(true),
            "PUBLISH under {prefixes:?}",
        );

        let config = read(&prefixes, "BUILD_ENV", r#"{"CC":"clang"}"#);
        let shared = config.remote_side_effects_cache.expect("shared cache config");
        assert_eq!(
            shared.build_env.expect("build env").get("CC").map(String::as_str),
            Some("clang"),
            "BUILD_ENV under {prefixes:?}",
        );

        let config = read(&prefixes, "TRUSTED_KEYS", r#"{"acme-2026":"AA=="}"#);
        let shared = config.remote_side_effects_cache.expect("shared cache config");
        assert_eq!(
            shared.trusted_keys.expect("trusted keys").get("acme-2026").map(String::as_str),
            Some("AA=="),
            "TRUSTED_KEYS under {prefixes:?}",
        );
    }
}

/// A malformed JSON variable is reported by name, so it has to be the name the
/// reader actually set — pointing at the spelling they did not use sends them
/// looking for a variable that is not in their environment.
#[test]
fn malformed_json_names_the_environment_variable_that_was_set() {
    struct Older;
    impl crate::EnvVar for Older {
        fn var(key: &str) -> Option<String> {
            (key == "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS").then(|| "not json".to_string())
        }
    }

    let warnings = crate::tests::capture_warnings(|| {
        let mut config = Config::new();
        config.apply_remote_side_effects_cache_env::<Older>();
    });

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("not a string-valued JSON object"))
        .expect("a warning about the malformed variable");
    assert!(
        warning.contains("PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS"),
        "expected the variable that was set, got {warning}",
    );
}

/// When both spellings are set, the one matching the setting decides.
#[test]
fn the_canonical_environment_spelling_wins() {
    struct Env;
    impl crate::EnvVar for Env {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID" => Some("canonical".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_KEY_ID" => Some("older".to_string()),
                _ => None,
            }
        }
    }

    let mut config = Config::new();
    config.apply_remote_side_effects_cache_env::<Env>();
    assert_eq!(
        config.remote_side_effects_cache.expect("shared cache config").key_id.as_deref(),
        Some("canonical"),
    );
}

/// The environment holds the signing material a CI runner must not commit, so
/// it is the last word on the section.
#[test]
fn remote_side_effects_cache_environment_overrides_the_files() {
    struct Env;
    impl crate::EnvVar for Env {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_PUBLISH" => Some("true".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_KEY_ID" => Some("acme-2026".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS" => {
                    Some(r#"{"acme-2026":"AA=="}"#.to_string())
                }
                _ => None,
            }
        }
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
  keyId: from-the-file
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));
    config.apply_remote_side_effects_cache_env::<Env>();

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.publish, Some(true));
    assert_eq!(shared.key_id.as_deref(), Some("acme-2026"));
    assert_eq!(shared.trusted_keys.expect("trusted keys").get("acme-2026").unwrap(), "AA==");
}

/// Lockfile-verification policy keys all live in `pnpm-workspace.yaml`
/// alongside the rest of the install settings. This test asserts the
/// camelCase rename + `apply_to` wiring for every new field
/// introduced by the gate: `cacheDir` (path-resolved against the
/// workspace dir), `minimumReleaseAge` / `…Exclude` / `…Strict` /
/// `…IgnoreMissingTime`, and `trustPolicy` / `…Exclude` /
/// `…IgnoreAfter`.
#[test]
fn parses_supply_chain_policy_settings_from_yaml_and_applies() {
    let yaml = r#"
cacheDir: ./.pacquet-cache
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - lodash
  - "is-*"
minimumReleaseAgeIgnoreMissingTime: true
minimumReleaseAgeStrict: true
trustLockfile: true
trustPolicy: no-downgrade
trustPolicyExclude:
  - "@scope/legacy"
trustPolicyIgnoreAfter: 525600
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.cache_dir.as_deref(), Some("./.pacquet-cache"));
    assert_eq!(settings.minimum_release_age, Some(1440));
    assert_eq!(
        settings.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert_eq!(settings.minimum_release_age_ignore_missing_time, Some(true));
    assert_eq!(settings.minimum_release_age_strict, Some(true));
    assert_eq!(settings.trust_lockfile, Some(true));
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));
    assert_eq!(settings.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(settings.trust_policy_ignore_after, Some(525_600));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/proj"));
    assert_eq!(config.cache_dir, Path::new("/proj/.pacquet-cache"));
    assert_eq!(config.minimum_release_age, Some(1440));
    assert_eq!(
        config.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert!(config.minimum_release_age_ignore_missing_time);
    assert_eq!(config.minimum_release_age_strict, Some(true));
    assert!(config.resolved_minimum_release_age_strict());
    assert!(config.trust_lockfile);
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(config.trust_policy_ignore_after, Some(525_600));
}

/// The deprecated `updateConfig.ignoreDependencies` parses from the nested
/// camelCase shape and lands on `Config.update_config`.
#[test]
fn parses_update_config_from_yaml_and_applies() {
    let yaml = r#"
updateConfig:
  ignoreDependencies:
    - "@pnpm.e2e/foo"
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.update_config.ignore_dependencies.is_none(), "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string(), "@pnpm.e2e/bar".to_string()][..]),
    );
}

#[test]
fn update_section_takes_precedence_over_update_config() {
    let yaml = r#"
update:
  ignoreDeps:
    - "@pnpm.e2e/foo"
updateConfig:
  ignoreDependencies:
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string()][..]),
        "the update section should override updateConfig",
    );
}

#[test]
fn audit_section_takes_precedence_over_audit_level_and_config() {
    let yaml = r"
audit:
  level: critical
  ignore:
    - GHSA-new
auditLevel: low
auditConfig:
  ignoreGhsas:
    - GHSA-old
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.audit_level, Some(AuditLevel::Critical));
    assert_eq!(config.audit_config.ignore_ghsas, vec!["GHSA-new".to_string()]);
}

/// The `add`-time save settings parse from `pnpm-workspace.yaml` and
/// apply onto the `Config`. `savePeer` and `saveCatalogName` are
/// workspace-only (pnpm's `excludedPnpmKeys`); `savePrefix` is an npm
/// key and stays readable from the global `config.yaml`.
#[test]
fn parses_save_settings_from_yaml_and_applies() {
    let yaml = "savePrefix: '~'\nsavePeer: true\nsaveCatalogName: shared\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.save_prefix.as_deref(), Some("~"));
    assert_eq!(settings.save_peer, Some(true));
    assert_eq!(settings.save_catalog_name.as_deref(), Some("shared"));

    let mut config = Config::new();
    assert_eq!(config.save_prefix, None, "default is unset");
    assert!(!config.save_peer, "default is false");
    assert_eq!(config.save_catalog_name, None, "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.save_prefix.as_deref(), Some("~"));
    assert!(config.save_peer);
    assert_eq!(config.save_catalog_name.as_deref(), Some("shared"));

    let mut global: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    global.clear_workspace_only_fields();
    assert_eq!(global.save_prefix.as_deref(), Some("~"));
    assert_eq!(global.save_peer, None);
    assert_eq!(global.save_catalog_name, None);
}

/// The registry URL is the key here, so the untrusted-environment gate has to
/// drop the entry rather than expanding a placeholder into a request URL. The
/// variable resolves, so a dropped entry is distinguishable from one expanded
/// to an empty string — expanding it is how a token would reach an
/// attacker-chosen host from a committed pnpm-workspace.yaml.
#[test]
fn drops_a_registry_declaration_whose_url_has_an_env_placeholder() {
    struct EnvWithToken;
    impl EnvVar for EnvWithToken {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_TEST_REGISTRY_TOKEN").then(|| "super-secret-token".to_owned())
        }
    }

    let yaml = r"
registries:
  https://evil.example.com/${PNPM_TEST_REGISTRY_TOKEN}/: {serverType: artifactory}
  https://npm.example.com/: {serverType: artifactory}
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithToken>();
    let entries = settings.registries.as_ref().expect("registries present");
    assert_eq!(entries.len(), 1);
    assert!(entries.contains_key("https://npm.example.com/"));
    assert!(
        !entries.keys().any(|registry| registry.contains("super-secret-token")),
        "the token must never be expanded into a registry URL: {entries:?}",
    );
}

#[test]
fn a_configured_jsr_route_beats_the_builtin_one() {
    let mut config = Config::new();
    config.registries_by_scope =
        BTreeMap::from([("@jsr".to_owned(), "https://jsr.corp.example/".to_owned())]);

    assert_eq!(
        config.resolved_registries().get("@jsr").map(String::as_str),
        Some("https://jsr.corp.example/"),
    );
}

/// The setting is the answer for every registry that does not describe itself.
#[test]
fn the_time_field_setting_answers_for_an_undeclared_registry() {
    let yaml = r"
resolutionMode: time-based
registrySupportsTimeField: true
registries:
  https://old.example.com/: {supportsTimeField: false}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_registry("https://registry.npmjs.org/"));
    assert!(config.requires_full_metadata_for_registry("https://old.example.com/"));
}

/// The filtered mirror is chosen once for the whole resolver, so it has to
/// cover the registry that asks for the most: one the setting exempts but a
/// declaration does not.
#[test]
fn the_filtered_mirror_covers_a_registry_the_setting_exempts() {
    let yaml = r"
resolutionMode: time-based
registrySupportsTimeField: true
registries:
  https://old.example.com/: {supportsTimeField: false}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_resolution());
    assert!(config.requires_full_metadata_for_registry("https://old.example.com/"));
    assert!(config.requires_filtered_full_metadata());
}

/// A nested key is not a setting of this file, so a catalog naming a package
/// after nothing pnpm knows is not something to report.
#[test]
fn load_at_ignores_keys_nested_under_a_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "catalog:\n  zzzNotASettingZzz: ^1\noverrides:\n  alsoNotASetting: 2\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

#[test]
fn rejects_an_unknown_task_setting_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    dependson: ['^build']\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::UnknownTaskSettingField { ref task, ref field }
            if task == "build" && field == "dependson"
    ));
}

#[test]
fn rejects_fractional_task_concurrency_as_an_invalid_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: 1.5\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "1.5"
    ));
}

#[test]
fn rejects_string_task_concurrency_as_an_invalid_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: '2'\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == r#""2""#
    ));
}
