use super::{
    CatalogSource, Conflict, DeclaredSpecifiers, MigrationPlan, is_migratable, plan_migration,
    render_report,
};
use pnpm_catalogs_types::Catalog;
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, BTreeSet};

fn declared(entries: &[(&str, &[&str])]) -> DeclaredSpecifiers {
    entries
        .iter()
        .map(|(name, specifiers)| {
            (
                name.to_string(),
                specifiers
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        })
        .collect()
}

fn catalog(entries: &[(&str, &str)]) -> Catalog {
    entries
        .iter()
        .map(|(name, specifier)| (name.to_string(), specifier.to_string()))
        .collect()
}

#[test]
fn keeps_specifiers_a_catalog_cannot_share() {
    for specifier in [
        "catalog:",
        "catalog:react18",
        "workspace:*",
        "workspace:^",
        "workspace:../foo",
        "link:../foo",
        "file:../foo",
        "../foo",
        "runtime:22",
    ] {
        assert!(!is_migratable(specifier), "{specifier} must stay direct");
    }
    for specifier in ["^1.0.0", "1.2.3", "latest", "npm:bar@^1", "github:user/repo"] {
        assert!(is_migratable(specifier), "{specifier} must be migratable");
    }
}

#[test]
fn moves_agreed_specifiers_and_lists_the_rest() {
    let plan = plan_migration(
        &declared(&[
            ("agreed", &["^1.0.0"]),
            ("split", &["^1.0.0", "^2.0.0"]),
            ("cataloged", &["^3.0.0"]),
            ("off-catalog", &["^4.0.0"]),
        ]),
        Some(&catalog(&[("cataloged", "^3.0.0"), ("off-catalog", "^4.1.0")])),
        CatalogSource::WorkspaceManifest,
    );
    assert_eq!(
        plan,
        MigrationPlan {
            migrated: BTreeMap::from([
                ("agreed".to_string(), "^1.0.0".to_string()),
                ("cataloged".to_string(), "^3.0.0".to_string()),
            ]),
            new_entries: catalog(&[("agreed", "^1.0.0")]),
            conflicts: vec![
                Conflict {
                    name: "off-catalog".to_string(),
                    declared: BTreeSet::from(["^4.0.0".to_string()]),
                    catalog_entry: Some("^4.1.0".to_string()),
                },
                Conflict {
                    name: "split".to_string(),
                    declared: BTreeSet::from(["^1.0.0".to_string(), "^2.0.0".to_string()]),
                    catalog_entry: None,
                },
            ],
            missing_from_hook_catalogs: Vec::new(),
        },
    );
    assert_eq!(
        render_report(&plan),
        "These dependencies were left unchanged because the projects or the default catalog disagree on their specifier:
  off-catalog: ^4.0.0 (catalog: ^4.1.0)
  split: ^1.0.0, ^2.0.0
Moved 2 dependencies to the default catalog. Run `pnpm install` to update the lockfile.",
    );
}

#[test]
fn reports_when_nothing_moves() {
    let plan = plan_migration(&DeclaredSpecifiers::new(), None, CatalogSource::WorkspaceManifest);
    assert_eq!(render_report(&plan), "No dependencies were moved to the default catalog.");
}

#[test]
fn strips_control_characters_from_conflicts() {
    let conflict = Conflict {
        name: "evil\u{1b}[2J".to_string(),
        declared: BTreeSet::from(["^1.0.0\u{1b}[31m".to_string(), "^2.0.0".to_string()]),
        catalog_entry: Some("^3.0.0\u{7}".to_string()),
    };
    assert_eq!(conflict.to_string(), "evil[2J: ^1.0.0[31m, ^2.0.0 (catalog: ^3.0.0)");
}

#[test]
fn adds_no_entries_to_catalogs_a_hook_supplies() {
    let plan = plan_migration(
        &declared(&[("cataloged", &["^1.0.0"]), ("uncataloged", &["^2.0.0"])]),
        Some(&catalog(&[("cataloged", "^1.0.0")])),
        CatalogSource::UpdateConfigHook,
    );
    assert_eq!(
        plan,
        MigrationPlan {
            migrated: BTreeMap::from([("cataloged".to_string(), "^1.0.0".to_string())]),
            new_entries: Catalog::new(),
            conflicts: Vec::new(),
            missing_from_hook_catalogs: vec!["uncataloged".to_string()],
        },
    );
    assert_eq!(
        render_report(&plan),
        "These dependencies were left unchanged because the catalogs come from an updateConfig hook that has no entry for them: uncataloged
Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile.",
    );
}
