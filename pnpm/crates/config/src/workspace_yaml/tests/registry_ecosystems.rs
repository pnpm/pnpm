//! The `ecosystem` a `registries` entry names, and the rules that span the
//! whole map because of it.

use super::{Config, Path, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs};
use crate::Ecosystem;

fn load(yaml: &str) -> Result<Config, String> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), yaml).unwrap();
    let settings = WorkspaceSettings::load_at(dir.path())
        .map_err(|error| error.to_string())?
        .expect("the workspace manifest was just written");
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));
    Ok(config)
}

#[test]
fn an_entry_that_names_no_ecosystem_is_an_npm_registry() {
    let config = load("registries:\n  https://npm.example.com/:\n    scopes: ['@acme']\n").unwrap();
    assert_eq!(
        config.registries_by_scope.get("@acme").map(String::as_str),
        Some("https://npm.example.com/"),
    );
    assert!(config.indexes_by_ecosystem.is_empty());
}

#[test]
fn python_indexes_are_canonicalized_without_search_priority() {
    let config = load(
        "registries:\n  https://extra.example.com/simple/:\n    ecosystem: pypi\n    packages: [alpha]\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n",
    )
    .unwrap();
    assert_eq!(
        config.python_indexes(),
        ["https://extra.example.com/simple/", "https://pypi.example.com/simple/"],
    );
    assert!(config.registries_by_scope.is_empty());
}

#[test]
fn python_index_urls_do_not_depend_on_declaration_order() {
    let config = load(
        "registries:\n  https://zzz.example.com/simple/:\n    ecosystem: pypi\n    packages: [alpha]\n  https://aaa.example.com/simple/:\n    ecosystem: pypi\n",
    )
    .unwrap();
    assert_eq!(
        config.python_indexes(),
        ["https://aaa.example.com/simple/", "https://zzz.example.com/simple/"],
    );
}

#[test]
fn a_lone_index_is_the_whole_search_order() {
    let config = load("registries:\n  https://index.example.com:\n    ecosystem: cargo\n").unwrap();
    assert_eq!(config.cargo_index_url(), "https://index.example.com/");
}

#[test]
fn an_ecosystem_no_entry_names_resolves_from_its_own_default() {
    let config = load("registries:\n  https://npm.example.com/:\n    scopes: ['@acme']\n").unwrap();
    assert_eq!(config.python_indexes(), [crate::DEFAULT_PYPI_INDEX_URL]);
    assert_eq!(config.cargo_index_url(), crate::DEFAULT_CARGO_INDEX_URL);
}

#[test]
fn a_second_cargo_index_is_refused() {
    let error = load(
        "registries:\n  https://one.example.com/:\n    ecosystem: cargo\n  https://two.example.com/:\n    ecosystem: cargo\n",
    )
    .unwrap_err();
    assert!(error.contains("Two Cargo registries"), "{error}");
}

/// Every one of these is read only for an npm registry, so an entry serving
/// another ecosystem that sets one is refused rather than silently ignored.
#[test]
fn an_index_may_not_carry_a_field_that_is_npms() {
    for (field, value) in [
        ("scopes", "['@acme']"),
        ("prefix", "work"),
        ("serverType", "artifactory"),
        ("supportsTimeField", "true"),
    ] {
        let error = load(&format!(
            "registries:\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n    {field}: {value}\n",
        ))
        .unwrap_err();
        assert!(error.contains(field), "{field}: {error}");
        assert!(error.contains("pypi"), "{field}: {error}");
    }
}

#[test]
fn an_unknown_ecosystem_is_refused() {
    let error = load("registries:\n  https://example.com/:\n    ecosystem: maven\n").unwrap_err();
    assert!(error.contains("maven") || error.contains("ecosystem"), "{error}");
}

#[test]
fn the_declared_indexes_round_trip_through_the_resolved_view() {
    let config = load(
        "registries:\n  https://extra.example.com/simple/:\n    ecosystem: pypi\n    packages: [alpha]\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n",
    )
    .unwrap();
    let declarations = config.resolved_registry_declarations();
    let indexes: Vec<&str> = declarations
        .iter()
        .filter(|(_, entry)| entry.ecosystem() == Ecosystem::Pypi)
        .map(|(registry, _)| registry.as_str())
        .collect();
    assert_eq!(indexes, ["https://extra.example.com/simple/", "https://pypi.example.com/simple/"]);
}

#[test]
fn package_routes_survive_into_the_declarations_sent_to_a_server() {
    let config = load("registries:\n  https://private.example.com/:\n    ecosystem: pypi\n    packages: [alpha]\n  https://public.example.com/:\n    ecosystem: pypi\n").unwrap();
    for declarations in [config.registry_declarations(), config.resolved_registry_declarations()] {
        assert_eq!(
            declarations["https://private.example.com/"].packages,
            Some(vec!["alpha".to_string()])
        );
    }
}

#[test]
fn two_spellings_of_one_index_are_refused() {
    let error = load(
        "registries:\n  https://pypi.example.com/simple:\n    ecosystem: pypi\n    packages: [alpha]\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n",
    )
    .unwrap_err();
    assert!(error.contains("declared twice"), "{error}");
    assert!(error.contains("pypi.example.com/simple/"), "{error}");
}

/// Each layer is validated on its own, so the machine may have routed npm
/// scopes to a URL the repository serves to `PyPI`. Keeping both would leave
/// one URL in two roles and rebuild into an entry no layer could have
/// written.
#[test]
fn a_url_a_layer_serves_to_pypi_loses_the_npm_routes_it_had() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://one.example.com/:\n    ecosystem: pypi\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.registries_by_scope.insert("@acme".to_string(), "https://one.example.com/".to_string());
    config.registries_by_prefix.insert("work".to_string(), "https://one.example.com/".to_string());
    config.registry_options_by_url.insert(
        "https://one.example.com/".to_string(),
        pnpm_lockfile::RegistryOptions {
            server_type: Some(pnpm_lockfile::RegistryServerType::Artifactory),
            supports_time_field: None,
        },
    );
    WorkspaceSettings::load_at(dir.path())
        .unwrap()
        .expect("the workspace manifest was just written")
        .apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(config.python_indexes(), ["https://one.example.com/"]);
    assert!(config.registries_by_scope.is_empty(), "{:?}", config.registries_by_scope);
    assert!(config.registries_by_prefix.is_empty(), "{:?}", config.registries_by_prefix);
    assert!(config.registry_options_by_url.is_empty(), "{:?}", config.registry_options_by_url);
    let declarations = config.resolved_registry_declarations();
    let entry = declarations.get("https://one.example.com/").expect("the index is declared");
    assert_eq!(entry.ecosystem(), Ecosystem::Pypi);
    assert!(entry.scopes.is_none(), "{entry:?}");
    assert!(entry.prefix.is_none(), "{entry:?}");
    assert!(entry.server_type.is_none(), "{entry:?}");
}

/// The mirror image, which the machine hits when a repository routes npm
/// scopes to a URL the machine had declared an index: the later layer has to
/// win here too, or its scopes are deleted as though they were the stale
/// role.
#[test]
fn a_url_a_layer_routes_to_npm_loses_the_index_role_it_had() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://one.example.com/:\n    scopes: ['@acme']\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.indexes_by_ecosystem.insert(
        Ecosystem::Pypi,
        vec!["https://one.example.com/".to_string().into()],
    );
    WorkspaceSettings::load_at(dir.path())
        .unwrap()
        .expect("the workspace manifest was just written")
        .apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(
        config.registries_by_scope.get("@acme").map(String::as_str),
        Some("https://one.example.com/"),
        "the scope the later layer routed was dropped",
    );
    assert_eq!(config.python_indexes(), [crate::DEFAULT_PYPI_INDEX_URL]);
}

/// One URL cannot be two ecosystems' index at once, or `cargo_index_url` and
/// `python_indexes` would both answer with it.
#[test]
fn a_url_reclassified_to_another_ecosystem_leaves_the_first_one() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://one.example.com/:\n    ecosystem: pypi\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.indexes_by_ecosystem.insert(
        Ecosystem::Cargo,
        vec!["https://one.example.com/".to_string().into()],
    );
    WorkspaceSettings::load_at(dir.path())
        .unwrap()
        .expect("the workspace manifest was just written")
        .apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(config.python_indexes(), ["https://one.example.com/"]);
    assert_eq!(
        config.cargo_index_url(),
        crate::DEFAULT_CARGO_INDEX_URL,
        "the URL stayed the Cargo index as well as the PyPI one",
    );
}

/// `namedRegistries` is applied after the roles are settled, so an alias
/// addressing an index would otherwise put that URL in two roles: an index of
/// its ecosystem and a bare-specifier prefix for npm.
#[test]
fn a_named_registry_alias_does_not_address_an_index() {
    let config = load(
        "registries:\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\nnamedRegistries:\n  work: https://pypi.example.com/simple\n  other: https://npm.example.com/\n",
    )
    .unwrap();
    assert_eq!(config.python_indexes(), ["https://pypi.example.com/simple/"]);
    assert!(
        !config.registries_by_prefix.contains_key("work"),
        "the alias addressed the PyPI index: {:?}",
        config.registries_by_prefix,
    );
    assert_eq!(
        config.registries_by_prefix.get("other").map(String::as_str),
        Some("https://npm.example.com/"),
        "an alias addressing an npm registry is still declared",
    );
}

/// `registries` outranks its deprecated spelling in whichever layer each of
/// them is written: only a `registries` entry can say a URL has stopped being
/// an index, so an alias in a later layer meets a role that is still in force.
#[test]
fn a_later_alias_does_not_address_an_index_an_earlier_layer_declared() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "namedRegistries:\n  work: https://pypi.example.com/simple\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.indexes_by_ecosystem.insert(
        Ecosystem::Pypi,
        vec!["https://pypi.example.com/simple/".to_string().into()],
    );
    WorkspaceSettings::load_at(dir.path())
        .unwrap()
        .expect("the workspace manifest was just written")
        .apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(config.python_indexes(), ["https://pypi.example.com/simple/"]);
    assert!(
        !config.registries_by_prefix.contains_key("work"),
        "the alias addressed the PyPI index: {:?}",
        config.registries_by_prefix,
    );
}

#[test]
fn overlapping_python_namespaces_are_rejected_before_resolution() {
    for (first, second) in [
        ("alpha", "Alpha"),
        ("Company-*", "company_tools"),
        ("company-*", "company-tool*"),
        ("**", "**"),
    ] {
        let error = load(&format!("registries:\n  https://one.example.com/:\n    ecosystem: pypi\n    packages: ['{first}']\n  https://two.example.com/:\n    ecosystem: pypi\n    packages: ['{second}']\n")).unwrap_err();
        assert!(error.contains("routed to two registries"), "{first}, {second}: {error}");
    }
}

#[test]
fn invalid_python_package_patterns_are_rejected() {
    for packages in
        ["[]", "['*']", "['company-**']", "['@scope/*']", "['alpha', 'ALPHA']", "['**', 'alpha']"]
    {
        let error = load(&format!("registries:\n  https://private.example.com/:\n    ecosystem: pypi\n    packages: {packages}\n")).unwrap_err();
        assert!(error.contains("Invalid Python package routes"), "{packages}: {error}");
    }
}

#[test]
fn multiple_python_defaults_require_explicit_package_routes() {
    let error = load("registries:\n  https://one.example.com/:\n    ecosystem: pypi\n  https://two.example.com/:\n    ecosystem: pypi\n").unwrap_err();
    assert!(error.contains("routed to two registries"), "{error}");
}

#[test]
fn package_patterns_are_not_silently_ignored_by_other_ecosystems() {
    for ecosystem in ["npm", "cargo"] {
        let error = load(&format!("registries:\n  https://one.example.com/:\n    ecosystem: {ecosystem}\n    packages: [alpha]\n")).unwrap_err();
        assert!(error.contains("only supported for pypi"), "{error}");
    }
}
