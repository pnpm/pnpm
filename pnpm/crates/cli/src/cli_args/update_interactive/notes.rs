//! The release notes `update --interactive` shows for the package under the
//! cursor: those between the installed version and the one offered, from a
//! registry declared `serverType: pnpr`.

use crate::cli_args::{
    outdated::OutdatedPackage,
    registry_client::{
        build_registry_client, resolve_registries_with_override, resolve_registry_for_package,
    },
    release_notes::{NotesReader, render_upgrade_notes, stores_notes},
};
use pnpm_config::Config;
use std::collections::HashMap;

/// Rendered notes by the package a choice selects.
pub(super) type ChoiceNotes = HashMap<String, String>;

/// The notes of every choice that has some. Notes are a help for choosing,
/// so a registry that fails to answer leaves its packages without them.
pub(super) async fn choice_notes(config: &Config, choices: &[OutdatedPackage]) -> ChoiceNotes {
    let registries = resolve_registries_with_override(config, None);
    let candidates: Vec<(&OutdatedPackage, String)> = choices
        .iter()
        .filter(|choice| {
            !choice.github_action && choice.metadata.display_versions.is_none()
        })
        .filter_map(|choice| {
            let registry = resolve_registry_for_package(&registries, &choice.package_name, None);
            stores_notes(config, &registry).then_some((choice, registry))
        })
        .collect();
    if candidates.is_empty() {
        return ChoiceNotes::new();
    }
    let Ok(client) = build_registry_client(config) else {
        return ChoiceNotes::new();
    };
    let reader = NotesReader::new(&client, config);
    let reads = candidates
        .iter()
        .map(|(choice, registry)| {
            let reader = &reader;
            async move {
                let notes = reader
                    .upgrade_notes(registry, &choice.package_name, &choice.current, &choice.target)
                    .await
                    .ok()
                    .filter(|notes| !notes.is_empty())?;
                Some((choice.alias.clone(), render_upgrade_notes(&notes)))
            }
        });
    let mut notes = ChoiceNotes::new();
    for (alias, text) in futures_util::future::join_all(reads).await.into_iter().flatten() {
        notes.entry(alias).or_insert(text);
    }
    notes
}
