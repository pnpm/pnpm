//! `pnpm outdated --notes`: the release notes between the current and the
//! latest version of each outdated package, read from a registry declared
//! `serverType: pnpr`.

use super::{OutdatedArgs, OutdatedFormat, OutdatedPackage};
use crate::cli_args::{
    registry_client::{
        build_registry_client, resolve_registries_with_override, resolve_registry_for_package,
    },
    release_notes::{
        NotesReader, UpgradeNotes, render_upgrade_notes, stores_notes, upgrade_notes_json,
    },
};
use pnpm_config::Config;
use serde_json::Value;

impl OutdatedArgs {
    /// The notes of each package, in order. Empty without `--notes`. A
    /// registry that fails to answer costs its packages their notes, not
    /// the report.
    pub(super) async fn release_notes(
        &self,
        config: &Config,
        packages: &[&OutdatedPackage],
    ) -> miette::Result<Vec<Option<UpgradeNotes>>> {
        if !self.output.notes {
            return Ok(Vec::new());
        }
        let client = build_registry_client(config)?;
        let reader = NotesReader::new(&client, config);
        let registries = resolve_registries_with_override(config, None);
        let reads = packages
            .iter()
            .map(|package| {
                let (reader, registries) = (&reader, &registries);
                async move {
                    let registry = notes_registry(config, registries, package)?;
                    let notes = reader.upgrade_notes(
                        &registry,
                        &package.package_name,
                        &package.current,
                        &package.target,
                    )
                    .await;
                    match notes {
                        Ok(notes) => (!notes.is_empty()).then_some(notes),
                        Err(error) => {
                            warn(&package.package_name, &error);
                            None
                        }
                    }
                }
            });
        Ok(futures_util::future::join_all(reads).await)
    }
}

/// The registry an npm package resolves from, when it stores notes.
fn notes_registry(
    config: &Config,
    registries: &std::collections::HashMap<String, String>,
    package: &OutdatedPackage,
) -> Option<String> {
    if package.github_action || package.metadata.display_versions.is_some() {
        return None;
    }
    let registry = resolve_registry_for_package(registries, &package.package_name, None);
    stores_notes(config, &registry).then_some(registry)
}

fn warn(package: &str, error: &miette::Report) {
    pnpm_diagnostics::eprintln!("WARN  Could not read the release notes of {package}: {error}");
}

/// `output` with the notes of `packages`: a `releaseNotes` field in the
/// JSON entry of each, found by its key in the report, or else a block per
/// package below the report.
pub(super) fn with_notes(
    output: String,
    format: OutdatedFormat,
    packages: &[&OutdatedPackage],
    notes: &[Option<UpgradeNotes>],
) -> String {
    if notes.iter().all(Option::is_none) {
        return output;
    }
    match format {
        OutdatedFormat::Json => with_json_notes(output, packages, notes),
        OutdatedFormat::Table | OutdatedFormat::List => {
            let blocks = packages
                .iter()
                .zip(notes)
                .filter_map(|(package, notes)| Some(text_block(package, notes.as_ref()?)));
            std::iter::once(output)
                .chain(blocks)
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    }
}

fn text_block(package: &OutdatedPackage, notes: &UpgradeNotes) -> String {
    let heading = format!(
        "{} {} => {}",
        pnpm_text_sanitize::sanitize_inline(&package.package_name),
        package.current_str(),
        package.target_str(),
    );
    let body = render_upgrade_notes(notes)
        .lines()
        .map(|line| if line.is_empty() { String::new() } else { format!("  {line}") })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{heading}\n{body}")
}

/// Two packages can render as one entry, such as two aliases of one
/// package at one version, so the report can have fewer entries than
/// `packages`: each finds its entry by key rather than by position. The
/// entry shows the last of them, as the report does, so it gets that one's
/// notes or none.
fn with_json_notes(
    output: String,
    packages: &[&OutdatedPackage],
    notes: &[Option<UpgradeNotes>],
) -> String {
    let Ok(Value::Object(mut report)) = serde_json::from_str::<Value>(&output) else {
        return output;
    };
    for (key, notes) in super::render::report_keys(packages).iter().zip(notes) {
        let Some(Value::Object(entry)) = report.get_mut(key) else {
            continue;
        };
        match notes {
            Some(notes) => {
                entry.insert("releaseNotes".to_owned(), upgrade_notes_json(notes));
            }
            None => {
                entry.remove("releaseNotes");
            }
        }
    }
    serde_json::to_string_pretty(&Value::Object(report)).unwrap_or(output)
}
