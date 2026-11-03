//! The check that lets a later `pnpm version -r` delete the intents a
//! release consumed: the registry holds the notes composed from them.

use super::http::{NotesHttp, PublicationLink};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_release_notes::{normalize_line_endings, note_digest};

/// The notes `pnpm version -r` composed for one release.
pub(crate) struct ComposedNotes<'a> {
    pub version_note: &'a str,
    /// The note of the publication the release is in, when it has one.
    pub publication_note: Option<&'a str>,
}

/// Whether the registry holds `composed` for `name@version`: each note
/// either still carries the composed text, or was created from it, which
/// its immutable initial digest proves after an edit or a deletion. A note
/// published with other text, or a version published without notes, does
/// not count, so its intents are kept. Failures read as "not held".
pub(crate) async fn registry_holds_notes(
    client: &ThrottledClient,
    config: &Config,
    (registry, name, version): (&str, &str, &str),
    composed: &ComposedNotes<'_>,
) -> bool {
    let http = NotesHttp { client, config };
    let Ok(Some(response)) = http.version_note(registry, name, version).await else {
        return false;
    };
    if !holds(response.text.as_deref(), response.initial_digest.as_deref(), composed.version_note) {
        return false;
    }
    match composed.publication_note.filter(|note| !note.trim().is_empty()) {
        None => true,
        Some(expected) => {
            let Some(link) = response.publication else {
                return false;
            };
            publication_holds(&http, &link, name, expected).await
        }
    }
}

async fn publication_holds(
    http: &NotesHttp<'_>,
    link: &PublicationLink,
    package: &str,
    expected: &str,
) -> bool {
    if link.initial_digest.as_deref() == Some(note_digest(expected).as_str()) {
        return true;
    }
    match http.publication_note(&link.url, package).await {
        Ok(Some((text, digest))) => holds(Some(&text), digest.as_deref(), expected),
        _ => false,
    }
}

fn holds(text: Option<&str>, initial_digest: Option<&str>, expected: &str) -> bool {
    let expected_text = expected.trim();
    expected_text.is_empty()
        || initial_digest == Some(note_digest(expected).as_str())
        || text.is_some_and(|text| normalize_line_endings(text).contains(expected_text))
}
