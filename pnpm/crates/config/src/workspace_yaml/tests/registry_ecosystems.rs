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
fn the_index_an_ecosystem_falls_back_to_heads_its_list() {
    let config = load(
        "registries:\n  https://extra.example.com/simple/:\n    ecosystem: pypi\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n    default: true\n",
    )
    .unwrap();
    assert_eq!(
        config.python_indexes(),
        ["https://pypi.example.com/simple/", "https://extra.example.com/simple/"],
    );
    assert!(config.registries_by_scope.is_empty());
}

#[test]
fn a_lone_index_needs_no_default() {
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
fn several_indexes_with_none_of_them_the_default_are_refused() {
    let error = load(
        "registries:\n  https://one.example.com/simple/:\n    ecosystem: pypi\n  https://two.example.com/simple/:\n    ecosystem: pypi\n",
    )
    .unwrap_err();
    assert!(error.contains("none of them is the default"), "{error}");
}

#[test]
fn two_indexes_of_one_ecosystem_declared_the_default_are_refused() {
    let error = load(
        "registries:\n  https://one.example.com/simple/:\n    ecosystem: pypi\n    default: true\n  https://two.example.com/simple/:\n    ecosystem: pypi\n    default: true\n",
    )
    .unwrap_err();
    assert!(error.contains("declared the default"), "{error}");
    assert!(error.contains("one.example.com"), "{error}");
    assert!(error.contains("two.example.com"), "{error}");
}

#[test]
fn a_second_cargo_index_is_refused() {
    let error = load(
        "registries:\n  https://one.example.com/:\n    ecosystem: cargo\n    default: true\n  https://two.example.com/:\n    ecosystem: cargo\n",
    )
    .unwrap_err();
    assert!(error.contains("Two Cargo registries"), "{error}");
}

#[test]
fn an_npm_registry_may_not_declare_itself_the_default() {
    let error = load("registries:\n  https://npm.example.com/:\n    default: true\n").unwrap_err();
    assert!(error.contains("npm.example.com"), "{error}");
    assert!(error.contains("not for an npm registry"), "{error}");
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
        "registries:\n  https://extra.example.com/simple/:\n    ecosystem: pypi\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n    default: true\n",
    )
    .unwrap();
    let declarations = config.resolved_registry_declarations();
    let default = declarations
        .get("https://pypi.example.com/simple/")
        .expect("the default index is declared");
    assert_eq!(default.ecosystem(), Ecosystem::Pypi);
    assert!(default.is_default());
    let extra =
        declarations.get("https://extra.example.com/simple/").expect("the extra index is declared");
    assert_eq!(extra.ecosystem(), Ecosystem::Pypi);
    assert!(!extra.is_default());
}

/// Keyed by URL, the map cannot see that two spellings address one index, so
/// it would count them as two: asking for a `default` that means nothing, or
/// filling both the fallback slot and a searched-first one with one endpoint.
#[test]
fn two_spellings_of_one_index_are_refused() {
    let error = load(
        "registries:\n  https://pypi.example.com/simple:\n    ecosystem: pypi\n  https://pypi.example.com/simple/:\n    ecosystem: pypi\n    default: true\n",
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
fn a_reclassified_url_loses_the_npm_routes_an_earlier_layer_gave_it() {
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
