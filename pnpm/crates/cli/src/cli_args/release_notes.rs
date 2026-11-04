//! Release notes on a registry declared `serverType: pnpr`: which registries
//! store them, and reading them for the commands that offer an upgrade. The
//! routes are those of the release-notes RFC
//! (<https://github.com/pnpm/rfcs/blob/12e86ef2bb6d942aaa3adec4e7ec42df22b7d763/pnpr/text/0000-release-notes.md>).

pub(crate) mod reader;

pub(crate) use confirm::{ComposedNotes, registry_holds_notes};
pub(crate) use reader::{NotesReader, UpgradeNotes};
pub(crate) use render::{render_upgrade_notes, upgrade_notes_json};

mod confirm;
mod http;
mod render;

use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_lockfile::RegistryServerType;
use pnpm_network::normalize_registry_url;
use pnpm_release_notes::normalize_line_endings;
use std::{collections::HashSet, path::Path};

/// The text of the file a `--release-notes` value names, relative to `dir`,
/// with its line endings normalized. A path that names no file is an error,
/// never the text itself, so a mistyped path is not published as a note.
pub(crate) fn read_release_notes_arg(dir: &Path, value: &str) -> miette::Result<String> {
    let path = dir.join(value);
    if !path.is_file() {
        let dir = dir.display();
        return Err(miette::miette!(
            code = "ERR_PNPM_RELEASE_NOTES_FILE_NOT_FOUND",
            "The release notes file {value} was not found in {dir}",
        ));
    }
    let text = std::fs::read_to_string(&path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read the release notes file {value}"))?;
    Ok(normalize_line_endings(&text))
}

/// The normalized URLs of the registries declared `serverType: pnpr`, the
/// only ones pnpm sends release notes to and reads them from.
pub(crate) fn notes_registries(config: &Config) -> HashSet<String> {
    config.registry_options_by_url
        .iter()
        .filter(|(_, options)| options.server_type == Some(RegistryServerType::Pnpr))
        .map(|(url, _)| normalize_registry_url(url).into_owned())
        .collect()
}

/// Whether `registry` is declared `serverType: pnpr`.
pub(crate) fn stores_notes(config: &Config, registry: &str) -> bool {
    pnpm_lockfile::registry_server_type(&config.registry_options_by_url, registry)
        == Some(RegistryServerType::Pnpr)
}

/// The normalized registry a manifest publishes to, when that registry
/// stores release notes.
pub(crate) fn notes_registry_of_manifest(
    config: &Config,
    manifest: &serde_json::Value,
) -> Option<String> {
    let name = manifest.get("name")?.as_str()?;
    let name = crate::cli_args::changelog::published_name(manifest).unwrap_or(name);
    let registry = pnpm_publish::find_registry_info(
        name,
        &config.registry,
        &config.registries_by_scope,
        pnpm_publish::publish_config_registry(manifest, name),
    )
    .ok()?;
    stores_notes(config, registry.as_str()).then(|| registry.as_str().to_owned())
}

#[cfg(test)]
mod tests;
