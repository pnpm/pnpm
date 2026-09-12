use super::{
    AuditLevel, BTreeMap, Config, GetHomeDir, LoadWorkspaceYamlError, NodeLinker, Path, PathBuf,
    RegistryDeclaration, RegistryEntry, RegistryOptions, RegistryServerType, ResolutionMode,
    TrustPolicy, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq, fs,
};

/// `trustPolicy` accepts the two upstream string values; an absent
/// key leaves the [`TrustPolicy::Off`] default in place.
#[test]
fn trust_policy_yaml_values_round_trip() {
    let yaml = "trustPolicy: off\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::Off));

    let yaml = "trustPolicy: no-downgrade\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));

    let yaml = "";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.trust_policy.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.trust_policy, TrustPolicy::Off, "default stays off when key is absent");
}

/// `registrySupportsTimeField` is a camelCase boolean; default `false`.
#[test]
fn parses_registry_supports_time_field_from_yaml_and_applies() {
    let yaml = "registrySupportsTimeField: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.registry_supports_time_field, Some(true));

    let mut config = Config::new();
    assert!(!config.registry_supports_time_field, "the default is `false`");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.registry_supports_time_field, "yaml override wins");
}

#[test]
fn parses_update_section_from_yaml_and_applies() {
    let yaml = r#"
update:
  changeset: true
  githubActions: true
  githubActionsServer: https://github.example.com
  ignoreDeps:
    - "@pnpm.e2e/foo"
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.update_config.changeset.is_none(), "default is unset");
    assert!(config.update_config.ignore_dependencies.is_none(), "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.update_config.changeset, Some(true));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string(), "@pnpm.e2e/bar".to_string()][..]),
    );
    assert_eq!(config.update_config.github_actions, Some(true));
    assert_eq!(
        config.update_config.github_actions_server.as_deref(),
        Some("https://github.example.com"),
    );
}

#[test]
fn parses_audit_section_from_yaml_and_applies() {
    let yaml = r"
audit:
  level: high
  ignore:
    - GHSA-1
    - GHSA-2
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.audit_level, Some(AuditLevel::High));
    assert_eq!(config.audit_config.ignore_ghsas, vec!["GHSA-1".to_string(), "GHSA-2".to_string()]);
}

/// The global config file, `PNPM_CONFIG_*`, and `updateConfig` hooks reach
/// `apply_to` without the resolution step, and their `scriptShell` stays as
/// written.
#[test]
fn apply_to_copies_script_shell_verbatim() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("scriptShell: ./scripts/shell.sh").unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace/root"));
    assert_eq!(config.script_shell.as_deref(), Some("./scripts/shell.sh"));
}

/// `apply_to` keys the per-registry options by registry URL with a trailing
/// slash so a lookup by the registry a package resolved from matches.
#[test]
fn parses_registry_declarations_from_yaml_and_normalizes_the_keys() {
    let yaml = r"
registries:
  https://artifactory.example/artifactory/api/npm/npm-virtual: {serverType: artifactory}
  https://npm.example.com/: {serverType: npm}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://artifactory.example/artifactory/api/npm/npm-virtual/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Artifactory)),
    );
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://npm.example.com/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Npm)),
    );
}

/// A misspelled field would otherwise sit there doing nothing.
#[test]
fn rejects_an_unknown_registry_declaration_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: {scope: '@acme'}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("an unknown declaration field must not load")
        .to_string();
    assert!(error.contains("scope"), "the field is named: {error}");
}

#[test]
fn rejects_an_unknown_registry_server_type() {
    let yaml = r"
registries:
  https://npm.example.com/: {serverType: nexus}
";
    let received = serde_saphyr::from_str::<WorkspaceSettings>(yaml);
    assert!(received.is_err(), "an unknown serverType must not parse");
}

/// A scope resolves to one registry while a registry serves many, so the
/// declaration reads the way it is written and the lookup is its inverse. A
/// bare `@` is the scope-less default registry.
#[test]
fn routes_the_scopes_a_registry_declares() {
    let yaml = r"
registries:
  https://npm.corp.example:
    serverType: npm
    scopes: ['@', '@foo', '@bar']
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.registry, "https://npm.corp.example/");
    assert_eq!(
        config.registries_by_scope.get("@foo").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
    assert_eq!(
        config.registries_by_scope.get("@bar").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
}

#[test]
fn rejects_a_scope_declared_without_its_at_sign() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {scopes: [foo]}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scope without its @ must not load")
        .to_string();
    assert!(error.contains(r#""foo""#), "the scope is named: {error}");
}

#[test]
fn rejects_one_scope_routed_to_two_registries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {scopes: ['@foo']}\n  https://artifactory.example/: {scopes: ['@foo']}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scope routed twice must not load")
        .to_string();
    assert!(error.contains("routed to two registries"), "{error}");
}

/// Keyed by the URL as written: a named registry's URL is what a lockfile's
/// recorded tarball URLs are matched against, so normalizing it here would
/// change what an existing lockfile verifies against.
#[test]
fn reads_a_declared_prefix_as_a_named_registry() {
    let yaml = r"
registries:
  https://npm.corp.example: {prefix: work, serverType: artifactory}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.corp.example"),
    );
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://npm.corp.example/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Artifactory)),
    );
}

#[test]
fn rejects_one_prefix_declared_by_two_registries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {prefix: work}\n  https://artifactory.example/: {prefix: work}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a prefix declared twice must not load")
        .to_string();
    assert!(error.contains("declared by two registries"), "{error}");
}

/// The deprecated spelling of the same thing, so a declared prefix wins.
#[test]
fn a_declared_prefix_wins_over_named_registries() {
    let yaml = r"
namedRegistries:
  work: https://stale.example/
  other: https://other.example/
registries:
  https://npm.corp.example/: {prefix: work}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("other").map(String::as_str),
        Some("https://other.example/"),
    );
}

#[test]
fn rejects_a_registries_map_that_mixes_both_shapes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  '@acme': https://npm.example.com/\n  https://artifactory.example/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a mixed registries map must not load")
        .to_string();
    assert!(error.contains("mixes registry declarations"), "{error}");
    assert!(error.contains(r#""@acme""#), "the scope-routed entry is named: {error}");
}

/// A scope routes to a registry, so a URL in that position routes nothing and
/// would sit there inert. It is the declaration shape, half-written.
#[test]
fn rejects_a_url_keyed_registries_entry_written_as_a_string() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: artifactory\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a URL-keyed string entry must not load")
        .to_string();
    assert!(error.contains("is a string"), "{error}");
}

/// The inverse of the split a `registries` setting goes through, for the
/// request a pnpr server reads. Every route to one registry lands in its entry.
#[test]
fn rebuilds_the_declarations_from_the_lookups() {
    let mut config = Config::new();
    config.registries_by_scope = BTreeMap::from([
        ("default".to_owned(), "https://registry.npmjs.org/".to_owned()),
        ("@acme".to_owned(), "https://npm.corp.example/".to_owned()),
        ("@acme-internal".to_owned(), "https://npm.corp.example/".to_owned()),
        ("@other".to_owned(), "https://npm.other.example/".to_owned()),
    ]);
    config.registries_by_prefix =
        BTreeMap::from([("work".to_owned(), "https://npm.corp.example/".to_owned())]);
    config.registry_options_by_url = BTreeMap::from([(
        "https://npm.corp.example/".to_owned(),
        RegistryOptions {
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
        },
    )]);

    let declarations = config.registry_declarations();
    assert_eq!(
        declarations.get("https://npm.corp.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@acme".to_owned(), "@acme-internal".to_owned()]),
            prefix: Some("work".to_owned()),
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
            unknown: BTreeMap::new(),
        }),
    );
    assert_eq!(
        declarations.get("https://npm.other.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@other".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    // The default registry travels as the request's own `registry` field.
    assert!(!declarations.contains_key("https://registry.npmjs.org/"), "{declarations:?}");
}

/// The resolved view declares every route: the default registry appears as
/// the bare `@` scope — first in the scope list it shares with real scopes —
/// and the built-in `@jsr` scope and `gh` / `npmjs` prefixes are declared
/// unless the user pointed them elsewhere.
#[test]
fn resolved_declarations_declare_every_route() {
    let mut config = Config::new();
    config.registry = "https://npm.corp.example/".to_owned();
    config.registries_by_scope =
        BTreeMap::from([("@acme".to_owned(), "https://npm.corp.example/".to_owned())]);
    config.registries_by_prefix =
        BTreeMap::from([("gh".to_owned(), "https://github.corp.example/".to_owned())]);

    let declarations = config.resolved_registry_declarations();
    assert_eq!(
        declarations.get("https://npm.corp.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@".to_owned(), "@acme".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    assert_eq!(
        declarations.get("https://npm.jsr.io/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@jsr".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    // The user's `gh` route wins over the built-in of the same name.
    assert_eq!(
        declarations.get("https://github.corp.example/"),
        Some(&RegistryDeclaration {
            prefix: Some("gh".to_owned()),
            ..RegistryDeclaration::default()
        }),
    );
    assert_eq!(declarations.get("https://npm.pkg.github.com/"), None);
    assert_eq!(
        declarations.get("https://registry.npmjs.org/"),
        Some(&RegistryDeclaration {
            prefix: Some("npmjs".to_owned()),
            ..RegistryDeclaration::default()
        }),
    );
}

/// Every scope-routed lookup resolves through this map, so the built-in `@jsr`
/// route has to be in it. See <https://github.com/pnpm/pnpm/issues/14649>.
#[test]
fn resolved_registries_carry_the_builtin_jsr_route() {
    let mut config = Config::new();
    config.registry = "https://npm.corp.example/".to_owned();

    let registries = config.resolved_registries();

    assert_eq!(registries.get("default").map(String::as_str), Some("https://npm.corp.example/"));
    assert_eq!(registries.get("@jsr").map(String::as_str), Some("https://npm.jsr.io/"));
}

/// A declaration map survives the round trip through the lookups it is split
/// into, which is what makes it safe to rebuild one for a pnpr request.
#[test]
fn declarations_round_trip_through_the_lookups() {
    let yaml = r"
registries:
  https://npm.corp.example/:
    serverType: artifactory
    scopes: ['@acme']
    prefix: work
  https://npm.other.example/:
    scopes: ['@other']
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let entries = settings.registries.clone().expect("registries present");
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    let rebuilt = config.registry_declarations();
    let original: std::collections::BTreeMap<String, RegistryDeclaration> = entries
        .into_iter()
        .map(|(registry, entry)| match entry {
            RegistryEntry::Declaration(declaration) => (registry, declaration),
            RegistryEntry::ScopeRoute(_) => panic!("declarations only"),
        })
        .collect();
    assert_eq!(rebuilt, original);
}

/// The public registry omits `time` from its abbreviated metadata, so a
/// time-based resolution reads the full document from it. A registry that
/// declares otherwise answers for itself, and paying for the full document at
/// every registry because one of them needs it is the cost this removes.
#[test]
fn a_registry_declaring_the_time_field_needs_no_full_metadata() {
    let yaml = r"
resolutionMode: time-based
registries:
  https://time.example.com/: {supportsTimeField: true}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_registry("https://time.example.com/"));
    assert!(config.requires_full_metadata_for_registry("https://registry.npmjs.org/"));
    // However either side spelled the trailing slash.
    assert!(!config.requires_full_metadata_for_registry("https://time.example.com"));
}

/// A reason that holds whatever the registry serves is not undone by one.
#[test]
fn a_declared_time_field_does_not_waive_the_trust_policy() {
    let yaml = r"
resolutionMode: time-based
trustPolicy: no-downgrade
registries:
  https://time.example.com/: {supportsTimeField: true}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(config.requires_full_metadata_for_registry("https://time.example.com/"));
}

/// Nothing is filtered when no registry can need full metadata.
#[test]
fn no_filtered_mirror_without_a_reason_for_full_metadata() {
    let mut config = Config::new();
    assert!(!config.requires_filtered_full_metadata());
    config.resolution_mode = ResolutionMode::TimeBased;
    assert!(config.requires_filtered_full_metadata());
}

/// The scan that decides whether the file is worth re-reading must never
/// answer "nothing here" for a key there is something to say about, so a
/// top-level key written in a shape it cannot classify still gets collected.
#[test]
fn load_at_collects_issues_from_a_key_it_cannot_scan() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), "{zzzNotASettingZzz: 1}\n").unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}

/// A root mapping may itself be indented, and the whole file is then more
/// indented than column zero without a single key being nested.
#[test]
fn load_at_collects_issues_from_an_indented_root_mapping() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "  zzzNotASettingZzz: 1\n  nodeLinker: hoisted\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}

#[test]
fn parses_a_valid_tasks_section_and_applies_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        concat!(
            "packages:\n  - packages/*\n",
            "tasks:\n",
            "  build:\n    concurrency: 2\n    dependsOn: ['^build']\n",
            "  test:\n    dependsOn: ['build']\n",
            "  lint: {}\n",
        ),
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");
    assert!(settings.key_issues.is_empty());

    let mut config = Config::default();
    settings.apply_to(&mut config, dir.path());
    assert_eq!(
        config.tasks.get("build").unwrap().depends_on.as_deref(),
        Some(&["^build".to_string()][..]),
    );
    assert_eq!(config.tasks.get("build").unwrap().concurrency, Some(2));
    assert_eq!(
        config.tasks.get("test").unwrap().depends_on.as_deref(),
        Some(&["build".to_string()][..]),
    );
    // `lint: {}` declares an explicitly empty dependency list — a different
    // statement from omitting the entry.
    assert_eq!(config.tasks.get("lint").unwrap().depends_on, None);
}

#[test]
fn rejects_a_depends_on_entry_with_no_task_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    dependsOn: ['^']\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::EmptyTaskDependsOnEntry { ref task, ref entry }
            if task == "build" && entry == "^"
    ));
}

#[test]
fn rejects_zero_task_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: 0\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "0"
    ));
}

#[test]
fn rejects_negative_task_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: -1\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "-1"
    ));
}

/// The odd suffixes pnpm's `path.join(homedir, rest)` swallows: a doubled
/// separator must not turn the value absolute, and a parent segment must
/// collapse rather than survive into the resolved path.
#[test]
fn expanding_a_home_prefix_joins_the_way_pnpm_does() {
    struct FakeHome;
    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/example"))
        }
    }

    // Compared as paths, not strings: the join separator is `\\` on Windows.
    let home = PathBuf::from("/home/example");
    for (configured, expected) in [
        ("~/bin", home.join("bin")),
        ("~//bin", home.join("bin")),
        ("~/../bin", PathBuf::from("/home").join("bin")),
        ("~/nested/../bin", home.join("bin")),
    ] {
        let mut settings = WorkspaceSettings {
            global_dir: Some(configured.to_string()),
            global_bin_dir: Some(configured.to_string()),
            ..WorkspaceSettings::default()
        };
        settings.expand_global_dir_home_prefixes::<FakeHome>();
        let expected = Some(expected.as_path());
        assert_eq!(
            settings.global_dir.as_deref().map(Path::new),
            expected,
            "globalDir {configured}",
        );
        assert_eq!(
            settings.global_bin_dir.as_deref().map(Path::new),
            expected,
            "globalBinDir {configured}",
        );
    }
}

/// A tilde that names no home-relative path is an ordinary value: pnpm's
/// `/^~[/\\]/` does not match it either.
#[test]
fn a_tilde_without_a_separator_is_left_alone() {
    struct FakeHome;
    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/example"))
        }
    }

    let mut settings = WorkspaceSettings {
        global_dir: Some("~backup/global".to_string()),
        global_bin_dir: Some("bin/~/nested".to_string()),
        ..WorkspaceSettings::default()
    };
    settings.expand_global_dir_home_prefixes::<FakeHome>();
    assert_eq!(settings.global_dir.as_deref(), Some("~backup/global"));
    assert_eq!(settings.global_bin_dir.as_deref(), Some("bin/~/nested"));
}

/// The settings that report as the user set them are outside this property
/// by design, since an unset one reports nothing to apply; see
/// [`from_resolved_leaves_explicitness_sensitive_settings_unset`].
#[test]
fn from_resolved_round_trips_through_apply_to() {
    let original = Config {
        node_linker: NodeLinker::Hoisted,
        registry: "https://reg.example/".to_string(),
        save_exact: true,
        fetch_retries: 9,
        user_agent: "pnpm/test".to_string(),
        ..Config::default()
    };

    let mut applied = Config::default();
    WorkspaceSettings::from_resolved(&original).apply_to(&mut applied, Path::new("/tmp/project"));

    assert_eq!(applied.node_linker, original.node_linker);
    assert_eq!(applied.registry, original.registry);
    assert_eq!(applied.save_exact, original.save_exact);
    assert_eq!(applied.fetch_retries, original.fetch_retries);
    assert_eq!(applied.user_agent, original.user_agent);
}
