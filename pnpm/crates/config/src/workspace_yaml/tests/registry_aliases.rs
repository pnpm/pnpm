//! The `aliases` a pnpr request carries next to a registry's `prefix`.

use super::{
    LoadWorkspaceYamlError, RegistryDeclaration, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs,
};
use crate::{Ecosystem, RegistryPrefixes, registries};
use indexmap::IndexMap;

fn declaration(prefix: Option<&str>, aliases: &[&str]) -> RegistryDeclaration {
    RegistryDeclaration {
        prefixes: RegistryPrefixes {
            prefix: prefix.map(str::to_owned),
            aliases: aliases
                .iter()
                .copied()
                .map(str::to_owned)
                .collect(),
        },
        ..RegistryDeclaration::default()
    }
}

#[test]
fn aliases_round_trip_with_the_smallest_as_the_prefix() {
    for aliases in [["zeta", "work", "alpha"], ["alpha", "work", "zeta"]] {
        let entries = IndexMap::from([(
            "https://npm.corp.example".to_owned(),
            declaration(Some("main"), &aliases),
        )]);
        registries::validate_declarations(entries.iter()).unwrap();
        let lookups = registries::declarations_into_lookups(entries);
        assert_eq!(lookups.registries_by_prefix.len(), 4);
        let rebuilt = registries::to_declarations(&lookups);
        let rebuilt_declaration = &rebuilt["https://npm.corp.example"];
        assert_eq!(rebuilt_declaration.prefixes.prefix.as_deref(), Some("alpha"));
        assert_eq!(rebuilt_declaration.prefixes.aliases, ["main", "work", "zeta"]);
        assert_eq!(
            registries::declarations_into_lookups(rebuilt).registries_by_prefix,
            lookups.registries_by_prefix,
        );
    }
}

#[test]
fn a_prefix_repeated_in_aliases_is_rejected() {
    for (prefix, aliases) in [(Some("work"), &["work"][..]), (None, &["zeta", "zeta"][..])] {
        let entries = IndexMap::from([(
            "https://npm.corp.example/".to_owned(),
            declaration(prefix, aliases),
        )]);
        assert!(matches!(
            registries::validate_declarations(entries.iter()),
            Err(LoadWorkspaceYamlError::PrefixDeclaredTwice { .. }),
        ));
    }
    let entries = IndexMap::from([
        ("https://npm.corp.example/".to_owned(), declaration(None, &["work"])),
        ("https://other.example/".to_owned(), declaration(Some("work"), &[])),
    ]);
    assert!(matches!(
        registries::validate_declarations(entries.iter()),
        Err(LoadWorkspaceYamlError::PrefixDeclaredTwice { .. }),
    ));
}

#[test]
fn a_declaration_without_aliases_keeps_its_wire_shape() {
    let parsed: RegistryDeclaration = serde_json::from_str(r#"{"prefix":"work"}"#).unwrap();
    assert!(parsed.prefixes.aliases.is_empty());
    assert_eq!(serde_json::to_value(parsed).unwrap(), serde_json::json!({"prefix": "work"}));
}

#[test]
fn malformed_aliases_are_rejected() {
    for aliases in [serde_json::json!(null), serde_json::json!("zeta"), serde_json::json!([42])] {
        assert!(
            serde_json::from_value::<RegistryDeclaration>(serde_json::json!({"aliases": aliases}))
                .is_err(),
        );
    }
}

#[test]
fn aliases_cannot_route_another_ecosystem() {
    let entries = IndexMap::from([(
        "https://cargo.corp.example/".to_owned(),
        RegistryDeclaration { ecosystem: Some(Ecosystem::Cargo), ..declaration(None, &["work"]) },
    )]);
    assert!(registries::validate_declarations(entries.iter()).is_err());
}

#[test]
fn the_workspace_manifest_refuses_aliases() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/:\n    prefix: work\n    aliases: [zeta]\n",
    )
    .unwrap();
    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(
        matches!(
            &error,
            LoadWorkspaceYamlError::UnknownRegistryDeclarationField { field, .. } if field == "aliases",
        ),
        "{error:?}",
    );
}
