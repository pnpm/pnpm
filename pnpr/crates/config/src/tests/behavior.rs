use super::{
    AmazonS3ConfigKey, BackendConfig, ClientConfigKey, Config, ConfigSource, Duration, Ecosystem,
    EnvGuard, FeatureOverrides, Identity, MINIMAL_YAML, Path, PathBuf, RegistryError,
    hosted_rules_config, hosted_rules_err, listen, normalize_key_prefix, resolve_relative,
    s3_settings_for, user, write_yaml,
};

#[test]
fn resolver_block_present_but_empty_defaults_to_enabled() {
    // A bare `resolver:` parses as YAML null and `resolver: {}` as an
    // empty map; both must mean "enabled", not fail to deserialize.
    for yaml in ["resolver:\n", "resolver: {}\n"] {
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert!(config.resolver.enabled, "for {yaml:?}");
    }
}

#[test]
fn cli_disabling_all_surfaces_with_bundled_config_errors_without_panicking() {
    // No config file → the bundled branch. Disabling all surfaces via
    // CLI must surface a clean error, not panic on the bundled `expect`.
    let overrides = FeatureOverrides {
        disable_registry: true,
        disable_resolver: true,
        disable_artifacts: true,
    };
    let err = Config::resolve_with_overrides(None, None, listen(), None, overrides)
        .expect_err("all surfaces disabled must error rather than panic");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn unknown_key_in_feature_block_is_a_config_error() {
    // A typo'd `enable` (vs `enabled`) must fail loudly rather than
    // silently leaving the surface enabled.
    let yaml = "resolver:\n  enable: false\n";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("an unknown key in a feature block must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn shared_artifacts_are_an_explicit_top_level_opt_in() {
    let default = Config::from_yaml_str("", Path::new("/x"), listen(), None).unwrap();
    assert!(!default.artifacts.enabled);

    let enabled = Config::from_yaml_str(
        "resolver:\n  enabled: false\nartifacts:\n  enabled: true\n",
        Path::new("/x"),
        listen(),
        None,
    )
    .unwrap();
    assert!(!enabled.resolver.enabled);
    assert!(enabled.artifacts.enabled);
}

#[test]
fn nested_artifacts_toggle_is_rejected() {
    let error =
        Config::from_yaml_str("resolver:\n  artifacts: true\n", Path::new("/x"), listen(), None)
            .unwrap_err();

    assert!(error.to_string().contains("unknown field `artifacts`"), "{error}");
}

#[test]
fn artifact_override_is_independent_from_the_resolver_override() {
    let yaml = "resolver:\n  enabled: false\nartifacts:\n  enabled: true\n";
    let enabled = Config::from_yaml_str_with_overrides(
        yaml,
        Path::new("/x"),
        listen(),
        None,
        FeatureOverrides::default(),
    )
    .unwrap();
    assert!(enabled.artifacts.enabled);

    let error = Config::from_yaml_str_with_overrides(
        yaml,
        Path::new("/x"),
        listen(),
        None,
        FeatureOverrides { disable_artifacts: true, ..FeatureOverrides::default() },
    )
    .unwrap_err();
    assert!(error.to_string().contains("nothing to serve"), "{error}");
}

#[test]
fn nothing_to_serve_is_a_config_error() {
    // No registries (⇒ no registry surface) and the resolver disabled leaves
    // only `/-/ping` and the account endpoints — a misconfiguration.
    let yaml = "resolver:\n  enabled: false\n";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("a server with no surface enabled must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
    assert!(err.to_string().contains("nothing to serve"), "unexpected error: {err}");
}

#[test]
fn resolve_relative_passes_absolute_paths_through() {
    let absolute = PathBuf::from("/tmp/storage");
    assert_eq!(resolve_relative("/tmp/storage", Path::new("/anywhere")), absolute);
}

#[test]
fn resolve_relative_joins_relative_paths_to_base() {
    assert_eq!(
        resolve_relative("./storage", Path::new("/etc/pnpr")),
        PathBuf::from("/etc/pnpr/./storage"),
    );
}

#[test]
fn proxy_constructor_serves_fixtures_locally_and_proxies_the_rest() {
    use pnpr_registry::{ConcreteKind, Resolved};
    let config = Config::proxy(listen(), PathBuf::from("/tmp"));
    assert!(config.upstreams.contains_key("npmjs"));
    assert_eq!(config.registries.default_registry(), Some("main"));
    // The flat-root hosted org serves the registry-mock fixture scopes.
    assert_eq!(config.hosted["local"].org, "");
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Npm, "@pnpm.e2e/dep-of-pkg-with-1-dep"),
        Resolved::Concrete { registry: "local", kind: ConcreteKind::Hosted },
    );
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Npm, "create-touch-file-one-bin"),
        Resolved::Concrete { registry: "local", kind: ConcreteKind::Hosted },
    );
    // Everything else proxies to the npm upstream.
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Npm, "is-positive"),
        Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn backend_defaults_to_local_without_a_block() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(matches!(config.backend, BackendConfig::Local));
}

#[test]
fn backend_block_rejects_empty_selection() {
    let yaml = "storage: /var/lib/pnpr\nbackend: {}\n";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("an empty backend block must not fall back to local");
    assert!(
        matches!(err, RegistryError::InvalidConfig { ref reason } if reason.contains("exactly one database backend")),
        "expected an InvalidConfig naming the backend selection, got {err:?}",
    );
}

#[test]
fn backend_block_rejects_unknown_only_selection() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  sqlite:
    url: sqlite:///var/lib/pnpr/auth.db
upstreams: {}
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("an unknown backend key must not fall back to local");
    assert!(
        matches!(err, RegistryError::InvalidConfig { ref reason } if reason.contains("exactly one database backend")),
        "expected an InvalidConfig naming the backend selection, got {err:?}",
    );
}

#[test]
fn postgres_backend_block_selects_postgres_record_store() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  postgres:
    url: postgres://pnpr:secret@db.example/pnpr
    maxConnections: 12
    timeout: 5s
    startupTimeout: 2m
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Postgres(settings) => {
            assert_eq!(settings.url, "postgres://pnpr:secret@db.example/pnpr");
            assert_eq!(settings.max_connections, Some(12));
            assert_eq!(settings.timeout, Duration::from_secs(5));
            assert_eq!(settings.startup_timeout, Duration::from_mins(2));
        }
        other => panic!("expected a postgres backend, got {other:?}"),
    }
}

#[test]
fn postgresql_backend_alias_selects_postgres_record_store() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  postgresql:
    url: postgresql://pnpr:secret@db.example/pnpr
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Postgres(settings) => {
            assert_eq!(settings.url, "postgresql://pnpr:secret@db.example/pnpr");
            assert_eq!(settings.max_connections, None);
            assert_eq!(settings.timeout, super::super::SqlBackendSettings::DEFAULT_TIMEOUT);
            assert_eq!(
                settings.startup_timeout,
                super::super::SqlBackendSettings::DEFAULT_STARTUP_TIMEOUT,
            );
        }
        other => panic!("expected a postgres backend, got {other:?}"),
    }
}

#[test]
fn mysql_backend_block_selects_mysql_record_store() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  mysql:
    url: mysql://pnpr:secret@db.example/pnpr
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Mysql(settings) => {
            assert_eq!(settings.url, "mysql://pnpr:secret@db.example/pnpr");
            assert_eq!(settings.max_connections, None);
            assert_eq!(settings.timeout, super::super::SqlBackendSettings::DEFAULT_TIMEOUT);
            assert_eq!(
                settings.startup_timeout,
                super::super::SqlBackendSettings::DEFAULT_STARTUP_TIMEOUT,
            );
        }
        other => panic!("expected a mysql backend, got {other:?}"),
    }
}

#[test]
fn resolve_bundled_when_no_path_supplied() {
    let (config, source) = Config::resolve(None, None, listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Bundled);
    // The bundled config has the `npmjs` upstream + `**` route.
    assert!(config.upstreams.contains_key("npmjs"));
}

#[test]
fn resolve_default_path_when_only_default_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
    let (_, source) = Config::resolve(None, Some(&path), listen(), None).unwrap();
    assert_eq!(source, ConfigSource::DefaultPath(path));
}

#[test]
fn resolve_cli_when_only_cli_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "explicit.yml", MINIMAL_YAML);
    let (_, source) = Config::resolve(Some(&path), None, listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Cli(path));
}

#[test]
fn resolve_cli_wins_over_default_path() {
    // Both paths exist. CLI must take priority — the auto-discovered
    // path is a *fallback*, not a merge target.
    //
    // Storage paths are derived from `tmp` so they're absolute on
    // every OS (Windows needs a drive-letter prefix to satisfy
    // `Path::is_absolute()`; a Unix-style `/a` is not absolute
    // there and gets joined to the config file's parent dir).
    let tmp = tempfile::tempdir().unwrap();
    let cli_storage = tmp.path().join("from-cli");
    let default_storage = tmp.path().join("from-default");
    let cli =
        write_yaml(tmp.path(), "explicit.yml", &format!("storage: {}\n", cli_storage.display()));
    let default =
        write_yaml(tmp.path(), "default.yml", &format!("storage: {}\n", default_storage.display()));
    let (config, source) = Config::resolve(Some(&cli), Some(&default), listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Cli(cli));
    // Confirms the *content* came from the CLI file, not the default.
    assert_eq!(config.storage, cli_storage);
}

#[test]
fn resolve_public_url_override_threads_through() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
    let (config, _) =
        Config::resolve(Some(&path), None, listen(), Some("http://override.test".to_string()))
            .unwrap();
    assert_eq!(config.public_url, "http://override.test");
}

#[test]
fn resolve_bundled_branch_honors_public_url_override() {
    let (config, source) =
        Config::resolve(None, None, listen(), Some("http://from-cli.test".to_string())).unwrap();
    assert_eq!(source, ConfigSource::Bundled);
    assert_eq!(config.public_url, "http://from-cli.test");
}

#[test]
fn most_specific_key_wins_regardless_of_declaration_order() {
    // Selection is by specificity, not key order: a formatter or `yq`
    // round-trip that reorders the YAML mapping must not change which
    // access rule applies. The same two keys, both orders, same answers.
    let scope_then_catch_all =
        "      '@secret/*':\n        access: $authenticated\n      '**':\n        access: $all\n";
    let catch_all_then_scope =
        "      '**':\n        access: $all\n      '@secret/*':\n        access: $authenticated\n";
    for packages in [scope_then_catch_all, catch_all_then_scope] {
        let config = hosted_rules_config(packages);
        let rules = &config.hosted["local"].rules;
        assert!(!rules.for_package("@secret/x").access.allows(&Identity::Anonymous), "{packages}");
        assert!(rules.for_package("anything").access.allows(&Identity::Anonymous), "{packages}");
    }
}

#[test]
fn empty_and_null_map_values_mean_default_rules() {
    for value in ["{}", "", "~"] {
        let config = hosted_rules_config(&format!("      'lodash': {value}\n"));
        let rules = &config.hosted["local"].rules;
        let effective = rules.for_package("lodash");
        assert!(effective.access.allows(&Identity::Anonymous), "value {value:?}");
        assert!(!effective.publish.allows(&Identity::Anonymous), "value {value:?}");
        assert!(effective.publish.allows(&user("alice")), "value {value:?}");
        assert!(!effective.unpublish.allows(&user("alice")), "value {value:?}");
    }
}

#[test]
fn rule_missing_unpublish_denies_destructive_writes() {
    let config = hosted_rules_config("      '@team/*':\n        publish: alice\n");
    let team = config.hosted["local"].rules.for_package("@team/x");
    assert!(team.publish.allows(&user("alice")));
    assert!(!team.publish.allows(&user("bob")));
    assert!(!team.unpublish.allows(&user("alice")));
    assert!(!team.unpublish.allows(&user("bob")));
}

#[test]
fn rule_empty_unpublish_denies_destructive_writes() {
    let as_null = "      '@team/*':\n        publish: $authenticated\n        unpublish:\n";
    let as_empty_sequence =
        "      '@team/*':\n        publish: $authenticated\n        unpublish: []\n";
    for packages in [as_null, as_empty_sequence] {
        let config = hosted_rules_config(packages);
        let team = config.hosted["local"].rules.for_package("@team/x");
        assert!(team.publish.allows(&user("alice")), "{packages}");
        assert!(!team.unpublish.allows(&Identity::Anonymous), "{packages}");
        assert!(!team.unpublish.allows(&user("alice")), "{packages}");
    }
}

#[test]
fn rule_empty_string_value_is_a_config_error() {
    // `''` is neither a token nor an unambiguous empty list; the error
    // names both spellings the author could have meant.
    let err = hosted_rules_err("      '@team/*':\n        unpublish: ''\n");
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains("`unpublish`") && reason.contains("use `[]`"),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn rule_anonymous_token_is_wired() {
    let config = hosted_rules_config("      '@anon/*':\n        access: $anonymous\n");
    let anon = config.hosted["local"].rules.for_package("@anon/x");
    assert!(anon.access.allows(&Identity::Anonymous));
    assert!(!anon.access.allows(&user("alice")));
}

#[test]
fn rule_alias_spellings_of_builtins_are_config_errors() {
    // Verdaccio also accepted `@`-prefixed and bare spellings of the
    // built-in groups. Treating them as user/group names would silently
    // flip `access: all` from world-readable to deny-everyone, so they
    // are rejected with the `$` spelling instead.
    for (token, suggestion) in [
        ("all", "$all"),
        ("'@all'", "$all"),
        ("authenticated", "$authenticated"),
        ("'@authenticated'", "$authenticated"),
        ("anonymous", "$anonymous"),
        ("'@anonymous'", "$anonymous"),
    ] {
        let err = hosted_rules_err(&format!("      '@team/*':\n        access: {token}\n"));
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason }
                    if reason.contains("did you mean") && reason.contains(suggestion),
            ),
            "unexpected error for {token:?}: {err}",
        );
    }
}

#[test]
fn rule_unknown_builtin_token_is_a_config_error() {
    // The `$` namespace is reserved for the built-in groups, so a typo'd
    // built-in cannot silently become a name that admits nobody.
    let err = hosted_rules_err("      '@team/*':\n        access: $team\n");
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains(r#"unknown built-in access token "$team""#),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn typed_token_grammar_is_validated() {
    // Only `team:` exists as a token type; `group:` gets a pointer at it,
    // and an empty team reference is named as such.
    for (token, needle) in [
        ("group:platform", r#"did you mean "team:platform""#),
        ("org:corp", "the only typed token is `team:<name>`"),
        ("'team:'", "names no team"),
        ("team:a:b", "a team name cannot contain `:`"),
    ] {
        let err = hosted_rules_err(&format!("      '@team/*':\n        access: {token}\n"));
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason } if reason.contains(needle),
            ),
            "unexpected error for {token:?}: {err}",
        );
    }
}

#[test]
fn top_level_groups_block_is_a_startup_error() {
    // The removed global block must not be silently dropped: its group
    // names used to grant access, so a stale config must be migrated to
    // per-registry `teams:`, not booted with silently changed grants.
    for stub in ["groups:\n  platform: [alice]\n", "groups:\n", "groups: {}\n"] {
        let yaml = format!("storage: ./s\nregistries:\n  local:\n    type: hosted\n{stub}");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a present top-level groups: key must be rejected");
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason }
                    if reason.contains("top-level `groups:`")
                        && reason.contains("registries.<name>.teams"),
            ),
            "unexpected error for {stub:?}: {err}",
        );
    }
}

#[test]
fn top_level_packages_block_is_a_startup_error() {
    // The removed global ACL must not be silently dropped like an unknown
    // verdaccio key: it used to *enforce* access, so ignoring it would
    // quietly open previously-gated packages on upgrade.
    let yaml = "\
storage: ./s
registries:
  local:
    type: hosted
packages:
  '@secret/*':
    access: $authenticated
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains("top-level `packages:`")
                    && reason.contains("registries.<name>.packages"),
        ),
        "unexpected error: {err}",
    );

    // A *bare* `packages:` (YAML null) and an empty block are the same
    // removed key and must be rejected identically — a plain `Option`
    // would map null to "absent" and let it slip through.
    for stub in ["packages:\n", "packages: {}\n", "packages: ~\n"] {
        let yaml = format!("storage: ./s\nregistries:\n  local:\n    type: hosted\n{stub}");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a present top-level packages: key must be rejected");
        assert!(
            matches!(
                &err,
                RegistryError::InvalidConfig { reason } if reason.contains("top-level `packages:`"),
            ),
            "unexpected error for {stub:?}: {err}",
        );
    }
}

#[test]
fn bundled_default_config_enforces_its_protections() {
    // The bundled YAML is the only place the registry-mock protections are
    // declared, so building from it must yield every one of them.
    let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
    let rules = &config.hosted["local"].rules;
    // The exact needs-auth key wins over the '@pnpm.e2e/*' scope key by
    // specificity (both are declared, in either order).
    let needs_auth = rules.for_package("@pnpm.e2e/needs-auth");
    assert!(!needs_auth.access.allows(&Identity::Anonymous));
    assert!(needs_auth.access.allows(&user("alice")));
    assert!(!rules.for_package("@private/foo").access.allows(&Identity::Anonymous));
    let public = rules.for_package("@pnpm.e2e/no-deps");
    assert!(public.access.allows(&Identity::Anonymous));
    assert!(!public.publish.allows(&Identity::Anonymous));
    // The registry-mock contract: any authenticated user may unpublish.
    assert!(public.unpublish.allows(&user("alice")));
    assert!(!public.unpublish.allows(&Identity::Anonymous));
    // `lodash` is not local: it is unclaimed by the hosted registry and
    // resolves to the npmjs catch-all through the router.
    use pnpr_registry::{ConcreteKind, Resolved};
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Npm, "lodash"),
        Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
    );
}

#[test]
fn route_policy_defaults_when_absent() {
    let config = Config::from_yaml_str("{}", Path::new("/x"), listen(), None).unwrap();
    assert!(config.route_policy.public.is_empty());
}

/// `AmazonS3Builder::from_env` imports `AWS_ENDPOINT_URL_S3` into the
/// S3-specific key, which `object_store` resolves ahead of the plain `endpoint`
/// whatever order they were set in. Writing the configured endpoint to the
/// plain key would let that environment variable silently redirect every
/// request, so the YAML must land on the S3-specific key.
#[test]
fn a_configured_endpoint_lands_on_the_key_that_wins() {
    let builder = crate::s3::s3_builder(&s3_settings_for(Some("https://minio.corp.example"), None));
    assert_eq!(
        builder.get_config_value(&AmazonS3ConfigKey::S3Endpoint).as_deref(),
        Some("https://minio.corp.example"),
    );
}

/// `from_env` also honours `AWS_ALLOW_HTTP`. The `allowHttp` field documents a
/// HTTPS-only default, so an absent setting has to say `false` rather than
/// leave the environment free to downgrade the connection to plaintext. The
/// variable is set here because that is the only state in which the two
/// behaviours differ.
#[test]
fn an_absent_allow_http_pins_https_only() {
    let _env = EnvGuard::snapshot(["AWS_ALLOW_HTTP"]);
    // SAFETY: `EnvGuard` holds the process-wide env-mutation lock for the
    // lifetime of `_env` and restores the variable on drop.
    unsafe { std::env::set_var("AWS_ALLOW_HTTP", "true") };

    let builder = crate::s3::s3_builder(&s3_settings_for(None, None));
    assert_eq!(
        builder.get_config_value(&AmazonS3ConfigKey::Client(ClientConfigKey::AllowHttp)).as_deref(),
        Some("false"),
    );
}

#[test]
fn an_explicit_allow_http_is_honoured() {
    let builder = crate::s3::s3_builder(&s3_settings_for(None, Some(true)));
    assert_eq!(
        builder.get_config_value(&AmazonS3ConfigKey::Client(ClientConfigKey::AllowHttp)).as_deref(),
        Some("true"),
    );
}

/// Every prefix reaching a store goes through this, including the one an
/// embedder hands to `HostedStoreConfig::ObjectStore`. A raw `packages` must
/// come back `/`-terminated: the store concatenates it onto the package name,
/// so without one it would key `packagesfoo/...`.
#[test]
fn a_key_prefix_is_normalized_to_empty_or_slash_terminated() {
    assert_eq!(normalize_key_prefix(Some("packages")), "packages/");
    assert_eq!(normalize_key_prefix(Some("/packages/")), "packages/");
    assert_eq!(normalize_key_prefix(Some("  packages  ")), "packages/");

    assert_eq!(normalize_key_prefix(None), "");
    assert_eq!(normalize_key_prefix(Some("")), "");
    assert_eq!(normalize_key_prefix(Some("   ")), "");
    assert_eq!(normalize_key_prefix(Some("/")), "");
}

#[test]
fn rejects_package_keys_that_normalize_to_the_same_name() {
    for (ecosystem, first, second) in [("cargo", "Demo", "demo"), ("pypi", "Foo.Bar", "foo-bar")] {
        let yaml = format!(
            "registries:\n  hosted:\n    type: hosted\n    ecosystem: {ecosystem}\n    packages:\n      {first}: {{ access: '$authenticated' }}\n      {second}: {{ access: '$all' }}\n",
        );
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(err.to_string().contains("duplicates normalized key"), "{err}");
    }
}

#[test]
fn oci_limits_are_positive_and_preserve_defaults() {
    let config = Config::from_yaml_str(
        "oci: {maxBlobBytes: 123, maxManifestBytes: 456, bearerAuth: true}",
        Path::new("/config"),
        listen(),
        None,
    )
    .unwrap();
    assert_eq!(config.oci.max_blob_bytes, 123);
    assert_eq!(config.oci.max_manifest_bytes, 456);
    assert!(config.oci.bearer_auth);
    let config = Config::from_yaml_str("{}", Path::new("/config"), listen(), None).unwrap();
    assert_eq!(config.oci.max_blob_bytes, 10 * 1024 * 1024 * 1024);
    assert_eq!(config.oci.max_manifest_bytes, 4 * 1024 * 1024);
    assert!(!config.oci.bearer_auth);
    for yaml in [
        "oci: {maxBlobBytes: 0}",
        "oci: {maxManifestBytes: 0}",
        "oci: {maxBlobBytes: -1}",
        "oci: {maxManifestByte: 1}",
    ] {
        assert!(
            Config::from_yaml_str(yaml, Path::new("/config"), listen(), None).is_err(),
            "{yaml}",
        );
    }
}
