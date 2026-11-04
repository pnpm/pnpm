//! Notes are the publisher's words, printed as plain text: control and
//! bidirectional override characters are removed and link targets shown.

use super::reader::UpgradeNotes;
use pnpm_release_notes::render_note;
use pnpm_text_sanitize::{sanitize, sanitize_inline};
use serde_json::{Value, json};

/// The notes, newest version first, each publication note once above the
/// version notes that came out of it.
pub(crate) fn render_upgrade_notes(notes: &UpgradeNotes) -> String {
    let mut printed = vec![false; notes.publications.len()];
    let mut blocks = Vec::new();
    for version in &notes.versions {
        if let Some(index) = version.publication
            && !printed[index]
        {
            printed[index] = true;
            let publication = &notes.publications[index];
            let title = publication.name
                .as_deref()
                .map_or_else(
                    || "Release notes of the publication".to_owned(),
                    |name| format!("Release {}", sanitize_inline(name)),
                );
            blocks.push(block(&title, &publication.text));
        }
        if let Some(text) = &version.text {
            blocks.push(block(&sanitize_inline(&version.version), text));
        }
    }
    blocks.join("\n\n")
}

fn block(title: &str, text: &str) -> String {
    let body = render_note(text)
        .lines()
        .map(|line| if line.is_empty() { String::new() } else { format!("  {line}") })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{title}:\n{body}")
}

/// The notes for `--json` output, sanitized but not rendered.
pub(crate) fn upgrade_notes_json(notes: &UpgradeNotes) -> Value {
    let versions: Vec<Value> = notes.versions
        .iter()
        .map(|version| {
            json!({
                "version": version.version,
                "notes": version.text.as_deref().map(sanitize),
                "publication": version.publication,
            })
        })
        .collect();
    let publications: Vec<Value> = notes.publications
        .iter()
        .map(|publication| {
            json!({
                "name": publication.name.as_deref().map(sanitize_inline),
                "url": pnpm_network::redact_url_credentials(&publication.url),
                "notes": sanitize(&publication.text),
            })
        })
        .collect();
    json!({ "versions": versions, "publications": publications })
}
