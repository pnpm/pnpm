use super::{Config, FakeEnv, FeatureOverrides, Path, RegistryError, auth_header, listen, user};
use crate::{
    PackageLists, RegistryGraphChanges, RegistryGraphDocument, RoutingConfig, RoutingOptions,
    RuleLists,
};
use indexmap::IndexMap;
use pnpr_registry::Ecosystem;

const RICH_DOCUMENT: &str = "
registries:
  local:
    type: hosted
    ecosystem: npm
    namespace: acme
    access: $authenticated
    tarballs: tar
    teams:
      t1: { name: platform, members: [u1, u2] }
      t2: { name: solo, members: u3 }
    packages:
      '@acme/*':
        access: [team:t1]
        publish: team:t1
        unpublish: [$owners]
      '@acme/open': {}
      '@acme/bare':
  corp:
    type: upstream
    url: https://npm.corp.example/
    auth: { type: bearer, tokenEnv: CORP_TOKEN }
    headers: { X-Trace: 'on' }
    packumentTtl: 120
    timeout: 30s
    maxPackumentSize: 10MiB
    maxFails: 3
    failTimeout: 5m
    cache: false
    search: true
    access: [$authenticated]
    packages: { '@corp/*': {} }
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
  crates:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
  main:
    type: router
    sources: [local, corp, npmjs]
    tarballs: tar
defaultRegistry: { npm: main, cargo: crates }
routes:
  public:
    - registry: https://registry.mirror.example/
    - package: '@oss/*'
      registry: https://npm.corp.example/
";

fn stored_options(registry_env: &[String]) -> RoutingOptions<'_> {
    RoutingOptions { registry_env, allowed_private_networks: Vec::new(), resolve_upstreams: true }
}

fn upstream_document(auth_and_headers: &str) -> RegistryGraphDocument {
    let yaml = format!(
        "registries:\n  corp:\n    type: upstream\n    url: https://npm.corp.example/\n    access: $authenticated\n{auth_and_headers}",
    );
    RegistryGraphDocument::from_yaml(&yaml).unwrap()
}

fn env(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn api_config(yaml: &str) -> Result<Config, RegistryError> {
    let yaml = format!("storage: ./s\n{yaml}");
    Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
}

#[test]
fn a_document_survives_serialization() {
    let document = RegistryGraphDocument::from_yaml(RICH_DOCUMENT).unwrap();
    let json = serde_json::to_value(&document).unwrap();
    let reparsed: RegistryGraphDocument = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&reparsed).unwrap(), json);
    let from_yaml = RegistryGraphDocument::from_yaml(&document.to_yaml().unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&from_yaml).unwrap(), json);
    RoutingConfig::from_document_with_env::<FakeEnv>(
        &reparsed,
        &RoutingOptions {
            registry_env: &["CORP_TOKEN".to_string()],
            allowed_private_networks: Vec::new(),
            resolve_upstreams: false,
        },
    )
    .expect("the reparsed document is still a valid graph");
}

#[test]
fn serialization_skips_defaults() {
    let document = RegistryGraphDocument::from_yaml(
        "registries:\n  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }\n",
    )
    .unwrap();
    let json = serde_json::to_value(&document).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "registries": {
                "npmjs": { "type": "upstream", "url": "https://registry.npmjs.org/", "public": true },
            },
        }),
    );
}

#[test]
fn a_document_rejects_unknown_keys() {
    for yaml in ["storage: ./s\n", "registries:\n  x: { type: router, extra: 1 }\n"] {
        RegistryGraphDocument::from_yaml(yaml).expect_err("unknown keys are refused");
    }
}

#[test]
fn a_stored_token_must_be_one_listed_reference() {
    let listed = env(&["CORP_TOKEN"]);
    let literal = upstream_document("    auth: { type: bearer, token: s3cret }\n");
    let error = literal.check_stored(&listed).expect_err("a literal token is refused");
    assert!(error.to_string().contains("auth.token"), "{error}");
    let embedded = upstream_document("    auth: { type: bearer, token: 'x${CORP_TOKEN}' }\n");
    embedded.check_stored(&listed).expect_err("a token with text around the reference is refused");
    let reference = upstream_document("    auth: { type: bearer, token: '${CORP_TOKEN}' }\n");
    reference.check_stored(&listed).unwrap();
    let error = reference
        .check_stored(&[])
        .expect_err("an unlisted variable is refused");
    assert!(error.to_string().contains("registryEnv"), "{error}");
}

#[test]
fn stored_token_env_and_headers_must_name_listed_variables() {
    let token_env = upstream_document("    auth: { type: bearer, tokenEnv: true }\n");
    token_env
        .check_stored(&env(&["CORP_TOKEN"]))
        .expect_err("`true` reads NPM_TOKEN");
    token_env
        .check_stored(&env(&["NPM_TOKEN"]))
        .unwrap();
    let header = upstream_document("    headers: { X-Api-Key: '${API_KEY}' }\n");
    header
        .check_stored(&env(&["API_KEY"]))
        .unwrap();
    header
        .check_stored(&[])
        .expect_err("an unlisted header variable is refused");
    for value in ["'key ${API_KEY}'", "'Bearer s3cret'", "''"] {
        let literal = upstream_document(&format!("    headers: {{ Authorization: {value} }}\n"));
        let error = literal
            .check_stored(&env(&["API_KEY"]))
            .expect_err("a header value holding anything but one reference is refused");
        assert!(error.to_string().contains("headers.Authorization"), "{value}: {error}");
    }
    let fallback = upstream_document("    headers: { X-Api-Key: '${API_KEY:-x}' }\n");
    let error = fallback
        .check_stored(&env(&["API_KEY"]))
        .expect_err("only `${VAR}` is accepted");
    assert!(error.to_string().contains("headers.X-Api-Key"), "{error}");
}

#[test]
fn a_stored_document_resolves_its_credentials_when_built() {
    let listed = env(&["CUSTOM_TOKEN", "MISSING_TOKEN"]);
    let document = upstream_document("    auth: { type: bearer, token: '${CUSTOM_TOKEN}' }\n");
    let routing =
        RoutingConfig::from_document_with_env::<FakeEnv>(&document, &stored_options(&listed))
            .unwrap();
    assert_eq!(auth_header(&routing.upstreams["corp"]), Some("Bearer custom-env-token"));
    assert_eq!(
        serde_json::to_value(&document).unwrap()["registries"]["corp"]["auth"]["token"],
        "${CUSTOM_TOKEN}",
        "building leaves the document's references in place",
    );

    let unset = upstream_document("    auth: { type: bearer, token: '${MISSING_TOKEN}' }\n");
    let error = RoutingConfig::from_document_with_env::<FakeEnv>(&unset, &stored_options(&listed))
        .expect_err("an unset variable fails the build");
    assert!(error.to_string().contains("MISSING_TOKEN"), "{error}");
    let validate_only = RoutingOptions { resolve_upstreams: false, ..stored_options(&listed) };
    RoutingConfig::from_document_with_env::<FakeEnv>(&unset, &validate_only)
        .expect("a tier that does not resolve upstreams does not read credentials");
}

#[test]
fn a_tier_that_reads_no_credential_still_checks_every_upstream_setting() {
    let listed = env(&["CUSTOM_TOKEN"]);
    let validate_only = RoutingOptions { resolve_upstreams: false, ..stored_options(&listed) };
    for setting in
        ["timeout: nonsense", "packumentTtl: soon", "failTimeout: x", "maxPackumentSize: big"]
    {
        let document = upstream_document(&format!(
            "    auth: {{ type: bearer, token: '${{CUSTOM_TOKEN}}' }}\n    {setting}\n",
        ));
        RoutingConfig::from_document_with_env::<FakeEnv>(&document, &validate_only)
            .expect_err(setting);
    }
}

#[test]
fn api_mode_serves_an_empty_graph_from_the_file() {
    let config = api_config("routes:\n  allowedPrivateNetworks: [10.0.0.0/8]\n").unwrap();
    assert!(config.features.registry.enabled);
    assert!(config.routing.hosted.is_empty() && config.routing.upstreams.is_empty());
    assert_eq!(config.routing.route_policy.allowed_private_networks.len(), 1);

    let overrides = FeatureOverrides { disable_registry: true, ..FeatureOverrides::default() };
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str_with_overrides(
        yaml,
        Path::new("/x"),
        Some(listen()),
        None,
        overrides,
    )
    .unwrap();
    assert!(!config.features.registry.enabled);
}

#[test]
fn api_mode_refuses_a_graph_in_the_file() {
    for (yaml, key) in [
        (
            "registries:\n  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }\n",
            "registries",
        ),
        ("defaultRegistry: main\n", "defaultRegistry"),
        ("routes:\n  public:\n    - registry: https://mirror.example/\n", "routes.public"),
    ] {
        let error = api_config(yaml).expect_err("the database holds the graph");
        assert!(
            error
                .to_string()
                .contains(&format!("`{key}`")),
            "{key}: {error}",
        );
    }
}

#[test]
fn api_mode_refuses_team_references_in_artifact_owners() {
    let yaml = "artifacts:\n  enabled: true\n  owners:\n    example:\n      access: $authenticated\n      publish: team:local:builders\n";
    let error = api_config(yaml).expect_err("teams live in the database");
    assert!(error.to_string().contains("artifacts.owners"), "{error}");
}

#[test]
fn registry_env_requires_api_mode_and_valid_names() {
    let error =
        Config::from_yaml_str("registryEnv: [CORP_TOKEN]\n", Path::new("/x"), listen(), None)
            .expect_err("registryEnv without api mode");
    assert!(error.to_string().contains("registriesManagedBy: api"), "{error}");
    let error = api_config("registryEnv: ['1BAD']\n").expect_err("not a variable name");
    assert!(error.to_string().contains("1BAD"), "{error}");
    let config = api_config("registryEnv: [CORP_TOKEN]\n").unwrap();
    assert_eq!(config.registry_env, ["CORP_TOKEN"]);
}

#[test]
fn registry_admins_are_separate_from_admins() {
    let yaml = "auth:\n  admins: [root]\n  registryAdmins: [ops]\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.identity.is_registry_admin(&user("ops")));
    assert!(!config.identity.is_admin(&user("ops")));
    assert!(!config.identity.is_registry_admin(&user("root")));
    let error = Config::from_yaml_str(
        "auth:\n  registryAdmins: ['team:x']\n",
        Path::new("/x"),
        listen(),
        None,
    )
    .expect_err("an access token is not an account ID");
    assert!(error.to_string().contains("auth.registryAdmins"), "{error}");
}

#[test]
fn team_edits_keep_rules_pointing_at_existing_teams() {
    let mut document = RegistryGraphDocument::from_yaml(RICH_DOCUMENT).unwrap();
    document.create_team("local", "t3", "docs").unwrap();
    assert!(matches!(
        document.create_team("local", "t4", "docs"),
        Err(RegistryError::AdminConflict { .. }),
    ));
    document.add_team_member("local", "docs", "u9").unwrap();
    document.add_team_member("local", "docs", "u9").unwrap();
    document.remove_team_member("local", "solo", "u3").unwrap();
    let teams = &serde_json::to_value(&document).unwrap()["registries"]["local"]["teams"];
    assert_eq!(teams["t3"]["members"], serde_json::json!(["u9"]));
    assert_eq!(teams["t2"]["members"], serde_json::json!([]));
    assert!(matches!(
        document.remove_team("local", "platform"),
        Err(RegistryError::AdminConflict { .. }),
    ));
    assert_eq!(document.remove_team("local", "docs").unwrap(), "t3");
    assert!(matches!(document.create_team("corp", "t5", "x"), Err(RegistryError::NotFound)));
    assert_eq!(document.team_id("local", "platform"), Some("t1"));
}

#[test]
fn rule_list_edits_cannot_add_claims() {
    let mut document = RegistryGraphDocument::from_yaml(RICH_DOCUMENT).unwrap();
    let mut lists = document.rule_lists("local").unwrap();
    assert_eq!(lists.access, Some(vec!["$authenticated".to_string()]));
    lists.access = None;
    lists.packages = IndexMap::from([(
        "@acme/open".to_string(),
        PackageLists { access: Some(vec!["team:t2".to_string()]), ..PackageLists::default() },
    )]);
    document.set_rule_lists("local", lists).unwrap();
    let lists = document.rule_lists("local").unwrap();
    assert_eq!(lists.access, None);
    assert_eq!(lists.packages["@acme/open"].access, Some(vec!["team:t2".to_string()]));
    assert_eq!(lists.packages["@acme/*"].publish, Some(vec!["team:t1".to_string()]));

    let undeclared = RuleLists {
        packages: IndexMap::from([("@other/*".to_string(), PackageLists::default())]),
        ..RuleLists::default()
    };
    assert!(matches!(
        document.set_rule_lists("local", undeclared),
        Err(RegistryError::BadRequest { .. }),
    ));
}

#[test]
fn changes_name_the_registries_that_differ() {
    let previous = RegistryGraphDocument::from_yaml(RICH_DOCUMENT).unwrap();
    let mut document = previous.clone();
    assert!(document.changes_since(&previous).is_empty());
    assert!(document.remove_registry("crates"));
    document
        .set_registry_entry(
            "mirror",
            serde_json::json!({ "type": "upstream", "url": "https://mirror.example/", "public": true }),
        )
        .unwrap();
    document
        .set_registry_entry("main", serde_json::json!({ "type": "router", "sources": ["local"] }))
        .unwrap();
    assert_eq!(
        document.changes_since(&previous),
        RegistryGraphChanges {
            added: vec!["mirror".to_string()],
            removed: vec!["crates".to_string()],
            changed: vec!["main".to_string()],
        },
    );
    assert!(matches!(
        document.set_registry_entry("bad", serde_json::json!({ "type": "router", "url": 1 })),
        Err(RegistryError::BadRequest { .. }),
    ));
    let storage = previous.hosted_storage("local").unwrap();
    assert_eq!((storage.namespace.as_str(), storage.ecosystem), ("acme", Ecosystem::Npm));
    assert!(previous.hosted_storage("corp").is_none());
}

#[test]
fn a_stored_url_must_not_carry_credentials() {
    for url in [
        "https://user:hunter2@npm.corp.example/",
        "https://token@npm.corp.example/",
        "https://npm.corp.example/?token=hunter2",
    ] {
        let yaml = format!(
            "registries:\n  corp:\n    type: upstream\n    url: {url}\n    access: $authenticated\n",
        );
        let document = RegistryGraphDocument::from_yaml(&yaml).unwrap();
        let error = document
            .check_stored(&[])
            .expect_err("credentials in `url` are refused");
        assert!(error.to_string().contains("`url`"), "{url}: {error}");
        assert!(!error.to_string().contains("hunter2"), "{url}: {error}");
    }
    let route = "routes:\n  public:\n    - registry: https://user:hunter2@mirror.example/\n";
    let document = RegistryGraphDocument::from_yaml(route).unwrap();
    let error = document
        .check_stored(&[])
        .expect_err("credentials in a public route are refused");
    assert!(error.to_string().contains("routes.public"), "{error}");
    assert!(error.to_string().contains("mirror.example"), "{error}");
    assert!(!error.to_string().contains("hunter2"), "{error}");
    let query_route = "routes:\n  public:\n    - registry: https://mirror.example/?token=hunter2\n";
    let error = RegistryGraphDocument::from_yaml(query_route)
        .unwrap()
        .check_stored(&[])
        .expect_err("a query string in a public route is refused");
    assert!(!error.to_string().contains("hunter2"), "{error}");
    upstream_document("")
        .check_stored(&[])
        .unwrap();
}
