use super::{
    Config, Ecosystem, FeatureOverrides, Identity, Path, PathBuf, RegistryError,
    hosted_rules_config, listen, user,
};

#[test]
fn registry_surface_is_derived_from_declared_registries() {
    // No registries ⇒ nothing to serve on the npm-registry surface; declaring
    // one turns the surface on. There is no YAML toggle in between.
    let config = Config::from_yaml_str("{}", Path::new("/x"), listen(), None).unwrap();
    assert!(!config.registry.enabled);
    assert!(config.resolver.enabled);

    let yaml = "
registries:
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.registry.enabled);
    assert!(config.resolver.enabled);
}

#[test]
fn static_constructor_serves_everything_from_one_hosted() {
    use pnpr_registry::{ConcreteKind, Resolved};
    let config = Config::static_serve(listen(), PathBuf::from("/tmp"));
    assert!(config.upstreams.is_empty());
    // Everything routes to the single local hosted registry, which serves the
    // flat storage root (its `org` namespace is empty).
    assert_eq!(config.hosted["local"].org, "");
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Npm, "anything"),
        Resolved::Concrete { registry: "local", kind: ConcreteKind::Hosted },
    );
}

/// A router registry routes each package to exactly one concrete source — the
/// first listed source whose declared patterns claim it — the safe
/// alternative to a multi-upstream fallback chain.
#[test]
fn from_yaml_str_router_routes_each_package_to_one_source() {
    let yaml = "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  corp:
    type: upstream
    url: https://npm.corp.example/
    access: $authenticated
    packages: { '@corp/*': {} }
  main:
    type: router
    sources: [corp, npmjs]
defaultRegistry: main
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    // Both upstream registries are exposed as upstreams for serving.
    assert!(config.upstreams.contains_key("npmjs"));
    assert!(config.upstreams.contains_key("corp"));
    // The public upstream carries no credential gate; the private one does.
    assert!(config.upstreams["npmjs"].access.is_none());
    assert!(config.upstreams["corp"].access.is_some());
    assert_eq!(config.registries.default_registry(), Some("main"));
    assert!(config.registries.is_router("main"));
    match config.registries.resolve("main", Ecosystem::Npm, "@corp/secret") {
        pnpr_registry::Resolved::Concrete { registry, .. } => assert_eq!(registry, "corp"),
        other => panic!("expected @corp/* -> corp, got {other:?}"),
    }
    match config.registries.resolve("main", Ecosystem::Npm, "lodash") {
        pnpr_registry::Resolved::Concrete { registry, .. } => assert_eq!(registry, "npmjs"),
        other => panic!("expected lodash -> npmjs, got {other:?}"),
    }
}

/// A misordered router (the pattern-less catch-all listed before a narrower
/// private source) fails config load rather than silently serving a private
/// scope from the public source.
#[test]
fn from_yaml_str_rejects_misordered_router() {
    let yaml = "\
storage: ./s
registries:
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
  acme: { type: hosted, org: acme, packages: { '@acme/*': {} } }
  main:
    type: router
    sources: [npmjs, acme]
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("misordered router must be rejected");
    assert!(err.to_string().contains("unreachable"), "unexpected error: {err}");
}

/// An unsupported wildcard key in a registry's `packages:` fails config load, named
/// for the offending registry, rather than becoming a claim that never matches.
#[test]
fn from_yaml_str_rejects_invalid_registry_pattern() {
    let yaml = "\
storage: ./s
registries:
  acme: { type: hosted, org: acme, packages: { '@acme/ba*r': {} } }
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("an unsupported registry pattern must be rejected");
    let message = err.to_string();
    assert!(message.contains("acme"), "expected the registry named, got: {message}");
    assert!(message.contains("@acme/ba*r"), "expected the pattern named, got: {message}");
}

/// A duplicate key within one registry's `packages:` map fails config load —
/// the one within-registry error (selection is by specificity, so nothing
/// else about the map can be a defect).
#[test]
fn from_yaml_str_rejects_duplicate_registry_pattern() {
    let yaml = "\
storage: ./s
registries:
  acme: { type: hosted, org: acme, packages: { '@acme/*': {}, '@acme/*': {} } }
";
    Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("a duplicate packages key must be rejected");
}

/// `defaultRegistry` naming an undefined registry fails closed.
#[test]
fn from_yaml_str_rejects_undefined_default_registry() {
    let yaml = "\
storage: ./s
registries:
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
defaultRegistry: ghost
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("undefined default target must be rejected");
    assert!(err.to_string().contains("defaultRegistry"), "unexpected error: {err}");
}

/// Two hosted registries sharing an `org` namespace would alias the same storage, so
/// the collision must be rejected at load.
#[test]
fn from_yaml_str_rejects_duplicate_hosted_org() {
    let yaml = "\
storage: ./s
registries:
  acme:
    type: hosted
    org: shared
  acme-mirror:
    type: hosted
    org: shared
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("two hosted registries on the same org must be rejected");
    assert!(err.to_string().contains("reuses the `org`"), "unexpected error: {err}");
}

/// A dot-prefixed `org` would alias a reserved dot-directory inside the
/// storage root — most dangerously `.pnpr-cache`, putting authoritative
/// packages under the path operators are told is safe to wipe.
#[test]
fn from_yaml_str_rejects_dot_prefixed_hosted_org() {
    for org in [".pnpr-cache", ".pnpr-journal", ".hidden"] {
        let yaml =
            format!("storage: ./s\nregistries:\n  sneaky:\n    type: hosted\n    org: {org}\n");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a dot-prefixed hosted org must be rejected");
        assert!(err.to_string().contains("path-safe"), "unexpected error for {org:?}: {err}");
    }
}

/// A registry name is addressed as the single URL path segment `/~<name>/` and is
/// embedded in rewritten tarball URLs, so a name that cannot survive that
/// round trip (separators, traversal, URL delimiters, whitespace) must fail at
/// load instead of becoming an unreachable or URL-ambiguous registry.
#[test]
fn from_yaml_str_rejects_url_unsafe_registry_names() {
    for name in ["'a/b'", "'..'", "'.hidden'", "'a b'", "'a%2Fb'", "'a?b'", "'a#b'", "'C:d'"] {
        let yaml = format!("storage: ./s\nregistries:\n  {name}:\n    type: hosted\n");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("a URL-unsafe registry name must be rejected");
        assert!(
            err.to_string().contains("URL-safe path segment"),
            "unexpected error for {name}: {err}",
        );
    }
}

/// `--disable-registry` skips upstream-credential resolution but still
/// validates the registry graph, so a misconfigured router fails startup on a
/// resolver-only tier too instead of surfacing only when the registry is
/// re-enabled.
#[test]
fn cli_disable_registry_still_validates_the_registry_graph() {
    let yaml = "\
storage: ./s
registries:
  main:
    type: router
    sources: [ghost]
";
    let overrides = FeatureOverrides {
        disable_registry: true,
        disable_resolver: false,
        disable_artifacts: false,
    };
    let err =
        Config::from_yaml_str_with_overrides(yaml, Path::new("/x"), listen(), None, overrides)
            .expect_err("a broken registry graph must fail even with the registry disabled");
    assert!(err.to_string().contains("ghost"), "unexpected error: {err}");
}

/// The internally-tagged registry enum names the valid kinds, so a typo'd `type:`
/// fails to load rather than being silently misrouted.
#[test]
fn from_yaml_str_rejects_unknown_registry_type() {
    let yaml = "\
storage: ./s
registries:
  npmjs:
    type: mirror
    url: https://registry.npmjs.org/
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("an unknown registry `type:` must be rejected");
    let message = err.to_string();
    assert!(message.contains("hosted"), "expected the valid kinds listed, got: {message}");
    assert!(message.contains("upstream"), "expected the valid kinds listed, got: {message}");
}

#[test]
fn rules_are_derived_from_the_registry_packages_map() {
    // The `access` / `publish` tokens in each entry drive the runtime
    // rules — not a hard-coded default set.
    let config = hosted_rules_config(
        "      '@secret/*':\n        access: $authenticated\n        publish: $authenticated\n        unpublish: admin\n      '**':\n        access: $all\n        publish: $authenticated\n",
    );
    let rules = &config.hosted["local"].rules;
    let secret = rules.for_package("@secret/thing");
    assert!(!secret.access.allows(&Identity::Anonymous));
    assert!(secret.access.allows(&user("alice")));
    let public = rules.for_package("lodash");
    assert!(public.access.allows(&Identity::Anonymous));
    assert!(!public.publish.allows(&Identity::Anonymous));
    assert!(!secret.unpublish.allows(&user("alice")));
    assert!(secret.unpublish.allows(&user("admin")));
}

#[test]
fn from_yaml_str_reads_registry_ecosystems() {
    let yaml = "
registries:
  crates:
    type: hosted
    ecosystem: cargo
    org: crates
    packages:
      Demo_Crate: {}
  cratesio:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
  cargo:
    type: router
    sources: [crates, cratesio]
  internal:
    type: hosted
    ecosystem: pypi
    org: python
    packages:
      Demo_Pkg.Extra: {}
      '**': {}
  local:
    type: hosted
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let registries = &config.registries;
    assert_eq!(registries.ecosystem("crates"), Some(Ecosystem::Cargo));
    assert_eq!(registries.ecosystem("cratesio"), Some(Ecosystem::Cargo));
    assert_eq!(registries.ecosystem("cargo"), None);
    assert_eq!(registries.ecosystem("internal"), Some(Ecosystem::Pypi));
    assert_eq!(registries.ecosystem("local"), Some(Ecosystem::Npm));
    // Exact-name keys are canonicalized the way each ecosystem compares names.
    assert!(matches!(
        registries.resolve("cargo", Ecosystem::Cargo, "demo_crate"),
        pnpr_registry::Resolved::Concrete { registry: "crates", .. }
    ));
    assert!(matches!(
        registries.resolve("internal", Ecosystem::Pypi, "demo-pkg-extra"),
        pnpr_registry::Resolved::Concrete { registry: "internal", .. }
    ));
}

#[test]
fn from_yaml_str_accepts_mixed_routers_and_rejects_unknown_ecosystems_and_bad_keys() {
    let mixed = "
registries:
  crates: { type: hosted, ecosystem: cargo, org: crates }
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
  main: { type: router, sources: [crates, npmjs] }
defaultRegistry: main
";
    let config = Config::from_yaml_str(mixed, Path::new("/x"), listen(), None).unwrap();
    assert!(matches!(
        config.registries.resolve_default(Ecosystem::Cargo, "serde"),
        pnpr_registry::Resolved::Concrete { registry: "crates", .. }
    ));
    assert!(matches!(
        config.registries.resolve_default(Ecosystem::Npm, "serde"),
        pnpr_registry::Resolved::Concrete { registry: "npmjs", .. }
    ));
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Pypi, "serde"),
        pnpr_registry::Resolved::Unclaimed,
    );

    let unknown = "
registries:
  gems: { type: hosted, ecosystem: rubygems }
";
    let err = Config::from_yaml_str(unknown, Path::new("/x"), listen(), None).unwrap_err();
    assert!(matches!(err, RegistryError::InvalidConfig { .. }), "{err}");

    let bad_crate_key = "
registries:
  crates:
    type: hosted
    ecosystem: cargo
    packages:
      'not a crate': {}
";
    let err = Config::from_yaml_str(bad_crate_key, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains(r#"cargo registry "crates" `packages:` key"#), "{err}");
}

#[test]
fn non_npm_router_sources_reject_scoped_wildcard_claims() {
    for ecosystem in ["cargo", "pypi"] {
        let yaml = format!(
            r"
registries:
  hosted:
    type: hosted
    ecosystem: {ecosystem}
    packages:
      '@scope/*': {{}}
  upstream:
    type: upstream
    ecosystem: {ecosystem}
    url: https://upstream.test/
    public: true
  main:
    type: router
    sources: [hosted, upstream]
defaultRegistry: main
",
        );
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(matches!(err, RegistryError::InvalidConfig { .. }), "{err}");
        assert!(err.to_string().contains("packages:"), "{err}");
    }
}

#[test]
fn an_image_registry_claims_a_namespace_and_exact_repositories() {
    let yaml = "
registries:
  images:
    type: hosted
    ecosystem: oci
    org: images
    packages:
      'acme/*': {}
      'library/nginx': {}
defaultRegistry: images
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    for repository in ["acme/app", "acme/team/tool", "library/nginx"] {
        assert!(
            matches!(
                config.registries.resolve_default(Ecosystem::Oci, repository),
                pnpr_registry::Resolved::Concrete { registry: "images", .. }
            ),
            "{repository} should resolve to the image registry",
        );
    }
    assert_eq!(
        config.registries.resolve_default(Ecosystem::Oci, "other/app"),
        pnpr_registry::Resolved::Unclaimed,
    );
}

#[test]
fn image_registry_keys_are_normalized_and_validated() {
    let case_folded = "
registries:
  images:
    type: hosted
    ecosystem: oci
    packages:
      'ACME/*': {}
defaultRegistry: images
";
    let config = Config::from_yaml_str(case_folded, Path::new("/x"), listen(), None).unwrap();
    assert!(matches!(
        config.registries.resolve_default(Ecosystem::Oci, "acme/app"),
        pnpr_registry::Resolved::Concrete { registry: "images", .. }
    ));

    let duplicate = "
registries:
  images:
    type: hosted
    ecosystem: oci
    packages:
      'ACME/*': {}
      'acme/*': {}
";
    let err = Config::from_yaml_str(duplicate, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("duplicates normalized key"), "{err}");

    let bad_key = "
registries:
  images:
    type: hosted
    ecosystem: oci
    packages:
      'acme/app/*': {}
";
    let err = Config::from_yaml_str(bad_key, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains(r#"oci registry "images" `packages:` key"#), "{err}");
}

#[test]
fn ecosystem_groups_reject_ambiguous_or_cross_ecosystem_configuration() {
    for yaml in [
        "registries:\n  npm:\n    internal: {type: hosted, ecosystem: cargo}\n",
        "registries:\n  internal: {type: hosted}\n  npm:\n    internal: {type: hosted}\n",
        "registries:\n  npm:\n    main: {type: router, sources: [internal]}\n  cargo:\n    internal: {type: hosted}\n",
        "registries:\n  npm:\n    main: {type: router, sources: ['cargo/internal']}\n  cargo:\n    internal: {type: hosted}\n",
        "registries:\n  cargo:\n    internal: {type: hosted}\ndefaultRegistry:\n  npm: internal\n",
        "registries:\n  cargo:\n    internal: {type: hosted, ecosystem: npm}\n",
        "registries:\n  npm:\n    internal: {type: hosted}\n    internal: {type: hosted}\n",
    ] {
        let result = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None);
        assert!(result.is_err(), "must reject ambiguous or cross-ecosystem config: {yaml}");
    }
}

#[test]
fn ecosystem_default_requires_a_router_source_for_that_ecosystem() {
    for (sources, valid) in [("npm", false), ("npm, crates", true)] {
        let yaml = format!(
            "registries:\n  npm: {{type: hosted, org: npm}}\n  crates: {{type: hosted, ecosystem: cargo, org: crates}}\n  main: {{type: router, sources: [{sources}]}}\ndefaultRegistry:\n  cargo: main\n",
        );
        let result = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None);
        if valid {
            let config = result.unwrap();
            assert_eq!(
                config.registries.resolve_default(Ecosystem::Cargo, "demo"),
                pnpr_registry::Resolved::Concrete {
                    registry: "crates",
                    kind: pnpr_registry::ConcreteKind::Hosted,
                },
            );
        } else {
            let error = result.expect_err("a Cargo default must have a Cargo source");
            assert!(error.to_string().contains("cargo"), "{error}");
            assert!(error.to_string().contains("main"), "{error}");
        }
    }
}
